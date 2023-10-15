mod system34;

fn main() {
    let mem = system34::memory::memory::new(32);
    let mut system34 = system34::system34::new(mem);

    system34.run();

    println!("Hello, System/34!");
}
