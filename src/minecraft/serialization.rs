use std::fmt::Debug;

const SEGMENT_BITS: i32 = 0x7F;
const CONTINUE_BIT: i32 = 0x80;

#[derive(Debug)]
#[derive(PartialEq)]
pub enum ReadingError {
    Insufficient,
    Invalid
}

pub struct SlicedStream<'a> {
    stream: &'a[u8],
    position: usize
}

impl<'a> SlicedStream<'a> {
    pub fn new(stream: &[u8]) -> SlicedStream {
        SlicedStream {
            stream,
            position: 0
        }
    }

    pub fn remain_len(&self) -> usize {
        self.stream.len() - self.position
    }

    pub fn take(&mut self, length: usize) -> &[u8] {
        let position = self.position;
        self.position = position + length;
        return &self.stream[position..position + length];
    }
}

pub fn read_var_i32(slice: &mut SlicedStream) -> Result<i32, ReadingError> {
    let mut value = 0;
    let mut position = 0;
    let mut current_byte: i32;

    loop {
        current_byte = read_byte(slice)? as i32;
        value |= (current_byte & SEGMENT_BITS) << position;

        if (current_byte & CONTINUE_BIT) == 0 {
            break;
        }

        position += 7;

        if position >= 32 {
            return Err(ReadingError::Invalid);
        }
    }

    Ok(value)
}

pub fn read_string(slice: &mut SlicedStream) -> Result<String, ReadingError> {
    let length = read_var_i32(slice)? as usize;

    if length > slice.remain_len() {
        return Err(ReadingError::Insufficient);
    }
    let mut vec: Vec<u8> = Vec::new();
    vec.resize(length, 0);
    vec.copy_from_slice(&slice.take(length));
    return Ok(String::from_utf8(vec).unwrap());
}

fn read_byte(slice: &mut SlicedStream) -> Result<u8, ReadingError> {
    if slice.position >= slice.stream.len() {
        return Err(ReadingError::Insufficient);
    }
    let position = slice.position;
    slice.position = position + 1;
    return Ok(slice.stream[position]);
}

pub fn truncate_to_zero(value: &str) -> &str {
    let index = &value.find('\0');
    match index {
        Some(v) => &value[0..*v],
        None => &value
    }
}
