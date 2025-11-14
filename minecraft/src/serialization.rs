use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use uuid::Uuid;

use crate::{
    buffer::Buffer,
    packets::{MinecraftPacket, PacketDeserializer, PacketSerializer},
};

const SEGMENT_BITS: i32 = 0x7F;
const CONTINUE_BIT: i32 = 0x80;

#[derive(Debug, PartialEq)]
pub enum ReadingError {
    Insufficient,
    Invalid,
    Closed,
}

impl From<ReadingError> for () {
    fn from(value: ReadingError) -> Self {
        let _ = value;
    }
}

#[derive(Debug, PartialEq)]
pub struct Signature {
    pub length: usize,
    pub packet_id: i32,
}

impl FieldWriter for Signature {
    fn write(&self, stream: &mut Buffer) -> Option<()>
    where
        Self: Sized,
    {
        (self.length as i32).write(stream);
        0.write(stream);
        Some(())
    }
}

pub(crate) trait FieldReader {
    fn read<RW>(stream: &mut MinecraftStream<RW>) -> Result<Self, ReadingError>
    where
        Self: Sized,
        RW: AsyncRead + AsyncWrite + Unpin;
}

pub(crate) trait FieldWriter {
    fn write(&self, stream: &mut Buffer) -> Option<()>
    where
        Self: Sized;
}

impl Buffer {
    pub(crate) fn write_field<T>(&mut self, value: &T) -> Option<()>
    where
        T: FieldWriter,
    {
        T::write(value, self)
    }
}

/// buffer: ▒▒▒▒▒▒▒▓▓▓▓▓▓▓▓▓▓░░░░░░░░░░░░░░░  
/// ▒ - may destroy  
/// ▓ - used memory  
/// ░ - not used yet  
/// `position` points to the start of used memory  
/// `free` points to the start of not used yet  
/// if there is no space left in not used yet memory, used memory will copy to the start of buffer
pub struct MinecraftStream<RW>
where
    RW: AsyncRead + AsyncWrite + Unpin,
{
    buffer: Vec<u8>,
    client: RW,
    free: usize,
    position: usize,
}

impl<RW: AsyncRead + AsyncWrite + Unpin> MinecraftStream<RW> {
    pub fn new(client: RW, init_buffer_size: usize) -> MinecraftStream<RW> {
        MinecraftStream {
            buffer: vec![0; init_buffer_size],
            client,
            position: 0,
            free: 0,
        }
    }

    pub fn get_position(&self) -> usize {
        self.position
    }

    pub fn data_len(&self) -> usize {
        self.free - self.position + 1
    }

    pub fn take_buffer(&mut self) -> Vec<u8> {
        self.buffer[self.position..self.free].to_vec()
    }

    /// Reads signature of packet to the end  
    /// Such as `length` and `id`, doesn't touch the packet data  
    /// https://wiki.vg/Protocol#Packet_format
    pub async fn read_signature(&mut self) -> Result<Signature, ReadingError> {
        let length: i32;
        let packet_id: i32;

        loop {
            match self.read_field::<i32>() {
                Ok(x) => length = x,
                Err(e) => {
                    if e == ReadingError::Invalid {
                        return Err(e);
                    } else if e == ReadingError::Insufficient {
                        match self.fill_buffer_from_source(0).await {
                            Ok(_) => {}
                            Err(_) => return Err(ReadingError::Closed),
                        }
                        continue;
                    }
                    return Err(ReadingError::Closed);
                }
            }
            break;
        }
        loop {
            match self.read_field::<i32>() {
                Ok(x) => packet_id = x,
                Err(e) => {
                    if e == ReadingError::Invalid {
                        return Err(e);
                    } else if e == ReadingError::Insufficient {
                        match self.fill_buffer_from_source(0).await {
                            Ok(_) => {}
                            Err(_) => return Err(ReadingError::Closed),
                        }
                        continue;
                    }
                    return Err(ReadingError::Closed);
                }
            }
            break;
        }

        if length < 0 {
            return Err(ReadingError::Invalid);
        }

        Ok(Signature {
            packet_id,
            length: length as usize,
        })
    }

    /// Reads `data` field of packet to the end  
    /// https://wiki.vg/Protocol#Packet_format
    pub async fn read_data<T>(&mut self, signature: Signature) -> Result<T, ReadingError>
    where
        T: PacketDeserializer,
    {
        if signature.length > self.data_len() {
            match &self.fill_buffer_from_source(signature.length).await {
                Ok(_) => {}
                Err(_) => return Err(ReadingError::Closed),
            };
        }

        T::from_raw(self)
    }

    /// Reads **exactly this packet** to the end ignoring packet id from signature.  
    /// Return error if client close the connection
    pub async fn read_packet<T>(&mut self) -> Result<T, ReadingError>
    where
        T: PacketDeserializer,
    {
        let signature = self.read_signature().await?;
        self.read_data(signature).await
    }

    pub async fn write_packet<T>(&mut self, packet: &T) -> Option<()>
    where
        T: PacketSerializer,
    {
        let packet = MinecraftPacket::make_raw(0, packet)?;
        match self.client.write_all(&packet[0..packet.len()]).await {
            Ok(_) => {}
            Err(_) => return None,
        };
        Some(())
    }

    pub(crate) fn read_field<T>(&mut self) -> Result<T, ReadingError>
    where
        T: FieldReader,
    {
        T::read(self)
    }

    fn remain_len(&self) -> usize {
        self.buffer.len() - self.position
    }

    fn copy_buffer_to_start(&mut self) {
        let data_len = self.free - self.position;
        self.buffer.copy_within(self.position..self.free, 0);
        self.free = data_len;
        self.position = 0;
    }

    fn expand_buffer(&mut self) {
        todo!()
    }

    async fn fill_buffer_from_source(&mut self, required: usize) -> Result<(), ()> {
        if self.free >= self.buffer.len() {
            if self.position != 0 {
                self.copy_buffer_to_start();
            } else {
                self.expand_buffer();
            }
        }
        loop {
            let pos = &self.free;
            let read = self.client.read(&mut self.buffer[*pos..]).await;
            match read {
                Ok(size) => {
                    if size == 0 {
                        return Err(());
                    }
                    self.free += size;
                }
                Err(_) => {
                    return Err(());
                }
            }

            if self.data_len() < required {
                continue;
            }
            break;
        }
        Ok(())
    }
}

impl FieldReader for i32 {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError> {
        let mut value = 0;
        let mut current_position = 0;
        let mut current_byte: i32;
        let mut index = stream.position;

        loop {
            if index >= stream.free {
                return Err(ReadingError::Insufficient);
            }
            current_byte = stream.buffer[index] as i32;
            index += 1;
            value |= (current_byte & SEGMENT_BITS) << current_position;

            if (current_byte & CONTINUE_BIT) == 0 {
                break;
            }
            current_position += 7;
            if current_position >= 32 {
                return Err(ReadingError::Invalid);
            }
        }

        stream.position = index;
        Ok(value)
    }
}

impl FieldWriter for i32 {
    fn write(&self, stream: &mut Buffer) -> Option<()> {
        let mut value = *self as u32;
        loop {
            if (value & !(SEGMENT_BITS as u32)) == 0 {
                stream.write_byte(value as u8);
                return Some(());
            }
            stream.write_byte(((value & SEGMENT_BITS as u32) | CONTINUE_BIT as u32) as u8);
            value >>= 7;
        }
    }
}

impl FieldReader for String {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError> {
        // todo: there is a bug - read_field changes position of the stream, but below can happen reading error if packet doesn't fully read
        let length = stream.read_field::<i32>()? as usize;

        if length > stream.remain_len() {
            return Err(ReadingError::Insufficient);
        }
        let mut vec: Vec<u8> = vec![0; length];
        vec.copy_from_slice(&stream.buffer[stream.position..stream.position + length]);
        stream.position += length;
        Ok(String::from_utf8(vec).unwrap())
    }
}

impl FieldWriter for String {
    fn write(&self, stream: &mut Buffer) -> Option<()> {
        let length = self.len() as i32;
        length.write(stream);
        for byte in self.as_bytes() {
            stream.write_byte(*byte)
        }
        Some(())
    }
}

impl FieldReader for u16 {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError> {
        if stream.data_len() < 2 {
            return Err(ReadingError::Insufficient);
        }
        let b1 = match stream.read_field::<u8>() {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        let b2 = match stream.read_field::<u8>() {
            Ok(x) => x,
            Err(e) => return Err(e),
        };
        Ok(b2 as u16 | (b1 as u16) << 8)
    }
}

impl FieldWriter for u16 {
    fn write(&self, stream: &mut Buffer) -> Option<()> {
        stream.write_byte((self >> 8 & 0xFF) as u8);
        stream.write_byte(((self) & 0xFF) as u8);

        Some(())
    }
}

impl FieldReader for bool {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError> {
        let byte = stream.read_field::<u8>()?;
        Ok(byte != 0)
    }
}

impl FieldWriter for bool {
    fn write(&self, stream: &mut Buffer) -> Option<()> {
        stream.write_byte(if *self { 1 } else { 0 });
        Some(())
    }
}

impl FieldReader for u8 {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError>
    where
        Self: Sized,
    {
        if stream.position >= stream.free {
            return Err(ReadingError::Insufficient);
        }
        let position = stream.position;
        stream.position = position + 1;
        Ok(stream.buffer[position])
    }
}

impl FieldReader for Uuid {
    fn read<RW: AsyncRead + AsyncWrite + Unpin>(
        stream: &mut MinecraftStream<RW>,
    ) -> Result<Self, ReadingError> {
        if stream.data_len() < 16 {
            return Err(ReadingError::Insufficient);
        }

        match Uuid::from_slice(&stream.buffer[stream.position..stream.position + 16]) {
            Ok(v) => {
                stream.position += 16;
                Ok(v)
            }
            Err(_) => Err(ReadingError::Insufficient),
        }
    }
}

impl FieldWriter for Uuid {
    fn write(&self, stream: &mut Buffer) -> Option<()> {
        for &byte in self.as_bytes() {
            stream.write_byte(byte);
        }
        Some(())
    }
}

pub fn truncate_to_zero(value: &str) -> &str {
    let index = &value.find('\0');
    match index {
        Some(v) => &value[0..*v],
        None => value,
    }
}
