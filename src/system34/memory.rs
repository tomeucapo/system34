use std::error::Error;
use std::fmt;

/// Byte-addressable main storage.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Memory {
    ram: Vec<u8>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MemoryError {
    pub address: usize,
    pub memory_size: usize,
}

impl fmt::Display for MemoryError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "memory address {:#x} is outside {} bytes of storage",
            self.address, self.memory_size
        )
    }
}

impl Error for MemoryError {}

impl Memory {
    /// Creates zeroed memory of `size_kib` kibibytes.
    pub fn new(size_kib: usize) -> Self {
        Self {
            ram: vec![0; size_kib * 1024],
        }
    }

    pub fn len(&self) -> usize {
        self.ram.len()
    }

    pub fn is_empty(&self) -> bool {
        self.ram.is_empty()
    }

    pub fn read(&self, address: usize) -> Result<u8, MemoryError> {
        self.ram.get(address).copied().ok_or(MemoryError {
            address,
            memory_size: self.ram.len(),
        })
    }

    pub fn write(&mut self, address: usize, value: u8) -> Result<(), MemoryError> {
        let memory_size = self.ram.len();
        let byte = self.ram.get_mut(address).ok_or(MemoryError {
            address,
            memory_size,
        })?;
        *byte = value;
        Ok(())
    }

    /// Loads a byte image at `address`. The operation is all-or-nothing if it
    /// does not fit in main storage.
    pub fn load(&mut self, address: usize, image: &[u8]) -> Result<(), MemoryError> {
        let end = address
            .checked_add(image.len())
            .filter(|&end| end <= self.len());
        if end.is_none() {
            return Err(MemoryError {
                address: address.saturating_add(image.len()),
                memory_size: self.len(),
            });
        }

        self.ram[address..end.unwrap()].copy_from_slice(image);
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::{Memory, MemoryError};

    #[test]
    fn reads_and_writes_bytes() {
        let mut memory = Memory::new(1);
        memory.write(17, 0x5a).unwrap();

        assert_eq!(memory.read(17), Ok(0x5a));
    }

    #[test]
    fn rejects_access_past_end() {
        let mut memory = Memory::new(1);

        assert_eq!(
            memory.read(1024),
            Err(MemoryError {
                address: 1024,
                memory_size: 1024,
            })
        );
        assert!(memory.write(1024, 0).is_err());
    }

    #[test]
    fn image_load_is_not_partial_when_it_does_not_fit() {
        let mut memory = Memory::new(1);

        assert!(memory.load(1023, &[0xaa, 0xbb]).is_err());
        assert_eq!(memory.read(1023), Ok(0));
    }
}
