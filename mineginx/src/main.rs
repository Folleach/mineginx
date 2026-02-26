use std::{
    borrow::BorrowMut, collections::HashMap, env, fs::{self}, io::ErrorKind, path::Path, process::ExitCode, sync::Arc, time::Duration
};
use config::{MinecraftServerDescription, MineginxConfig};
use log::{error, info, warn};
use minecraft::{packets::{HandshakeC2SPacket, MinecraftPacket}, serialization::{truncate_to_zero, MinecraftStream}};
use simple_logger::SimpleLogger;
use tokio::{io::AsyncWriteExt, net::{TcpListener, TcpStream}, task::JoinHandle, time::timeout};
use stream::forward_half;

mod stream;
mod config;

fn find_upstream(domain: &String, config: Arc<MineginxConfig>) -> Option<MinecraftServerDescription> {
    for x in &config.servers {
        for server_name in &x.server_names {
            if server_name == domain {
                return Some(x.clone());
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
    if let Err(e) = client.set_nodelay(true) {
        error!("failed to set no_delay for client: {}", e);
        return;
    }
    let mut minecraft = MinecraftStream::new(client.borrow_mut(), 4096);
    let timeout_future = Duration::from_millis(config.handshake_timeout_ms.unwrap_or(10_000));
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
    let upstream_server = match find_upstream(&domain, config.clone()) {
        Some(x) => x,
        None => {
            warn!("there is no upstream for domain {:#?}", &domain);
            return;
        }
    };

    info!("new connection (protocol_version: {}, domain: {}, upstream: {})", &handshake.protocol_version, &domain, upstream_server.proxy_pass);

    let mut upstream = match TcpStream::connect(&upstream_server.proxy_pass).await {
        Ok(x) => x,
        Err(e) => {
            error!("failed to connect upstream: {}, {e}", &upstream_server.proxy_pass);
            return;
        }
    };
    if let Err(e) = upstream.set_nodelay(true) {
        error!("failed to set no_delay for upstream: {}", e);
        return;
    }
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
    let buf_size = upstream_server.buffer_size.map(|b| b as usize).unwrap_or(8192);

    // Each direction gets its own spawned task so the tokio scheduler
    // can freely interleave them with the accept loop and other connections.
    // When one direction reads EOF it calls shutdown() on its writer,
    // sending FIN to the remote — the opposite task then naturally reads
    // EOF from its side and terminates. No explicit signaling needed.
    let c2s = forward_half(client_reader, upstream_writer, buf_size);
    let s2c = forward_half(upstream_reader, client_writer, buf_size);

    let _ = tokio::join!(c2s, s2c);
    info!("connection closed (domain: {}, upstream: {})", &domain, upstream_server.proxy_pass);
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

fn read_config() -> Result<MineginxConfig, String> {
    let config = fs::read(CONFIG_FILE);

    let config: MineginxConfig = match config {
        Ok(v) => serde_yaml::from_slice(&v).map_err(|e| format!("failed to parse config file: '{CONFIG_FILE}': {e}"))?,
        Err(e) if e.kind() == ErrorKind::NotFound => generate_config().map_err(|e| format!("config not found. failed to generate new one: {e}"))?,
        Err(e) => return Err(format!("failed to read config file: '{CONFIG_FILE}', error: {e}"))
    };

    Ok(config)
}

fn generate_config() -> Result<MineginxConfig, String> {
    info!("generate new configuration file");
    let default_server = MinecraftServerDescription {
        listen: "0.0.0.0:25565".to_string(),
        server_names: vec!["mineginx.localhost".to_string()],
        proxy_pass: "127.0.0.1:7878".to_string(),
        buffer_size: None
    };
    let servers: Vec<MinecraftServerDescription> = vec![default_server];
    let config = MineginxConfig {
        handshake_timeout_ms: Some(30_000),
        servers
    };
    let yaml = match serde_yaml::to_string(&config) {
        Ok(x) => x,
        Err(err) => return Err(format!("failed to serialize default configuration: {}", err))
    };

    if !Path::new("./config").exists() {
        if let Err(err) = fs::create_dir("./config") {
            return Err(format!("failed to create config directory: {}", err));
        };
    }
    if let Err(err) = fs::write("./config/mineginx.yaml", yaml) {
        return Err(format!("failed to save default configuration: {}", err));
    }

    Ok(config)
}

async fn check_config() -> Option<()> {
    info!("trying to parse config and exit");
    match read_config() {
        Ok(_) => {
            info!("it's fine! let's try to run");
            Some(())
        },
        Err(e) => {
            error!("there are some errors: {e}");
            None
        }
    }
}

#[allow(dead_code)]
struct ListeningAddress(JoinHandle<()>);

const CONFIG_FILE: &str = "./config/mineginx.yaml";

#[tokio::main(flavor = "multi_thread")]
async fn main() -> ExitCode {
    SimpleLogger::new().init().unwrap();

    std::panic::set_hook(Box::new(|panic_info| {
        eprintln!("PANIC: {}", panic_info);
        log::error!("panic occurred: {}", panic_info);
    }));

    let mut args = env::args();
    if args.any(|x| &x == "-t") {
        return match check_config().await {
            Some(_) => ExitCode::from(0),
            None => ExitCode::from(1)
        };
    }

    info!("mineginx version: {} ({})", env!("MINEGINX_VERSION"), env!("MINEGINX_HASH"));
    let config: Arc<MineginxConfig> = match read_config() {
        Ok(x) => Arc::new(x),
        Err(e) => {
            error!("failed to read config: {e}");
            return  ExitCode::from(2);
        }
    };
    let mut listening = HashMap::<String, ListeningAddress>::new();
    for server in &config.servers {
        if listening.contains_key(&server.listen) {
            continue;
        }
        info!("listening {}", &server.listen);
        let listener = match TcpListener::bind(&server.listen).await {
            Ok(l) => l,
            Err(e) => {
                error!("failed to bind {}: {e}", &server.listen);
                return ExitCode::from(3);
            }
        };
        let conf = config.clone();
        let task = tokio::spawn(async move {
            handle_address(&listener, conf).await;
        });
        listening.insert(server.listen.to_string(), ListeningAddress(task));
    }
    if let Err(e) = tokio::signal::ctrl_c().await {
        error!("failed to listen for ctrl_c signal: {e}");
    }
    info!("shutdown");
    ExitCode::from(0)
}
