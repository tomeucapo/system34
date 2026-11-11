use std::collections::VecDeque;
use std::io::{self, Write};

/// Screen size of the directly attached 5251 console models.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum DisplayStationModel {
    Model1,
    #[default]
    Model11,
}

impl DisplayStationModel {
    pub const fn rows(self) -> usize {
        match self {
            Self::Model1 => 12,
            Self::Model11 => 24,
        }
    }

    pub const fn columns(self) -> usize {
        80
    }
}

/// Keys reported by the operator-console keyboard.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TerminalKey {
    Character(char),
    Enter,
    Backspace,
    Tab,
    CursorUp,
    CursorDown,
    CursorLeft,
    CursorRight,
    Clear,
    Attention,
}

/// Local model of the System/34's directly attached 5251 operator console.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OperatorConsole {
    model: DisplayStationModel,
    screen: Vec<Vec<char>>,
    cursor_row: usize,
    cursor_column: usize,
    keyboard: VecDeque<TerminalKey>,
}

impl Default for OperatorConsole {
    fn default() -> Self {
        Self::new(DisplayStationModel::default())
    }
}

impl OperatorConsole {
    pub fn new(model: DisplayStationModel) -> Self {
        Self {
            model,
            screen: vec![vec![' '; model.columns()]; model.rows()],
            cursor_row: 0,
            cursor_column: 0,
            keyboard: VecDeque::new(),
        }
    }

    pub fn model(&self) -> DisplayStationModel {
        self.model
    }

    pub fn rows(&self) -> usize {
        self.model.rows()
    }

    pub fn columns(&self) -> usize {
        self.model.columns()
    }

    pub fn screen(&self) -> &[Vec<char>] {
        &self.screen
    }

    pub fn cursor(&self) -> (usize, usize) {
        (self.cursor_row, self.cursor_column)
    }

    pub fn clear_screen(&mut self) {
        for row in &mut self.screen {
            row.fill(' ');
        }
        self.cursor_row = 0;
        self.cursor_column = 0;
    }

    /// Positions the display cursor using zero-based row and column values.
    pub fn set_cursor(&mut self, row: usize, column: usize) -> Result<(), TerminalError> {
        if row >= self.rows() || column >= self.columns() {
            return Err(TerminalError::PositionOutOfBounds {
                row,
                column,
                rows: self.rows(),
                columns: self.columns(),
            });
        }
        self.cursor_row = row;
        self.cursor_column = column;
        Ok(())
    }

    /// Writes printable characters at the current cursor, wrapping at the end
    /// of each row and back to the top after the final screen position.
    pub fn write_text(&mut self, text: &str) {
        for character in text.chars() {
            match character {
                '\n' => {
                    self.cursor_row = (self.cursor_row + 1) % self.rows();
                    self.cursor_column = 0;
                }
                '\r' => self.cursor_column = 0,
                character => self.write_character(character),
            }
        }
    }

    pub fn write_at(&mut self, row: usize, column: usize, text: &str) -> Result<(), TerminalError> {
        self.set_cursor(row, column)?;
        self.write_text(text);
        Ok(())
    }

    /// Adds a key to the console's input queue.
    pub fn queue_key(&mut self, key: TerminalKey) {
        self.keyboard.push_back(key);
    }

    /// Returns the next queued key, if one is available.
    pub fn read_key(&mut self) -> Option<TerminalKey> {
        self.keyboard.pop_front()
    }

    pub fn pending_keys(&self) -> usize {
        self.keyboard.len()
    }

    fn write_character(&mut self, character: char) {
        let character = if (' '..='~').contains(&character) {
            character
        } else {
            '?'
        };
        self.screen[self.cursor_row][self.cursor_column] = character;
        self.cursor_column += 1;
        if self.cursor_column == self.columns() {
            self.cursor_column = 0;
            self.cursor_row = (self.cursor_row + 1) % self.rows();
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalError {
    PositionOutOfBounds {
        row: usize,
        column: usize,
        rows: usize,
        columns: usize,
    },
}

impl std::fmt::Display for TerminalError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::PositionOutOfBounds {
                row,
                column,
                rows,
                columns,
            } => write!(
                f,
                "terminal cursor position ({row}, {column}) is outside {rows}x{columns} screen"
            ),
        }
    }
}

impl std::error::Error for TerminalError {}

/// ANSI renderer and keyboard-byte decoder for a local operator-console view.
///
/// The input decoder accepts printable ASCII, Enter, Backspace, Tab, Ctrl-C,
/// Ctrl-L, and the standard ANSI cursor-key sequences. A host frontend can feed
/// it bytes from its terminal and render the virtual console to that terminal.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct TerminalEmulator {
    input_state: InputState,
    last_was_carriage_return: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum InputState {
    #[default]
    Normal,
    Escape,
    Csi,
}

impl TerminalEmulator {
    /// Redraws the virtual screen and positions the host cursor over the 5251
    /// cursor. ANSI row/column coordinates are one-based.
    pub fn render<W: Write>(&self, console: &OperatorConsole, output: &mut W) -> io::Result<()> {
        output.write_all(b"\x1b[2J\x1b[H")?;
        for (index, row) in console.screen.iter().enumerate() {
            for character in row {
                write!(output, "{character}")?;
            }
            if index + 1 != console.rows() {
                output.write_all(b"\r\n")?;
            }
        }
        let (row, column) = console.cursor();
        write!(output, "\x1b[{};{}H", row + 1, column + 1)?;
        output.flush()
    }

    /// Translates host terminal input bytes into queued 5251 keyboard events.
    /// Incomplete escape sequences are retained for the next call.
    pub fn handle_input(&mut self, bytes: &[u8], console: &mut OperatorConsole) {
        for byte in bytes.iter().copied() {
            match self.input_state {
                InputState::Normal => self.handle_normal_byte(byte, console),
                InputState::Escape => {
                    self.input_state = if byte == b'[' {
                        InputState::Csi
                    } else {
                        InputState::Normal
                    };
                }
                InputState::Csi => {
                    let key = match byte {
                        b'A' => Some(TerminalKey::CursorUp),
                        b'B' => Some(TerminalKey::CursorDown),
                        b'C' => Some(TerminalKey::CursorRight),
                        b'D' => Some(TerminalKey::CursorLeft),
                        _ => None,
                    };
                    if let Some(key) = key {
                        console.queue_key(key);
                    }
                    self.input_state = InputState::Normal;
                }
            }
        }
    }

    fn handle_normal_byte(&mut self, byte: u8, console: &mut OperatorConsole) {
        if byte == 0x1b {
            self.input_state = InputState::Escape;
            self.last_was_carriage_return = false;
            return;
        }

        if byte == b'\n' && self.last_was_carriage_return {
            self.last_was_carriage_return = false;
            return;
        }

        self.last_was_carriage_return = byte == b'\r';
        let key = match byte {
            b'\r' | b'\n' => Some(TerminalKey::Enter),
            0x08 | 0x7f => Some(TerminalKey::Backspace),
            b'\t' => Some(TerminalKey::Tab),
            0x03 => Some(TerminalKey::Attention),
            0x0c => Some(TerminalKey::Clear),
            0x20..=0x7e => Some(TerminalKey::Character(byte as char)),
            _ => None,
        };
        if let Some(key) = key {
            console.queue_key(key);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::{DisplayStationModel, OperatorConsole, TerminalEmulator, TerminalKey};

    #[test]
    fn models_5251_screen_sizes_and_defaults_to_the_console_model() {
        let console = OperatorConsole::default();
        assert_eq!(console.model(), DisplayStationModel::Model11);
        assert_eq!((console.rows(), console.columns()), (24, 80));
        assert_eq!(
            (OperatorConsole::new(DisplayStationModel::Model1).rows(), 80),
            (12, 80)
        );
    }

    #[test]
    fn writes_at_cursor_and_wraps_across_the_screen() {
        let mut console = OperatorConsole::new(DisplayStationModel::Model1);
        console.write_at(0, 78, "ABCD").unwrap();

        assert_eq!(&console.screen()[0][78..80], &['A', 'B']);
        assert_eq!(&console.screen()[1][..2], &['C', 'D']);
        assert_eq!(console.cursor(), (1, 2));
    }

    #[test]
    fn rejects_cursor_positions_outside_the_model_screen() {
        let mut console = OperatorConsole::default();
        assert!(console.set_cursor(24, 0).is_err());
        assert!(console.set_cursor(0, 80).is_err());
    }

    #[test]
    fn decodes_keyboard_bytes_and_split_cursor_sequences() {
        let mut console = OperatorConsole::default();
        let mut emulator = TerminalEmulator::default();
        emulator.handle_input(b"A\r\n\x7f\x03\x1b[", &mut console);
        emulator.handle_input(b"D", &mut console);

        assert_eq!(console.pending_keys(), 5);
        assert_eq!(console.read_key(), Some(TerminalKey::Character('A')));
        assert_eq!(console.read_key(), Some(TerminalKey::Enter));
        assert_eq!(console.read_key(), Some(TerminalKey::Backspace));
        assert_eq!(console.read_key(), Some(TerminalKey::Attention));
        assert_eq!(console.read_key(), Some(TerminalKey::CursorLeft));
    }

    #[test]
    fn renders_ansi_screen_and_cursor() {
        let mut console = OperatorConsole::new(DisplayStationModel::Model1);
        console.write_at(1, 2, "OK").unwrap();
        let mut output = Vec::new();

        TerminalEmulator::default()
            .render(&console, &mut output)
            .unwrap();

        let rendered = String::from_utf8(output).unwrap();
        assert!(rendered.starts_with("\x1b[2J\x1b[H"));
        assert!(rendered.contains("\x1b[2;5H"));
        assert!(rendered.contains("OK"));
    }
}
