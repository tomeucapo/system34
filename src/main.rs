use std::error::Error;
use std::ffi::OsString;
use std::io::{self, BufRead, Write};
use std::path::Path;

use system34::{
    memory::Memory, DisketteError, ImdImage, IplError, IplState, OperatorConsole, System34,
    TerminalEmulator, TerminalKey,
};

fn main() -> Result<(), Box<dyn Error>> {
    let memory = Memory::new(48);
    let mut machine = System34::new(memory);

    let mut arguments = std::env::args_os().skip(1);
    match arguments.next() {
        Some(argument) if argument == "--terminal-demo" => terminal_demo()?,
        Some(argument) if argument == "--inspect-csipl" => {
            let paths: Vec<_> = arguments.collect();
            if paths.is_empty() {
                return Err("Usage: system34 --inspect-csipl image.IMD [image2.IMD ...]".into());
            }
            for path in paths {
                inspect_csipl(path)?;
            }
        }
        Some(path) => inspect_diskette(&mut machine, path)?,
        None => println!(
            "IBM System/34 initialized with {} bytes of main storage",
            machine.memory().len()
        ),
    }

    Ok(())
}

fn inspect_csipl(path: OsString) -> Result<(), Box<dyn Error>> {
    let mut machine = System34::new(Memory::new(48));
    machine.mount_diskette_imd_file(&path)?;
    let image = machine
        .diskette_controller()
        .image()
        .expect("image was just mounted");
    let path_display = Path::new(&path).display();

    print!(
        "{}: {} tracks, {} sectors; ",
        path_display,
        image.tracks().len(),
        image.sector_count()
    );
    let large_sectors = four_kib_sectors(image);
    match machine.load_csipl_bootstrap_from_diskette() {
        Ok(IplState::CsiplBootstrapLoaded { bytes, words }) => println!(
            "C0/H0/R1 contains a {bytes}-byte CSIPL-sized record ({words} words), not deleted and no CRC error; contents not validated"
        ),
        Ok(IplState::NotStarted) => println!("CSIPL bootstrap was not loaded"),
        Ok(IplState::CsiplTransferInProgress {
            transferred, total, ..
        }) => println!("CSIPL transfer is in progress ({transferred}/{total} bytes)"),
        Ok(IplState::MainStorageTransferInProgress {
            transferred, total, ..
        }) => println!("main-storage transfer is in progress ({transferred}/{total} bytes)"),
        Ok(IplState::MainStorageProgramLoaded {
            bytes,
            load_address,
            entry_point,
        }) => println!(
            "loaded {bytes} raw program bytes at {load_address:#x}, entry point {entry_point:#x}"
        ),
        Err(IplError::DeletedBootstrapRecord) => {
            println!("C0/H0/R1 exists but is marked deleted; not a valid CSIPL candidate")
        }
        Err(IplError::InvalidBootstrapSize { expected, actual }) => println!(
            "C0/H0/R1 is {actual} bytes (CSIPL requires {expected}); not a valid CSIPL candidate"
        ),
        Err(IplError::Diskette(DisketteError::SectorUnavailable { .. })) => println!(
            "C0/H0/R1 is marked unavailable in this IMD image; its payload was not captured, so the source disk's CSIPL contents are unknown"
        ),
        Err(error) => println!("no usable CSIPL bootstrap at C0/H0/R1: {error}"),
    }
    if large_sectors.is_empty() {
        println!("  No 4-KiB sectors found in the image.");
    } else {
        println!("  4-KiB sectors: {}", large_sectors.join(", "));
    }
    Ok(())
}

fn four_kib_sectors(image: &ImdImage) -> Vec<String> {
    let mut candidates = Vec::new();
    for track in image.tracks() {
        for sector_id in &track.sector_ids {
            let Some(sector) = image.sector(track.cylinder, track.head, *sector_id) else {
                continue;
            };
            if sector.size != 4096 {
                continue;
            }
            let status = match (sector.data.is_some(), sector.deleted, sector.crc_error) {
                (false, _, _) => "unavailable",
                (true, true, true) => "deleted + CRC error",
                (true, true, false) => "deleted",
                (true, false, true) => "CRC error",
                (true, false, false) => "data present",
            };
            let modulation = if track.mode <= 2 { "FM" } else { "MFM" };
            candidates.push(format!(
                "C{:02}/H{}/R{} ({} bytes, {} mode {}, IMD type {}, {status})",
                sector.cylinder,
                sector.head,
                sector.id,
                sector.size,
                modulation,
                track.mode,
                sector.record_type
            ));
        }
    }
    candidates
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
