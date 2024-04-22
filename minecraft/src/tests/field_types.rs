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

// todo: make more tests for FieldWriter and FieldReader
