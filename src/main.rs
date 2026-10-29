use std::error::Error;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};

use system34::{memory::Memory, OperatorConsole, System34, TerminalEmulator, TerminalKey};

fn main() -> Result<(), Box<dyn Error>> {
    let memory = Memory::new(48);
    let mut machine = System34::new(memory);

    match std::env::args_os().nth(1) {
        Some(argument) if argument == "--terminal-demo" => terminal_demo()?,
        Some(path) => inspect_diskette(&mut machine, path)?,
        None => println!(
            "IBM System/34 initialized with {} bytes of main storage",
            machine.memory().len()
        ),
    }

    Ok(())
}

fn terminal_demo() -> Result<(), Box<dyn Error>> {
    let mut console = OperatorConsole::default();
    let mut emulator = TerminalEmulator::default();
    console.write_text(
        "IBM SYSTEM/34 - 5251 OPERATOR CONSOLE\nModel 11 (24x80)\nType a line and press Enter. Use /quit to exit.\n\n> ",
    );

    let stdin = io::stdin();
    let mut input = String::new();
    let mut stdout = io::stdout().lock();
    loop {
        emulator.render(&console, &mut stdout)?;
        input.clear();
        if stdin.lock().read_line(&mut input)? == 0 {
            break;
        }

        emulator.handle_input(input.as_bytes(), &mut console);
        let mut entered = String::new();
        let mut submitted = false;
        while let Some(key) = console.read_key() {
            match key {
                TerminalKey::Character(character) => entered.push(character),
                TerminalKey::Backspace => {
                    entered.pop();
                }
                TerminalKey::Tab => entered.push_str("    "),
                TerminalKey::Enter => submitted = true,
                TerminalKey::Clear => {
                    console.clear_screen();
                    entered.clear();
                }
                TerminalKey::Attention
                | TerminalKey::CursorUp
                | TerminalKey::CursorDown
                | TerminalKey::CursorLeft
                | TerminalKey::CursorRight => {}
            }
        }

        if submitted && entered.trim() == "/quit" {
            break;
        }
        if submitted {
            console.write_text(&entered);
            console.write_text("\n> ");
        }
    }

    stdout.write_all(b"\x1b[0m\r\n")?;
    Ok(())
}

fn inspect_diskette(machine: &mut System34, path: OsString) -> Result<(), Box<dyn Error>> {
    machine.mount_diskette_imd_file(path)?;
    let image = machine
        .diskette_controller()
        .image()
        .expect("image was just mounted");

    println!("IMD comment: {}", image.comment().trim());
    println!(
        "Mounted diskette image: {} tracks, {} sector IDs",
        image.tracks().len(),
        image.sector_count()
    );
    Ok(())
}
