use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Video};

use super::builder::build_file;
use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};
use super::filesystem::{DirectoryEntry, SharedFilesystem, shared_filesystem};
use super::ui_colors::{SharedUiColors, UiColors, parse_palette_index, shared_ui_colors};
use super::{EDITOR_DISPLAY_HEIGHT, EDITOR_DISPLAY_WIDTH};

const DUMP_OFFSET_COLOR: u8 = 251;
const DUMP_BYTE_COLOR: u8 = 252;
const DUMP_ASCII_COLOR: u8 = 253;
const DUMP_ZERO_COLOR: u8 = 254;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppMode {
    Game,
    #[default]
    Editor,
}

impl AppMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Game => "GAME",
            Self::Editor => "EDITOR",
        }
    }

    pub const fn display_dimensions(self) -> (usize, usize) {
        match self {
            Self::Game => (DISPLAY_WIDTH, DISPLAY_HEIGHT),
            Self::Editor => (EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT),
        }
    }

    const fn text_dimensions(self) -> (usize, usize) {
        let (width, height) = self.display_dimensions();
        (width / GLYPH_WIDTH, height / GLYPH_HEIGHT)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalAction {
    None,
    SwitchMode(AppMode),
    Edit(Option<String>),
}

pub struct Terminal {
    cells: Vec<u8>,
    cell_foregrounds: Vec<Option<u8>>,
    columns: usize,
    rows: usize,
    cursor_x: usize,
    cursor_y: usize,
    input: String,
    mode: AppMode,
    filesystem: SharedFilesystem,
    colors: SharedUiColors,
}

impl Terminal {
    pub fn new(mode: AppMode) -> Self {
        let (columns, rows) = mode.text_dimensions();
        let cell_count = columns * rows;
        let mut terminal = Self {
            cells: vec![b' '; cell_count],
            cell_foregrounds: vec![None; cell_count],
            columns,
            rows,
            cursor_x: 0,
            cursor_y: 0,
            input: String::with_capacity(columns - 3),
            mode,
            filesystem: shared_filesystem(),
            colors: shared_ui_colors(),
        };
        terminal.show_banner();
        terminal
    }

    pub fn input_character(&mut self, character: char) {
        if self.input.len() >= self.columns - 3 || !character.is_ascii() {
            return;
        }
        let byte = character.to_ascii_uppercase() as u8;
        if byte.is_ascii_graphic() || byte == b' ' {
            self.input.push(character);
            self.put_byte(byte);
        }
    }

    pub fn backspace(&mut self) {
        if self.input.pop().is_some() && self.cursor_x > 0 {
            self.cursor_x -= 1;
            self.cells[self.cursor_y * self.columns + self.cursor_x] = b' ';
            self.cell_foregrounds[self.cursor_y * self.columns + self.cursor_x] = None;
        }
    }

    pub fn submit(&mut self) -> TerminalAction {
        self.newline();
        let command = core::mem::take(&mut self.input);
        let action = self.execute(command.trim());
        if matches!(&action, TerminalAction::None | TerminalAction::Edit(_)) {
            self.prompt();
        }
        action
    }

    pub fn switch_mode(&mut self, mode: AppMode) {
        self.mode = mode;
        (self.columns, self.rows) = mode.text_dimensions();
        self.cells = vec![b' '; self.columns * self.rows];
        self.cell_foregrounds = vec![None; self.columns * self.rows];
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.input.clear();
        self.show_banner();
    }

    pub fn filesystem(&self) -> SharedFilesystem {
        self.filesystem.clone()
    }

    pub fn colors(&self) -> SharedUiColors {
        self.colors.clone()
    }

    pub const fn display_dimensions(&self) -> (usize, usize) {
        self.mode.display_dimensions()
    }

    pub fn render(&self, video: &mut Video, cursor_visible: bool) {
        let (display_width, display_height) = self.mode.display_dimensions();
        debug_assert_eq!(video.dimensions(), (display_width, display_height));
        let colors = self.colors.get();
        configure_dump_palette(video);
        let pixels = video.pixels_mut();
        pixels.fill(colors.background);

        for cell_y in 0..self.rows {
            for cell_x in 0..self.columns {
                let character = self.cells[cell_y * self.columns + cell_x] as usize;
                let glyph = CHARACTER_ROM[character.min(CHARACTER_ROM.len() - 1)];
                for (glyph_y, bits) in glyph.into_iter().enumerate() {
                    for glyph_x in 0..GLYPH_WIDTH {
                        if bits & (0x80 >> glyph_x) != 0 {
                            let x = cell_x * GLYPH_WIDTH + glyph_x;
                            let y = cell_y * GLYPH_HEIGHT + glyph_y;
                            pixels[y * display_width + x] = self.cell_foregrounds
                                [cell_y * self.columns + cell_x]
                                .unwrap_or(colors.foreground);
                        }
                    }
                }
            }
        }

        if cursor_visible {
            let start_x = self.cursor_x * GLYPH_WIDTH + 1;
            let y = self.cursor_y * GLYPH_HEIGHT + GLYPH_HEIGHT - 1;
            for x in start_x..(start_x + 5).min(display_width) {
                pixels[y * display_width + x] = colors.foreground;
            }
        }
    }

    fn execute(&mut self, command: &str) -> TerminalAction {
        let (name, arguments) = command.split_once(' ').unwrap_or((command, ""));
        let name = name.to_ascii_uppercase();
        let arguments = arguments.trim();
        match name.as_str() {
            "" => TerminalAction::None,
            "HELP" => {
                self.write("HELP CLS MODE EDITOR GAME EDIT\n");
                self.write("ECHO VERSION COLOR CD MKDIR\n");
                self.write("RMDIR DIR LS ASM BUILD DUMP\n");
                TerminalAction::None
            }
            "CLS" | "CLEAR" => {
                self.clear();
                TerminalAction::None
            }
            "MODE" => {
                self.write("CURRENT MODE: ");
                self.write(self.mode.name());
                self.newline();
                TerminalAction::None
            }
            "EDITOR" => TerminalAction::SwitchMode(AppMode::Editor),
            "GAME" => TerminalAction::SwitchMode(AppMode::Game),
            "ECHO" => {
                self.write(arguments);
                self.newline();
                TerminalAction::None
            }
            "VERSION" => {
                self.write(concat!("FANTICON ", env!("CARGO_PKG_VERSION"), "\n"));
                TerminalAction::None
            }
            "COLOR" => {
                self.set_colors(arguments);
                TerminalAction::None
            }
            "EDIT" => TerminalAction::Edit((!arguments.is_empty()).then(|| arguments.to_owned())),
            "ASM" | "BUILD" => {
                self.build(arguments);
                TerminalAction::None
            }
            "DUMP" => {
                self.dump(arguments);
                TerminalAction::None
            }
            "CD" => {
                if arguments.is_empty() {
                    let directory = self.filesystem.borrow().current_directory();
                    self.write(&directory);
                    self.newline();
                } else {
                    let result = self.filesystem.borrow_mut().change_directory(arguments);
                    if let Err(error) = result {
                        self.write_error(&error);
                    }
                }
                TerminalAction::None
            }
            "MKDIR" => {
                let result = self.filesystem.borrow_mut().create_directory(arguments);
                if let Err(error) = result {
                    self.write_error(&error);
                }
                TerminalAction::None
            }
            "RMDIR" => {
                let result = self.filesystem.borrow_mut().remove_directory(arguments);
                if let Err(error) = result {
                    self.write_error(&error);
                }
                TerminalAction::None
            }
            "DIR" | "LS" => {
                let path = (!arguments.is_empty()).then_some(arguments);
                let result = self.filesystem.borrow().list(path);
                match result {
                    Ok(entries) => self.write_directory(entries),
                    Err(error) => self.write_error(&error),
                }
                TerminalAction::None
            }
            _ => {
                self.write("?UNKNOWN COMMAND: ");
                self.write(&name);
                self.newline();
                TerminalAction::None
            }
        }
    }

    fn show_banner(&mut self) {
        self.write("FANTICON SYSTEM 0.1\n");
        self.write(self.mode.name());
        self.write(" MODE READY.\n");
        if matches!(self.mode, AppMode::Game) {
            self.write("TYPE HELP OR EDITOR.\n\n");
        } else {
            self.write("NATIVE TOOLS READY. TYPE HELP.\n\n");
        }
        self.prompt();
    }

    fn write_directory(&mut self, entries: Vec<DirectoryEntry>) {
        if entries.is_empty() {
            self.write("<EMPTY>\n");
            return;
        }
        for entry in entries {
            if entry.is_directory {
                self.write("<DIR> ");
            } else {
                self.write("      ");
            }
            self.write(&entry.name);
            self.newline();
        }
    }

    fn write_error(&mut self, error: &str) {
        self.write("?");
        self.write(error);
        self.newline();
    }

    fn set_colors(&mut self, arguments: &str) {
        if arguments.is_empty() {
            let colors = self.colors.get();
            self.write(&format!("COLOR BG {} FG {}\n", colors.background, colors.foreground));
            return;
        }
        let values = arguments.split_whitespace().collect::<Vec<_>>();
        if values.len() != 2 {
            self.write_error("USAGE: COLOR BG FG");
            return;
        }
        match (parse_palette_index(values[0]), parse_palette_index(values[1])) {
            (Ok(background), Ok(foreground)) => {
                self.colors.set(UiColors { background, foreground });
            }
            (Err(error), _) | (_, Err(error)) => self.write_error(&error),
        }
    }

    fn build(&mut self, arguments: &str) {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields.len() > 2 {
            self.write_error("USAGE: BUILD SOURCE [OUTPUT]");
            return;
        }
        match build_file(&self.filesystem, fields[0], fields.get(1).copied()) {
            Ok(success) => {
                self.write("BUILT ");
                self.write(&success.output);
                self.newline();
                self.write(&format!("ORIGIN ${:04X}  {} BYTES\n", success.origin, success.size));
            }
            Err(diagnostics) => {
                self.write(&format!("{} ERROR(S)\n", diagnostics.len()));
                for diagnostic in diagnostics {
                    self.write(&format!(
                        "{}:{}:{} {}\n",
                        diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
                    ));
                }
            }
        }
    }

    fn dump(&mut self, arguments: &str) {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields.len() > 3 {
            self.write_error("USAGE: DUMP FILE [OFFSET [LENGTH]]");
            return;
        }
        let offset = match fields.get(1).map_or(Ok(0), |value| parse_dump_number(value)) {
            Ok(value) => value,
            Err(error) => {
                self.write_error(&error);
                return;
            }
        };
        let length = match fields.get(2).map_or(Ok(128), |value| parse_dump_number(value)) {
            Ok(0) => {
                self.write_error("LENGTH MUST BE GREATER THAN ZERO");
                return;
            }
            Ok(value) => value,
            Err(error) => {
                self.write_error(&error);
                return;
            }
        };
        let read_result = self.filesystem.borrow().read_binary(fields[0]);
        let bytes = match read_result {
            Ok(bytes) => bytes,
            Err(error) => {
                self.write_error(&error);
                return;
            }
        };
        if bytes.is_empty() {
            self.write("<EMPTY FILE>\n");
            return;
        }
        if offset >= bytes.len() {
            self.write_error("OFFSET OUT OF RANGE");
            return;
        }

        let end = offset.saturating_add(length).min(bytes.len());
        let last_line_address = offset + (end - offset - 1) / 8 * 8;
        let address_width = hex_digits(last_line_address);
        for (line, chunk) in bytes[offset..end].chunks(8).enumerate() {
            let address = offset + line * 8;
            self.write_colored(&format!("{address:>address_width$X}: "), DUMP_OFFSET_COLOR);
            for index in 0..8 {
                if let Some(byte) = chunk.get(index) {
                    let color = if *byte == 0 { DUMP_ZERO_COLOR } else { DUMP_BYTE_COLOR };
                    self.write_colored(&format!("{byte:02X} "), color);
                } else {
                    self.write("   ");
                }
            }
            for byte in chunk {
                let character =
                    if byte.is_ascii_graphic() || *byte == b' ' { char::from(*byte) } else { '.' };
                let color = if *byte == 0 { DUMP_ZERO_COLOR } else { DUMP_ASCII_COLOR };
                self.put_colored_byte(character.to_ascii_uppercase() as u8, color);
            }
            self.newline();
        }
        self.write(&format!("{} BYTE(S) FROM ${offset:X}\n", end - offset));
    }

    fn prompt(&mut self) {
        self.input.clear();
        let directory = self.filesystem.borrow().current_directory();
        self.write(&directory);
        self.write("> ");
    }

    fn clear(&mut self) {
        self.cells.fill(b' ');
        self.cell_foregrounds.fill(None);
        self.cursor_x = 0;
        self.cursor_y = 0;
        self.input.clear();
    }

    fn write(&mut self, text: &str) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.newline();
            } else {
                self.put_byte(byte.to_ascii_uppercase());
            }
        }
    }

    fn write_colored(&mut self, text: &str, foreground: u8) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.newline();
            } else {
                self.put_colored_byte(byte.to_ascii_uppercase(), foreground);
            }
        }
    }

    fn put_byte(&mut self, byte: u8) {
        self.put_byte_with_color(byte, None);
    }

    fn put_colored_byte(&mut self, byte: u8, foreground: u8) {
        self.put_byte_with_color(byte, Some(foreground));
    }

    fn put_byte_with_color(&mut self, byte: u8, foreground: Option<u8>) {
        if self.cursor_x >= self.columns {
            self.newline();
        }
        let index = self.cursor_y * self.columns + self.cursor_x;
        self.cells[index] = byte;
        self.cell_foregrounds[index] = foreground;
        self.cursor_x += 1;
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= self.rows {
            self.cells.copy_within(self.columns.., 0);
            self.cell_foregrounds.copy_within(self.columns.., 0);
            self.cells[(self.rows - 1) * self.columns..].fill(b' ');
            self.cell_foregrounds[(self.rows - 1) * self.columns..].fill(None);
            self.cursor_y = self.rows - 1;
        }
    }
}

fn parse_dump_number(value: &str) -> Result<usize, String> {
    let (radix, digits) = if let Some(hex) = value.strip_prefix('$') {
        (16, hex)
    } else if let Some(hex) = value.strip_prefix("0x").or_else(|| value.strip_prefix("0X")) {
        (16, hex)
    } else {
        (10, value)
    };
    if digits.is_empty() {
        return Err("INVALID NUMBER".to_owned());
    }
    usize::from_str_radix(digits, radix).map_err(|_| "INVALID NUMBER".to_owned())
}

fn hex_digits(mut value: usize) -> usize {
    let mut digits = 1;
    while value >= 16 {
        value /= 16;
        digits += 1;
    }
    digits
}

fn configure_dump_palette(video: &mut Video) {
    video.set_palette(DUMP_OFFSET_COLOR, [180, 190, 254, 255]);
    video.set_palette(DUMP_BYTE_COLOR, [250, 179, 135, 255]);
    video.set_palette(DUMP_ASCII_COLOR, [166, 227, 161, 255]);
    video.set_palette(DUMP_ZERO_COLOR, [88, 91, 112, 255]);
}

#[cfg(test)]
mod tests {
    use super::*;

    fn type_command(terminal: &mut Terminal, command: &str) -> TerminalAction {
        for character in command.chars() {
            terminal.input_character(character);
        }
        terminal.submit()
    }

    fn screen_text(terminal: &Terminal) -> String {
        terminal
            .cells
            .chunks(terminal.columns)
            .map(|row| String::from_utf8_lossy(row).into_owned())
            .collect::<Vec<_>>()
            .join("\n")
    }

    fn screen_position(terminal: &Terminal, text: &str) -> usize {
        terminal
            .cells
            .chunks(terminal.columns)
            .enumerate()
            .find_map(|(row, cells)| {
                String::from_utf8_lossy(cells)
                    .find(text)
                    .map(|column| row * terminal.columns + column)
            })
            .expect("text should be visible")
    }

    #[test]
    fn editor_command_requests_native_mode_switch() {
        let mut terminal = Terminal::new(AppMode::Game);
        assert_eq!(
            type_command(&mut terminal, "editor"),
            TerminalAction::SwitchMode(AppMode::Editor)
        );
    }

    #[test]
    fn edit_command_launches_text_editor_with_optional_filename() {
        let mut terminal = Terminal::new(AppMode::Editor);
        assert_eq!(
            type_command(&mut terminal, "edit notes.txt"),
            TerminalAction::Edit(Some("notes.txt".to_owned()))
        );
    }

    #[test]
    fn editor_is_the_default_application_mode() {
        assert_eq!(AppMode::default(), AppMode::Editor);
    }

    #[test]
    fn switching_mode_rebuilds_prompt() {
        let mut terminal = Terminal::new(AppMode::Game);
        assert_eq!((terminal.columns, terminal.rows), (40, 25));
        terminal.switch_mode(AppMode::Editor);
        assert_eq!(terminal.mode, AppMode::Editor);
        assert_eq!((terminal.columns, terminal.rows), (80, 50));
        assert_eq!(type_command(&mut terminal, "mode"), TerminalAction::None);
    }

    #[test]
    fn backspace_edits_current_command() {
        let mut terminal = Terminal::new(AppMode::Game);
        for character in "EDITOX".chars() {
            terminal.input_character(character);
        }
        terminal.backspace();
        terminal.input_character('R');
        assert_eq!(terminal.submit(), TerminalAction::SwitchMode(AppMode::Editor));
    }

    #[test]
    fn terminal_renders_character_rom_into_framebuffer() {
        let terminal = Terminal::new(AppMode::Game);
        let mut video = Video::new();
        terminal.render(&mut video, true);
        assert!(video.pixels().contains(&255));
    }

    #[test]
    fn filesystem_commands_update_the_prompt_directory() {
        let mut terminal = Terminal::new(AppMode::Editor);
        assert_eq!(terminal.filesystem.borrow().current_directory(), "/");
        type_command(&mut terminal, "mkdir Project");
        type_command(&mut terminal, "cd PROJECT");
        assert_eq!(terminal.filesystem.borrow().current_directory(), "/project");
        type_command(&mut terminal, "cd /");
        type_command(&mut terminal, "rmdir proJECT");
        assert!(terminal.filesystem.borrow().list(None).unwrap().is_empty());
    }

    #[test]
    fn color_command_sets_shared_background_and_foreground_indexes() {
        let mut terminal = Terminal::new(AppMode::Editor);
        type_command(&mut terminal, "color 3 $e0");
        assert_eq!(terminal.colors.get(), UiColors { background: 3, foreground: 224 });
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        terminal.render(&mut video, false);
        assert!(video.pixels().contains(&3));
        assert!(video.pixels().contains(&224));
    }

    #[test]
    fn build_command_writes_a_binary_file() {
        let mut terminal = Terminal::new(AppMode::Editor);
        terminal
            .filesystem
            .borrow_mut()
            .write_text("demo.asm", " ORG $2000\n LDX #$10\n RTS")
            .unwrap();

        assert_eq!(type_command(&mut terminal, "build demo.asm"), TerminalAction::None);
        assert_eq!(
            terminal.filesystem.borrow().read_binary("demo.bin").unwrap(),
            [0xa2, 0x10, 0x60]
        );
    }

    #[test]
    fn dump_command_shows_hex_ascii_and_supports_ranges() {
        let mut terminal = Terminal::new(AppMode::Editor);
        terminal
            .filesystem
            .borrow_mut()
            .write_binary("bytes.bin", &[0x00, 0x01, b'A', b'B', b' ', b'~', 0x7f, 0xff])
            .unwrap();

        type_command(&mut terminal, "dump bytes.bin");
        assert!(screen_text(&terminal).contains("0: 00 01 41 42 20 7E 7F FF ..AB ~.."));
        let dump_start = screen_position(&terminal, "0: 00 01");
        assert_eq!(terminal.cell_foregrounds[dump_start], Some(DUMP_OFFSET_COLOR));
        assert_eq!(terminal.cell_foregrounds[dump_start + 3], Some(DUMP_ZERO_COLOR));
        assert_eq!(terminal.cell_foregrounds[dump_start + 6], Some(DUMP_BYTE_COLOR));
        assert_eq!(terminal.cell_foregrounds[dump_start + 27], Some(DUMP_ZERO_COLOR));
        assert_eq!(terminal.cell_foregrounds[dump_start + 28], Some(DUMP_ASCII_COLOR));

        type_command(&mut terminal, "dump bytes.bin $2 2");
        let screen = screen_text(&terminal);
        assert!(screen.contains("2: 41 42                   AB"));
        assert!(screen.contains("2 BYTE(S) FROM $2"));
    }

    #[test]
    fn dump_numbers_accept_decimal_and_hex() {
        assert_eq!(parse_dump_number("16").unwrap(), 16);
        assert_eq!(parse_dump_number("$10").unwrap(), 16);
        assert_eq!(parse_dump_number("0x10").unwrap(), 16);
        assert!(parse_dump_number("$NOPE").is_err());
        assert_eq!(hex_digits(0), 1);
        assert_eq!(hex_digits(0xff), 2);
        assert_eq!(hex_digits(0x100), 3);
    }

    #[test]
    fn dump_addresses_are_zero_suppressed_and_right_aligned() {
        let mut terminal = Terminal::new(AppMode::Editor);
        terminal.filesystem.borrow_mut().write_binary("align.bin", &[0; 17]).unwrap();

        type_command(&mut terminal, "dump align.bin");
        let screen = screen_text(&terminal);
        assert!(screen.contains(" 0: 00 00"));
        assert!(screen.contains(" 8: 00 00"));
        assert!(screen.contains("10: 00"));
    }
}
