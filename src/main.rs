use system34::{memory::Memory, System34};

fn main() {
    let memory = Memory::new(48);
    let machine = System34::new(memory);

    println!(
        "IBM System/34 initialized with {} bytes of main storage",
        machine.memory().len()
    );
}
