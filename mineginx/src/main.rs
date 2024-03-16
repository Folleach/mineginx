use std::{
    sync::Arc, fs, collections::HashMap, time::Duration,
};
use config::MineginxConfig;
use minecraft::{packets::{HandshakeC2SPacket, PacketSerializer}, serialization::{read_var_i32, truncate_to_zero, ReadingError, SlicedStream}};
use tokio::{net::{TcpListener, TcpStream}, io::{AsyncReadExt, AsyncWriteExt}, sync::oneshot::{self}, task::JoinHandle, time::timeout};
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

struct MineginxPacket<T> {
    packet: T,
    #[allow(dead_code)]
    packet_id: i32,
    already_read: usize,
    raw: Vec<u8>
}

async fn read_packet<T>(client: &mut TcpStream) -> Result<MineginxPacket<T>, ()> where T: PacketSerializer<T> {
    let mut raw: Vec<u8> = vec![0; 32];
    let mut length: i32;
    let mut packet_id: i32;
    let mut already_read: usize = 0;
    let mut slice: SlicedStream;

    'l: loop {
        let start = already_read;
        let end = raw.len();
        let bslc = &mut raw[start..end];
        let read = client.read(bslc).await;
        match read {
            Ok(size) => {
                if size == 0 {
                    return Err(());
                }
                already_read += size;
            },
            Err(_) => {
                return Err(());
            }
        }
        slice = SlicedStream::new(&raw);
        match read_var_i32(&mut slice) {
            Ok(x) => length = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return Err(());
                }
                raw.resize(raw.len() * 2, 0);
                continue 'l;
            }
        }
        match read_var_i32(&mut slice) {
            Ok(x) => packet_id = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return Err(());
                }
                raw.resize(raw.len() * 2, 0);
                continue 'l;
            }
        }

        if length < 0 {
            return Err(());
        }

        if already_read < (length as usize) {
            continue 'l;
        }
        break 'l;
    }

    let packet = match T::from_raw(&raw[slice.get_position()..]) {
        Ok(packet) => packet,
        Err(_) => return Err(())
    };
    Ok(MineginxPacket { packet, packet_id, already_read, raw })
}

async fn read_handshake_packet(client: &mut TcpStream) -> Result<MineginxPacket<HandshakeC2SPacket>, ()> {
    read_packet::<HandshakeC2SPacket>(client).await
}

async fn handle_client(mut client: TcpStream, config: Arc<MineginxConfig>) {
    let timeout_future = Duration::from_millis(if let Some(milliseconds) = config.handshake_timeout_ms { milliseconds } else { 10_000 });
    let handshake_result = timeout(timeout_future, read_handshake_packet(&mut client)).await;
    let handshake = match handshake_result {
        Ok(result) => match result {
            Ok(handshake) => {
                handshake
            }
            Err(_) => {
                println!("handshake failed for someone");
                return;
            }
        },
        Err(_) => {
            println!("handshake timeout for someone");
            return;
        }
    };

    let domain = truncate_to_zero(&handshake.packet.domain).to_string();
    let upstream_address = match find_upstream(&domain, config.clone()) {
        Some(x) => x,
        None => {
            println!("there is no upstream for domain {:#?}", &domain);
            return;
        }
    };

    println!("new connection (protocol_version: {}, domain: {}, upstream: {})", &handshake.packet.protocol_version, &domain, upstream_address);

    let mut upstream = match TcpStream::connect(&upstream_address).await {
        Ok(x) => x,
        Err(e) => {
            println!("failed to connect upstream: {}, {:#?}", &upstream_address, e);
            return;
        }
    };
    match upstream.write_all(&handshake.raw[..handshake.already_read]).await {
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
                println!("failed to accept client: {:#?}", e);
                continue;
            }
        };
        let conf = config.clone();
        tokio::spawn(async move {
            handle_client(socket, conf).await;
        });
    }
}

struct ListeningAddress(JoinHandle<()>);

const CONFIG_FILE: &str = "./config/mineginx.yaml";

#[tokio::main(flavor = "multi_thread")]
async fn main() {
    let yaml = match fs::read(CONFIG_FILE) {
        Ok(x) => x,
        Err(e) => {
            println!("failed to open config file: '{}', error: '{}'", CONFIG_FILE, e.kind());
            return;
        }
    };
    let config: Arc<MineginxConfig> = Arc::new(serde_yaml::from_slice(&yaml).unwrap());
    let mut listening = HashMap::<String, ListeningAddress>::new();
    for server in &config.servers {
        if listening.contains_key(&server.listen) {
            continue;
        }
        println!("listening {}", &server.listen);
        let listener = TcpListener::bind(&server.listen).await.unwrap();
        let conf = config.clone();
        let task = tokio::spawn(async move {
            handle_address(&listener, conf).await;
        });
        listening.insert(server.listen.to_string(), ListeningAddress(task));
    }
    tokio::signal::ctrl_c().await.unwrap();
    println!("shutdown mineginx");
}
