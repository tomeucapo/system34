

pub struct memory {
    ram: Vec<u8>,   // Main RAM (48K to 256K based on configuration)
}

impl memory {
    pub fn new(size_kb: usize) -> memory {
        memory { ram: vec![0; size_kb * 1024] }
    }

    pub fn read(&self, address: usize) -> u8 {
        self.ram[address]
    }

    pub fn write(&mut self, address: usize, data: u8) {
        self.ram[address] = data;
    }
}
