use std::{borrow::BorrowMut, io::Cursor};

use tokio::io::{AsyncReadExt, AsyncSeekExt, BufStream};

use crate::{packets::HandshakeC2SPacket, serialization::MinecraftStream};

#[tokio::test]
async fn read_handshake() {
    let array: Vec<u8> = vec![
        0x09, // signature: packet length
        0x00, // signature: packet id
        0x10, // protocol version
        0x3, 0x6E, 0x65, 0x74, // domain string
        0xFF, 0xFF, // server port
        0x02, // next state
    ];
    let mut minecraft = make_minecraft_stream(array);
    let handshake = minecraft.read_packet::<HandshakeC2SPacket>().await.unwrap();
    assert_eq!(handshake.protocol_version, 16);
    assert_eq!(handshake.domain, "net");
    assert_eq!(handshake.server_port, 65535);
    assert_eq!(handshake.next_state, 2);
}

#[tokio::test]
async fn read_signature() {
    let array: Vec<u8> = vec![
        0x80, 0x01, // signature: packet length
        0x0B, // signature: packet id
    ];
    let mut minecraft = make_minecraft_stream(array);
    let signature = minecraft.read_signature().await.unwrap();
    assert_eq!(signature.length, 128);
    assert_eq!(signature.packet_id, 11);
}

#[tokio::test]
async fn write_packet() {
    let mut stream = BufStream::new(Cursor::new(vec![0; 1024]));
    {
        let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
        minecraft
            .write_packet(&HandshakeC2SPacket {
                protocol_version: 16,
                domain: "net".to_owned(),
                server_port: 65535,
                next_state: 2,
            })
            .await;
    }
    let mut array = vec![0_u8; 1024];
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    _ = stream.read(&mut array[0..1024]).await.unwrap();
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
    let packet = minecraft.read_packet::<HandshakeC2SPacket>().await.unwrap();
    assert_eq!(packet.protocol_version, 16);
    assert_eq!(packet.domain, "net");
    assert_eq!(packet.server_port, 65535);
    assert_eq!(packet.next_state, 2);
}

#[tokio::test]
async fn write_packet_with_negative_protocol_version() {
    let mut stream = BufStream::new(Cursor::new(vec![0; 1024]));
    {
        let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
        minecraft
            .write_packet(&HandshakeC2SPacket {
                protocol_version: -1,
                domain: "mc.kaydax.xyz".to_owned(),
                server_port: 25565,
                next_state: 1,
            })
            .await;
    }
    let mut array = vec![0_u8; 1024];
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    _ = stream.read(&mut array[0..1024]).await.unwrap();
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
    let packet = minecraft.read_packet::<HandshakeC2SPacket>().await.unwrap();
    assert_eq!(packet.protocol_version, -1);
    assert_eq!(packet.domain, "mc.kaydax.xyz");
    assert_eq!(packet.server_port, 25565);
    assert_eq!(packet.next_state, 1);
}

#[tokio::test]
async fn i32_write_and_read_large_negative() {
    let mut stream = BufStream::new(Cursor::new(vec![0; 1024]));
    {
        let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
        minecraft
            .write_packet(&HandshakeC2SPacket {
                protocol_version: 1,
                domain: "mc.kaydax.xyz".to_owned(),
                server_port: 25565,
                next_state: -1599979007,
            })
            .await;
    }
    let mut array = vec![0_u8; 1024];
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    _ = stream.read(&mut array[0..1024]).await.unwrap();
    stream.seek(std::io::SeekFrom::Start(0)).await.unwrap();
    let mut minecraft = MinecraftStream::new(stream.borrow_mut(), 1024);
    let packet = minecraft.read_packet::<HandshakeC2SPacket>().await.unwrap();
    assert_eq!(packet.protocol_version, 1);
    assert_eq!(packet.domain, "mc.kaydax.xyz");
    assert_eq!(packet.server_port, 25565);
    assert_eq!(packet.next_state, -1599979007);
}

fn make_minecraft_stream(array: Vec<u8>) -> MinecraftStream<BufStream<Cursor<Vec<u8>>>> {
    let stream = BufStream::new(Cursor::new(array.clone()));

    MinecraftStream::new(stream, 1024)
}
