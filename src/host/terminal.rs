use fanticon::project::{ProjectManifest, generate_cartridge_id};
use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Video};

use super::builder::{
    GameLaunch, build_and_load_project, build_file, build_project, load_cartridge,
};
use super::character_rom::{
    CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH, configure_text_gradient, gradient_color,
};
use super::filesystem::{DirectoryEntry, SharedFilesystem, shared_filesystem};
use super::help::{HelpCategory, format_guide_body, shared_help_index};
use super::nsf_player::{MusicCommand, import_nsf_to_mus};
use super::ui_colors::{SharedUiColors, UiColors, parse_palette_index, shared_ui_colors};
use super::{EDITOR_DISPLAY_HEIGHT, EDITOR_DISPLAY_WIDTH};

const DUMP_OFFSET_COLOR: u8 = 251;
const DUMP_BYTE_COLOR: u8 = 252;
const DUMP_ASCII_COLOR: u8 = 253;
const DUMP_ZERO_COLOR: u8 = 254;
const DEFAULT_PROJECT_SOURCE: &str = "         FIXED\n         ORG   $C100\nRESET    SEI\nLOOP     JMP   LOOP\nNMI      RTI\nIRQ      RTI\n         ORG   $FFFA\n         DA    NMI,RESET,IRQ\n";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub enum AppMode {
    Game,
    #[default]
    Editor,
}

impl AppMode {
    pub const fn name(self) -> &'static str {
        match self {
            Self::Game => "Game",
            Self::Editor => "Editor",
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
    Run(GameLaunch),
    Music(MusicCommand),
    /// Typed at the console prompt: shuts down the whole virtual console, not
    /// just the current mode or document. Distinct from the editor's own
    /// FILE > EXIT, which only leaves the editor for the console.
    Exit,
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
        let byte = character as u8;
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

    /// Resume the command line after an Editor-origin game exits. `RUN` does
    /// not print its next prompt when it launches, so returning must do so.
    pub fn resume_after_game(&mut self) {
        self.prompt();
    }

    pub fn finish_music_command(&mut self, result: Result<String, String>) {
        match result {
            Ok(message) => {
                self.write(&message);
                self.newline();
            }
            Err(error) => self.write_error(&error),
        }
        self.prompt();
    }

    pub fn render(&self, video: &mut Video, cursor_visible: bool) {
        let (display_width, display_height) = self.mode.display_dimensions();
        debug_assert_eq!(video.dimensions(), (display_width, display_height));
        let colors = self.colors.get();
        configure_dump_palette(video);
        let gradient = configure_text_gradient(
            video,
            self.cell_foregrounds
                .iter()
                .map(|color| color.unwrap_or(colors.foreground))
                .chain([colors.foreground, colors.background]),
        );
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
                            let foreground = self.cell_foregrounds[cell_y * self.columns + cell_x]
                                .unwrap_or(colors.foreground);
                            pixels[y * display_width + x] =
                                gradient_color(&gradient, foreground, glyph_y);
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
            "HELP" if arguments.is_empty() => {
                self.write("help cls mode editor game edit new exit\n");
                self.write("echo version color cd mkdir\n");
                self.write("rmdir rm del dir ls asm build run dump\n");
                self.write("playnsf nsfplay nsfpause nsfstop\n");
                self.write("nsfnext nsfprev nsfloop nsfinfo\n");
                self.write("nsf2mus input.nsf output.mus [track]\n");
                self.write("Help topic for one opcode, directive,\n");
                self.write("command, or guide section by name.\n");
                TerminalAction::None
            }
            "HELP" => {
                self.show_help_topic(arguments);
                TerminalAction::None
            }
            "CLS" | "CLEAR" => {
                self.clear();
                TerminalAction::None
            }
            "MODE" => {
                self.write("Current mode: ");
                self.write(self.mode.name());
                self.newline();
                TerminalAction::None
            }
            "EDITOR" => TerminalAction::SwitchMode(AppMode::Editor),
            "GAME" => TerminalAction::SwitchMode(AppMode::Game),
            "EXIT" | "QUIT" => TerminalAction::Exit,
            "ECHO" => {
                self.write(arguments);
                self.newline();
                TerminalAction::None
            }
            "VERSION" => {
                self.write(concat!("Fanticon ", env!("CARGO_PKG_VERSION"), "\n"));
                TerminalAction::None
            }
            "COLOR" => {
                self.set_colors(arguments);
                TerminalAction::None
            }
            "EDIT" => TerminalAction::Edit((!arguments.is_empty()).then(|| arguments.to_owned())),
            "NEW" => {
                self.new_project(arguments);
                TerminalAction::None
            }
            "ASM" => self.build_raw(arguments),
            "BUILD" => self.build(arguments),
            "RUN" => self.run(arguments),
            "PLAYNSF" => self.play_nsf(arguments),
            "NSF2MUS" => {
                self.import_nsf(arguments);
                TerminalAction::None
            }
            "NSFPLAY" => TerminalAction::Music(MusicCommand::Play),
            "NSFPAUSE" => TerminalAction::Music(MusicCommand::Pause),
            "NSFSTOP" => TerminalAction::Music(MusicCommand::Stop),
            "NSFNEXT" => TerminalAction::Music(MusicCommand::Next),
            "NSFPREV" => TerminalAction::Music(MusicCommand::Previous),
            "NSFLOOP" => TerminalAction::Music(MusicCommand::ToggleLoop),
            "NSFINFO" => TerminalAction::Music(MusicCommand::Status),
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
            "RM" | "DEL" => {
                if arguments.is_empty() || arguments.split_ascii_whitespace().count() != 1 {
                    self.write_error("Usage: rm file");
                } else {
                    let result = self.filesystem.borrow_mut().remove_file(arguments);
                    if let Err(error) = result {
                        self.write_error(&error);
                    }
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
                self.write("?Unknown command: ");
                self.write(&name);
                self.newline();
                TerminalAction::None
            }
        }
    }

    fn show_banner(&mut self) {
        self.write(concat!("Fanticon System ", env!("CARGO_PKG_VERSION"), "\n"));
        self.write(self.mode.name());
        self.write(" mode ready.\n");
        if matches!(self.mode, AppMode::Game) {
            self.write("Type help or editor.\n\n");
        } else {
            self.write("Native tools ready. Type help.\n\n");
        }
        self.prompt();
    }

    fn write_directory(&mut self, entries: Vec<DirectoryEntry>) {
        if entries.is_empty() {
            self.write("<Empty>\n");
            return;
        }
        for entry in entries {
            if entry.is_directory {
                self.write("<Dir> ");
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

    /// The console's exact/alias lookup for one help topic. There is no
    /// overlay system here, so unlike the editor's F1 finder this always
    /// prints a single card as plain scrolling text.
    fn show_help_topic(&mut self, topic: &str) {
        match shared_help_index().lookup(topic) {
            Some(entry) => {
                self.write(&format!("{} - {}\n", entry.key, entry.summary));
                if matches!(entry.category, HelpCategory::Guide) {
                    let width = self.columns.saturating_sub(2).max(20);
                    for line in format_guide_body(&entry.body, width) {
                        self.write(&line);
                        self.newline();
                    }
                } else {
                    for line in &entry.body {
                        self.write(line);
                        self.newline();
                    }
                }
                if let Some(source) = &entry.source {
                    self.write(&format!("(From {source})\n"));
                }
            }
            None => self.write(&format!("?No help for {topic}\n")),
        }
    }

    fn set_colors(&mut self, arguments: &str) {
        if arguments.is_empty() {
            let colors = self.colors.get();
            self.write(&format!("Color bg {} fg {}\n", colors.background, colors.foreground));
            return;
        }
        let values = arguments.split_whitespace().collect::<Vec<_>>();
        if values.len() != 2 {
            self.write_error("Usage: color bg fg");
            return;
        }
        match (parse_palette_index(values[0]), parse_palette_index(values[1])) {
            (Ok(background), Ok(foreground)) => {
                self.colors.set(UiColors { background, foreground });
            }
            (Err(error), _) | (_, Err(error)) => self.write_error(&error),
        }
    }

    fn build_raw(&mut self, arguments: &str) -> TerminalAction {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields.len() > 2 {
            self.write_error("Usage: asm source [output]");
            return TerminalAction::None;
        }
        match build_file(&self.filesystem, fields[0], fields.get(1).copied()) {
            Ok(success) => {
                self.write("Built ");
                self.write(&success.output);
                self.newline();
                self.write(&format!("Origin ${:04X}  {} bytes\n", success.origin, success.size));
            }
            Err(diagnostics) => {
                self.write(&format!("{} error(s)\n", diagnostics.len()));
                for diagnostic in diagnostics {
                    self.write(&format!(
                        "{}:{}:{} {}\n",
                        diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
                    ));
                }
            }
        }
        TerminalAction::None
    }

    fn build(&mut self, arguments: &str) -> TerminalAction {
        if !arguments.is_empty() {
            return self.build_raw(arguments);
        }
        match build_project(&self.filesystem) {
            Ok(success) => {
                self.write(&format!("Built {}\n", success.output));
                self.write(&format!(
                    "{}  {} bank(s)  {} bytes\n",
                    success.title, success.banks, success.size
                ));
            }
            Err(diagnostics) => self.write_diagnostics(diagnostics),
        }
        TerminalAction::None
    }

    fn run(&mut self, arguments: &str) -> TerminalAction {
        let result = if arguments.is_empty() {
            build_and_load_project(&self.filesystem)
        } else if arguments.split_whitespace().count() == 1 {
            load_cartridge(&self.filesystem, arguments)
        } else {
            self.write_error("Usage: run [cartridge.fcn]");
            return TerminalAction::None;
        };
        match result {
            Ok(launch) => TerminalAction::Run(launch),
            Err(diagnostics) => {
                self.write_diagnostics(diagnostics);
                TerminalAction::None
            }
        }
    }

    fn play_nsf(&mut self, arguments: &str) -> TerminalAction {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields.len() > 2 {
            self.write_error("Usage: playnsf file.nsf [track]");
            return TerminalAction::None;
        }
        if !fields[0].to_ascii_lowercase().ends_with(".nsf") {
            self.write_error("Playnsf requires an nsf file");
            return TerminalAction::None;
        }
        let track = match fields.get(1) {
            Some(value) => match value.parse::<u8>() {
                Ok(track) if track != 0 => track,
                _ => {
                    self.write_error("Track must be 1-255");
                    return TerminalAction::None;
                }
            },
            None => 0,
        };
        let read = self.filesystem.borrow().read_binary(fields[0]);
        match read {
            Ok(bytes) => TerminalAction::Music(MusicCommand::Load {
                filename: fields[0].to_ascii_lowercase(),
                bytes,
                track,
            }),
            Err(error) => {
                self.write_error(&error);
                TerminalAction::None
            }
        }
    }

    fn import_nsf(&mut self, arguments: &str) {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if !(2..=3).contains(&fields.len())
            || !fields[0].to_ascii_lowercase().ends_with(".nsf")
            || !fields[1].to_ascii_lowercase().ends_with(".mus")
        {
            self.write_error("Usage: nsf2mus input.nsf output.mus [track]");
            return;
        }
        let track = match fields.get(2) {
            Some(value) => match value.parse::<u8>() {
                Ok(value) if value != 0 => value,
                _ => {
                    self.write_error("Track must be 1-255");
                    return;
                }
            },
            None => 0,
        };
        let bytes = self.filesystem.borrow().read_binary(fields[0]);
        let result = bytes.and_then(|bytes| import_nsf_to_mus(&bytes, track, fields[1])).and_then(
            |import| {
                self.filesystem.borrow_mut().write_text(fields[1], &import.source)?;
                Ok(import)
            },
        );
        match result {
            Ok(import) => {
                self.write(&format!(
                    "Wrote {} ({} video frames)\n",
                    fields[1], import.captured_frames
                ));
                if import.dpcm_omitted {
                    self.write("Warning: nsf dpcm channel was omitted\n");
                }
            }
            Err(error) => self.write_error(&error),
        }
    }

    fn write_diagnostics(&mut self, diagnostics: Vec<fanticon::assembler::Diagnostic>) {
        self.write(&format!("{} error(s)\n", diagnostics.len()));
        for diagnostic in diagnostics {
            self.write(&format!(
                "{}:{}:{} {}\n",
                diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
            ));
        }
    }

    fn new_project(&mut self, name: &str) {
        if name.is_empty() || name.split_whitespace().count() != 1 {
            self.write_error("Usage: new project");
            return;
        }
        let id = match generate_cartridge_id() {
            Ok(id) => id,
            Err(error) => {
                self.write_error(&error);
                return;
            }
        };
        let output_stem = name.split('.').next().unwrap_or(name);
        let output = format!("{}.fcn", &output_stem[..output_stem.len().min(8)]);
        let title = name.to_owned();
        let manifest = match ProjectManifest::template(&title, "main.asm", &output, 0, id) {
            Ok(manifest) => manifest,
            Err(error) => {
                self.write_error(&error);
                return;
            }
        };
        let result = (|| {
            self.filesystem.borrow_mut().create_directory(name)?;
            self.filesystem.borrow_mut().change_directory(name)?;
            self.filesystem.borrow_mut().write_text("fanticon.cfg", &manifest)?;
            self.filesystem.borrow_mut().write_text("main.asm", DEFAULT_PROJECT_SOURCE)?;
            Ok::<(), String>(())
        })();
        match result {
            Ok(()) => self.write(&format!("Created /{}\n", name.to_ascii_lowercase())),
            Err(error) => self.write_error(&error),
        }
    }

    fn dump(&mut self, arguments: &str) {
        let fields = arguments.split_whitespace().collect::<Vec<_>>();
        if fields.is_empty() || fields.len() > 3 {
            self.write_error("Usage: dump file [offset [length]]");
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
                self.write_error("Length must be greater than zero");
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
            self.write("<Empty file>\n");
            return;
        }
        if offset >= bytes.len() {
            self.write_error("Offset out of range");
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
                self.put_colored_byte(character as u8, color);
            }
            self.newline();
        }
        self.write(&format!("{} byte(s) from ${offset:X}\n", end - offset));
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
                self.put_byte(byte);
            }
        }
    }

    fn write_colored(&mut self, text: &str, foreground: u8) {
        for byte in text.bytes() {
            if byte == b'\n' {
                self.newline();
            } else {
                self.put_colored_byte(byte, foreground);
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
        return Err("Invalid number".to_owned());
    }
    usize::from_str_radix(digits, radix).map_err(|_| "Invalid number".to_owned())
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
    fn exit_and_quit_commands_request_shutdown() {
        let mut terminal = Terminal::new(AppMode::Game);
        assert_eq!(type_command(&mut terminal, "exit"), TerminalAction::Exit);
        let mut terminal = Terminal::new(AppMode::Editor);
        assert_eq!(type_command(&mut terminal, "quit"), TerminalAction::Exit);
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
    fn command_names_stay_case_insensitive_but_typed_text_keeps_its_case() {
        let mut terminal = Terminal::new(AppMode::Editor);
        // The command keyword itself is still case-insensitive...
        assert_eq!(type_command(&mut terminal, "echo Hello World"), TerminalAction::None);
        // ...but the argument it echoes back is not silently forced to caps.
        assert!(screen_text(&terminal).contains("Hello World"));
        assert!(!screen_text(&terminal).contains("HELLO WORLD"));
    }

    #[test]
    fn playnsf_loads_a_track_and_radio_commands_are_forwarded() {
        let mut terminal = Terminal::new(AppMode::Editor);
        let mut bytes = vec![0; 0x81];
        bytes[..5].copy_from_slice(b"NESM\x1A");
        bytes[5] = 1;
        bytes[6] = 3;
        bytes[7] = 1;
        bytes[8..14].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x00, 0x80]);
        bytes[0x80] = 0x60;
        terminal.filesystem.borrow_mut().write_binary("radio.nsf", &bytes).unwrap();

        assert!(matches!(
            type_command(&mut terminal, "playnsf radio.nsf 2"),
            TerminalAction::Music(MusicCommand::Load { filename, track: 2, .. })
                if filename == "radio.nsf"
        ));
        terminal.finish_music_command(Ok("PLAYING RADIO.NSF".to_owned()));
        assert!(screen_text(&terminal).contains("PLAYING RADIO.NSF"));
        assert_eq!(
            type_command(&mut terminal, "nsfpause"),
            TerminalAction::Music(MusicCommand::Pause)
        );
        assert_eq!(
            type_command(&mut terminal, "nsfnext"),
            TerminalAction::Music(MusicCommand::Next)
        );
        assert_eq!(
            type_command(&mut terminal, "nsfprev"),
            TerminalAction::Music(MusicCommand::Previous)
        );
        assert_eq!(
            type_command(&mut terminal, "nsfloop"),
            TerminalAction::Music(MusicCommand::ToggleLoop)
        );
        assert_eq!(
            type_command(&mut terminal, "nsfstop"),
            TerminalAction::Music(MusicCommand::Stop)
        );
    }

    #[test]
    fn nsf2mus_command_writes_an_editable_tracker_resource() {
        let mut program = vec![0xea; 0x40];
        program[..19].copy_from_slice(&[
            0xa9, 0x01, 0x8d, 0x15, 0x40, 0xa9, 0x9f, 0x8d, 0x00, 0x40, 0xa9, 0x20, 0x8d, 0x02,
            0x40, 0x8d, 0x03, 0x40, 0x60,
        ]);
        program[0x20] = 0x60;
        let mut nsf = vec![0; 0x80];
        nsf[..5].copy_from_slice(b"NESM\x1A");
        nsf[5] = 1;
        nsf[6] = 1;
        nsf[7] = 1;
        nsf[8..14].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x20, 0x80]);
        nsf[0x6e..0x70].copy_from_slice(&16_639_u16.to_le_bytes());
        nsf.extend_from_slice(&program);

        let mut terminal = Terminal::new(AppMode::Editor);
        terminal.filesystem.borrow_mut().write_binary("song.nsf", &nsf).unwrap();
        assert_eq!(
            type_command(&mut terminal, "nsf2mus song.nsf tune.mus 1"),
            TerminalAction::None
        );
        let source = terminal.filesystem.borrow().read_text("tune.mus").unwrap();
        assert!(source.contains(";@FANTICON-MUSIC 2"));
        assert!(screen_text(&terminal).contains("Wrote tune.mus"));
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
    fn returning_from_game_prints_a_fresh_prompt() {
        let mut terminal = Terminal::new(AppMode::Editor);
        terminal.input_character('R');
        terminal.input_character('U');
        terminal.input_character('N');
        terminal.newline();
        let cursor_before = (terminal.cursor_x, terminal.cursor_y);
        terminal.resume_after_game();
        assert_eq!(terminal.cursor_y, cursor_before.1);
        assert!(terminal.cursor_x > cursor_before.0);
        let row = &terminal.cells
            [terminal.cursor_y * terminal.columns..(terminal.cursor_y + 1) * terminal.columns];
        assert!(row.starts_with(b"/> "));
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
    fn rm_and_del_remove_files_but_not_directories() {
        let mut terminal = Terminal::new(AppMode::Editor);
        terminal.filesystem.borrow_mut().write_text("one.txt", "one").unwrap();
        terminal.filesystem.borrow_mut().write_text("two.txt", "two").unwrap();
        terminal.filesystem.borrow_mut().create_directory("assets").unwrap();

        type_command(&mut terminal, "rm ONE.TXT");
        type_command(&mut terminal, "del two.txt");

        assert_eq!(terminal.filesystem.borrow().read_text("one.txt"), Err("File not found".into()));
        assert_eq!(terminal.filesystem.borrow().read_text("two.txt"), Err("File not found".into()));
        assert!(terminal.filesystem.borrow().list(None).unwrap()[0].is_directory);
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
        assert!(screen.contains("2 byte(s) from $2"));
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
