use super::serialization::{read_string, read_unsigned_short, read_var_i32, ReadingError, SlicedStream};

pub trait PacketSerializer<T> {
    fn from_raw(bytes: &[u8]) -> Result<T, ReadingError>;
}

pub struct HandshakeC2SPacket {
    pub protocol_version: i32,
    pub domain: String,
    pub server_port: u16,
    pub next_state: i32
}

// todo: we need a macro for this impl
impl PacketSerializer<HandshakeC2SPacket> for HandshakeC2SPacket {
    fn from_raw(bytes: &[u8]) -> Result<HandshakeC2SPacket, ReadingError> {
        let mut stream = SlicedStream::new(&bytes);
        let protocol_version = match read_var_i32(&mut stream) {
            Ok(value) => value,
            Err(e) => return Err(e)
        };
        let domain = match read_string(&mut stream) {
            Ok(value) => value,
            Err(e) => return Err(e)
        };
        let server_port = match read_unsigned_short(&mut stream) {
            Ok(value) => value,
            Err(e) => return Err(e)
        };
        let next_state = match read_var_i32(&mut stream) {
            Ok(value) => value,
            Err(e) => return Err(e)
        };
        Ok(HandshakeC2SPacket { protocol_version, domain, server_port, next_state })
    }
}
