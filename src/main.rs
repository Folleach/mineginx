use std::{
    sync::Arc, fs, collections::HashMap,
};
use config::MineginxConfig;
use tokio::{net::{TcpListener, TcpStream}, io::{AsyncReadExt, AsyncWriteExt}, sync::oneshot::{self}, task::JoinHandle};
use crate::{minecraft::serialization::{ read_string, read_var_i32, SlicedStream, ReadingError }, stream::forward_stream};

mod minecraft;
mod stream;
mod config;

fn find_upstream(domain: &String, config: Arc<MineginxConfig>) -> Option<String> {
    for x in &config.servers {
        if x.server_name == *domain {
            return Some(x.proxy_pass.clone());
        }
    }
    None
}

async fn handle_client(mut client: TcpStream, config: Arc<MineginxConfig>) {
    let mut handshake: Vec<u8> = vec![0; 32];
    let mut _length: i32;
    let mut packet_id: i32;
    let mut protocol_version: i32;
    let domain: String;
    let mut already_read: usize = 0;

    // todo: make reading packages painless
    loop {
        let start = already_read;
        let end = handshake.len();
        let bslc = &mut handshake[start..end];
        let read = client.read(bslc).await;
        match read {
            Ok(size) => already_read += size,
            Err(_) => {
                println!("error in handshake");
            }
        }
        let mut slice = SlicedStream::new(&handshake);
        match read_var_i32(&mut slice) {
            Ok(x) => _length = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return;
                }
                handshake.resize(handshake.len() * 2, 0);
                continue;
            }
        }
        match read_var_i32(&mut slice) {
            Ok(x) => packet_id = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return;
                }
                handshake.resize(handshake.len() * 2, 0);
                continue;
            }
        }

        if packet_id != 0 {
            return;
        }

        match read_var_i32(&mut slice) {
            Ok(x) => protocol_version = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return;
                }
                handshake.resize(handshake.len() * 2, 0);
                continue;
            }
        }
        match read_string(&mut slice) {
            Ok(x) => domain = x,
            Err(e) => {
                if e == ReadingError::Invalid {
                    return;
                }
                handshake.resize(handshake.len() * 2, 0);
                continue;
            }
        }
        break;
    }

    let upstream_address = match find_upstream(&domain, config.clone()) {
        Some(x) => x,
        None => {
            println!("there is no upstream for domain {:#?}", &domain);
            return;
        }
    };

    println!("new connection (protocol_version: {}, domain: {}, upstream: {})", protocol_version, domain, upstream_address);

    let mut upstream = match TcpStream::connect(&upstream_address).await {
        Ok(x) => x,
        Err(e) => {
            println!("failed to connect upstream: {}, {:#?}", &upstream_address, e);
            return;
        }
    };
    match upstream.write_all(&handshake[..already_read]).await {
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
