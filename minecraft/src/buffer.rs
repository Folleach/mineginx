pub struct Buffer {
    array: Vec<u8>,
    position: usize
}

impl Buffer {
    pub fn new(init_size: usize) -> Buffer {
        Buffer {
            array: vec![0_u8; init_size],
            position: 0
        }
    }

    pub fn write_byte(&mut self, value: u8) {
        if self.array.len() == self.position {
            self.expand();
        }
        self.array[self.position] = value;
        self.position += 1;
    }

    pub fn take(&self) -> &[u8] {
        &self.array[0..self.position]
    }

    pub fn reset(&mut self) {
        self.position = 0;
    }

    fn expand(&mut self) {
        let mut new_vec = vec![0_u8; self.array.len() * 2];
        new_vec[0..self.array.len()].copy_from_slice(&self.array);
        self.array = new_vec;
    }
}
