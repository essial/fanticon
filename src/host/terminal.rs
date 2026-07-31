use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Video};

use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};
use super::filesystem::{DirectoryEntry, SharedFilesystem, shared_filesystem};
use super::ui_colors::{SharedUiColors, UiColors, parse_palette_index, shared_ui_colors};

const COLUMNS: usize = DISPLAY_WIDTH / GLYPH_WIDTH;
const ROWS: usize = DISPLAY_HEIGHT / GLYPH_HEIGHT;
const CELL_COUNT: usize = COLUMNS * ROWS;
const MAX_INPUT: usize = COLUMNS - 3;

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
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum TerminalAction {
    None,
    SwitchMode(AppMode),
    Edit(Option<String>),
}

pub struct Terminal {
    cells: [u8; CELL_COUNT],
    cursor_x: usize,
    cursor_y: usize,
    input: String,
    mode: AppMode,
    filesystem: SharedFilesystem,
    colors: SharedUiColors,
}

impl Terminal {
    pub fn new(mode: AppMode) -> Self {
        let mut terminal = Self {
            cells: [b' '; CELL_COUNT],
            cursor_x: 0,
            cursor_y: 0,
            input: String::with_capacity(MAX_INPUT),
            mode,
            filesystem: shared_filesystem(),
            colors: shared_ui_colors(),
        };
        terminal.show_banner();
        terminal
    }

    pub fn input_character(&mut self, character: char) {
        if self.input.len() >= MAX_INPUT || !character.is_ascii() {
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
            self.cells[self.cursor_y * COLUMNS + self.cursor_x] = b' ';
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
        self.clear();
        self.show_banner();
    }

    pub fn filesystem(&self) -> SharedFilesystem {
        self.filesystem.clone()
    }

    pub fn colors(&self) -> SharedUiColors {
        self.colors.clone()
    }

    pub fn render(&self, video: &mut Video, cursor_visible: bool) {
        let colors = self.colors.get();
        let pixels = video.pixels_mut();
        pixels.fill(colors.background);

        for cell_y in 0..ROWS {
            for cell_x in 0..COLUMNS {
                let character = self.cells[cell_y * COLUMNS + cell_x] as usize;
                let glyph = CHARACTER_ROM[character.min(CHARACTER_ROM.len() - 1)];
                for (glyph_y, bits) in glyph.into_iter().enumerate() {
                    for glyph_x in 0..GLYPH_WIDTH {
                        if bits & (0x80 >> glyph_x) != 0 {
                            let x = cell_x * GLYPH_WIDTH + glyph_x;
                            let y = cell_y * GLYPH_HEIGHT + glyph_y;
                            pixels[y * DISPLAY_WIDTH + x] = colors.foreground;
                        }
                    }
                }
            }
        }

        if cursor_visible {
            let start_x = self.cursor_x * GLYPH_WIDTH + 1;
            let y = self.cursor_y * GLYPH_HEIGHT + GLYPH_HEIGHT - 1;
            for x in start_x..(start_x + 5).min(DISPLAY_WIDTH) {
                pixels[y * DISPLAY_WIDTH + x] = colors.foreground;
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
                self.write("RMDIR DIR LS\n");
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

    fn prompt(&mut self) {
        self.input.clear();
        let directory = self.filesystem.borrow().current_directory();
        self.write(&directory);
        self.write("> ");
    }

    fn clear(&mut self) {
        self.cells.fill(b' ');
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

    fn put_byte(&mut self, byte: u8) {
        if self.cursor_x >= COLUMNS {
            self.newline();
        }
        self.cells[self.cursor_y * COLUMNS + self.cursor_x] = byte;
        self.cursor_x += 1;
    }

    fn newline(&mut self) {
        self.cursor_x = 0;
        self.cursor_y += 1;
        if self.cursor_y >= ROWS {
            self.cells.copy_within(COLUMNS.., 0);
            self.cells[(ROWS - 1) * COLUMNS..].fill(b' ');
            self.cursor_y = ROWS - 1;
        }
    }
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
        terminal.switch_mode(AppMode::Editor);
        assert_eq!(terminal.mode, AppMode::Editor);
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
        let mut video = Video::new();
        terminal.render(&mut video, false);
        assert!(video.pixels().contains(&3));
        assert!(video.pixels().contains(&224));
    }
}
