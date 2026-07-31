use fanticon::{assembler::Diagnostic, video::Video};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::{
    EDITOR_DISPLAY_HEIGHT, EDITOR_DISPLAY_WIDTH,
    builder::build_source,
    character_rom::{
        BOX_BOTTOM_LEFT, BOX_BOTTOM_RIGHT, BOX_HORIZONTAL, BOX_TOP_LEFT, BOX_TOP_RIGHT,
        BOX_VERTICAL, CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH, SYMBOL_ARROW_RIGHT, SYMBOL_BUSY,
        SYMBOL_CHECK, SYMBOL_CROSS,
    },
    filesystem::SharedFilesystem,
    ui_colors::SharedUiColors,
};

const COLUMNS: usize = EDITOR_DISPLAY_WIDTH / GLYPH_WIDTH;
const ROWS: usize = EDITOR_DISPLAY_HEIGHT / GLYPH_HEIGHT;
const TEXT_ROWS: usize = ROWS - 2;
const ASM_TEXT_COLOR: u8 = 240;
const ASM_LABEL_COLOR: u8 = 241;
const ASM_OPCODE_COLOR: u8 = 242;
const ASM_DIRECTIVE_COLOR: u8 = 243;
const ASM_NUMBER_COLOR: u8 = 244;
const ASM_COMMENT_COLOR: u8 = 245;
const ASM_STRING_COLOR: u8 = 246;
const ASM_ERROR_COLOR: u8 = 247;
const UI_WHITE_COLOR: u8 = 248;
const UI_ERROR_BACKGROUND: u8 = 249;
const UI_SUCCESS_BACKGROUND: u8 = 250;
const BUILD_PROGRESS_FRAMES: u8 = 8;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: usize,
    column: usize,
}

#[derive(Clone, Copy)]
struct CellRect {
    x: usize,
    y: usize,
    width: usize,
    height: usize,
}

#[derive(Clone, Copy)]
struct CellStyle {
    foreground: u8,
    background: u8,
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    cursor: Position,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
    Build,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogKind {
    Open,
    SaveAs,
}

enum Overlay {
    None,
    Menu { menu: MenuKind, selected: usize },
    Dialog { kind: DialogKind, input: String, error: Option<String> },
    Building { frames_remaining: u8 },
    Message { title: String, lines: Vec<String> },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum EditorAction {
    None,
    Exit,
}

pub struct TextEditor {
    filesystem: SharedFilesystem,
    colors: SharedUiColors,
    lines: Vec<String>,
    cursor: Position,
    selection_anchor: Option<Position>,
    scroll_line: usize,
    scroll_column: usize,
    filename: Option<String>,
    clipboard: String,
    undo: Vec<Snapshot>,
    dirty: bool,
    overlay: Overlay,
    diagnostics: Vec<Diagnostic>,
    diagnostic_index: Option<usize>,
    build_message: Option<String>,
}

impl TextEditor {
    pub fn new(
        filesystem: SharedFilesystem,
        colors: SharedUiColors,
        filename: Option<String>,
    ) -> Self {
        let mut editor = Self {
            filesystem,
            colors,
            lines: vec![String::new()],
            cursor: Position::default(),
            selection_anchor: None,
            scroll_line: 0,
            scroll_column: 0,
            filename: None,
            clipboard: String::new(),
            undo: Vec::new(),
            dirty: false,
            overlay: Overlay::None,
            diagnostics: Vec::new(),
            diagnostic_index: None,
            build_message: None,
        };
        if let Some(filename) = filename
            && let Err(error) = editor.load(&filename)
        {
            editor.overlay =
                Overlay::Dialog { kind: DialogKind::Open, input: filename, error: Some(error) };
        }
        editor
    }

    pub fn handle_key(
        &mut self,
        key: &Key,
        physical_key: PhysicalKey,
        modifiers: ModifiersState,
    ) -> EditorAction {
        if !matches!(self.overlay, Overlay::None) {
            return self.handle_overlay_key(key, modifiers);
        }

        if (modifiers.control_key() || modifiers.super_key())
            && let Key::Character(text) = key
        {
            return match text.to_ascii_lowercase().as_str() {
                "a" => {
                    self.select_all();
                    EditorAction::None
                }
                "c" => {
                    self.copy_selection();
                    EditorAction::None
                }
                "x" => {
                    self.cut_selection();
                    EditorAction::None
                }
                "v" => {
                    self.paste();
                    EditorAction::None
                }
                "z" => {
                    self.undo();
                    EditorAction::None
                }
                "s" => {
                    self.save_or_prompt();
                    EditorAction::None
                }
                "o" => {
                    self.open_dialog(DialogKind::Open);
                    EditorAction::None
                }
                "n" => {
                    self.new_document();
                    EditorAction::None
                }
                "b" => {
                    self.start_build();
                    EditorAction::None
                }
                _ => EditorAction::None,
            };
        }

        if modifiers.alt_key() {
            match physical_key {
                PhysicalKey::Code(KeyCode::KeyF) => self.open_menu(MenuKind::File),
                PhysicalKey::Code(KeyCode::KeyE) => self.open_menu(MenuKind::Edit),
                PhysicalKey::Code(KeyCode::KeyB) => self.open_menu(MenuKind::Build),
                _ => {}
            }
            return EditorAction::None;
        }

        match key {
            Key::Named(NamedKey::F10) => self.open_menu(MenuKind::File),
            Key::Named(NamedKey::F2) => self.save_or_prompt(),
            Key::Named(NamedKey::F3) => self.open_dialog(DialogKind::Open),
            Key::Named(NamedKey::F4) => self.move_diagnostic(!modifiers.shift_key()),
            Key::Named(NamedKey::F5) => self.start_build(),
            Key::Named(NamedKey::ArrowLeft) => self.move_cursor(modifiers.shift_key(), |editor| {
                if editor.cursor.column > 0 {
                    editor.cursor.column -= 1;
                } else if editor.cursor.line > 0 {
                    editor.cursor.line -= 1;
                    editor.cursor.column = editor.lines[editor.cursor.line].len();
                }
            }),
            Key::Named(NamedKey::ArrowRight) => self.move_cursor(modifiers.shift_key(), |editor| {
                if editor.cursor.column < editor.lines[editor.cursor.line].len() {
                    editor.cursor.column += 1;
                } else if editor.cursor.line + 1 < editor.lines.len() {
                    editor.cursor.line += 1;
                    editor.cursor.column = 0;
                }
            }),
            Key::Named(NamedKey::ArrowUp) => self.move_vertical(-1, modifiers.shift_key()),
            Key::Named(NamedKey::ArrowDown) => self.move_vertical(1, modifiers.shift_key()),
            Key::Named(NamedKey::Home) => self.move_cursor(modifiers.shift_key(), |editor| {
                editor.cursor.column = 0;
            }),
            Key::Named(NamedKey::End) => self.move_cursor(modifiers.shift_key(), |editor| {
                editor.cursor.column = editor.lines[editor.cursor.line].len();
            }),
            Key::Named(NamedKey::PageUp) => self.move_page(-1, modifiers.shift_key()),
            Key::Named(NamedKey::PageDown) => self.move_page(1, modifiers.shift_key()),
            Key::Named(NamedKey::Backspace) => self.backspace(),
            Key::Named(NamedKey::Delete) => self.delete_forward(),
            Key::Named(NamedKey::Enter) => self.insert_newline(),
            Key::Named(NamedKey::Tab) => self.insert_tab(),
            Key::Named(NamedKey::Escape) => {}
            Key::Named(NamedKey::Space) => self.insert_text(" "),
            Key::Character(text) => {
                let filtered = text.chars().filter(char::is_ascii_graphic).collect::<String>();
                self.insert_text(&filtered);
            }
            _ => {}
        }
        self.ensure_cursor_visible();
        EditorAction::None
    }

    pub fn update(&mut self) {
        let run_build = match &mut self.overlay {
            Overlay::Building { frames_remaining: 0 } => true,
            Overlay::Building { frames_remaining } => {
                *frames_remaining -= 1;
                false
            }
            _ => false,
        };
        if run_build {
            self.overlay = Overlay::None;
            self.perform_build();
        }
    }

    pub fn render(&self, video: &mut Video, cursor_visible: bool) {
        debug_assert_eq!(video.dimensions(), (EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT));
        let colors = self.colors.get();
        let assembly_mode = self.assembly_mode();
        configure_ui_palette(video);
        if assembly_mode {
            configure_assembly_palette(video);
        }
        let background = if assembly_mode { 0 } else { colors.background };
        let foreground = if assembly_mode { ASM_TEXT_COLOR } else { colors.foreground };
        let mut cells = [b' '; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        let mut foregrounds = [foreground; COLUMNS * ROWS];
        let mut backgrounds = [background; COLUMNS * ROWS];

        put_text(&mut cells, 0, 0, " FILE  EDIT  BUILD");
        inverse[..COLUMNS].fill(true);

        for screen_y in 0..TEXT_ROWS {
            let line_index = self.scroll_line + screen_y;
            let Some(line) = self.lines.get(line_index) else { break };
            let syntax = assembly_mode.then(|| assembly_syntax_colors(line, foreground));
            for (screen_x, byte) in line.bytes().skip(self.scroll_column).take(COLUMNS).enumerate()
            {
                let index = (screen_y + 1) * COLUMNS + screen_x;
                let source_column = self.scroll_column + screen_x;
                cells[index] = byte.to_ascii_uppercase();
                if let Some(syntax) = &syntax
                    && let Some(color) = syntax.get(source_column)
                {
                    foregrounds[index] = *color;
                }
                if self.line_has_error(line_index) {
                    foregrounds[index] = ASM_ERROR_COLOR;
                }
                let position = Position { line: line_index, column: source_column };
                inverse[index] = self.position_selected(position);
            }
        }

        let name = self.filename.as_deref().unwrap_or("UNTITLED.TXT");
        let dirty = if self.dirty { "*" } else { " " };
        let status = self
            .current_diagnostic()
            .map(|diagnostic| {
                format!(
                    " {}:{}:{} {}",
                    diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
                )
            })
            .or_else(|| self.build_message.as_ref().map(|message| format!(" {message}")))
            .unwrap_or_else(|| {
                format!(
                    " {name}{dirty}  LN {} COL {}",
                    self.cursor.line + 1,
                    self.cursor.column + 1
                )
            });
        put_text(&mut cells, 0, ROWS - 1, &status);
        inverse[(ROWS - 1) * COLUMNS..].fill(true);

        self.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut inverse,
            CellStyle { foreground, background },
        );
        render_cells(
            video,
            &cells,
            &foregrounds,
            &backgrounds,
            &inverse,
            CellStyle { foreground, background },
        );

        if cursor_visible && matches!(self.overlay, Overlay::None) {
            let x = self.cursor.column.saturating_sub(self.scroll_column) * GLYPH_WIDTH + 1;
            let y = (self.cursor.line.saturating_sub(self.scroll_line) + 1) * GLYPH_HEIGHT
                + GLYPH_HEIGHT
                - 1;
            if x < EDITOR_DISPLAY_WIDTH && y < EDITOR_DISPLAY_HEIGHT - GLYPH_HEIGHT {
                for pixel_x in x..(x + 5).min(EDITOR_DISPLAY_WIDTH) {
                    video.pixels_mut()[y * EDITOR_DISPLAY_WIDTH + pixel_x] = foreground;
                }
            }
        }
    }

    fn handle_overlay_key(&mut self, key: &Key, modifiers: ModifiersState) -> EditorAction {
        if matches!(self.overlay, Overlay::Message { .. }) {
            match key {
                Key::Named(NamedKey::F4) => self.move_diagnostic(!modifiers.shift_key()),
                Key::Named(NamedKey::Enter | NamedKey::Escape) => self.overlay = Overlay::None,
                Key::Named(NamedKey::F5) => self.start_build(),
                _ => {}
            }
            return EditorAction::None;
        }
        if matches!(self.overlay, Overlay::Building { .. }) {
            if matches!(key, Key::Named(NamedKey::Escape)) {
                self.overlay = Overlay::None;
            }
            return EditorAction::None;
        }

        match &mut self.overlay {
            Overlay::Menu { menu, selected } => {
                let count = menu_items(*menu).len();
                match key {
                    Key::Named(NamedKey::Escape) | Key::Named(NamedKey::F10) => {
                        self.overlay = Overlay::None;
                    }
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                        *menu = match (*menu, key) {
                            (MenuKind::File, Key::Named(NamedKey::ArrowLeft)) => MenuKind::Build,
                            (MenuKind::File, _) => MenuKind::Edit,
                            (MenuKind::Edit, Key::Named(NamedKey::ArrowLeft)) => MenuKind::File,
                            (MenuKind::Edit, _) => MenuKind::Build,
                            (MenuKind::Build, Key::Named(NamedKey::ArrowLeft)) => MenuKind::Edit,
                            (MenuKind::Build, _) => MenuKind::File,
                        };
                        *selected = 0;
                    }
                    Key::Named(NamedKey::ArrowUp) => *selected = (*selected + count - 1) % count,
                    Key::Named(NamedKey::ArrowDown) => *selected = (*selected + 1) % count,
                    Key::Named(NamedKey::Enter) => {
                        let menu = *menu;
                        let selected = *selected;
                        self.overlay = Overlay::None;
                        return self.activate_menu(menu, selected);
                    }
                    Key::Character(text) => {
                        if let Some(item) = menu_hotkey(*menu, text) {
                            let menu = *menu;
                            self.overlay = Overlay::None;
                            return self.activate_menu(menu, item);
                        }
                    }
                    _ => {}
                }
            }
            Overlay::Dialog { kind, input, .. } => match key {
                Key::Named(NamedKey::Escape) => self.overlay = Overlay::None,
                Key::Named(NamedKey::Backspace) => {
                    input.pop();
                }
                Key::Named(NamedKey::Enter) => {
                    let kind = *kind;
                    let filename = input.clone();
                    self.submit_dialog(kind, filename);
                }
                Key::Character(text) => {
                    for character in text.chars().filter(|character| character.is_ascii()) {
                        if input.len() < 26
                            && (character.is_ascii_alphanumeric() || "._-/\\".contains(character))
                        {
                            input.push(character.to_ascii_lowercase());
                        }
                    }
                }
                _ => {}
            },
            Overlay::None | Overlay::Building { .. } | Overlay::Message { .. } => {}
        }
        EditorAction::None
    }

    fn activate_menu(&mut self, menu: MenuKind, selected: usize) -> EditorAction {
        match (menu, selected) {
            (MenuKind::File, 0) => self.new_document(),
            (MenuKind::File, 1) => self.open_dialog(DialogKind::Open),
            (MenuKind::File, 2) => self.save_or_prompt(),
            (MenuKind::File, 3) => self.open_dialog(DialogKind::SaveAs),
            (MenuKind::File, 4) => return EditorAction::Exit,
            (MenuKind::Edit, 0) => self.undo(),
            (MenuKind::Edit, 1) => self.cut_selection(),
            (MenuKind::Edit, 2) => self.copy_selection(),
            (MenuKind::Edit, 3) => self.paste(),
            (MenuKind::Edit, 4) => self.select_all(),
            (MenuKind::Build, 0) => self.start_build(),
            (MenuKind::Build, 1) => self.move_diagnostic(true),
            (MenuKind::Build, 2) => self.move_diagnostic(false),
            _ => {}
        }
        EditorAction::None
    }

    fn render_overlay(
        &self,
        cells: &mut [u8],
        foregrounds: &mut [u8],
        backgrounds: &mut [u8],
        inverse: &mut [bool],
        style: CellStyle,
    ) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Menu { menu, selected } => {
                let x = match menu {
                    MenuKind::File => 0,
                    MenuKind::Edit => 6,
                    MenuKind::Build => 12,
                };
                let width = 16;
                let y = 1;
                draw_window(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    CellRect { x, y, width, height: menu_labels(*menu).len() + 2 },
                    style,
                );
                for (index, item) in menu_labels(*menu).iter().enumerate() {
                    let row = y + index + 1;
                    put_text_width(cells, x + 3, row, item, width - 4);
                    if index == *selected {
                        inverse[row * COLUMNS + x + 1..row * COLUMNS + x + width - 1].fill(true);
                        put_cell(cells, x + 1, row, SYMBOL_ARROW_RIGHT);
                    }
                }
            }
            Overlay::Dialog { kind, input, error } => {
                let title = if *kind == DialogKind::Open { "OPEN FILE" } else { "SAVE FILE" };
                let width = 32;
                let height = 8;
                let x = (COLUMNS - width) / 2;
                let y = (ROWS - height) / 2;
                draw_window(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    CellRect { x, y, width, height },
                    style,
                );
                put_text_width(cells, x + 3, y, title, width - 6);
                put_cell(cells, x + 2, y + 2, SYMBOL_ARROW_RIGHT);
                put_text(cells, x + 4, y + 2, "NAME:");
                put_text_width(cells, x + 10, y + 2, input, width - 11);
                put_text(cells, x + 3, y + height - 2, "ENTER=OK  ESC=CANCEL");
                if let Some(error) = error {
                    put_cell(cells, x + 2, y + 4, SYMBOL_CROSS);
                    put_text_width(cells, x + 4, y + 4, error, width - 5);
                }
            }
            Overlay::Building { .. } => {
                render_message_box(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    style,
                    "BUILD",
                    &["ASSEMBLING...".to_owned()],
                );
            }
            Overlay::Message { title, lines } => {
                let message_style = if title == "BUILD SUCCESSFUL" {
                    CellStyle { foreground: UI_WHITE_COLOR, background: UI_SUCCESS_BACKGROUND }
                } else if title.contains("ERROR") {
                    CellStyle { foreground: UI_WHITE_COLOR, background: UI_ERROR_BACKGROUND }
                } else {
                    style
                };
                render_message_box(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    message_style,
                    title,
                    lines,
                );
            }
        }
    }

    fn open_menu(&mut self, menu: MenuKind) {
        self.overlay = Overlay::Menu { menu, selected: 0 };
    }

    fn open_dialog(&mut self, kind: DialogKind) {
        self.overlay =
            Overlay::Dialog { kind, input: self.filename.clone().unwrap_or_default(), error: None };
    }

    fn submit_dialog(&mut self, kind: DialogKind, filename: String) {
        let result = match kind {
            DialogKind::Open => self.load(&filename),
            DialogKind::SaveAs => self.save_as(&filename),
        };
        if let Err(error) = result {
            self.overlay = Overlay::Dialog { kind, input: filename, error: Some(error) };
        } else {
            self.overlay = Overlay::None;
        }
    }

    fn save_or_prompt(&mut self) {
        if let Some(filename) = self.filename.clone() {
            if let Err(error) = self.save_as(&filename) {
                self.overlay = Overlay::Dialog {
                    kind: DialogKind::SaveAs,
                    input: filename,
                    error: Some(error),
                };
            }
        } else {
            self.open_dialog(DialogKind::SaveAs);
        }
    }

    fn start_build(&mut self) {
        self.overlay = Overlay::Building { frames_remaining: BUILD_PROGRESS_FRAMES };
    }

    fn perform_build(&mut self) {
        let Some(filename) = self.filename.clone() else {
            self.diagnostics.clear();
            self.diagnostic_index = None;
            self.build_message = Some("SAVE AS ASM/INC BEFORE BUILD".to_owned());
            self.show_build_message("BUILD ERROR", &["SAVE AS ASM/INC BEFORE BUILD".to_owned()]);
            return;
        };
        if !assembly_filename(&filename) {
            self.diagnostics.clear();
            self.diagnostic_index = None;
            self.build_message = Some("BUILD REQUIRES AN ASM/INC FILE".to_owned());
            self.show_build_message("BUILD ERROR", &["BUILD REQUIRES AN ASM/INC FILE".to_owned()]);
            return;
        }

        let source = self.lines.join("\n");
        match build_source(&self.filesystem, &filename, &source, None) {
            Ok(success) => {
                self.diagnostics.clear();
                self.diagnostic_index = None;
                self.build_message = Some(format!(
                    "BUILT {} ${:04X} {} BYTES",
                    success.output, success.origin, success.size
                ));
                self.show_build_message(
                    "BUILD SUCCESSFUL",
                    &[
                        format!("OUTPUT: {}", success.output),
                        format!("ORIGIN: ${:04X}", success.origin),
                        format!("SIZE: {} BYTES", success.size),
                    ],
                );
            }
            Err(diagnostics) => {
                self.diagnostics = diagnostics;
                self.diagnostic_index = (!self.diagnostics.is_empty()).then_some(0);
                self.build_message = None;
                self.goto_current_diagnostic();
                self.show_current_diagnostic_dialog();
            }
        }
    }

    fn move_diagnostic(&mut self, forward: bool) {
        if self.diagnostics.is_empty() {
            self.build_message = Some("NO BUILD ERRORS".to_owned());
            return;
        }
        let current = self.diagnostic_index.unwrap_or(0);
        self.diagnostic_index = Some(if forward {
            (current + 1) % self.diagnostics.len()
        } else {
            (current + self.diagnostics.len() - 1) % self.diagnostics.len()
        });
        self.goto_current_diagnostic();
        self.show_current_diagnostic_dialog();
    }

    fn show_current_diagnostic_dialog(&mut self) {
        let Some(index) = self.diagnostic_index else { return };
        let Some(diagnostic) = self.diagnostics.get(index) else { return };
        let mut lines = vec![
            format!("ERROR {} OF {}", index + 1, self.diagnostics.len()),
            format!("{}:{}:{}", diagnostic.source, diagnostic.line, diagnostic.column),
        ];
        lines.extend(wrap_dialog_text(&diagnostic.message, 30));
        self.show_build_message("BUILD ERRORS", &lines);
    }

    fn show_build_message(&mut self, title: &str, lines: &[String]) {
        self.overlay = Overlay::Message { title: title.to_owned(), lines: lines.to_vec() };
    }

    fn goto_current_diagnostic(&mut self) {
        let Some(diagnostic) = self.current_diagnostic() else { return };
        if !self.source_is_current(&diagnostic.source) {
            return;
        }
        let line = diagnostic.line.saturating_sub(1).min(self.lines.len() - 1);
        let column = diagnostic.column.saturating_sub(1).min(self.lines[line].len());
        self.cursor = Position { line, column };
        self.selection_anchor = None;
        self.ensure_cursor_visible();
    }

    fn current_diagnostic(&self) -> Option<&Diagnostic> {
        self.diagnostic_index.and_then(|index| self.diagnostics.get(index))
    }

    fn line_has_error(&self, line: usize) -> bool {
        self.diagnostics.iter().any(|diagnostic| {
            diagnostic.line == line + 1 && self.source_is_current(&diagnostic.source)
        })
    }

    fn source_is_current(&self, source: &str) -> bool {
        self.filename.as_deref().is_some_and(|filename| filename.eq_ignore_ascii_case(source))
    }

    fn invalidate_build(&mut self) {
        self.diagnostics.clear();
        self.diagnostic_index = None;
        self.build_message = None;
    }

    fn save_as(&mut self, filename: &str) -> Result<(), String> {
        let mut lines = self.lines.clone();
        if assembly_filename(filename) {
            format_assembly_lines(&mut lines);
        }
        let text = lines.join("\n");
        self.filesystem.borrow_mut().write_text(filename, &text)?;
        self.lines = lines;
        self.clamp_cursor();
        self.selection_anchor = None;
        self.filename = Some(filename.to_ascii_lowercase());
        self.dirty = false;
        Ok(())
    }

    fn load(&mut self, filename: &str) -> Result<(), String> {
        let text = self.filesystem.borrow().read_text(filename)?;
        self.lines =
            text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_owned).collect();
        if assembly_filename(filename) {
            format_assembly_lines(&mut self.lines);
        }
        if self.lines.is_empty() {
            self.lines.push(String::new());
        }
        self.cursor = Position::default();
        self.selection_anchor = None;
        self.scroll_line = 0;
        self.scroll_column = 0;
        self.filename = Some(filename.to_ascii_lowercase());
        self.undo.clear();
        self.dirty = false;
        self.invalidate_build();
        Ok(())
    }

    fn new_document(&mut self) {
        self.record_undo();
        self.lines = vec![String::new()];
        self.cursor = Position::default();
        self.selection_anchor = None;
        self.filename = None;
        self.dirty = false;
        self.invalidate_build();
    }

    fn record_undo(&mut self) {
        self.invalidate_build();
        if self.undo.len() == 64 {
            self.undo.remove(0);
        }
        self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo.pop() {
            self.invalidate_build();
            self.lines = snapshot.lines;
            self.cursor = snapshot.cursor;
            self.selection_anchor = None;
            self.dirty = true;
            self.ensure_cursor_visible();
        }
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() {
            return;
        }
        self.record_undo();
        self.delete_selection_without_undo();
        self.lines[self.cursor.line].insert_str(self.cursor.column, text);
        self.cursor.column += text.len();
        self.dirty = true;
    }

    fn insert_newline(&mut self) {
        self.record_undo();
        self.delete_selection_without_undo();
        if self.assembly_mode() && self.cursor.column == self.lines[self.cursor.line].len() {
            self.lines[self.cursor.line] = format_assembly_line(&self.lines[self.cursor.line]);
            self.cursor.column = self.lines[self.cursor.line].len();
        }
        let remainder = self.lines[self.cursor.line].split_off(self.cursor.column);
        self.cursor.line += 1;
        self.cursor.column = 0;
        self.lines.insert(self.cursor.line, remainder);
        self.dirty = true;
    }

    fn insert_tab(&mut self) {
        let spaces = if self.assembly_mode() {
            [9, 15, 26]
                .into_iter()
                .find(|column| *column > self.cursor.column)
                .map_or(4, |column| column - self.cursor.column)
        } else {
            4
        };
        self.insert_text(&" ".repeat(spaces));
    }

    fn backspace(&mut self) {
        if self.has_selection() {
            self.record_undo();
            self.delete_selection_without_undo();
        } else if self.cursor.column > 0 {
            self.record_undo();
            self.cursor.column -= 1;
            self.lines[self.cursor.line].remove(self.cursor.column);
        } else if self.cursor.line > 0 {
            self.record_undo();
            let line = self.lines.remove(self.cursor.line);
            self.cursor.line -= 1;
            self.cursor.column = self.lines[self.cursor.line].len();
            self.lines[self.cursor.line].push_str(&line);
        } else {
            return;
        }
        self.dirty = true;
    }

    fn delete_forward(&mut self) {
        if self.has_selection() {
            self.record_undo();
            self.delete_selection_without_undo();
        } else if self.cursor.column < self.lines[self.cursor.line].len() {
            self.record_undo();
            self.lines[self.cursor.line].remove(self.cursor.column);
        } else if self.cursor.line + 1 < self.lines.len() {
            self.record_undo();
            let next = self.lines.remove(self.cursor.line + 1);
            self.lines[self.cursor.line].push_str(&next);
        } else {
            return;
        }
        self.dirty = true;
    }

    fn selection(&self) -> Option<(Position, Position)> {
        let anchor = self.selection_anchor?;
        (anchor != self.cursor).then(|| {
            if anchor < self.cursor { (anchor, self.cursor) } else { (self.cursor, anchor) }
        })
    }

    fn has_selection(&self) -> bool {
        self.selection().is_some()
    }

    fn position_selected(&self, position: Position) -> bool {
        self.selection().is_some_and(|(start, end)| position >= start && position < end)
    }

    fn selected_text(&self) -> Option<String> {
        let (start, end) = self.selection()?;
        if start.line == end.line {
            return Some(self.lines[start.line][start.column..end.column].to_owned());
        }
        let mut text = self.lines[start.line][start.column..].to_owned();
        for line in start.line + 1..end.line {
            text.push('\n');
            text.push_str(&self.lines[line]);
        }
        text.push('\n');
        text.push_str(&self.lines[end.line][..end.column]);
        Some(text)
    }

    fn copy_selection(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard = text;
        }
    }

    fn cut_selection(&mut self) {
        if let Some(text) = self.selected_text() {
            self.clipboard = text;
            self.record_undo();
            self.delete_selection_without_undo();
            self.dirty = true;
        }
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() {
            return;
        }
        let clipboard = self.clipboard.clone();
        self.record_undo();
        self.delete_selection_without_undo();
        let suffix = self.lines[self.cursor.line].split_off(self.cursor.column);
        let parts = clipboard.split('\n').collect::<Vec<_>>();
        self.lines[self.cursor.line].push_str(parts[0]);
        if parts.len() == 1 {
            self.cursor.column += parts[0].len();
            self.lines[self.cursor.line].push_str(&suffix);
        } else {
            for part in &parts[1..] {
                self.cursor.line += 1;
                self.lines.insert(self.cursor.line, (*part).to_owned());
            }
            self.cursor.column = parts.last().expect("clipboard parts").len();
            self.lines[self.cursor.line].push_str(&suffix);
        }
        self.dirty = true;
    }

    fn delete_selection_without_undo(&mut self) {
        let Some((start, end)) = self.selection() else { return };
        if start.line == end.line {
            self.lines[start.line].replace_range(start.column..end.column, "");
        } else {
            let suffix = self.lines[end.line][end.column..].to_owned();
            self.lines[start.line].truncate(start.column);
            self.lines[start.line].push_str(&suffix);
            self.lines.drain(start.line + 1..=end.line);
        }
        self.cursor = start;
        self.selection_anchor = None;
    }

    fn select_all(&mut self) {
        self.selection_anchor = Some(Position::default());
        self.cursor.line = self.lines.len() - 1;
        self.cursor.column = self.lines[self.cursor.line].len();
        self.ensure_cursor_visible();
    }

    fn move_cursor(&mut self, selecting: bool, movement: impl FnOnce(&mut Self)) {
        let previous_line = self.cursor.line;
        if selecting {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        movement(self);
        if !selecting && self.cursor.line != previous_line {
            self.format_departed_line(previous_line);
        }
    }

    fn format_departed_line(&mut self, line: usize) {
        if !self.assembly_mode() {
            return;
        }
        let formatted = format_assembly_line(&self.lines[line]);
        if formatted != self.lines[line] {
            self.record_undo();
            self.lines[line] = formatted;
            self.dirty = true;
        }
    }

    fn move_vertical(&mut self, direction: isize, selecting: bool) {
        self.move_cursor(selecting, |editor| {
            editor.cursor.line =
                editor.cursor.line.saturating_add_signed(direction).min(editor.lines.len() - 1);
            editor.cursor.column = editor.cursor.column.min(editor.lines[editor.cursor.line].len());
        });
    }

    fn move_page(&mut self, direction: isize, selecting: bool) {
        self.move_cursor(selecting, |editor| {
            let distance = direction * TEXT_ROWS as isize;
            editor.cursor.line =
                editor.cursor.line.saturating_add_signed(distance).min(editor.lines.len() - 1);
            editor.cursor.column = editor.cursor.column.min(editor.lines[editor.cursor.line].len());
        });
    }

    fn ensure_cursor_visible(&mut self) {
        if self.cursor.line < self.scroll_line {
            self.scroll_line = self.cursor.line;
        } else if self.cursor.line >= self.scroll_line + TEXT_ROWS {
            self.scroll_line = self.cursor.line + 1 - TEXT_ROWS;
        }
        if self.cursor.column < self.scroll_column {
            self.scroll_column = self.cursor.column;
        } else if self.cursor.column >= self.scroll_column + COLUMNS {
            self.scroll_column = self.cursor.column + 1 - COLUMNS;
        }
    }

    fn clamp_cursor(&mut self) {
        self.cursor.line = self.cursor.line.min(self.lines.len() - 1);
        self.cursor.column = self.cursor.column.min(self.lines[self.cursor.line].len());
    }

    fn assembly_mode(&self) -> bool {
        self.filename.as_deref().is_some_and(assembly_filename)
    }
}

fn menu_items(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => &["NEW", "OPEN...", "SAVE", "SAVE AS...", "EXIT"],
        MenuKind::Edit => &["UNDO", "CUT", "COPY", "PASTE", "SELECT ALL"],
        MenuKind::Build => &["ASSEMBLE", "NEXT ERROR", "PREV ERROR"],
    }
}

fn menu_labels(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => {
            &["NEW       N", "OPEN      O", "SAVE      S", "SAVE AS   A", "EXIT      X"]
        }
        MenuKind::Edit => {
            &["UNDO      U", "CUT       T", "COPY      C", "PASTE     P", "SELECT ALL A"]
        }
        MenuKind::Build => &["ASSEMBLE  B", "NEXT ERR  N", "PREV ERR  P"],
    }
}

fn menu_hotkey(menu: MenuKind, key: &str) -> Option<usize> {
    let key = key.to_ascii_lowercase();
    let hotkeys = match menu {
        MenuKind::File => ["n", "o", "s", "a", "x"],
        MenuKind::Edit => ["u", "t", "c", "p", "a"],
        MenuKind::Build => ["b", "n", "p", "", ""],
    };
    hotkeys.iter().position(|hotkey| *hotkey == key)
}

fn render_message_box(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    inverse: &mut [bool],
    style: CellStyle,
    title: &str,
    lines: &[String],
) {
    let width = 34;
    let x = (COLUMNS - width) / 2;
    let visible_lines = lines.len().min(7);
    let height = visible_lines + 4;
    let y = (ROWS - height) / 2;
    draw_window(cells, foregrounds, backgrounds, inverse, CellRect { x, y, width, height }, style);
    let symbol = if title == "BUILD SUCCESSFUL" {
        SYMBOL_CHECK
    } else if title.contains("ERROR") {
        SYMBOL_CROSS
    } else {
        SYMBOL_BUSY
    };
    put_cell(cells, x + 2, y, symbol);
    put_text_width(cells, x + 4, y, title, width - 7);
    for (index, line) in lines.iter().take(visible_lines).enumerate() {
        put_text_width(cells, x + 2, y + 2 + index, line, width - 4);
    }
    let footer = if title == "BUILD ERRORS" { "ENTER=OK  F4=NEXT" } else { "ENTER=OK  ESC=CLOSE" };
    put_text(cells, x + 2, y + height - 2, footer);
}

fn draw_window(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    inverse: &mut [bool],
    rect: CellRect,
    style: CellStyle,
) {
    let CellRect { x, y, width, height } = rect;
    debug_assert!(width >= 2 && height >= 2 && x + width <= COLUMNS && y + height <= ROWS);
    for row in y..y + height {
        let range = row * COLUMNS + x..row * COLUMNS + x + width;
        cells[range.clone()].fill(b' ');
        foregrounds[range.clone()].fill(style.foreground);
        backgrounds[range.clone()].fill(style.background);
        inverse[range].fill(false);
    }

    put_cell(cells, x, y, BOX_TOP_LEFT);
    put_cell(cells, x + width - 1, y, BOX_TOP_RIGHT);
    put_cell(cells, x, y + height - 1, BOX_BOTTOM_LEFT);
    put_cell(cells, x + width - 1, y + height - 1, BOX_BOTTOM_RIGHT);
    for column in x + 1..x + width - 1 {
        put_cell(cells, column, y, BOX_HORIZONTAL);
        put_cell(cells, column, y + height - 1, BOX_HORIZONTAL);
    }
    for row in y + 1..y + height - 1 {
        put_cell(cells, x, row, BOX_VERTICAL);
        put_cell(cells, x + width - 1, row, BOX_VERTICAL);
    }
}

fn wrap_dialog_text(text: &str, width: usize) -> Vec<String> {
    let mut lines = Vec::new();
    let mut current = String::new();
    for word in text.split_whitespace() {
        if !current.is_empty() && current.len() + 1 + word.len() > width {
            lines.push(core::mem::take(&mut current));
        }
        if !current.is_empty() {
            current.push(' ');
        }
        current.push_str(word);
    }
    if !current.is_empty() {
        lines.push(current);
    }
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn put_text(cells: &mut [u8], x: usize, y: usize, text: &str) {
    put_text_width(cells, x, y, text, COLUMNS.saturating_sub(x));
}

fn put_text_width(cells: &mut [u8], x: usize, y: usize, text: &str, width: usize) {
    if y >= ROWS {
        return;
    }
    for (offset, byte) in text.bytes().take(width.min(COLUMNS.saturating_sub(x))).enumerate() {
        cells[y * COLUMNS + x + offset] = byte.to_ascii_uppercase();
    }
}

fn put_cell(cells: &mut [u8], x: usize, y: usize, character: u8) {
    if x < COLUMNS && y < ROWS {
        cells[y * COLUMNS + x] = character;
    }
}

fn render_cells(
    video: &mut Video,
    cells: &[u8],
    foregrounds: &[u8],
    backgrounds: &[u8],
    inverse: &[bool],
    style: CellStyle,
) {
    let CellStyle { foreground, background } = style;
    let pixels = video.pixels_mut();
    pixels.fill(background);
    for cell_y in 0..ROWS {
        for cell_x in 0..COLUMNS {
            let index = cell_y * COLUMNS + cell_x;
            let (cell_foreground, cell_background) = if inverse[index] {
                (background, foreground)
            } else {
                (foregrounds[index], backgrounds[index])
            };
            if cell_background != background {
                for y in cell_y * GLYPH_HEIGHT..(cell_y + 1) * GLYPH_HEIGHT {
                    pixels[y * EDITOR_DISPLAY_WIDTH + cell_x * GLYPH_WIDTH
                        ..y * EDITOR_DISPLAY_WIDTH + (cell_x + 1) * GLYPH_WIDTH]
                        .fill(cell_background);
                }
            }
            let glyph = CHARACTER_ROM[(cells[index] as usize).min(CHARACTER_ROM.len() - 1)];
            for (glyph_y, bits) in glyph.into_iter().enumerate() {
                for glyph_x in 0..GLYPH_WIDTH {
                    if bits & (0x80 >> glyph_x) != 0 {
                        let x = cell_x * GLYPH_WIDTH + glyph_x;
                        let y = cell_y * GLYPH_HEIGHT + glyph_y;
                        pixels[y * EDITOR_DISPLAY_WIDTH + x] = cell_foreground;
                    }
                }
            }
        }
    }
}

fn assembly_filename(filename: &str) -> bool {
    filename.rsplit(['/', '\\']).next().is_some_and(|name| {
        name.rsplit_once('.').is_some_and(|(_, extension)| {
            matches!(extension.to_ascii_lowercase().as_str(), "asm" | "inc")
        })
    })
}

fn configure_ui_palette(video: &mut Video) {
    video.set_palette(UI_WHITE_COLOR, [255, 255, 255, 255]);
    video.set_palette(UI_ERROR_BACKGROUND, [192, 32, 40, 255]);
    video.set_palette(UI_SUCCESS_BACKGROUND, [32, 80, 192, 255]);
}

fn configure_assembly_palette(video: &mut Video) {
    // Catppuccin Mocha accents over Fanticon's required true-black background.
    video.set_palette(ASM_TEXT_COLOR, [205, 214, 244, 255]);
    video.set_palette(ASM_LABEL_COLOR, [180, 190, 254, 255]);
    video.set_palette(ASM_OPCODE_COLOR, [137, 180, 250, 255]);
    video.set_palette(ASM_DIRECTIVE_COLOR, [203, 166, 247, 255]);
    video.set_palette(ASM_NUMBER_COLOR, [250, 179, 135, 255]);
    video.set_palette(ASM_COMMENT_COLOR, [127, 132, 156, 255]);
    video.set_palette(ASM_STRING_COLOR, [166, 227, 161, 255]);
    video.set_palette(ASM_ERROR_COLOR, [243, 139, 168, 255]);
}

fn format_assembly_lines(lines: &mut [String]) {
    for line in lines {
        *line = format_assembly_line(line);
    }
}

fn format_assembly_line(line: &str) -> String {
    let trimmed_end = line.trim_end();
    let trimmed_start = trimmed_end.trim_start();
    if trimmed_start.is_empty() || trimmed_start.starts_with([';', '*']) {
        return trimmed_start.to_owned();
    }

    let leading_whitespace = trimmed_end.len() != trimmed_start.len();
    let (code, comment) = split_assembly_comment(trimmed_start);
    let fields = code.split_whitespace().collect::<Vec<_>>();
    if fields.is_empty() {
        return comment.unwrap_or_default().to_owned();
    }

    let has_label = !leading_whitespace && !is_operation(fields[0]);
    let (label, opcode_index) = if has_label { (Some(fields[0]), 1) } else { (None, 0) };
    let opcode = fields.get(opcode_index).copied();
    let operand = fields.get(opcode_index + 1..).unwrap_or_default().join(" ");

    let mut output = String::new();
    if let Some(label) = label {
        output.push_str(label);
    }
    if let Some(opcode) = opcode {
        pad_to_column(&mut output, 9);
        output.push_str(opcode);
    }
    if !operand.is_empty() {
        pad_to_column(&mut output, 15);
        output.push_str(&operand);
    }
    if let Some(comment) = comment {
        pad_to_column(&mut output, 26);
        output.push_str(comment);
    }
    output
}

fn pad_to_column(output: &mut String, column: usize) {
    let spaces = column.saturating_sub(output.len()).max(1);
    output.extend(core::iter::repeat_n(' ', spaces));
}

fn split_assembly_comment(line: &str) -> (&str, Option<&str>) {
    let mut quote = None;
    for (index, character) in line.char_indices() {
        match character {
            '\'' | '"' if quote == Some(character) => quote = None,
            '\'' | '"' if quote.is_none() => quote = Some(character),
            ';' if quote.is_none() => return (line[..index].trim_end(), Some(&line[index..])),
            _ => {}
        }
    }
    (line.trim_end(), None)
}

fn assembly_syntax_colors(line: &str, default: u8) -> Vec<u8> {
    let mut colors = vec![default; line.len()];
    let trimmed = line.trim_start();
    let leading = line.len() - trimmed.len();
    if trimmed.starts_with([';', '*']) {
        colors[leading..].fill(ASM_COMMENT_COLOR);
        return colors;
    }

    let (code, comment) = split_assembly_comment(line);
    if let Some(comment) = comment {
        let start = comment.as_ptr() as usize - line.as_ptr() as usize;
        colors[start..].fill(ASM_COMMENT_COLOR);
    }

    let tokens = assembly_tokens(code);
    if let Some(operation_index) = tokens.iter().position(|(_, token)| is_operation(token)) {
        if operation_index > 0 {
            let (start, token) = tokens[0];
            colors[start..start + token.len()].fill(ASM_LABEL_COLOR);
        }
        let (start, token) = tokens[operation_index];
        let color = if is_directive(token) { ASM_DIRECTIVE_COLOR } else { ASM_OPCODE_COLOR };
        colors[start..start + token.len()].fill(color);
    } else if let Some((start, token)) = tokens.first().copied()
        && start == 0
    {
        colors[start..start + token.len()].fill(ASM_LABEL_COLOR);
    }

    let bytes = code.as_bytes();
    let mut index = 0;
    while index < bytes.len() {
        if matches!(bytes[index], b'\'' | b'"') {
            let start = index;
            let quote = bytes[index];
            index += 1;
            while index < bytes.len() && bytes[index] != quote {
                index += 1;
            }
            index = (index + 1).min(bytes.len());
            colors[start..index].fill(ASM_STRING_COLOR);
        } else if bytes[index].is_ascii_digit() || matches!(bytes[index], b'$' | b'%' | b'#') {
            let start = index;
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_hexdigit()
                    || matches!(bytes[index], b'$' | b'%' | b'x' | b'X'))
            {
                index += 1;
            }
            colors[start..index].fill(ASM_NUMBER_COLOR);
        } else {
            index += 1;
        }
    }
    colors
}

fn assembly_tokens(code: &str) -> Vec<(usize, &str)> {
    let mut tokens = Vec::new();
    let mut start = None;
    for (index, character) in code.char_indices() {
        if character.is_whitespace() {
            if let Some(start) = start.take() {
                tokens.push((start, &code[start..index]));
            }
        } else if start.is_none() {
            start = Some(index);
        }
    }
    if let Some(start) = start {
        tokens.push((start, &code[start..]));
    }
    tokens
}

fn is_operation(token: &str) -> bool {
    is_opcode(token) || is_directive(token)
}

fn is_opcode(token: &str) -> bool {
    matches!(
        token.trim_end_matches(':').to_ascii_uppercase().as_str(),
        "ADC"
            | "AND"
            | "ASL"
            | "BCC"
            | "BCS"
            | "BEQ"
            | "BIT"
            | "BMI"
            | "BNE"
            | "BPL"
            | "BRK"
            | "BVC"
            | "BVS"
            | "CLC"
            | "CLD"
            | "CLI"
            | "CLV"
            | "CMP"
            | "CPX"
            | "CPY"
            | "DEC"
            | "DEX"
            | "DEY"
            | "EOR"
            | "INC"
            | "INX"
            | "INY"
            | "JMP"
            | "JSR"
            | "LDA"
            | "LDX"
            | "LDY"
            | "LSR"
            | "NOP"
            | "ORA"
            | "PHA"
            | "PHP"
            | "PLA"
            | "PLP"
            | "ROL"
            | "ROR"
            | "RTI"
            | "RTS"
            | "SBC"
            | "SEC"
            | "SED"
            | "SEI"
            | "STA"
            | "STX"
            | "STY"
            | "TAX"
            | "TAY"
            | "TSX"
            | "TXA"
            | "TXS"
            | "TYA"
            | "KIL"
            | "SLO"
            | "RLA"
            | "SRE"
            | "RRA"
            | "SAX"
            | "LAX"
            | "DCP"
            | "ISC"
            | "ANC"
            | "ALR"
            | "ARR"
            | "XAA"
            | "AXS"
            | "AHX"
            | "SHY"
            | "SHX"
            | "TAS"
            | "LAS"
    )
}

fn is_directive(token: &str) -> bool {
    matches!(
        token.trim_start_matches('.').to_ascii_uppercase().as_str(),
        "ORG"
            | "EQU"
            | "EQ"
            | "DS"
            | "DFB"
            | "DB"
            | "DW"
            | "DA"
            | "HEX"
            | "ASC"
            | "PUT"
            | "USE"
            | "ENT"
            | "EXT"
            | "DO"
            | "ELSE"
            | "FIN"
            | "LUP"
            | "DUM"
            | "DEND"
            | "XC"
            | "MX"
            | "INCLUDE"
            | "BYTE"
            | "WORD"
            | "TEXT"
            | "MAC"
            | "EOM"
            | "PMC"
            | "<<<"
            | ">>>"
            | "END"
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::filesystem::shared_filesystem;
    use crate::host::ui_colors::shared_ui_colors;

    fn key(character: &str) -> Key {
        Key::Character(character.into())
    }

    fn finish_pending_build(editor: &mut TextEditor) {
        for _ in 0..=BUILD_PROGRESS_FRAMES {
            editor.update();
        }
    }

    #[test]
    fn typing_selection_copy_paste_and_undo_work() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.handle_key(&key("hello"), PhysicalKey::Code(KeyCode::KeyH), ModifiersState::empty());
        editor.select_all();
        editor.copy_selection();
        editor.cursor = Position { line: 0, column: 5 };
        editor.selection_anchor = None;
        editor.paste();
        assert_eq!(editor.lines, ["hellohello"]);
        editor.undo();
        assert_eq!(editor.lines, ["hello"]);
    }

    #[test]
    fn save_dialog_and_open_use_shared_virtual_filesystem() {
        let filesystem = shared_filesystem();
        let colors = shared_ui_colors();
        let mut editor = TextEditor::new(filesystem.clone(), colors.clone(), None);
        editor.insert_text("saved text");
        editor.save_as("notes.txt").unwrap();
        let reopened = TextEditor::new(filesystem, colors, Some("NOTES.TXT".to_owned()));
        assert_eq!(reopened.lines, ["saved text"]);
        assert_eq!(reopened.filename.as_deref(), Some("notes.txt"));
    }

    #[test]
    fn editor_renders_menu_and_document_to_framebuffer() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.insert_text("hello");
        editor.open_menu(MenuKind::File);
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, true);
        assert!(video.pixels().contains(&255));
        assert!(video.pixels().contains(&0));
    }

    #[test]
    fn popup_windows_clear_underlying_text_and_use_rom_borders() {
        let mut cells = [b'X'; COLUMNS * ROWS];
        let mut foregrounds = [ASM_ERROR_COLOR; COLUMNS * ROWS];
        let mut backgrounds = [ASM_ERROR_COLOR; COLUMNS * ROWS];
        let mut inverse = [true; COLUMNS * ROWS];

        draw_window(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut inverse,
            CellRect { x: 3, y: 4, width: 12, height: 6 },
            CellStyle { foreground: UI_WHITE_COLOR, background: UI_ERROR_BACKGROUND },
        );

        assert_eq!(cells[4 * COLUMNS + 3], BOX_TOP_LEFT);
        assert_eq!(cells[4 * COLUMNS + 14], BOX_TOP_RIGHT);
        assert_eq!(cells[9 * COLUMNS + 3], BOX_BOTTOM_LEFT);
        assert_eq!(cells[9 * COLUMNS + 14], BOX_BOTTOM_RIGHT);
        assert_eq!(cells[6 * COLUMNS + 8], b' ');
        assert_eq!(foregrounds[6 * COLUMNS + 8], UI_WHITE_COLOR);
        assert_eq!(backgrounds[6 * COLUMNS + 8], UI_ERROR_BACKGROUND);
        assert!(!inverse[6 * COLUMNS + 8]);
    }

    #[test]
    fn mac_option_shortcuts_use_physical_letter_keys() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.handle_key(&key("ƒ"), PhysicalKey::Code(KeyCode::KeyF), ModifiersState::ALT);
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, .. }));
    }

    #[test]
    fn escape_does_not_exit_editor_and_open_menu_accepts_hotkeys() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::Escape),
                PhysicalKey::Code(KeyCode::Escape),
                ModifiersState::empty(),
            ),
            EditorAction::None
        );
        editor.open_menu(MenuKind::File);
        editor.handle_key(&key("o"), PhysicalKey::Code(KeyCode::KeyO), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::Dialog { kind: DialogKind::Open, .. }));
    }

    #[test]
    fn asm_files_use_merlin_columns_when_opened_and_saved() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "START LDA #$01 ; VALUE\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem.clone(), shared_ui_colors(), Some("CODE.ASM".to_owned()));
        assert_eq!(editor.lines[0], "START    LDA   #$01       ; VALUE");
        assert_eq!(editor.lines[1], "         RTS");

        editor.save_as("defs.inc").unwrap();
        assert!(editor.assembly_mode());
        assert_eq!(filesystem.borrow().read_text("DEFS.INC").unwrap(), editor.lines.join("\n"));
    }

    #[test]
    fn asm_highlighting_colors_each_source_category() {
        let line = "START    LDA   #$01       ; VALUE";
        let colors = assembly_syntax_colors(line, 255);
        assert_eq!(colors[0], ASM_LABEL_COLOR);
        assert_eq!(colors[9], ASM_OPCODE_COLOR);
        assert_eq!(colors[15], ASM_NUMBER_COLOR);
        assert_eq!(colors[26], ASM_COMMENT_COLOR);

        let directive = assembly_syntax_colors("         ORG   $8000", 255);
        assert_eq!(directive[9], ASM_DIRECTIVE_COLOR);
        assert_eq!(directive[15], ASM_NUMBER_COLOR);

        let string = assembly_syntax_colors("         ASC   \"HELLO\"", ASM_TEXT_COLOR);
        assert_eq!(string[15], ASM_STRING_COLOR);
    }

    #[test]
    fn semicolons_inside_assembly_strings_are_not_comments() {
        assert_eq!(
            format_assembly_line(" msg asc \"hello;world\" ; real comment"),
            "         msg   asc \"hello;world\" ; real comment"
        );
    }

    #[test]
    fn asm_tab_and_enter_follow_merlin_fields() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.insert_text("START");
        editor.insert_tab();
        editor.insert_text("LDA #$20");
        editor.insert_newline();
        assert_eq!(editor.lines[0], "START    LDA   #$20");
        assert_eq!(editor.cursor, Position { line: 1, column: 0 });
    }

    #[test]
    fn leaving_an_asm_line_formats_it_except_while_selecting() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "\n").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.lines = vec!["START LDA #$20 ; VALUE".to_owned(), String::new()];

        editor.move_vertical(1, true);
        assert_eq!(editor.lines[0], "START LDA #$20 ; VALUE");

        editor.selection_anchor = None;
        editor.cursor = Position::default();
        editor.move_vertical(1, false);
        assert_eq!(editor.lines[0], "START    LDA   #$20       ; VALUE");
        assert!(editor.dirty);
    }

    #[test]
    fn asm_palette_categories_reach_rendered_pixels() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "START LDA #$01 ; VALUE").unwrap();
        let editor = TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, false);
        assert_eq!(video.palette()[ASM_TEXT_COLOR as usize], [205, 214, 244, 255]);
        assert_eq!(video.palette()[ASM_COMMENT_COLOR as usize], [127, 132, 156, 255]);
        assert_eq!(video.palette()[ASM_ERROR_COLOR as usize], [243, 139, 168, 255]);
        assert!(video.pixels().contains(&ASM_LABEL_COLOR));
        assert!(video.pixels().contains(&ASM_OPCODE_COLOR));
        assert!(video.pixels().contains(&ASM_NUMBER_COLOR));
        assert!(video.pixels().contains(&ASM_COMMENT_COLOR));
    }

    #[test]
    fn build_assembles_the_unsaved_editor_buffer_to_bin() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", " ORG $8000\n NOP").unwrap();
        let mut editor =
            TextEditor::new(filesystem.clone(), shared_ui_colors(), Some("code.asm".to_owned()));
        editor.lines = vec!["         ORG   $8000".to_owned(), "         LDA   #$42".to_owned()];
        editor.dirty = true;

        editor.start_build();
        assert!(matches!(editor.overlay, Overlay::Building { .. }));
        finish_pending_build(&mut editor);

        assert_eq!(filesystem.borrow().read_binary("code.bin").unwrap(), [0xa9, 0x42]);
        assert!(editor.diagnostics.is_empty());
        assert!(editor.build_message.as_deref().is_some_and(|message| message.contains("BUILT")));
        assert!(matches!(
            editor.overlay,
            Overlay::Message { ref title, .. } if title == "BUILD SUCCESSFUL"
        ));
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, false);
        assert!(video.pixels().contains(&UI_WHITE_COLOR));
        assert!(video.pixels().contains(&UI_SUCCESS_BACKGROUND));
    }

    #[test]
    fn build_errors_select_the_source_location_and_edits_clear_them() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("bad.asm", " ORG $8000\n LDA #").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("bad.asm".to_owned()));

        editor.start_build();
        finish_pending_build(&mut editor);

        assert!(!editor.diagnostics.is_empty());
        assert_eq!(editor.cursor.line, 1);
        assert!(editor.line_has_error(1));
        assert!(matches!(
            editor.overlay,
            Overlay::Message { ref title, .. } if title == "BUILD ERRORS"
        ));
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, false);
        assert!(video.pixels().contains(&UI_WHITE_COLOR));
        assert!(video.pixels().contains(&UI_ERROR_BACKGROUND));
        editor.overlay = Overlay::None;
        editor.insert_text("1");
        assert!(editor.diagnostics.is_empty());
    }
}
