use minecraft_macros::{PacketDeserializer, PacketSerializer};
use tokio::io::{AsyncRead, AsyncWrite};
use uuid::Uuid;

use crate::serialization::{Buffer, FieldWriter};

use super::serialization::{ReadingError, MinecraftStream};

pub trait PacketDeserializer {
    fn from_raw<RW>(stream: &mut MinecraftStream<RW>) -> Result<Self, ReadingError>
    where
        Self : Sized,
        RW : AsyncRead + AsyncWrite + Unpin;
}

pub trait PacketSerializer {
    fn to_raw(&self, stream: &mut Buffer) -> Option<()> where Self : Sized;
}

pub struct MinecraftPacket {
}

impl MinecraftPacket {
    pub fn make_raw<T>(id: i32, packet: &T) -> Option<Vec<u8>> where T: PacketSerializer {
        let mut data_buffer = Buffer::new(1024);
        T::to_raw(packet, &mut data_buffer)?;
        let mut packet_id_buffer = Buffer::new(5);
        id.write(&mut packet_id_buffer);
        let mut packet_length_buffer = Buffer::new(5);

        let d2 = packet_id_buffer.take();
        let d3 = data_buffer.take();
        (d2.len() as i32 + d3.len() as i32).write(&mut packet_length_buffer);

        let d1 = packet_length_buffer.take();
        let array = [d1, d2, d3].concat();
        Some(array)
    }
}

#[derive(PacketDeserializer, PacketSerializer)]
pub struct HandshakeC2SPacket {
    pub protocol_version: i32,
    pub domain: String,
    pub server_port: u16,
    pub next_state: i32
}

#[derive(PacketDeserializer)]
pub struct LoginC2SPacket {
    pub name: String,
    pub has_uuid: bool,
    pub player_uuid: Uuid
}
