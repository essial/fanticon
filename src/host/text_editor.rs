use fanticon::video::{DISPLAY_HEIGHT, DISPLAY_WIDTH, Video};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::{
    character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH},
    filesystem::SharedFilesystem,
    ui_colors::SharedUiColors,
};

const COLUMNS: usize = DISPLAY_WIDTH / GLYPH_WIDTH;
const ROWS: usize = DISPLAY_HEIGHT / GLYPH_HEIGHT;
const TEXT_ROWS: usize = ROWS - 2;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
struct Position {
    line: usize,
    column: usize,
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
            return self.handle_overlay_key(key);
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
                _ => EditorAction::None,
            };
        }

        if modifiers.alt_key() {
            match physical_key {
                PhysicalKey::Code(KeyCode::KeyF) => self.open_menu(MenuKind::File),
                PhysicalKey::Code(KeyCode::KeyE) => self.open_menu(MenuKind::Edit),
                _ => {}
            }
            return EditorAction::None;
        }

        match key {
            Key::Named(NamedKey::F10) => self.open_menu(MenuKind::File),
            Key::Named(NamedKey::F2) => self.save_or_prompt(),
            Key::Named(NamedKey::F3) => self.open_dialog(DialogKind::Open),
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
            Key::Named(NamedKey::Tab) => self.insert_text("    "),
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

    pub fn render(&self, video: &mut Video, cursor_visible: bool) {
        let colors = self.colors.get();
        let mut cells = [b' '; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];

        put_text(&mut cells, 0, 0, " FILE  EDIT");
        inverse[..COLUMNS].fill(true);

        for screen_y in 0..TEXT_ROWS {
            let line_index = self.scroll_line + screen_y;
            let Some(line) = self.lines.get(line_index) else { break };
            for (screen_x, byte) in line.bytes().skip(self.scroll_column).take(COLUMNS).enumerate()
            {
                let index = (screen_y + 1) * COLUMNS + screen_x;
                cells[index] = byte.to_ascii_uppercase();
                let position = Position { line: line_index, column: self.scroll_column + screen_x };
                inverse[index] = self.position_selected(position);
            }
        }

        let name = self.filename.as_deref().unwrap_or("UNTITLED.TXT");
        let dirty = if self.dirty { "*" } else { " " };
        let status =
            format!(" {name}{dirty}  LN {} COL {}", self.cursor.line + 1, self.cursor.column + 1);
        put_text(&mut cells, 0, ROWS - 1, &status);
        inverse[(ROWS - 1) * COLUMNS..].fill(true);

        self.render_overlay(&mut cells, &mut inverse);
        render_cells(video, &cells, &inverse, colors.background, colors.foreground);

        if cursor_visible && matches!(self.overlay, Overlay::None) {
            let x = self.cursor.column.saturating_sub(self.scroll_column) * GLYPH_WIDTH + 1;
            let y = (self.cursor.line.saturating_sub(self.scroll_line) + 1) * GLYPH_HEIGHT
                + GLYPH_HEIGHT
                - 1;
            if x < DISPLAY_WIDTH && y < DISPLAY_HEIGHT - GLYPH_HEIGHT {
                for pixel_x in x..(x + 5).min(DISPLAY_WIDTH) {
                    video.pixels_mut()[y * DISPLAY_WIDTH + pixel_x] = colors.foreground;
                }
            }
        }
    }

    fn handle_overlay_key(&mut self, key: &Key) -> EditorAction {
        match &mut self.overlay {
            Overlay::Menu { menu, selected } => {
                let count = menu_items(*menu).len();
                match key {
                    Key::Named(NamedKey::Escape) | Key::Named(NamedKey::F10) => {
                        self.overlay = Overlay::None;
                    }
                    Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                        *menu =
                            if *menu == MenuKind::File { MenuKind::Edit } else { MenuKind::File };
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
            Overlay::None => {}
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
            _ => {}
        }
        EditorAction::None
    }

    fn render_overlay(&self, cells: &mut [u8], inverse: &mut [bool]) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Menu { menu, selected } => {
                let x = if *menu == MenuKind::File { 0 } else { 6 };
                let width = 14;
                for (index, item) in menu_labels(*menu).iter().enumerate() {
                    put_text(cells, x, index + 1, &format!(" {item:<12}"));
                    if index == *selected {
                        inverse[(index + 1) * COLUMNS + x..(index + 1) * COLUMNS + x + width]
                            .fill(true);
                    }
                }
            }
            Overlay::Dialog { kind, input, error } => {
                let title = if *kind == DialogKind::Open { "OPEN FILE" } else { "SAVE FILE" };
                let x = 5;
                let y = 9;
                let width = 30;
                for row in y..y + 6 {
                    inverse[row * COLUMNS + x..row * COLUMNS + x + width].fill(true);
                }
                put_text(cells, x + 2, y, title);
                put_text(cells, x + 2, y + 2, "NAME:");
                put_text(cells, x + 8, y + 2, input);
                put_text(cells, x + 2, y + 4, "ENTER=OK  ESC=CANCEL");
                if let Some(error) = error {
                    put_text(cells, x + 2, y + 3, &format!("?{error}"));
                }
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

    fn save_as(&mut self, filename: &str) -> Result<(), String> {
        let text = self.lines.join("\n");
        self.filesystem.borrow_mut().write_text(filename, &text)?;
        self.filename = Some(filename.to_ascii_lowercase());
        self.dirty = false;
        Ok(())
    }

    fn load(&mut self, filename: &str) -> Result<(), String> {
        let text = self.filesystem.borrow().read_text(filename)?;
        self.lines =
            text.replace("\r\n", "\n").replace('\r', "\n").split('\n').map(str::to_owned).collect();
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
        Ok(())
    }

    fn new_document(&mut self) {
        self.record_undo();
        self.lines = vec![String::new()];
        self.cursor = Position::default();
        self.selection_anchor = None;
        self.filename = None;
        self.dirty = false;
    }

    fn record_undo(&mut self) {
        if self.undo.len() == 64 {
            self.undo.remove(0);
        }
        self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
    }

    fn undo(&mut self) {
        if let Some(snapshot) = self.undo.pop() {
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
        let remainder = self.lines[self.cursor.line].split_off(self.cursor.column);
        self.cursor.line += 1;
        self.cursor.column = 0;
        self.lines.insert(self.cursor.line, remainder);
        self.dirty = true;
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
        if selecting {
            self.selection_anchor.get_or_insert(self.cursor);
        } else {
            self.selection_anchor = None;
        }
        movement(self);
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
}

fn menu_items(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => &["NEW", "OPEN...", "SAVE", "SAVE AS...", "EXIT"],
        MenuKind::Edit => &["UNDO", "CUT", "COPY", "PASTE", "SELECT ALL"],
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
    }
}

fn menu_hotkey(menu: MenuKind, key: &str) -> Option<usize> {
    let key = key.to_ascii_lowercase();
    let hotkeys = match menu {
        MenuKind::File => ["n", "o", "s", "a", "x"],
        MenuKind::Edit => ["u", "t", "c", "p", "a"],
    };
    hotkeys.iter().position(|hotkey| *hotkey == key)
}

fn put_text(cells: &mut [u8], x: usize, y: usize, text: &str) {
    if y >= ROWS {
        return;
    }
    for (offset, byte) in text.bytes().take(COLUMNS.saturating_sub(x)).enumerate() {
        cells[y * COLUMNS + x + offset] = byte.to_ascii_uppercase();
    }
}

fn render_cells(video: &mut Video, cells: &[u8], inverse: &[bool], background: u8, foreground: u8) {
    let pixels = video.pixels_mut();
    pixels.fill(background);
    for cell_y in 0..ROWS {
        for cell_x in 0..COLUMNS {
            let index = cell_y * COLUMNS + cell_x;
            let (cell_foreground, cell_background) =
                if inverse[index] { (background, foreground) } else { (foreground, background) };
            if cell_background != background {
                for y in cell_y * GLYPH_HEIGHT..(cell_y + 1) * GLYPH_HEIGHT {
                    pixels[y * DISPLAY_WIDTH + cell_x * GLYPH_WIDTH
                        ..y * DISPLAY_WIDTH + (cell_x + 1) * GLYPH_WIDTH]
                        .fill(cell_background);
                }
            }
            let glyph = CHARACTER_ROM[(cells[index] as usize).min(CHARACTER_ROM.len() - 1)];
            for (glyph_y, bits) in glyph.into_iter().enumerate() {
                for glyph_x in 0..GLYPH_WIDTH {
                    if bits & (0x80 >> glyph_x) != 0 {
                        let x = cell_x * GLYPH_WIDTH + glyph_x;
                        let y = cell_y * GLYPH_HEIGHT + glyph_y;
                        pixels[y * DISPLAY_WIDTH + x] = cell_foreground;
                    }
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::filesystem::shared_filesystem;
    use crate::host::ui_colors::shared_ui_colors;

    fn key(character: &str) -> Key {
        Key::Character(character.into())
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
        let mut video = Video::new();
        editor.render(&mut video, true);
        assert!(video.pixels().contains(&255));
        assert!(video.pixels().contains(&0));
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
}
