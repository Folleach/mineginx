use std::{
    borrow::BorrowMut, collections::HashMap, fs, sync::Arc, time::Duration
};
use config::MineginxConfig;
use log::{error, info, warn};
use minecraft::{packets::{HandshakeC2SPacket, MinecraftPacket}, serialization::{truncate_to_zero, MinecraftStream}};
use simple_logger::SimpleLogger;
use tokio::{io::AsyncWriteExt, net::{TcpListener, TcpStream}, sync::oneshot, task::JoinHandle, time::timeout};
use stream::forward_stream;

mod stream;
mod config;

fn find_upstream(domain: &String, config: Arc<MineginxConfig>) -> Option<String> {
    for x in &config.servers {
        for server_name in &x.server_names {
            if server_name == domain {
                return Some(x.proxy_pass.clone());
            }
        }
    }
    None
}

async fn read_handshake_packet(client: &mut MinecraftStream<&mut TcpStream>) -> Result<HandshakeC2SPacket, ()> {
    let signature = client.read_signature().await?;
    if signature.packet_id != 0 {
        return Err(());
    }
    let handshake = client.read_data::<HandshakeC2SPacket>(signature).await?;
    Ok(handshake)
}

async fn handle_client(mut client: TcpStream, config: Arc<MineginxConfig>) {
    let mut minecraft = MinecraftStream::new(client.borrow_mut(), 4096);
    let timeout_future = Duration::from_millis(if let Some(milliseconds) = config.handshake_timeout_ms { milliseconds } else { 10_000 });
    let handshake_result = timeout(timeout_future, read_handshake_packet(&mut minecraft)).await;
    let handshake = match handshake_result {
        Ok(result) => match result {
            Ok(handshake) => {
                handshake
            }
            Err(_) => {
                error!("handshake failed for someone");
                return;
            }
        },
        Err(err) => {
            error!("handshake timeout for someone {err}");
            return;
        }
    };

    let domain = truncate_to_zero(&handshake.domain).to_string();
    let upstream_address = match find_upstream(&domain, config.clone()) {
        Some(x) => x,
        None => {
            warn!("there is no upstream for domain {:#?}", &domain);
            return;
        }
    };

    info!("new connection (protocol_version: {}, domain: {}, upstream: {})", &handshake.protocol_version, &domain, upstream_address);

    let mut upstream = match TcpStream::connect(&upstream_address).await {
        Ok(x) => x,
        Err(e) => {
            error!("failed to connect upstream: {}, {e}", &upstream_address);
            return;
        }
    };
    let packet = match MinecraftPacket::make_raw(0, &handshake) {
        Some(v) => v,
        None => return
    };
    match upstream.write_all(&packet[0..packet.len()]).await {
        Ok(_) => { },
        Err(_) => return
    };
    // flush unread buffer to the upstream
    match upstream.write_all(&minecraft.take_buffer()).await {
        Ok(_) => {},
        Err(_) => {
            return;
        }
    }
    let (client_reader, client_writer) = client.into_split();
    let (upstream_reader, upstream_writer) = upstream.into_split();
    let (client_close_sender, client_close_receiver) = oneshot::channel::<()>();
    let (upstream_close_sender, upstream_close_receiver) = oneshot::channel::<()>();
    forward_stream(
        client_close_sender,
        upstream_close_receiver,
        client_reader,
        upstream_writer);
    forward_stream(
        upstream_close_sender,
        client_close_receiver,
        upstream_reader,
        client_writer);
}

async fn handle_address(listener: &TcpListener, config: Arc<MineginxConfig>) {
    loop {
        let (socket, _address) = match listener.accept().await {
            Ok(x) => x,
            Err(e) => {
                error!("failed to accept client: {e}");
                continue;
            }
        };
        let conf = config.clone();
        tokio::spawn(async move {
            handle_client(socket, conf).await;
        });
    }
}

#[allow(dead_code)]
struct ListeningAddress(JoinHandle<()>);

const CONFIG_FILE: &str = "./config/mineginx.yaml";

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    SimpleLogger::new().init().unwrap();
    let yaml = match fs::read(CONFIG_FILE) {
        Ok(x) => x,
        Err(err) => {
            error!("failed to open config file: '{}', error: {err}", CONFIG_FILE);
            return;
        }
    };
    let config: Arc<MineginxConfig> = match serde_yaml::from_slice(&yaml) {
        Ok(c) => Arc::new(c),
        Err(err) => {
            error!("failed to parse config file: '{}', error: {err}", CONFIG_FILE);
            return;
        }
    };
    let mut listening = HashMap::<String, ListeningAddress>::new();
    for server in &config.servers {
        if listening.contains_key(&server.listen) {
            continue;
        }
        info!("listening {}", &server.listen);
        let listener = TcpListener::bind(&server.listen).await.unwrap();
        let conf = config.clone();
        let task = tokio::spawn(async move {
            handle_address(&listener, conf).await;
        });
        listening.insert(server.listen.to_string(), ListeningAddress(task));
    }
    tokio::signal::ctrl_c().await.unwrap();
    info!("shutdown");
}
