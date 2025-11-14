use crate::{buffer::Buffer, serialization::FieldWriter};

#[test]
fn bool_write_true() {
    let mut buffer = Buffer::new(1024);
    true.write(&mut buffer);
    assert_eq!(buffer.take()[0], 1);
}

#[test]
fn bool_write_false() {
    let mut buffer = Buffer::new(1024);
    false.write(&mut buffer);
    assert_eq!(buffer.take()[0], 0);
}

#[test]
fn i32_write_positive() {
    let mut buffer = Buffer::new(1024);
    300.write(&mut buffer);
    let bytes = buffer.take();
    // 300 in VarInt: 0xAC 0x02
    assert_eq!(bytes, &[0xAC, 0x02]);
}

#[test]
fn i32_write_negative_one() {
    let mut buffer = Buffer::new(1024);
    (-1_i32).write(&mut buffer);
    let bytes = buffer.take();
    // -1 in VarInt should be encoded properly (5 bytes for negative numbers)
    // -1 = 0xFFFFFFFF in binary, encoded as VarInt
    assert_eq!(bytes.len(), 5);
    assert_eq!(bytes, &[0xFF, 0xFF, 0xFF, 0xFF, 0x0F]);
}

#[test]
fn i32_write_zero() {
    let mut buffer = Buffer::new(1024);
    0.write(&mut buffer);
    let bytes = buffer.take();
    assert_eq!(bytes, &[0x00]);
}

// todo: make more tests for FieldWriter and FieldReader
