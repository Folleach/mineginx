use std::{
    borrow::BorrowMut, collections::HashMap, env, fs::{self}, io::ErrorKind, net::SocketAddr, path::Path, process::ExitCode, sync::Arc, time::Duration
};
use config::{MinecraftServerDescription, MineginxConfig};
use log::{debug, error, info, warn};
use minecraft::{packets::{HandshakeC2SPacket, MinecraftPacket}, serialization::{truncate_to_zero, MinecraftStream}};
use simple_logger::SimpleLogger;
use tokio::{io::AsyncWriteExt, net::{TcpListener, TcpStream}, sync::RwLock, task::JoinHandle, time::timeout};
use stream::forward_half;

mod stream;
mod config;

/// Holds upstream info with a pre-resolved socket address so that
/// per-connection connects skip DNS entirely. On musl + scratch containers,
/// getaddrinfo() is synchronous and blocks the tokio blocking threadpool,
/// which can stall the accept loop and block all new connections/pings.
#[derive(Clone)]
struct ResolvedUpstream {
    proxy_pass: String,
    addr: SocketAddr,
    buffer_size: Option<u32>,
}

/// Try to resolve an upstream hostname to a SocketAddr asynchronously.
/// Uses a 2-second timeout to avoid blocking startup or connections.
async fn try_resolve(proxy_pass: &str) -> Option<SocketAddr> {
    match timeout(Duration::from_secs(2), tokio::net::lookup_host(proxy_pass)).await {
        Ok(Ok(mut addrs)) => addrs.next(),
        _ => None,
    }
}

fn find_upstream_config<'a>(domain: &str, config: &'a MineginxConfig) -> Option<&'a MinecraftServerDescription> {
    let domain = domain.trim_end_matches('.');
    for x in &config.servers {
        for server_name in &x.server_names {
            let cfg_name = server_name.trim_end_matches('.');
            if cfg_name.eq_ignore_ascii_case(domain) {
                return Some(x);
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

async fn handle_client(mut client: TcpStream, config: Arc<MineginxConfig>, resolved: Arc<RwLock<HashMap<String, ResolvedUpstream>>>) {
    let peer = client.peer_addr().ok();
    debug!("accepted connection from {:?}", peer);
    if let Err(e) = client.set_nodelay(true) {
        error!("failed to set no_delay for client: {}", e);
        return;
    }
    let mut minecraft = MinecraftStream::new(client.borrow_mut(), 4096);
    debug!("reading handshake from {:?}", peer);
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

    debug!("handshake complete from {:?}, domain: {}", peer, &handshake.domain);
    let domain = truncate_to_zero(&handshake.domain).to_string();
    let server_desc = match find_upstream_config(&domain, &config) {
        Some(x) => x.clone(),
        None => {
            warn!("there is no upstream for domain {:#?}", &domain);
            return;
        }
    };

    // Look up the pre-resolved address, or try resolving on-the-fly
    // for upstreams that weren't available at startup
    let upstream = {
        let cache = resolved.read().await;
        cache.get(&server_desc.proxy_pass).cloned()
    };
    let upstream = match upstream {
        Some(u) => u,
        None => {
            // Upstream wasn't resolved at startup — try now (container may have come online)
            match try_resolve(&server_desc.proxy_pass).await {
                Some(addr) => {
                    let entry = ResolvedUpstream {
                        proxy_pass: server_desc.proxy_pass.clone(),
                        addr,
                        buffer_size: server_desc.buffer_size,
                    };
                    info!("lazily resolved upstream {} -> {}", &server_desc.proxy_pass, addr);
                    let mut cache = resolved.write().await;
                    cache.insert(server_desc.proxy_pass.clone(), entry.clone());
                    entry
                },
                None => {
                    error!("failed to resolve upstream '{}' for domain {:#?}", &server_desc.proxy_pass, &domain);
                    return;
                }
            }
        }
    };

    info!("new connection (protocol_version: {}, domain: {}, upstream: {})", &handshake.protocol_version, &domain, upstream.proxy_pass);

    // Connect using the pre-resolved SocketAddr — no DNS lookup at connection time
    let mut upstream_conn = match TcpStream::connect(upstream.addr).await {
        Ok(x) => x,
        Err(e) => {
            error!("failed to connect upstream: {}, {e}", &upstream.proxy_pass);
            return;
        }
    };
    if let Err(e) = upstream_conn.set_nodelay(true) {
        error!("failed to set no_delay for upstream: {}", e);
        return;
    }
    let packet = match MinecraftPacket::make_raw(0, &handshake) {
        Some(v) => v,
        None => return
    };
    match upstream_conn.write_all(&packet[0..packet.len()]).await {
        Ok(_) => { },
        Err(_) => return
    };
    // flush unread buffer to the upstream
    match upstream_conn.write_all(&minecraft.take_buffer()).await {
        Ok(_) => {},
        Err(_) => {
            return;
        }
    }

    let (client_reader, client_writer) = client.into_split();
    let (upstream_reader, upstream_writer) = upstream_conn.into_split();
    let buf_size = upstream.buffer_size.map(|b| b as usize).unwrap_or(8192);

    // Each direction gets its own spawned task so the tokio scheduler
    // can freely interleave them with the accept loop and other connections.
    // When one direction reads EOF it calls shutdown() on its writer,
    // sending FIN to the remote — the opposite task then naturally reads
    // EOF from its side and terminates. No explicit signaling needed.
    let c2s = forward_half(client_reader, upstream_writer, buf_size);
    let s2c = forward_half(upstream_reader, client_writer, buf_size);

    let _ = tokio::join!(c2s, s2c);
    info!("connection closed (domain: {}, upstream: {})", &domain, upstream.proxy_pass);
}

async fn handle_address(listener: &TcpListener, config: Arc<MineginxConfig>, resolved: Arc<RwLock<HashMap<String, ResolvedUpstream>>>) {
    loop {
        debug!("accept loop waiting for connection");
        let (socket, _address) = match listener.accept().await {
            Ok(x) => {
                debug!("accept() returned a connection");
                x
            },
            Err(e) => {
                error!("failed to accept client: {e}");
                continue;
            }
        };
        let conf = config.clone();
        let res = resolved.clone();
        tokio::spawn(async move {
            handle_client(socket, conf, res).await;
        });
        debug!("spawned handler, looping back to accept");
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

    // Pre-resolve all upstream DNS at startup so per-connection connects
    // use raw SocketAddr and never touch DNS. On musl (used in scratch
    // containers), getaddrinfo() is synchronous and blocks tokio's
    // blocking threadpool, which can stall the accept loop.
    // Upstreams that fail to resolve are skipped — they'll be resolved
    // lazily when a client first connects to them.
    let mut resolved = HashMap::<String, ResolvedUpstream>::new();
    for server in &config.servers {
        if resolved.contains_key(&server.proxy_pass) {
            continue;
        }
        match try_resolve(&server.proxy_pass).await {
            Some(addr) => {
                info!("resolved upstream {} -> {}", &server.proxy_pass, addr);
                resolved.insert(server.proxy_pass.clone(), ResolvedUpstream {
                    proxy_pass: server.proxy_pass.clone(),
                    addr,
                    buffer_size: server.buffer_size,
                });
            },
            None => {
                warn!("upstream '{}' not available yet, will resolve on first connection", &server.proxy_pass);
            }
        }
    }
    let resolved = Arc::new(RwLock::new(resolved));

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
        let res = resolved.clone();
        let task = tokio::spawn(async move {
            handle_address(&listener, conf, res).await;
        });
        listening.insert(server.listen.to_string(), ListeningAddress(task));
    }
    if let Err(e) = tokio::signal::ctrl_c().await {
        error!("failed to listen for ctrl_c signal: {e}");
    }
    info!("shutdown");
    ExitCode::from(0)
}
