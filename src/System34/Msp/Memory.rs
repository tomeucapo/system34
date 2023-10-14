pub struct Memory {
    ram: Vec<u8>,   // Main RAM (48K to 256K based on configuration)
}

impl Memory {
    fn new(size_kb: usize) -> Memory {
        Memory { ram: vec![0; size_kb * 1024] }
    }

    fn read(&self, address: usize) -> u8 {
        self.ram[address]
    }

    fn write(&mut self, address: usize, data: u8) {
        self.ram[address] = data;
    }
}
