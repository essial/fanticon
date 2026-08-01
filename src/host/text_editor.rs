use std::collections::BTreeSet;

use fanticon::{
    assembler::{CartridgeSourceMapEntry, Diagnostic, SymbolSection},
    debugger::DebugSnapshot,
    disassemble_instruction,
    machine::{VIDEO_DOTS_PER_CPU_CYCLE, bank_kind},
    project::MANIFEST_NAME,
    video::{DOTS_PER_SCANLINE, SCANLINES_PER_FRAME, Video},
};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::nsf_player::{MusicCommand, MusicStatus};
use super::{
    EDITOR_DISPLAY_HEIGHT, EDITOR_DISPLAY_WIDTH,
    builder::{GameLaunch, build_and_load_project, build_project, build_source},
    character_rom::{
        BOX_BOTTOM_LEFT, BOX_BOTTOM_RIGHT, BOX_HORIZONTAL, BOX_TOP_LEFT, BOX_TOP_RIGHT,
        BOX_VERTICAL, CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH, SYMBOL_ARROW_RIGHT, SYMBOL_BUSY,
        SYMBOL_CHECK, SYMBOL_CROSS, configure_text_gradient, gradient_color,
    },
    filesystem::{ConsoleFilesystem, SharedFilesystem},
    ui_colors::SharedUiColors,
};

const COLUMNS: usize = EDITOR_DISPLAY_WIDTH / GLYPH_WIDTH;
const ROWS: usize = EDITOR_DISPLAY_HEIGHT / GLYPH_HEIGHT;
const EDITOR_FIRST_ROW: usize = 2;
const TEXT_ROWS: usize = ROWS - EDITOR_FIRST_ROW - 1;
const PROJECT_WIDTH: usize = 20;
const EDITOR_START: usize = PROJECT_WIDTH + 1;
const EDITOR_CODE_START: usize = EDITOR_START + 2;
const EDITOR_COLUMNS: usize = COLUMNS - EDITOR_CODE_START;
const TAB_WIDTH: usize = 14;
const VISIBLE_TABS: usize = (EDITOR_COLUMNS - 2) / TAB_WIDTH;
const SEARCH_RESULTS_X: usize = 2;
const SEARCH_RESULTS_Y: usize = 3;
const SEARCH_RESULTS_WIDTH: usize = COLUMNS - 4;
const SEARCH_RESULTS_HEIGHT: usize = ROWS - 6;
const SEARCH_RESULTS_VISIBLE: usize = SEARCH_RESULTS_HEIGHT - 5;
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
const UI_DEBUG_CURRENT_BACKGROUND: u8 = 251;
const UI_BREAKPOINT_BACKGROUND: u8 = 252;
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

#[derive(Clone)]
struct DocumentState {
    id: u32,
    lines: Vec<String>,
    cursor: Position,
    selection_anchor: Option<Position>,
    scroll_line: usize,
    scroll_column: usize,
    filename: Option<String>,
    undo: Vec<Snapshot>,
    dirty: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct ProjectEntry {
    name: String,
    path: String,
    depth: usize,
    is_directory: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum MenuKind {
    File,
    Edit,
    Build,
    Debug,
    Music,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DialogKind {
    Open,
    SaveAs,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchMode {
    Find,
    Replace,
    Project,
    GoToLine,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum SearchField {
    Query,
    Replacement,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum DebugPromptKind {
    ReadWatchpoint,
    WriteWatchpoint,
    RasterBreakpoint,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct SearchResult {
    path: String,
    line: usize,
    column: usize,
    length: usize,
    preview: String,
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Location {
    document_id: u32,
    filename: Option<String>,
    position: Position,
}

enum Overlay {
    None,
    Menu {
        menu: MenuKind,
        selected: usize,
    },
    Dialog {
        kind: DialogKind,
        input: String,
        error: Option<String>,
    },
    Building {
        frames_remaining: u8,
    },
    Message {
        title: String,
        lines: Vec<String>,
    },
    CloseTab {
        tab: usize,
    },
    SearchPrompt {
        mode: SearchMode,
        query: String,
        replacement: String,
        field: SearchField,
        error: Option<String>,
    },
    SearchResults {
        query: String,
        results: Vec<SearchResult>,
        selected: usize,
        scroll: usize,
    },
    DebugPrompt {
        kind: DebugPromptKind,
        input: String,
        error: Option<String>,
    },
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum EditorAction {
    None,
    Exit,
    Run(GameLaunch),
    Debug(DebugCommand),
    Music(MusicCommand),
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum DebugCommand {
    Continue,
    Stop,
    StepInstruction,
    StepCycle,
    StepOver,
    StepOut,
    SyncBreakpoints(Vec<(SymbolSection, u16)>),
    AddReadWatchpoint(u16),
    AddWriteWatchpoint(u16),
    AddRasterBreakpoint { dot: u16, line: u16 },
    ClearBreakpoints,
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
    build_and_run: bool,
    pending_launch: Option<GameLaunch>,
    mouse_selecting: bool,
    wheel_remainder: (f64, f64),
    project_entries: Vec<ProjectEntry>,
    project_selected: usize,
    project_scroll: usize,
    project_focused: bool,
    expanded_directories: BTreeSet<String>,
    tabs: Vec<DocumentState>,
    active_tab: usize,
    tab_scroll: usize,
    document_id: u32,
    next_document_id: u32,
    close_after_save: Option<u32>,
    last_search: String,
    navigation_back: Vec<Location>,
    navigation_forward: Vec<Location>,
    source_breakpoints: BTreeSet<(String, usize)>,
    debug_source_map: Vec<CartridgeSourceMapEntry>,
    debug_snapshot: Option<DebugSnapshot>,
    debug_active: bool,
    debug_location: Option<(String, usize)>,
    music_status: Option<MusicStatus>,
    music_marquee_frame: u8,
    music_marquee_offset: usize,
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
            build_and_run: false,
            pending_launch: None,
            mouse_selecting: false,
            wheel_remainder: (0.0, 0.0),
            project_entries: Vec::new(),
            project_selected: 0,
            project_scroll: 0,
            project_focused: false,
            expanded_directories: BTreeSet::new(),
            tabs: Vec::new(),
            active_tab: 0,
            tab_scroll: 0,
            document_id: 1,
            next_document_id: 2,
            close_after_save: None,
            last_search: String::new(),
            navigation_back: Vec::new(),
            navigation_forward: Vec::new(),
            source_breakpoints: BTreeSet::new(),
            debug_source_map: Vec::new(),
            debug_snapshot: None,
            debug_active: false,
            debug_location: None,
            music_status: None,
            music_marquee_frame: 0,
            music_marquee_offset: 0,
        };
        if let Some(filename) = filename
            && let Err(error) = editor.load(&filename)
        {
            editor.overlay =
                Overlay::Dialog { kind: DialogKind::Open, input: filename, error: Some(error) };
        }
        editor.tabs.push(editor.capture_document());
        editor.refresh_project_browser();
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
            && matches!(key, Key::Named(NamedKey::Tab))
        {
            self.cycle_tabs(!modifiers.shift_key());
            return EditorAction::None;
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
                    if modifiers.shift_key() {
                        self.save_all();
                    } else {
                        self.save_or_prompt();
                    }
                    EditorAction::None
                }
                "w" => {
                    self.request_close_tab(self.active_tab);
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
                    self.start_build(false);
                    EditorAction::None
                }
                "f" => {
                    self.open_search_prompt(if modifiers.shift_key() {
                        SearchMode::Project
                    } else {
                        SearchMode::Find
                    });
                    EditorAction::None
                }
                "h" => {
                    self.open_search_prompt(SearchMode::Replace);
                    EditorAction::None
                }
                "g" => {
                    self.open_search_prompt(SearchMode::GoToLine);
                    EditorAction::None
                }
                _ => EditorAction::None,
            };
        }

        if modifiers.alt_key() && matches!(key, Key::Named(NamedKey::ArrowLeft)) {
            self.navigate_history(false);
            return EditorAction::None;
        }
        if modifiers.alt_key() && matches!(key, Key::Named(NamedKey::ArrowRight)) {
            self.navigate_history(true);
            return EditorAction::None;
        }

        if matches!(key, Key::Named(NamedKey::F5)) {
            if modifiers.shift_key() && self.debug_active {
                return EditorAction::Debug(DebugCommand::Stop);
            }
            if self.debug_active && self.debug_snapshot.is_some() {
                self.debug_snapshot = None;
                return EditorAction::Debug(DebugCommand::Continue);
            }
            self.start_build(true);
            return EditorAction::None;
        }
        if matches!(key, Key::Named(NamedKey::F7)) {
            let Some(status) = &self.music_status else { return EditorAction::None };
            if modifiers.shift_key() {
                return EditorAction::Music(MusicCommand::Stop);
            }
            return EditorAction::Music(if status.paused {
                MusicCommand::Play
            } else {
                MusicCommand::Pause
            });
        }
        if matches!(key, Key::Named(NamedKey::F8)) && self.music_status.is_some() {
            return EditorAction::Music(if modifiers.control_key() || modifiers.super_key() {
                MusicCommand::ToggleLoop
            } else if modifiers.shift_key() {
                MusicCommand::Previous
            } else {
                MusicCommand::Next
            });
        }
        if matches!(key, Key::Named(NamedKey::F9)) {
            return self.toggle_source_breakpoint();
        }
        if self.debug_active
            && self.debug_snapshot.is_some()
            && matches!(key, Key::Named(NamedKey::F10))
        {
            return EditorAction::Debug(DebugCommand::StepOver);
        }
        if self.debug_active
            && self.debug_snapshot.is_some()
            && matches!(key, Key::Named(NamedKey::F11))
        {
            return EditorAction::Debug(if modifiers.control_key() || modifiers.super_key() {
                DebugCommand::StepCycle
            } else if modifiers.shift_key() {
                DebugCommand::StepOut
            } else {
                DebugCommand::StepInstruction
            });
        }

        if matches!(key, Key::Named(NamedKey::F6)) {
            self.project_focused = !self.project_focused;
            return EditorAction::None;
        }

        if self.project_focused {
            return self.handle_project_key(key);
        }

        if modifiers.alt_key() {
            match physical_key {
                PhysicalKey::Code(KeyCode::KeyF) => self.open_menu(MenuKind::File),
                PhysicalKey::Code(KeyCode::KeyE) => self.open_menu(MenuKind::Edit),
                PhysicalKey::Code(KeyCode::KeyB) => self.open_menu(MenuKind::Build),
                PhysicalKey::Code(KeyCode::KeyD) => self.open_menu(MenuKind::Debug),
                PhysicalKey::Code(KeyCode::KeyM) => self.open_menu(MenuKind::Music),
                _ => {}
            }
            return EditorAction::None;
        }

        match key {
            Key::Named(NamedKey::F10) => self.open_menu(MenuKind::File),
            Key::Named(NamedKey::F2) => self.save_or_prompt(),
            Key::Named(NamedKey::F3) => self.find_next(!modifiers.shift_key()),
            Key::Named(NamedKey::F4) => self.move_diagnostic(!modifiers.shift_key()),
            Key::Named(NamedKey::F12) => {
                if modifiers.shift_key() {
                    self.find_symbol_references();
                } else {
                    self.goto_symbol_definition();
                }
            }
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

    pub fn update(&mut self) -> EditorAction {
        if self.music_status.is_some() {
            self.music_marquee_frame += 1;
            if self.music_marquee_frame >= 20 {
                self.music_marquee_frame = 0;
                self.music_marquee_offset = self.music_marquee_offset.wrapping_add(1);
            }
        }
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
        self.pending_launch.take().map_or(EditorAction::None, EditorAction::Run)
    }

    pub fn set_debug_snapshot(&mut self, snapshot: DebugSnapshot) {
        self.debug_active = true;
        let (section, address) = if snapshot.instruction_boundary {
            (execution_section(&snapshot), snapshot.pc)
        } else {
            snapshot.trace.last().map_or((execution_section(&snapshot), snapshot.pc), |trace| {
                (trace.section, trace.address)
            })
        };
        let location = section.and_then(|section| {
            self.debug_source_map
                .iter()
                .find(|entry| {
                    entry.section == section
                        && address >= entry.address
                        && usize::from(address - entry.address) < entry.length
                })
                .cloned()
        });
        self.debug_snapshot = Some(snapshot);
        self.debug_location =
            location.as_ref().map(|entry| (entry.source.clone(), entry.line.saturating_sub(1)));
        if let Some(location) = location {
            if !self
                .filename
                .as_deref()
                .is_some_and(|filename| filename.eq_ignore_ascii_case(&location.source))
            {
                let _ = self.load(&location.source);
            }
            self.cursor.line = location.line.saturating_sub(1).min(self.lines.len() - 1);
            self.cursor.column = 0;
            self.selection_anchor = None;
            self.ensure_cursor_visible();
        }
        self.overlay = Overlay::None;
    }

    pub fn set_music_status(&mut self, status: Option<MusicStatus>) {
        let changed = self
            .music_status
            .as_ref()
            .map(|current| (&current.filename, &current.title, current.track))
            != status.as_ref().map(|current| (&current.filename, &current.title, current.track));
        if changed {
            self.music_marquee_frame = 0;
            self.music_marquee_offset = 0;
        }
        self.music_status = status;
    }

    pub fn stop_debug_session(&mut self) {
        self.debug_active = false;
        self.debug_snapshot = None;
        self.debug_source_map.clear();
        self.debug_location = None;
    }

    fn toggle_source_breakpoint(&mut self) -> EditorAction {
        let Some(source) = self.filename.clone().filter(|filename| assembly_filename(filename))
        else {
            self.show_build_message("BREAKPOINT", &["OPEN AN ASM OR INC FILE".to_owned()]);
            return EditorAction::None;
        };
        let breakpoint = (source, self.cursor.line);
        if !self.source_breakpoints.insert(breakpoint.clone()) {
            self.source_breakpoints.remove(&breakpoint);
        }
        if self.debug_active {
            EditorAction::Debug(DebugCommand::SyncBreakpoints(
                self.resolved_source_breakpoints(&self.debug_source_map),
            ))
        } else {
            EditorAction::None
        }
    }

    fn resolved_source_breakpoints(
        &self,
        source_map: &[CartridgeSourceMapEntry],
    ) -> Vec<(SymbolSection, u16)> {
        let mut resolved = BTreeSet::new();
        for (source, line) in &self.source_breakpoints {
            let requested = *line + 1;
            let next_line = source_map
                .iter()
                .filter(|entry| {
                    entry.source.eq_ignore_ascii_case(source) && entry.line >= requested
                })
                .map(|entry| entry.line)
                .min();
            if let Some(next_line) = next_line {
                resolved.extend(
                    source_map
                        .iter()
                        .filter(|entry| {
                            entry.source.eq_ignore_ascii_case(source) && entry.line == next_line
                        })
                        .map(|entry| (entry.section, entry.address)),
                );
            }
        }
        resolved.into_iter().collect()
    }

    fn line_has_breakpoint(&self, line: usize) -> bool {
        self.filename.as_deref().is_some_and(|filename| {
            self.source_breakpoints.iter().any(|(source, breakpoint_line)| {
                *breakpoint_line == line && source.eq_ignore_ascii_case(filename)
            })
        })
    }

    fn open_search_prompt(&mut self, mode: SearchMode) {
        let query = if mode == SearchMode::GoToLine {
            (self.cursor.line + 1).to_string()
        } else if let Some(selected) = self.selected_text().filter(|text| !text.contains('\n')) {
            selected
        } else {
            self.last_search.clone()
        };
        self.overlay = Overlay::SearchPrompt {
            mode,
            query,
            replacement: String::new(),
            field: SearchField::Query,
            error: None,
        };
    }

    fn submit_search_prompt(&mut self, mode: SearchMode, query: String, replacement: String) {
        if query.is_empty() {
            self.overlay = Overlay::SearchPrompt {
                mode,
                query,
                replacement,
                field: SearchField::Query,
                error: Some("ENTER SEARCH TEXT".to_owned()),
            };
            return;
        }
        match mode {
            SearchMode::Find => {
                self.last_search = query;
                self.overlay = Overlay::None;
                self.find_next(true);
            }
            SearchMode::Replace => {
                self.last_search = query.clone();
                self.overlay = Overlay::None;
                self.replace_next(&query, &replacement);
            }
            SearchMode::Project => self.show_project_search(&query),
            SearchMode::GoToLine => match query.parse::<usize>() {
                Ok(line) if line > 0 && line <= self.lines.len() => {
                    self.push_navigation_origin();
                    self.cursor = Position { line: line - 1, column: 0 };
                    self.selection_anchor = None;
                    self.ensure_cursor_visible();
                    self.overlay = Overlay::None;
                }
                _ => {
                    self.overlay = Overlay::SearchPrompt {
                        mode,
                        query,
                        replacement,
                        field: SearchField::Query,
                        error: Some(format!("LINE MUST BE 1-{}", self.lines.len())),
                    };
                }
            },
        }
    }

    fn find_next(&mut self, forward: bool) {
        if self.last_search.is_empty() {
            self.open_search_prompt(SearchMode::Find);
            return;
        }
        let matches = line_matches(&self.lines, &self.last_search, false);
        if matches.is_empty() {
            self.show_build_message("FIND", &[format!("NOT FOUND: {}", self.last_search)]);
            return;
        }
        let boundary = if forward {
            self.selection().map_or(self.cursor, |(_, end)| end)
        } else {
            self.selection().map_or(self.cursor, |(start, _)| start)
        };
        let found = if forward {
            matches.iter().copied().find(|position| *position >= boundary).unwrap_or(matches[0])
        } else {
            matches
                .iter()
                .rev()
                .copied()
                .find(|position| *position < boundary)
                .unwrap_or_else(|| *matches.last().expect("non-empty matches"))
        };
        self.select_search_match(found, self.last_search.len());
    }

    fn select_search_match(&mut self, start: Position, length: usize) {
        self.selection_anchor = Some(start);
        self.cursor = Position { line: start.line, column: start.column + length };
        self.ensure_cursor_visible();
    }

    fn replace_next(&mut self, query: &str, replacement: &str) {
        let selection_matches =
            self.selected_text().is_some_and(|selected| selected.eq_ignore_ascii_case(query));
        if !selection_matches {
            self.find_next(true);
        }
        if self.selected_text().is_some_and(|selected| selected.eq_ignore_ascii_case(query)) {
            self.insert_text(replacement);
            self.find_next(true);
        }
    }

    fn replace_all(&mut self, query: &str, replacement: &str) -> usize {
        if query.is_empty() {
            return 0;
        }
        let matches = line_matches(&self.lines, query, false);
        if matches.is_empty() {
            return 0;
        }
        self.record_undo();
        for position in matches.iter().rev() {
            self.lines[position.line]
                .replace_range(position.column..position.column + query.len(), replacement);
        }
        self.selection_anchor = None;
        self.dirty = true;
        self.clamp_cursor();
        matches.len()
    }

    fn show_project_search(&mut self, query: &str) {
        self.last_search = query.to_owned();
        let results = self.search_project(query, false, false);
        if results.is_empty() {
            self.show_build_message("PROJECT SEARCH", &[format!("NOT FOUND: {query}")]);
        } else {
            self.overlay =
                Overlay::SearchResults { query: query.to_owned(), results, selected: 0, scroll: 0 };
        }
    }

    fn search_project(
        &mut self,
        query: &str,
        assembly_only: bool,
        definitions_only: bool,
    ) -> Vec<SearchResult> {
        self.sync_active_document();
        let mut paths = Vec::new();
        collect_project_files(&self.filesystem.borrow(), "/", &mut paths);
        let mut results = Vec::new();
        for path in paths {
            if assembly_only && !assembly_filename(&path) {
                continue;
            }
            let lines = if let Some(document) = self.tabs.iter().find(|document| {
                document
                    .filename
                    .as_deref()
                    .is_some_and(|filename| filename.eq_ignore_ascii_case(&path))
            }) {
                document.lines.clone()
            } else {
                let Ok(text) = self.filesystem.borrow().read_text(&path) else { continue };
                normalized_lines(&text)
            };
            for (line_index, line) in lines.iter().enumerate() {
                if definitions_only {
                    if assembly_definition(line)
                        .is_some_and(|symbol| symbol.eq_ignore_ascii_case(query))
                    {
                        let column = line
                            .find(|character: char| !character.is_ascii_whitespace())
                            .unwrap_or(0);
                        results.push(SearchResult {
                            path: path.clone(),
                            line: line_index,
                            column,
                            length: query.len(),
                            preview: search_preview(line),
                        });
                    }
                    continue;
                }
                for position in text_matches(line, query, assembly_only) {
                    results.push(SearchResult {
                        path: path.clone(),
                        line: line_index,
                        column: position,
                        length: query.len(),
                        preview: search_preview(line),
                    });
                }
            }
        }
        results
    }

    fn word_under_cursor(&self) -> Option<String> {
        let line = self.lines.get(self.cursor.line)?;
        symbol_at(line, self.cursor.column).map(str::to_owned)
    }

    fn goto_symbol_definition(&mut self) {
        let Some(symbol) = self.word_under_cursor() else {
            self.show_build_message("DEFINITION", &["NO SYMBOL AT CURSOR".to_owned()]);
            return;
        };
        let results = self.search_project(&symbol, true, true);
        let Some(result) = results.first().cloned() else {
            self.show_build_message("DEFINITION", &[format!("NOT FOUND: {symbol}")]);
            return;
        };
        self.navigate_to_result(&result, true);
    }

    fn find_symbol_references(&mut self) {
        let Some(symbol) = self.word_under_cursor() else {
            self.show_build_message("REFERENCES", &["NO SYMBOL AT CURSOR".to_owned()]);
            return;
        };
        let results = self.search_project(&symbol, true, false);
        if results.is_empty() {
            self.show_build_message("REFERENCES", &[format!("NOT FOUND: {symbol}")]);
        } else {
            self.overlay =
                Overlay::SearchResults { query: symbol, results, selected: 0, scroll: 0 };
        }
    }

    fn current_location(&self) -> Location {
        Location {
            document_id: self.document_id,
            filename: self.filename.clone(),
            position: self.cursor,
        }
    }

    fn push_navigation_origin(&mut self) {
        let location = self.current_location();
        if self.navigation_back.last() != Some(&location) {
            self.navigation_back.push(location);
            if self.navigation_back.len() > 64 {
                self.navigation_back.remove(0);
            }
        }
        self.navigation_forward.clear();
    }

    fn navigate_to_result(&mut self, result: &SearchResult, record_history: bool) {
        if record_history {
            self.push_navigation_origin();
        }
        if !self
            .filename
            .as_deref()
            .is_some_and(|filename| filename.eq_ignore_ascii_case(&result.path))
            && self.load(&result.path).is_err()
        {
            return;
        }
        let line = result.line.min(self.lines.len() - 1);
        let column = result.column.min(self.lines[line].len());
        let length = result.length.min(self.lines[line].len() - column);
        self.select_search_match(Position { line, column }, length);
        self.overlay = Overlay::None;
    }

    fn navigate_history(&mut self, forward: bool) {
        let destination =
            if forward { self.navigation_forward.pop() } else { self.navigation_back.pop() };
        let Some(destination) = destination else { return };
        let current = self.current_location();
        if forward {
            self.navigation_back.push(current);
        } else {
            self.navigation_forward.push(current);
        }
        self.goto_location(destination);
    }

    fn goto_location(&mut self, location: Location) {
        if let Some(tab) = self.tabs.iter().position(|document| document.id == location.document_id)
        {
            self.switch_tab(tab);
        } else if let Some(filename) = &location.filename
            && self.load(filename).is_err()
        {
            return;
        }
        self.cursor.line = location.position.line.min(self.lines.len() - 1);
        self.cursor.column = location.position.column.min(self.lines[self.cursor.line].len());
        self.selection_anchor = None;
        self.ensure_cursor_visible();
        self.overlay = Overlay::None;
    }

    fn capture_document(&self) -> DocumentState {
        DocumentState {
            id: self.document_id,
            lines: self.lines.clone(),
            cursor: self.cursor,
            selection_anchor: self.selection_anchor,
            scroll_line: self.scroll_line,
            scroll_column: self.scroll_column,
            filename: self.filename.clone(),
            undo: self.undo.clone(),
            dirty: self.dirty,
        }
    }

    fn restore_document(&mut self, document: DocumentState) {
        self.document_id = document.id;
        self.lines = document.lines;
        self.cursor = document.cursor;
        self.selection_anchor = document.selection_anchor;
        self.scroll_line = document.scroll_line;
        self.scroll_column = document.scroll_column;
        self.filename = document.filename;
        self.undo = document.undo;
        self.dirty = document.dirty;
    }

    fn sync_active_document(&mut self) {
        if self.active_tab < self.tabs.len() {
            self.tabs[self.active_tab] = self.capture_document();
        }
    }

    fn switch_tab(&mut self, tab: usize) {
        if tab >= self.tabs.len() || tab == self.active_tab {
            return;
        }
        if self.selection_anchor.is_none() {
            self.format_departed_line(self.cursor.line);
        }
        self.sync_active_document();
        self.active_tab = tab;
        self.restore_document(self.tabs[tab].clone());
        self.project_focused = false;
        self.mouse_selecting = false;
        self.ensure_active_tab_visible();
    }

    fn cycle_tabs(&mut self, forward: bool) {
        if self.tabs.len() < 2 {
            return;
        }
        let next = if forward {
            (self.active_tab + 1) % self.tabs.len()
        } else {
            (self.active_tab + self.tabs.len() - 1) % self.tabs.len()
        };
        self.switch_tab(next);
    }

    fn ensure_active_tab_visible(&mut self) {
        if self.active_tab < self.tab_scroll {
            self.tab_scroll = self.active_tab;
        } else if self.active_tab >= self.tab_scroll + VISIBLE_TABS {
            self.tab_scroll = self.active_tab + 1 - VISIBLE_TABS;
        }
        self.tab_scroll = self.tab_scroll.min(self.tabs.len().saturating_sub(VISIBLE_TABS));
    }

    fn tab_dirty(&self, tab: usize) -> bool {
        if tab == self.active_tab { self.dirty } else { self.tabs[tab].dirty }
    }

    fn any_dirty_tabs(&self) -> bool {
        (0..self.tabs.len()).any(|tab| self.tab_dirty(tab))
    }

    fn active_tab_is_disposable(&self) -> bool {
        self.filename.is_none() && !self.dirty
    }

    fn tab_title(&self, tab: usize) -> String {
        let (filename, id) = if tab == self.active_tab {
            (self.filename.as_deref(), self.document_id)
        } else {
            let document = &self.tabs[tab];
            (document.filename.as_deref(), document.id)
        };
        filename
            .and_then(|path| path.rsplit('/').next())
            .map(str::to_ascii_uppercase)
            .unwrap_or_else(|| format!("UNTITLED{id}"))
    }

    fn request_close_tab(&mut self, tab: usize) {
        if tab >= self.tabs.len() {
            return;
        }
        if self.tab_dirty(tab) {
            self.overlay = Overlay::CloseTab { tab };
        } else {
            self.close_tab(tab);
        }
    }

    fn close_tab(&mut self, tab: usize) {
        if tab >= self.tabs.len() {
            return;
        }
        let closing_active = tab == self.active_tab;
        let active_id = self.document_id;
        self.sync_active_document();
        self.tabs.remove(tab);
        if self.tabs.is_empty() {
            let document = self.blank_document();
            self.tabs.push(document);
        }
        let next = if closing_active {
            tab.min(self.tabs.len() - 1)
        } else {
            self.tabs.iter().position(|document| document.id == active_id).unwrap_or(0)
        };
        self.active_tab = next;
        self.restore_document(self.tabs[next].clone());
        self.ensure_active_tab_visible();
        self.overlay = Overlay::None;
    }

    fn blank_document(&mut self) -> DocumentState {
        let id = self.next_document_id;
        self.next_document_id += 1;
        DocumentState {
            id,
            lines: vec![String::new()],
            cursor: Position::default(),
            selection_anchor: None,
            scroll_line: 0,
            scroll_column: 0,
            filename: None,
            undo: Vec::new(),
            dirty: false,
        }
    }

    pub fn handle_mouse_press(&mut self, x: usize, y: usize, shift: bool) -> EditorAction {
        let cell_x = (x / GLYPH_WIDTH).min(COLUMNS - 1);
        let cell_y = (y / GLYPH_HEIGHT).min(ROWS - 1);

        if cell_y == 0
            && let Some(menu) = menu_bar_hit(cell_x)
        {
            self.mouse_selecting = false;
            self.open_menu(menu);
            return EditorAction::None;
        }

        match &self.overlay {
            Overlay::Menu { menu, .. } => {
                let menu = *menu;
                let x = menu_origin(menu);
                let width = menu_width(menu);
                let selected = cell_y
                    .checked_sub(2)
                    .filter(|item| *item < menu_items(menu).len())
                    .filter(|_| cell_x > x && cell_x < x + width - 1);
                if selected.is_some_and(|item| menu_item_is_separator(menu, item)) {
                    return EditorAction::None;
                }
                self.overlay = Overlay::None;
                return selected.map_or(EditorAction::None, |item| self.activate_menu(menu, item));
            }
            Overlay::Dialog { kind, input, .. } => {
                let kind = *kind;
                let input = input.clone();
                let width = 32;
                let height = 8;
                let dialog_x = (COLUMNS - width) / 2;
                let dialog_y = (ROWS - height) / 2;
                if cell_y == dialog_y + height - 2 {
                    if cell_x < dialog_x + width / 2 {
                        self.submit_dialog(kind, input);
                    } else {
                        self.close_after_save = None;
                        self.overlay = Overlay::None;
                    }
                }
                return EditorAction::None;
            }
            Overlay::Message { .. } => {
                self.overlay = Overlay::None;
                return EditorAction::None;
            }
            Overlay::CloseTab { .. } => return EditorAction::None,
            Overlay::SearchPrompt { .. } => return EditorAction::None,
            Overlay::DebugPrompt { .. } => return EditorAction::None,
            Overlay::SearchResults { results, scroll, .. } => {
                let result = (cell_x > SEARCH_RESULTS_X
                    && cell_x < SEARCH_RESULTS_X + SEARCH_RESULTS_WIDTH - 1)
                    .then_some(cell_y)
                    .and_then(|row| row.checked_sub(SEARCH_RESULTS_Y + 3))
                    .filter(|row| *row < SEARCH_RESULTS_VISIBLE)
                    .map(|row| *scroll + row)
                    .and_then(|index| results.get(index))
                    .cloned();
                if let Some(result) = result {
                    self.navigate_to_result(&result, true);
                }
                return EditorAction::None;
            }
            Overlay::Building { .. } => return EditorAction::None,
            Overlay::None => {}
        }

        if cell_y == 1 && cell_x >= EDITOR_START {
            if cell_x == EDITOR_START && self.tab_scroll > 0 {
                self.tab_scroll -= 1;
                return EditorAction::None;
            }
            if cell_x == COLUMNS - 1 && self.tab_scroll + VISIBLE_TABS < self.tabs.len() {
                self.tab_scroll += 1;
                return EditorAction::None;
            }
            let relative = cell_x.saturating_sub(EDITOR_START + 1);
            let slot = relative / TAB_WIDTH;
            let tab = self.tab_scroll + slot;
            if slot < VISIBLE_TABS && tab < self.tabs.len() {
                if relative % TAB_WIDTH == TAB_WIDTH - 1 {
                    self.request_close_tab(tab);
                } else {
                    self.switch_tab(tab);
                }
            }
            return EditorAction::None;
        }

        if cell_x < PROJECT_WIDTH && (1..ROWS - 1).contains(&cell_y) {
            self.mouse_selecting = false;
            self.project_focused = true;
            if let Some(selected) = cell_y
                .checked_sub(2)
                .map(|row| self.project_scroll + row)
                .filter(|selected| *selected < self.project_entries.len())
            {
                self.project_selected = selected;
                return self.activate_project_entry(selected);
            }
            return EditorAction::None;
        }
        if cell_x <= PROJECT_WIDTH {
            return EditorAction::None;
        }

        if !(EDITOR_FIRST_ROW..ROWS - 1).contains(&cell_y) {
            return EditorAction::None;
        }
        if cell_x < EDITOR_CODE_START {
            let line = (self.scroll_line + cell_y - EDITOR_FIRST_ROW).min(self.lines.len() - 1);
            self.cursor.line = line;
            self.cursor.column = self.cursor.column.min(self.lines[line].len());
            return self.toggle_source_breakpoint();
        }
        self.project_focused = false;
        let previous = self.cursor;
        let position = self.document_position(cell_x - EDITOR_CODE_START, cell_y);
        if shift {
            self.selection_anchor.get_or_insert(previous);
        } else {
            self.selection_anchor = Some(position);
            if previous.line != position.line {
                self.format_departed_line(previous.line);
            }
        }
        self.cursor = position;
        self.mouse_selecting = true;
        self.ensure_cursor_visible();
        EditorAction::None
    }

    pub fn handle_mouse_move(&mut self, x: usize, y: usize) {
        let cell_x = (x / GLYPH_WIDTH).min(COLUMNS - 1);
        let cell_y = (y / GLYPH_HEIGHT).min(ROWS - 1);
        if let Overlay::Menu { menu, selected } = &mut self.overlay {
            let origin = menu_origin(*menu);
            let width = menu_width(*menu);
            if cell_x > origin
                && cell_x < origin + width - 1
                && let Some(item) = cell_y.checked_sub(2)
                && item < menu_items(*menu).len()
                && !menu_item_is_separator(*menu, item)
            {
                *selected = item;
            }
            return;
        }
        if self.mouse_selecting
            && cell_x >= EDITOR_CODE_START
            && (EDITOR_FIRST_ROW..ROWS - 1).contains(&cell_y)
        {
            self.cursor = self.document_position(cell_x - EDITOR_CODE_START, cell_y);
            self.ensure_cursor_visible();
        }
    }

    pub fn handle_mouse_release(&mut self) {
        self.mouse_selecting = false;
        if self.selection_anchor == Some(self.cursor) {
            self.selection_anchor = None;
        }
    }

    pub fn handle_mouse_wheel(&mut self, horizontal: f64, vertical: f64) {
        if !matches!(self.overlay, Overlay::None) {
            return;
        }
        self.wheel_remainder.0 += horizontal;
        self.wheel_remainder.1 += vertical;
        let horizontal = self.wheel_remainder.0.trunc() as isize;
        let vertical = self.wheel_remainder.1.trunc() as isize;
        self.wheel_remainder.0 -= horizontal as f64;
        self.wheel_remainder.1 -= vertical as f64;
        let max_line = self.lines.len().saturating_sub(TEXT_ROWS);
        let max_column =
            self.lines.iter().map(String::len).max().unwrap_or(0).saturating_sub(EDITOR_COLUMNS);
        if self.project_focused {
            if !self.project_entries.is_empty() {
                self.project_selected = self
                    .project_selected
                    .saturating_add_signed(-vertical)
                    .min(self.project_entries.len() - 1);
                self.ensure_project_selection_visible();
            }
        } else {
            self.scroll_line = self.scroll_line.saturating_add_signed(-vertical).min(max_line);
            self.scroll_column =
                self.scroll_column.saturating_add_signed(horizontal).min(max_column);
        }
    }

    fn document_position(&self, cell_x: usize, cell_y: usize) -> Position {
        let line = (self.scroll_line + cell_y - EDITOR_FIRST_ROW).min(self.lines.len() - 1);
        let column = (self.scroll_column + cell_x).min(self.lines[line].len());
        Position { line, column }
    }

    fn handle_project_key(&mut self, key: &Key) -> EditorAction {
        match key {
            Key::Named(NamedKey::Escape | NamedKey::F6) => self.project_focused = false,
            Key::Named(NamedKey::ArrowUp) if !self.project_entries.is_empty() => {
                self.project_selected = self.project_selected.saturating_sub(1);
            }
            Key::Named(NamedKey::ArrowDown) if !self.project_entries.is_empty() => {
                self.project_selected =
                    (self.project_selected + 1).min(self.project_entries.len() - 1);
            }
            Key::Named(NamedKey::Home) if !self.project_entries.is_empty() => {
                self.project_selected = 0;
            }
            Key::Named(NamedKey::End) if !self.project_entries.is_empty() => {
                self.project_selected = self.project_entries.len() - 1;
            }
            Key::Named(NamedKey::Enter | NamedKey::ArrowRight)
                if !self.project_entries.is_empty() =>
            {
                return self.activate_project_entry(self.project_selected);
            }
            Key::Named(NamedKey::ArrowLeft) if !self.project_entries.is_empty() => {
                let entry = self.project_entries[self.project_selected].clone();
                if entry.is_directory && self.expanded_directories.remove(&entry.path) {
                    self.refresh_project_browser();
                } else if let Some((parent, _)) = entry.path.rsplit_once('/')
                    && let Some(index) = self
                        .project_entries
                        .iter()
                        .position(|candidate| candidate.path.eq_ignore_ascii_case(parent))
                {
                    self.project_selected = index;
                }
            }
            _ => {}
        }
        self.ensure_project_selection_visible();
        EditorAction::None
    }

    fn activate_project_entry(&mut self, selected: usize) -> EditorAction {
        let Some(entry) = self.project_entries.get(selected).cloned() else {
            return EditorAction::None;
        };
        if entry.is_directory {
            if !self.expanded_directories.insert(entry.path.clone()) {
                self.expanded_directories.remove(&entry.path);
            }
            self.refresh_project_browser();
            return EditorAction::None;
        }
        if self
            .filename
            .as_deref()
            .is_some_and(|filename| filename.eq_ignore_ascii_case(&entry.path))
        {
            self.project_focused = false;
            return EditorAction::None;
        }
        match self.load(&entry.path) {
            Ok(()) => {
                self.project_focused = false;
                self.refresh_project_browser();
            }
            Err(error) => self.show_build_message("OPEN ERROR", &[error]),
        }
        EditorAction::None
    }

    fn refresh_project_browser(&mut self) {
        let selected_path =
            self.project_entries.get(self.project_selected).map(|entry| entry.path.clone());
        let mut entries = Vec::new();
        collect_project_entries(
            &self.filesystem.borrow(),
            "",
            0,
            &self.expanded_directories,
            &mut entries,
        );
        self.project_entries = entries;
        self.project_selected = selected_path
            .and_then(|path| {
                self.project_entries.iter().position(|entry| entry.path.eq_ignore_ascii_case(&path))
            })
            .unwrap_or(0)
            .min(self.project_entries.len().saturating_sub(1));
        self.ensure_project_selection_visible();
    }

    fn ensure_project_selection_visible(&mut self) {
        let visible_rows = ROWS - 3;
        if self.project_selected < self.project_scroll {
            self.project_scroll = self.project_selected;
        } else if self.project_selected >= self.project_scroll + visible_rows {
            self.project_scroll = self.project_selected + 1 - visible_rows;
        }
        self.project_scroll =
            self.project_scroll.min(self.project_entries.len().saturating_sub(visible_rows));
    }

    fn render_project_browser(&self, cells: &mut [u8], inverse: &mut [bool]) {
        put_text_width(cells, 0, 1, " PROJECT", PROJECT_WIDTH);
        inverse[COLUMNS..COLUMNS + PROJECT_WIDTH].fill(true);
        for row in 1..ROWS - 1 {
            put_cell(cells, PROJECT_WIDTH, row, BOX_VERTICAL);
        }
        for (screen_row, entry) in
            self.project_entries.iter().skip(self.project_scroll).take(ROWS - 3).enumerate()
        {
            let row = screen_row + 2;
            let mut label = " ".repeat((entry.depth * 2).min(PROJECT_WIDTH - 3));
            if entry.is_directory {
                label.push(if self.expanded_directories.contains(&entry.path) { '-' } else { '+' });
            } else if self
                .filename
                .as_deref()
                .is_some_and(|filename| filename.eq_ignore_ascii_case(&entry.path))
            {
                label.push('*');
            } else {
                label.push(' ');
            }
            label.push(' ');
            label.push_str(&entry.name);
            put_text_width(cells, 0, row, &label, PROJECT_WIDTH);
            if self.project_focused && self.project_scroll + screen_row == self.project_selected {
                inverse[row * COLUMNS..row * COLUMNS + PROJECT_WIDTH].fill(true);
            }
        }
    }

    fn render_tabs(&self, cells: &mut [u8], foregrounds: &mut [u8], inverse: &mut [bool]) {
        let row = 1;
        let row_start = row * COLUMNS;
        foregrounds[row_start + EDITOR_START..row_start + COLUMNS].fill(UI_WHITE_COLOR);
        inverse[row_start + EDITOR_START..row_start + COLUMNS].fill(true);
        put_cell(cells, EDITOR_START, row, if self.tab_scroll > 0 { b'<' } else { b' ' });
        put_cell(
            cells,
            COLUMNS - 1,
            row,
            if self.tab_scroll + VISIBLE_TABS < self.tabs.len() { b'>' } else { b' ' },
        );
        for slot in 0..VISIBLE_TABS {
            let tab = self.tab_scroll + slot;
            let start = EDITOR_START + 1 + slot * TAB_WIDTH;
            if tab >= self.tabs.len() {
                continue;
            }
            let (filename, id) = if tab == self.active_tab {
                (self.filename.as_deref(), self.document_id)
            } else {
                let document = &self.tabs[tab];
                (document.filename.as_deref(), document.id)
            };
            let title = filename
                .and_then(|path| path.rsplit('/').next())
                .map(str::to_ascii_uppercase)
                .unwrap_or_else(|| format!("UNTITLED{id}"));
            let mut label = format!(" {title}");
            label.truncate(TAB_WIDTH - 3);
            while label.len() < TAB_WIDTH - 3 {
                label.push(' ');
            }
            label.push(if self.tab_dirty(tab) { '*' } else { ' ' });
            label.push(' ');
            label.push('X');
            put_text_width(cells, start, row, &label, TAB_WIDTH);
            if tab == self.active_tab {
                inverse[row_start + start..row_start + start + TAB_WIDTH].fill(false);
            }
        }
    }

    fn render_debug_panel(
        &self,
        cells: &mut [u8],
        foregrounds: &mut [u8],
        backgrounds: &mut [u8],
        inverse: &mut [bool],
        style: CellStyle,
    ) {
        let Some(snapshot) = &self.debug_snapshot else { return };
        let x = 52;
        let y = 2;
        let width = 28;
        let height = ROWS - 3;
        draw_window(
            cells,
            foregrounds,
            backgrounds,
            inverse,
            CellRect { x, y, width, height },
            style,
        );
        put_text(cells, x + 3, y, "DEBUGGER - PAUSED");
        put_text(cells, x + 2, y + 2, &format!("PC ${:04X}  SP ${:02X}", snapshot.pc, snapshot.sp));
        put_text(
            cells,
            x + 2,
            y + 3,
            &format!("A ${:02X} X ${:02X} Y ${:02X}", snapshot.a, snapshot.x, snapshot.y),
        );
        put_text(
            cells,
            x + 2,
            y + 4,
            &format!("P ${:02X}  {}", snapshot.status, status_flags(snapshot.status)),
        );
        put_text(cells, x + 2, y + 5, &format!("CYCLES {}", snapshot.cycles));
        put_text(
            cells,
            x + 2,
            y + 6,
            &format!("BANK {}:{:02X}", bank_kind_name(snapshot.bank_kind), snapshot.bank_number),
        );
        put_text(
            cells,
            x + 2,
            y + 7,
            &format!("IRQ {:X}/{:X}", snapshot.irq_pending, snapshot.irq_enable),
        );
        put_text(
            cells,
            x + 2,
            y + 8,
            &format!("RASTER {},{}", snapshot.raster_line, snapshot.raster_dot),
        );
        put_text(
            cells,
            x + 2,
            y + 9,
            &format!("APU {:02X} OUT {:04X}", snapshot.apu.master, snapshot.apu.sample),
        );
        put_text(
            cells,
            x + 2,
            y + 10,
            &format!(
                "P1 {:02X}/{:03X} P2 {:02X}/{:03X}",
                snapshot.apu.pulse_control[0],
                snapshot.apu.pulse_timer[0],
                snapshot.apu.pulse_control[1],
                snapshot.apu.pulse_timer[1]
            ),
        );
        put_text(
            cells,
            x + 2,
            y + 11,
            &format!(
                "TRI {:02X}/{:03X} NOI {:02X}/{:X}",
                snapshot.apu.triangle_control,
                snapshot.apu.triangle_timer,
                snapshot.apu.noise_control,
                snapshot.apu.noise_period
            ),
        );
        put_text_width(cells, x + 2, y + 12, &format!("STOP {:?}", snapshot.reason), width - 4);
        let current_instruction = if snapshot.instruction_boundary {
            format!("NEXT {}", disassemble_instruction(snapshot.pc, snapshot.instruction_bytes))
        } else {
            format!("BUS PC ${:04X}", snapshot.pc)
        };
        put_text_width(cells, x + 2, y + 13, &current_instruction, width - 4);
        put_text(cells, x + 2, y + 14, "STACK");
        for row in 0..4 {
            let offset = row * 4;
            put_text(
                cells,
                x + 2,
                y + 15 + row,
                &format!(
                    "{:02X}: {:02X} {:02X} {:02X} {:02X}",
                    snapshot.sp.wrapping_add(1 + offset as u8),
                    snapshot.stack[offset],
                    snapshot.stack[offset + 1],
                    snapshot.stack[offset + 2],
                    snapshot.stack[offset + 3]
                ),
            );
        }
        put_text(cells, x + 2, y + 20, "RECENT CODE");
        for (row, trace) in snapshot.trace.iter().rev().take(8).enumerate() {
            let section = match trace.section {
                Some(SymbolSection::Fixed) => "F".to_owned(),
                Some(SymbolSection::Bank(bank)) => format!("B{bank:02X}"),
                None => "RAM".to_owned(),
            };
            put_text_width(
                cells,
                x + 2,
                y + 21 + row,
                &format!(
                    "{section} {:04X}: {}",
                    trace.address,
                    disassemble_instruction(trace.address, trace.bytes)
                ),
                width - 4,
            );
        }
        put_text(cells, x + 2, y + 30, &format!("MEMORY ${:04X}", snapshot.memory_start));
        for row in 0..4 {
            let offset = row * 4;
            put_text(
                cells,
                x + 2,
                y + 31 + row,
                &format!(
                    "{:04X}: {:02X} {:02X} {:02X} {:02X}",
                    snapshot.memory_start + offset as u16,
                    snapshot.memory[offset],
                    snapshot.memory[offset + 1],
                    snapshot.memory[offset + 2],
                    snapshot.memory[offset + 3]
                ),
            );
        }
        put_text(cells, x + 2, y + height - 6, "F5 CONT  SF5 STOP");
        put_text(cells, x + 2, y + height - 5, "F10 OVER  F11 INTO");
        put_text(cells, x + 2, y + height - 4, "SF11 OUT  CF11 CYCLE");
        put_text(cells, x + 2, y + height - 3, "F9 TOGGLE BREAKPOINT");
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

        put_text(&mut cells, 0, 0, " FILE  EDIT  BUILD  DEBUG  MUSIC");
        if let Some(status) = &self.music_status {
            let text = status.display_marquee(self.music_marquee_offset);
            let start = COLUMNS.saturating_sub(text.len());
            put_text_width(&mut cells, start, 0, &text, COLUMNS - start);
            backgrounds[start..COLUMNS].fill(UI_SUCCESS_BACKGROUND);
        }
        inverse[..COLUMNS].fill(true);
        foregrounds[..COLUMNS].fill(UI_WHITE_COLOR);

        for screen_y in 0..TEXT_ROWS {
            let line_index = self.scroll_line + screen_y;
            let Some(line) = self.lines.get(line_index) else { break };
            let syntax = assembly_mode.then(|| assembly_syntax_colors(line, foreground));
            for (screen_x, byte) in
                line.bytes().skip(self.scroll_column).take(EDITOR_COLUMNS).enumerate()
            {
                let index = (screen_y + EDITOR_FIRST_ROW) * COLUMNS + EDITOR_CODE_START + screen_x;
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
            let row = screen_y + EDITOR_FIRST_ROW;
            let breakpoint = self.line_has_breakpoint(line_index);
            let executing = self.debug_location.as_ref().is_some_and(|(source, line)| {
                *line == line_index
                    && self
                        .filename
                        .as_deref()
                        .is_some_and(|filename| filename.eq_ignore_ascii_case(source))
            });
            if breakpoint {
                put_cell(&mut cells, EDITOR_START, row, b'@');
                foregrounds[row * COLUMNS + EDITOR_START] = UI_WHITE_COLOR;
            }
            if executing {
                put_cell(&mut cells, EDITOR_START + 1, row, SYMBOL_ARROW_RIGHT);
                foregrounds[row * COLUMNS + EDITOR_START + 1] = UI_WHITE_COLOR;
            }
            if executing || breakpoint {
                let start = row * COLUMNS + EDITOR_START;
                let end = row * COLUMNS + COLUMNS;
                foregrounds[start..end].fill(UI_WHITE_COLOR);
                backgrounds[start..end].fill(if executing {
                    UI_DEBUG_CURRENT_BACKGROUND
                } else {
                    UI_BREAKPOINT_BACKGROUND
                });
                inverse[start..end].fill(false);
                if executing && breakpoint {
                    backgrounds[start] = UI_BREAKPOINT_BACKGROUND;
                }
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
        foregrounds[(ROWS - 1) * COLUMNS..].fill(UI_WHITE_COLOR);

        self.render_project_browser(&mut cells, &mut inverse);
        self.render_tabs(&mut cells, &mut foregrounds, &mut inverse);
        for row in 1..ROWS - 1 {
            foregrounds[row * COLUMNS..row * COLUMNS + EDITOR_START].fill(UI_WHITE_COLOR);
        }

        self.render_debug_panel(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut inverse,
            CellStyle { foreground: UI_WHITE_COLOR, background },
        );

        self.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut inverse,
            CellStyle { foreground: UI_WHITE_COLOR, background },
        );
        render_cells(
            video,
            &cells,
            &foregrounds,
            &backgrounds,
            &inverse,
            CellStyle { foreground, background },
        );

        if cursor_visible
            && !self.project_focused
            && matches!(self.overlay, Overlay::None)
            && let Some(screen_line) = self.cursor.line.checked_sub(self.scroll_line)
            && screen_line < TEXT_ROWS
            && let Some(screen_column) = self.cursor.column.checked_sub(self.scroll_column)
            && screen_column < EDITOR_COLUMNS
        {
            let cell_x = EDITOR_CODE_START + screen_column;
            let cell_y = screen_line + EDITOR_FIRST_ROW;
            draw_block_cursor(video, cell_x, cell_y, cells[cell_y * COLUMNS + cell_x]);
        }
    }

    fn handle_overlay_key(&mut self, key: &Key, modifiers: ModifiersState) -> EditorAction {
        if matches!(self.overlay, Overlay::DebugPrompt { .. }) {
            let mut submit = None;
            if let Overlay::DebugPrompt { kind, input, error } = &mut self.overlay {
                match key {
                    Key::Named(NamedKey::Escape) => self.overlay = Overlay::None,
                    Key::Named(NamedKey::Backspace) => {
                        input.pop();
                        *error = None;
                    }
                    Key::Named(NamedKey::Enter) => submit = Some((*kind, input.clone())),
                    Key::Named(NamedKey::Space) => input.push(' '),
                    Key::Character(text) if !modifiers.control_key() && !modifiers.super_key() => {
                        input.extend(text.chars().filter(|character| {
                            character.is_ascii_hexdigit()
                                || matches!(character, '$' | 'x' | 'X' | ',' | ':' | ' ')
                        }));
                        *error = None;
                    }
                    _ => {}
                }
            }
            return submit
                .map_or(EditorAction::None, |(kind, input)| self.submit_debug_prompt(kind, input));
        }

        if matches!(self.overlay, Overlay::SearchResults { .. }) {
            let mut activate = None;
            if let Overlay::SearchResults { results, selected, scroll, .. } = &mut self.overlay {
                match key {
                    Key::Named(NamedKey::Escape) => self.overlay = Overlay::None,
                    Key::Named(NamedKey::ArrowUp) => {
                        *selected = selected.saturating_sub(1);
                        *scroll = (*scroll).min(*selected);
                    }
                    Key::Named(NamedKey::ArrowDown) if !results.is_empty() => {
                        *selected = (*selected + 1).min(results.len() - 1);
                        if *selected >= *scroll + SEARCH_RESULTS_VISIBLE {
                            *scroll = *selected + 1 - SEARCH_RESULTS_VISIBLE;
                        }
                    }
                    Key::Named(NamedKey::PageUp) => {
                        *selected = selected.saturating_sub(SEARCH_RESULTS_VISIBLE);
                        *scroll = scroll.saturating_sub(SEARCH_RESULTS_VISIBLE);
                    }
                    Key::Named(NamedKey::PageDown) if !results.is_empty() => {
                        *selected = (*selected + SEARCH_RESULTS_VISIBLE).min(results.len() - 1);
                        *scroll = (*selected + 1)
                            .saturating_sub(SEARCH_RESULTS_VISIBLE)
                            .min(results.len().saturating_sub(SEARCH_RESULTS_VISIBLE));
                    }
                    Key::Named(NamedKey::Enter) => activate = results.get(*selected).cloned(),
                    _ => {}
                }
            }
            if let Some(result) = activate {
                self.navigate_to_result(&result, true);
            }
            return EditorAction::None;
        }

        if matches!(self.overlay, Overlay::SearchPrompt { .. }) {
            let mut submit = None;
            let mut replace_all = None;
            if let Overlay::SearchPrompt { mode, query, replacement, field, error } =
                &mut self.overlay
            {
                match key {
                    Key::Named(NamedKey::Escape) => self.overlay = Overlay::None,
                    Key::Named(NamedKey::Tab) if *mode == SearchMode::Replace => {
                        *field = if *field == SearchField::Query {
                            SearchField::Replacement
                        } else {
                            SearchField::Query
                        };
                        *error = None;
                    }
                    Key::Named(NamedKey::Backspace) => {
                        if *field == SearchField::Query {
                            query.pop();
                        } else {
                            replacement.pop();
                        }
                        *error = None;
                    }
                    Key::Named(NamedKey::Enter) => {
                        submit = Some((*mode, query.clone(), replacement.clone()));
                    }
                    Key::Named(NamedKey::F8) if *mode == SearchMode::Replace => {
                        replace_all = Some((query.clone(), replacement.clone()));
                    }
                    Key::Named(NamedKey::Space) if *mode != SearchMode::GoToLine => {
                        if *field == SearchField::Query {
                            query.push(' ');
                        } else {
                            replacement.push(' ');
                        }
                    }
                    Key::Character(text) if !modifiers.control_key() && !modifiers.super_key() => {
                        let filtered = text
                            .chars()
                            .filter(|character| {
                                character.is_ascii_graphic()
                                    && (*mode != SearchMode::GoToLine || character.is_ascii_digit())
                            })
                            .collect::<String>();
                        if *field == SearchField::Query {
                            query.push_str(&filtered);
                        } else {
                            replacement.push_str(&filtered);
                        }
                        *error = None;
                    }
                    _ => {}
                }
            }
            if let Some((mode, query, replacement)) = submit {
                self.submit_search_prompt(mode, query, replacement);
            } else if let Some((query, replacement)) = replace_all {
                let count = self.replace_all(&query, &replacement);
                self.last_search = query;
                self.show_build_message("REPLACE", &[format!("REPLACED {count} MATCHES")]);
            }
            return EditorAction::None;
        }

        if let Overlay::CloseTab { tab } = &self.overlay {
            let tab = *tab;
            match key {
                Key::Named(NamedKey::Escape) => self.overlay = Overlay::None,
                Key::Character(text) if text.eq_ignore_ascii_case("d") => self.close_tab(tab),
                Key::Character(text) if text.eq_ignore_ascii_case("s") => {
                    match self.save_tab(tab) {
                        Ok(true) => self.close_tab(tab),
                        Ok(false) => {
                            self.close_after_save = self.tabs.get(tab).map(|document| document.id);
                            self.overlay = Overlay::None;
                            self.switch_tab(tab);
                            self.open_dialog(DialogKind::SaveAs);
                        }
                        Err(error) => self.show_build_message("SAVE ERROR", &[error]),
                    }
                }
                _ => {}
            }
            return EditorAction::None;
        }
        if matches!(self.overlay, Overlay::Message { .. }) {
            match key {
                Key::Named(NamedKey::F4) => self.move_diagnostic(!modifiers.shift_key()),
                Key::Named(NamedKey::Enter | NamedKey::Escape) => self.overlay = Overlay::None,
                Key::Named(NamedKey::F5) => self.start_build(true),
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
            Overlay::Menu { menu, selected } => match key {
                Key::Named(NamedKey::Escape) | Key::Named(NamedKey::F10) => {
                    self.overlay = Overlay::None;
                }
                Key::Named(NamedKey::ArrowLeft | NamedKey::ArrowRight) => {
                    *menu = adjacent_menu(*menu, matches!(key, Key::Named(NamedKey::ArrowRight)));
                    *selected = 0;
                }
                Key::Named(NamedKey::ArrowUp) => {
                    *selected = next_menu_item(*menu, *selected, false)
                }
                Key::Named(NamedKey::ArrowDown) => {
                    *selected = next_menu_item(*menu, *selected, true)
                }
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
            },
            Overlay::Dialog { kind, input, .. } => match key {
                Key::Named(NamedKey::Escape) => {
                    self.close_after_save = None;
                    self.overlay = Overlay::None;
                }
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
            Overlay::None
            | Overlay::Building { .. }
            | Overlay::Message { .. }
            | Overlay::CloseTab { .. }
            | Overlay::SearchPrompt { .. }
            | Overlay::SearchResults { .. }
            | Overlay::DebugPrompt { .. } => {}
        }
        EditorAction::None
    }

    fn activate_menu(&mut self, menu: MenuKind, selected: usize) -> EditorAction {
        match (menu, selected) {
            (MenuKind::File, 0) => self.new_document(),
            (MenuKind::File, 1) => self.open_dialog(DialogKind::Open),
            (MenuKind::File, 3) => self.save_or_prompt(),
            (MenuKind::File, 4) => self.open_dialog(DialogKind::SaveAs),
            (MenuKind::File, 5) => self.save_all(),
            (MenuKind::File, 7) => self.request_close_tab(self.active_tab),
            (MenuKind::File, 9) => {
                if self.any_dirty_tabs() {
                    self.show_build_message(
                        "UNSAVED TABS",
                        &["SAVE OR CLOSE DIRTY TABS BEFORE EXIT".to_owned()],
                    );
                } else {
                    return EditorAction::Exit;
                }
            }
            (MenuKind::Edit, 0) => self.undo(),
            (MenuKind::Edit, 2) => self.cut_selection(),
            (MenuKind::Edit, 3) => self.copy_selection(),
            (MenuKind::Edit, 4) => self.paste(),
            (MenuKind::Edit, 5) => self.select_all(),
            (MenuKind::Edit, 7) => self.open_search_prompt(SearchMode::Find),
            (MenuKind::Edit, 8) => self.open_search_prompt(SearchMode::Replace),
            (MenuKind::Edit, 9) => self.open_search_prompt(SearchMode::Project),
            (MenuKind::Edit, 10) => self.open_search_prompt(SearchMode::GoToLine),
            (MenuKind::Edit, 12) => self.navigate_history(false),
            (MenuKind::Edit, 13) => self.navigate_history(true),
            (MenuKind::Build, 0) => self.start_build(false),
            (MenuKind::Build, 1) => self.start_build(true),
            (MenuKind::Build, 3) => self.move_diagnostic(true),
            (MenuKind::Build, 4) => self.move_diagnostic(false),
            (MenuKind::Debug, 0) => {
                if self.debug_active && self.debug_snapshot.is_some() {
                    self.debug_snapshot = None;
                    return EditorAction::Debug(DebugCommand::Continue);
                }
                self.start_build(true);
            }
            (MenuKind::Debug, 1) if self.debug_active => {
                return EditorAction::Debug(DebugCommand::Stop);
            }
            (MenuKind::Debug, 2) => return self.toggle_source_breakpoint(),
            (MenuKind::Debug, 4) if self.debug_active && self.debug_snapshot.is_some() => {
                return EditorAction::Debug(DebugCommand::StepOver);
            }
            (MenuKind::Debug, 5) if self.debug_active && self.debug_snapshot.is_some() => {
                return EditorAction::Debug(DebugCommand::StepInstruction);
            }
            (MenuKind::Debug, 6) if self.debug_active && self.debug_snapshot.is_some() => {
                return EditorAction::Debug(DebugCommand::StepOut);
            }
            (MenuKind::Debug, 7) if self.debug_active && self.debug_snapshot.is_some() => {
                return EditorAction::Debug(DebugCommand::StepCycle);
            }
            (MenuKind::Debug, 9) => self.open_debug_prompt(DebugPromptKind::ReadWatchpoint),
            (MenuKind::Debug, 10) => self.open_debug_prompt(DebugPromptKind::WriteWatchpoint),
            (MenuKind::Debug, 11) => self.open_debug_prompt(DebugPromptKind::RasterBreakpoint),
            (MenuKind::Debug, 13) => {
                self.source_breakpoints.clear();
                return EditorAction::Debug(DebugCommand::ClearBreakpoints);
            }
            (MenuKind::Music, 0) if self.music_status.is_some() => {
                return EditorAction::Music(if self.music_status.as_ref().unwrap().paused {
                    MusicCommand::Play
                } else {
                    MusicCommand::Pause
                });
            }
            (MenuKind::Music, 1) if self.music_status.is_some() => {
                return EditorAction::Music(MusicCommand::Previous);
            }
            (MenuKind::Music, 2) if self.music_status.is_some() => {
                return EditorAction::Music(MusicCommand::Next);
            }
            (MenuKind::Music, 3) if self.music_status.is_some() => {
                return EditorAction::Music(MusicCommand::ToggleLoop);
            }
            (MenuKind::Music, 5) if self.music_status.is_some() => {
                return EditorAction::Music(MusicCommand::Stop);
            }
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
                    MenuKind::Debug => 19,
                    MenuKind::Music => 27,
                };
                let width = menu_width(*menu);
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
                    if item.is_empty() {
                        for column in x + 1..x + width - 1 {
                            put_cell(cells, column, row, BOX_HORIZONTAL);
                        }
                        continue;
                    }
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
            Overlay::DebugPrompt { kind, input, error } => {
                let title = match kind {
                    DebugPromptKind::ReadWatchpoint => "READ WATCHPOINT",
                    DebugPromptKind::WriteWatchpoint => "WRITE WATCHPOINT",
                    DebugPromptKind::RasterBreakpoint => "RASTER BREAKPOINT",
                };
                let label = if *kind == DebugPromptKind::RasterBreakpoint {
                    "LINE,DOT:"
                } else {
                    "ADDRESS:"
                };
                let width = 42;
                let height = 9;
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
                put_text(cells, x + 4, y + 2, label);
                put_text_width(cells, x + 14, y + 2, input, width - 16);
                put_text(cells, x + 3, y + height - 2, "ENTER=ADD  ESC=CANCEL");
                if let Some(error) = error {
                    put_cell(cells, x + 2, y + 4, SYMBOL_CROSS);
                    put_text_width(cells, x + 4, y + 4, error, width - 6);
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
            Overlay::CloseTab { tab } => {
                let name = self.tab_title(*tab);
                render_message_box(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    style,
                    "UNSAVED TAB",
                    &[name],
                );
            }
            Overlay::SearchPrompt { mode, query, replacement, field, error } => {
                let title = match mode {
                    SearchMode::Find => "FIND",
                    SearchMode::Replace => "FIND AND REPLACE",
                    SearchMode::Project => "FIND IN PROJECT",
                    SearchMode::GoToLine => "GO TO LINE",
                };
                let width = 54;
                let height = if *mode == SearchMode::Replace { 11 } else { 9 };
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
                let query_label = if *mode == SearchMode::GoToLine { "LINE:" } else { "FIND:" };
                put_cell(
                    cells,
                    x + 2,
                    y + 2,
                    if *field == SearchField::Query { SYMBOL_ARROW_RIGHT } else { b' ' },
                );
                put_text(cells, x + 4, y + 2, query_label);
                put_text_width(cells, x + 13, y + 2, query, width - 15);
                if *mode == SearchMode::Replace {
                    put_cell(
                        cells,
                        x + 2,
                        y + 4,
                        if *field == SearchField::Replacement { SYMBOL_ARROW_RIGHT } else { b' ' },
                    );
                    put_text(cells, x + 4, y + 4, "REPLACE:");
                    put_text_width(cells, x + 13, y + 4, replacement, width - 15);
                    put_text(
                        cells,
                        x + 3,
                        y + height - 2,
                        "ENTER=NEXT  F8=ALL  TAB=FIELD  ESC=CANCEL",
                    );
                } else {
                    put_text(cells, x + 3, y + height - 2, "ENTER=OK  ESC=CANCEL");
                }
                if let Some(error) = error {
                    put_cell(cells, x + 2, y + height - 4, SYMBOL_CROSS);
                    put_text_width(cells, x + 4, y + height - 4, error, width - 6);
                }
            }
            Overlay::SearchResults { query, results, selected, scroll } => {
                draw_window(
                    cells,
                    foregrounds,
                    backgrounds,
                    inverse,
                    CellRect {
                        x: SEARCH_RESULTS_X,
                        y: SEARCH_RESULTS_Y,
                        width: SEARCH_RESULTS_WIDTH,
                        height: SEARCH_RESULTS_HEIGHT,
                    },
                    style,
                );
                put_text_width(
                    cells,
                    SEARCH_RESULTS_X + 3,
                    SEARCH_RESULTS_Y,
                    &format!("SEARCH: {query}  {} MATCHES", results.len()),
                    SEARCH_RESULTS_WIDTH - 6,
                );
                put_text(
                    cells,
                    SEARCH_RESULTS_X + 2,
                    SEARCH_RESULTS_Y + 2,
                    "FILE:LINE:COL  SOURCE",
                );
                for (screen_row, result) in
                    results.iter().skip(*scroll).take(SEARCH_RESULTS_VISIBLE).enumerate()
                {
                    let row = SEARCH_RESULTS_Y + 3 + screen_row;
                    let label = format!(
                        "{}:{}:{}  {}",
                        result.path,
                        result.line + 1,
                        result.column + 1,
                        result.preview
                    );
                    put_text_width(
                        cells,
                        SEARCH_RESULTS_X + 2,
                        row,
                        &label,
                        SEARCH_RESULTS_WIDTH - 4,
                    );
                    if *scroll + screen_row == *selected {
                        inverse[row * COLUMNS + SEARCH_RESULTS_X + 1
                            ..row * COLUMNS + SEARCH_RESULTS_X + SEARCH_RESULTS_WIDTH - 1]
                            .fill(true);
                        put_cell(cells, SEARCH_RESULTS_X + 1, row, SYMBOL_ARROW_RIGHT);
                    }
                }
                put_text(
                    cells,
                    SEARCH_RESULTS_X + 2,
                    SEARCH_RESULTS_Y + SEARCH_RESULTS_HEIGHT - 2,
                    "ENTER/CLICK=OPEN  ESC=CLOSE",
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

    fn open_debug_prompt(&mut self, kind: DebugPromptKind) {
        if !self.debug_active {
            self.show_build_message("DEBUGGER", &["START A DEBUG SESSION FIRST".to_owned()]);
            return;
        }
        self.overlay = Overlay::DebugPrompt { kind, input: String::new(), error: None };
    }

    fn submit_debug_prompt(&mut self, kind: DebugPromptKind, input: String) -> EditorAction {
        let result = match kind {
            DebugPromptKind::ReadWatchpoint | DebugPromptKind::WriteWatchpoint => {
                parse_debug_number(&input).map(|address| {
                    if kind == DebugPromptKind::ReadWatchpoint {
                        DebugCommand::AddReadWatchpoint(address)
                    } else {
                        DebugCommand::AddWriteWatchpoint(address)
                    }
                })
            }
            DebugPromptKind::RasterBreakpoint => parse_raster_breakpoint(&input)
                .map(|(line, dot)| DebugCommand::AddRasterBreakpoint { dot, line }),
        };
        match result {
            Ok(command) => {
                self.overlay = Overlay::None;
                EditorAction::Debug(command)
            }
            Err(error) => {
                self.overlay = Overlay::DebugPrompt { kind, input, error: Some(error) };
                EditorAction::None
            }
        }
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

    fn save_all(&mut self) {
        self.sync_active_document();
        if self.tabs.iter().any(|document| document.dirty && document.filename.is_none()) {
            self.show_build_message(
                "SAVE ALL",
                &["SAVE UNTITLED TABS WITH SAVE AS FIRST".to_owned()],
            );
            return;
        }
        match self.save_named_tabs() {
            Ok(()) => self.show_build_message("SAVE ALL", &["ALL NAMED FILES SAVED".to_owned()]),
            Err(error) => self.show_build_message("SAVE ERROR", &[error]),
        }
    }

    fn save_named_tabs(&mut self) -> Result<(), String> {
        self.sync_active_document();
        let mut failure = None;
        for document in &mut self.tabs {
            let Some(filename) = document.filename.clone().filter(|_| document.dirty) else {
                continue;
            };
            let mut lines = document.lines.clone();
            if assembly_filename(&filename) {
                format_assembly_lines(&mut lines);
            }
            let save_result = self.filesystem.borrow_mut().write_text(&filename, &lines.join("\n"));
            if let Err(error) = save_result {
                failure = Some(format!("{filename}: {error}"));
                break;
            }
            document.lines = lines;
            document.cursor.line = document.cursor.line.min(document.lines.len() - 1);
            document.cursor.column =
                document.cursor.column.min(document.lines[document.cursor.line].len());
            document.selection_anchor = None;
            document.dirty = false;
        }
        self.restore_document(self.tabs[self.active_tab].clone());
        self.refresh_project_browser();
        failure.map_or(Ok(()), Err)
    }

    fn save_tab(&mut self, tab: usize) -> Result<bool, String> {
        if tab >= self.tabs.len() {
            return Ok(true);
        }
        self.sync_active_document();
        let Some(filename) = self.tabs[tab].filename.clone() else { return Ok(false) };
        let mut lines = self.tabs[tab].lines.clone();
        if assembly_filename(&filename) {
            format_assembly_lines(&mut lines);
        }
        self.filesystem.borrow_mut().write_text(&filename, &lines.join("\n"))?;
        self.tabs[tab].lines = lines;
        self.tabs[tab].cursor.line = self.tabs[tab].cursor.line.min(self.tabs[tab].lines.len() - 1);
        self.tabs[tab].cursor.column = self.tabs[tab]
            .cursor
            .column
            .min(self.tabs[tab].lines[self.tabs[tab].cursor.line].len());
        self.tabs[tab].selection_anchor = None;
        self.tabs[tab].dirty = false;
        if tab == self.active_tab {
            self.restore_document(self.tabs[tab].clone());
        }
        self.refresh_project_browser();
        Ok(true)
    }

    fn start_build(&mut self, run_after: bool) {
        self.build_and_run = run_after;
        self.overlay = Overlay::Building { frames_remaining: BUILD_PROGRESS_FRAMES };
    }

    fn perform_build(&mut self) {
        if let Err(error) = self.save_named_tabs() {
            self.show_build_message("BUILD ERROR", &[error]);
            return;
        }
        if self.build_and_run {
            match build_and_load_project(&self.filesystem) {
                Ok(mut launch) => {
                    let title = launch.cartridge.title.clone();
                    launch.breakpoints = self.resolved_source_breakpoints(&launch.source_map);
                    self.debug_source_map = launch.source_map.clone();
                    self.debug_snapshot = None;
                    self.debug_active = true;
                    self.show_build_message("BUILD SUCCESSFUL", &[format!("RUNNING: {title}")]);
                    self.refresh_project_browser();
                    self.pending_launch = Some(launch);
                }
                Err(diagnostics) => {
                    self.diagnostics = diagnostics;
                    self.diagnostic_index = (!self.diagnostics.is_empty()).then_some(0);
                    self.goto_current_diagnostic();
                    self.show_current_diagnostic_dialog();
                }
            }
            return;
        }

        // File presence selects cartridge mode. Parse errors belong to the
        // project build and must never fall through to raw `.BIN` assembly.
        if self.filesystem.borrow().read_binary(MANIFEST_NAME).is_ok() {
            match build_project(&self.filesystem) {
                Ok(success) => {
                    self.diagnostics.clear();
                    self.diagnostic_index = None;
                    self.build_message =
                        Some(format!("BUILT {} {} BYTES", success.output, success.size));
                    self.show_build_message(
                        "BUILD SUCCESSFUL",
                        &[
                            format!("OUTPUT: {}", success.output),
                            format!("TITLE: {}", success.title),
                            format!("ROM BANKS: {}", success.banks),
                            format!("SIZE: {} BYTES", success.size),
                        ],
                    );
                    self.refresh_project_browser();
                }
                Err(diagnostics) => {
                    self.diagnostics = diagnostics;
                    self.diagnostic_index = (!self.diagnostics.is_empty()).then_some(0);
                    self.build_message = None;
                    self.goto_current_diagnostic();
                    self.show_current_diagnostic_dialog();
                }
            }
            return;
        }

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
                self.refresh_project_browser();
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
        let Some(diagnostic) = self.current_diagnostic().cloned() else { return };
        if !self.source_is_current(&diagnostic.source) {
            let diagnostics = self.diagnostics.clone();
            let diagnostic_index = self.diagnostic_index;
            if self.load(&diagnostic.source).is_err() {
                return;
            }
            self.diagnostics = diagnostics;
            self.diagnostic_index = diagnostic_index;
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
        if self.tabs.iter().enumerate().any(|(tab, document)| {
            tab != self.active_tab
                && document
                    .filename
                    .as_deref()
                    .is_some_and(|open| open.eq_ignore_ascii_case(filename))
        }) {
            return Err("FILE IS ALREADY OPEN".to_owned());
        }
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
        self.sync_active_document();
        self.refresh_project_browser();
        if self.close_after_save.take() == Some(self.document_id) {
            self.close_tab(self.active_tab);
        }
        Ok(())
    }

    fn load(&mut self, filename: &str) -> Result<(), String> {
        if let Some(tab) = self.tabs.iter().position(|document| {
            document.filename.as_deref().is_some_and(|open| open.eq_ignore_ascii_case(filename))
        }) {
            let disposable_tab = self.active_tab_is_disposable().then_some(self.active_tab);
            self.switch_tab(tab);
            if let Some(disposable_tab) = disposable_tab {
                self.close_tab(disposable_tab);
            }
            return Ok(());
        }
        let text = self.filesystem.borrow().read_text(filename)?;
        let mut lines = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if assembly_filename(filename) {
            format_assembly_lines(&mut lines);
        }
        if lines.is_empty() {
            lines.push(String::new());
        }
        if self.tabs.is_empty() {
            self.lines = lines;
            self.cursor = Position::default();
            self.selection_anchor = None;
            self.scroll_line = 0;
            self.scroll_column = 0;
            self.filename = Some(filename.to_ascii_lowercase());
            self.undo.clear();
            self.dirty = false;
        } else if self.active_tab_is_disposable() {
            let document = DocumentState {
                id: self.document_id,
                lines,
                cursor: Position::default(),
                selection_anchor: None,
                scroll_line: 0,
                scroll_column: 0,
                filename: Some(filename.to_ascii_lowercase()),
                undo: Vec::new(),
                dirty: false,
            };
            self.tabs[self.active_tab] = document.clone();
            self.restore_document(document);
        } else {
            self.sync_active_document();
            let id = self.next_document_id;
            self.next_document_id += 1;
            let document = DocumentState {
                id,
                lines,
                cursor: Position::default(),
                selection_anchor: None,
                scroll_line: 0,
                scroll_column: 0,
                filename: Some(filename.to_ascii_lowercase()),
                undo: Vec::new(),
                dirty: false,
            };
            self.tabs.push(document.clone());
            self.active_tab = self.tabs.len() - 1;
            self.restore_document(document);
            self.ensure_active_tab_visible();
        }
        self.invalidate_build();
        Ok(())
    }

    fn new_document(&mut self) {
        self.sync_active_document();
        let document = self.blank_document();
        self.tabs.push(document.clone());
        self.active_tab = self.tabs.len() - 1;
        self.restore_document(document);
        self.ensure_active_tab_visible();
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
        } else if self.cursor.column >= self.scroll_column + EDITOR_COLUMNS {
            self.scroll_column = self.cursor.column + 1 - EDITOR_COLUMNS;
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

fn collect_project_entries(
    filesystem: &ConsoleFilesystem,
    directory: &str,
    depth: usize,
    expanded: &BTreeSet<String>,
    output: &mut Vec<ProjectEntry>,
) {
    let listing = filesystem.list((!directory.is_empty()).then_some(directory));
    let Ok(entries) = listing else { return };
    for entry in entries {
        let path = if directory.is_empty() {
            entry.name.clone()
        } else {
            format!("{directory}/{}", entry.name)
        };
        output.push(ProjectEntry {
            name: entry.name,
            path: path.clone(),
            depth,
            is_directory: entry.is_directory,
        });
        if entry.is_directory && expanded.contains(&path) {
            collect_project_entries(filesystem, &path, depth + 1, expanded, output);
        }
    }
}

fn collect_project_files(
    filesystem: &ConsoleFilesystem,
    directory: &str,
    output: &mut Vec<String>,
) {
    let Ok(entries) = filesystem.list(Some(directory)) else { return };
    for entry in entries {
        let path =
            if directory == "/" { entry.name } else { format!("{directory}/{}", entry.name) };
        if entry.is_directory {
            collect_project_files(filesystem, &path, output);
        } else {
            output.push(path);
        }
    }
}

fn menu_items(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => {
            &["NEW", "OPEN...", "", "SAVE", "SAVE AS...", "SAVE ALL", "", "CLOSE TAB", "", "EXIT"]
        }
        MenuKind::Edit => &[
            "UNDO",
            "",
            "CUT",
            "COPY",
            "PASTE",
            "SELECT ALL",
            "",
            "FIND",
            "REPLACE",
            "PROJECT FIND",
            "GO TO LINE",
            "",
            "BACK",
            "FORWARD",
        ],
        MenuKind::Build => &["ASSEMBLE", "BUILD & RUN", "", "NEXT ERROR", "PREV ERROR"],
        MenuKind::Debug => &[
            "START/CONTINUE",
            "STOP",
            "TOGGLE BREAK",
            "",
            "STEP OVER",
            "STEP INTO",
            "STEP OUT",
            "STEP CYCLE",
            "",
            "READ WATCH",
            "WRITE WATCH",
            "RASTER BREAK",
            "",
            "CLEAR BREAKS",
        ],
        MenuKind::Music => &["PLAY/PAUSE", "PREVIOUS", "NEXT", "LOOP", "", "STOP"],
    }
}

const fn menu_origin(menu: MenuKind) -> usize {
    match menu {
        MenuKind::File => 0,
        MenuKind::Edit => 6,
        MenuKind::Build => 12,
        MenuKind::Debug => 19,
        MenuKind::Music => 27,
    }
}

const fn menu_width(menu: MenuKind) -> usize {
    match menu {
        MenuKind::Debug => 27,
        MenuKind::Music => 26,
        _ => 16,
    }
}

const fn menu_bar_hit(column: usize) -> Option<MenuKind> {
    match column {
        0..=5 => Some(MenuKind::File),
        6..=11 => Some(MenuKind::Edit),
        12..=18 => Some(MenuKind::Build),
        19..=25 => Some(MenuKind::Debug),
        27..=33 => Some(MenuKind::Music),
        _ => None,
    }
}

fn menu_labels(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => &[
            "NEW       N",
            "OPEN      O",
            "",
            "SAVE      S",
            "SAVE AS   A",
            "SAVE ALL  L",
            "",
            "CLOSE TAB W",
            "",
            "EXIT      X",
        ],
        MenuKind::Edit => &[
            "UNDO      U",
            "",
            "CUT       T",
            "COPY      C",
            "PASTE     P",
            "SELECT ALL A",
            "",
            "FIND      F",
            "REPLACE   R",
            "PROJ FIND J",
            "GO LINE   G",
            "",
            "BACK      K",
            "FORWARD   L",
        ],
        MenuKind::Build => &["ASSEMBLE  B", "BUILD+RUN F5", "", "NEXT ERR  N", "PREV ERR  P"],
        MenuKind::Debug => &[
            "CONTINUE             F5",
            "STOP           SHIFT+F5",
            "TOGGLE BREAK         F9",
            "",
            "STEP OVER           F10",
            "STEP INTO           F11",
            "STEP OUT      SHIFT+F11",
            "STEP CYCLE CTRL/CMD+F11",
            "",
            "READ WATCH            R",
            "WRITE WATCH           W",
            "RASTER BREAK          A",
            "",
            "CLEAR BREAKS          C",
        ],
        MenuKind::Music => &[
            "PLAY/PAUSE          F7",
            "PREVIOUS      SHIFT+F8",
            "NEXT                F8",
            "LOOP       CTRL/CMD+F8",
            "",
            "STOP          SHIFT+F7",
        ],
    }
}

fn menu_hotkey(menu: MenuKind, key: &str) -> Option<usize> {
    let key = key.to_ascii_lowercase();
    let hotkeys: &[(usize, &str)] = match menu {
        MenuKind::File => &[(0, "n"), (1, "o"), (3, "s"), (4, "a"), (5, "l"), (7, "w"), (9, "x")],
        MenuKind::Edit => &[
            (0, "u"),
            (2, "t"),
            (3, "c"),
            (4, "p"),
            (5, "a"),
            (7, "f"),
            (8, "r"),
            (9, "j"),
            (10, "g"),
            (12, "k"),
            (13, "l"),
        ],
        MenuKind::Build => &[(0, "b"), (1, "r"), (3, "n"), (4, "p")],
        MenuKind::Debug => {
            &[(0, "g"), (1, "s"), (2, "b"), (9, "r"), (10, "w"), (11, "a"), (13, "c")]
        }
        MenuKind::Music => &[(0, "p"), (1, "r"), (2, "n"), (3, "l"), (5, "s")],
    };
    hotkeys.iter().find_map(|(index, hotkey)| (*hotkey == key).then_some(*index))
}

fn menu_item_is_separator(menu: MenuKind, item: usize) -> bool {
    menu_items(menu).get(item).is_some_and(|label| label.is_empty())
}

fn next_menu_item(menu: MenuKind, current: usize, forward: bool) -> usize {
    let count = menu_items(menu).len();
    let mut item = current;
    loop {
        item = if forward { (item + 1) % count } else { (item + count - 1) % count };
        if !menu_item_is_separator(menu, item) {
            return item;
        }
    }
}

const fn adjacent_menu(menu: MenuKind, forward: bool) -> MenuKind {
    match (menu, forward) {
        (MenuKind::File, true) | (MenuKind::Build, false) => MenuKind::Edit,
        (MenuKind::Edit, true) | (MenuKind::Debug, false) => MenuKind::Build,
        (MenuKind::Build, true) | (MenuKind::Music, false) => MenuKind::Debug,
        (MenuKind::Debug, true) | (MenuKind::File, false) => MenuKind::Music,
        (MenuKind::Music, true) | (MenuKind::Edit, false) => MenuKind::File,
    }
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
    let footer = match title {
        "BUILD ERRORS" => "ENTER=OK  F4=NEXT",
        "UNSAVED TAB" => "S=SAVE D=DISCARD ESC=CANCEL",
        _ => "ENTER=OK  ESC=CLOSE",
    };
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
    let gradient = configure_text_gradient(
        video,
        foregrounds
            .iter()
            .copied()
            .chain(backgrounds.iter().copied())
            .chain([foreground, background]),
    );
    let pixels = video.pixels_mut();
    pixels.fill(background);
    for cell_y in 0..ROWS {
        for cell_x in 0..COLUMNS {
            let index = cell_y * COLUMNS + cell_x;
            let (cell_foreground, cell_background) = if inverse[index] {
                (backgrounds[index], foregrounds[index])
            } else {
                (foregrounds[index], backgrounds[index])
            };
            if cell_background != background {
                let shaded_background = matches!(
                    cell_background,
                    UI_DEBUG_CURRENT_BACKGROUND | UI_BREAKPOINT_BACKGROUND
                );
                for glyph_y in 0..GLYPH_HEIGHT {
                    let y = cell_y * GLYPH_HEIGHT + glyph_y;
                    let color = if shaded_background {
                        gradient_color(&gradient, cell_background, glyph_y)
                    } else {
                        cell_background
                    };
                    pixels[y * EDITOR_DISPLAY_WIDTH + cell_x * GLYPH_WIDTH
                        ..y * EDITOR_DISPLAY_WIDTH + (cell_x + 1) * GLYPH_WIDTH]
                        .fill(color);
                }
            }
            let glyph = CHARACTER_ROM[(cells[index] as usize).min(CHARACTER_ROM.len() - 1)];
            for (glyph_y, bits) in glyph.into_iter().enumerate() {
                for glyph_x in 0..GLYPH_WIDTH {
                    if bits & (0x80 >> glyph_x) != 0 {
                        let x = cell_x * GLYPH_WIDTH + glyph_x;
                        let y = cell_y * GLYPH_HEIGHT + glyph_y;
                        pixels[y * EDITOR_DISPLAY_WIDTH + x] =
                            gradient_color(&gradient, cell_foreground, glyph_y);
                    }
                }
            }
        }
    }
}

fn draw_block_cursor(video: &mut Video, cell_x: usize, cell_y: usize, character: u8) {
    let origin_x = cell_x * GLYPH_WIDTH;
    let origin_y = cell_y * GLYPH_HEIGHT;
    let pixels = video.pixels_mut();
    for y in origin_y..origin_y + GLYPH_HEIGHT {
        pixels[y * EDITOR_DISPLAY_WIDTH + origin_x
            ..y * EDITOR_DISPLAY_WIDTH + origin_x + GLYPH_WIDTH]
            .fill(UI_WHITE_COLOR);
    }
    let glyph = CHARACTER_ROM[(character as usize).min(CHARACTER_ROM.len() - 1)];
    for (glyph_y, bits) in glyph.into_iter().enumerate() {
        for glyph_x in 0..GLYPH_WIDTH {
            if bits & (0x80 >> glyph_x) != 0 {
                pixels[(origin_y + glyph_y) * EDITOR_DISPLAY_WIDTH + origin_x + glyph_x] = 0;
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

fn execution_section(snapshot: &DebugSnapshot) -> Option<SymbolSection> {
    match snapshot.pc {
        0x8000..=0xbfff if snapshot.bank_kind == bank_kind::CARTRIDGE_ROM => {
            Some(SymbolSection::Bank(snapshot.bank_number))
        }
        0xc100..=0xffff => Some(SymbolSection::Fixed),
        _ => None,
    }
}

fn parse_debug_number(input: &str) -> Result<u16, String> {
    let input = input.trim();
    if input.is_empty() {
        return Err("VALUE REQUIRED".to_owned());
    }
    let (digits, radix) = input.strip_prefix('$').map_or_else(
        || input.strip_prefix("0x").map_or((input, 10), |digits| (digits, 16)),
        |digits| (digits, 16),
    );
    u16::from_str_radix(digits, radix).map_err(|_| "ENTER A 16-BIT ADDRESS".to_owned())
}

fn parse_raster_breakpoint(input: &str) -> Result<(u16, u16), String> {
    let fields = input.split([',', ':', ' ']).filter(|field| !field.is_empty()).collect::<Vec<_>>();
    if fields.len() != 2 {
        return Err("USE LINE,DOT".to_owned());
    }
    let line = parse_debug_number(fields[0])?;
    let dot = parse_debug_number(fields[1])?;
    if line >= SCANLINES_PER_FRAME || dot >= DOTS_PER_SCANLINE {
        return Err(format!("LINE 0-{} DOT 0-{}", SCANLINES_PER_FRAME - 1, DOTS_PER_SCANLINE - 1));
    }
    if !u32::from(dot).is_multiple_of(VIDEO_DOTS_PER_CPU_CYCLE) {
        return Err("DOT MUST BE EVEN (CPU CYCLE BOUNDARY)".to_owned());
    }
    Ok((line, dot))
}

fn status_flags(status: u8) -> String {
    [(0x80, 'N'), (0x40, 'V'), (0x08, 'D'), (0x04, 'I'), (0x02, 'Z'), (0x01, 'C')]
        .into_iter()
        .map(|(mask, name)| if status & mask != 0 { name } else { '-' })
        .collect()
}

fn bank_kind_name(kind: u8) -> &'static str {
    match kind {
        bank_kind::CARTRIDGE_ROM => "ROM",
        bank_kind::WORK_RAM => "WRK",
        bank_kind::VIDEO_RAM => "VID",
        bank_kind::SAVE_RAM => "SAV",
        _ => "???",
    }
}

fn normalized_lines(text: &str) -> Vec<String> {
    let mut lines = text
        .replace("\r\n", "\n")
        .replace('\r', "\n")
        .split('\n')
        .map(str::to_owned)
        .collect::<Vec<_>>();
    if lines.is_empty() {
        lines.push(String::new());
    }
    lines
}

fn line_matches(lines: &[String], query: &str, whole_symbol: bool) -> Vec<Position> {
    lines
        .iter()
        .enumerate()
        .flat_map(|(line, text)| {
            text_matches(text, query, whole_symbol)
                .into_iter()
                .map(move |column| Position { line, column })
        })
        .collect()
}

fn text_matches(text: &str, query: &str, whole_symbol: bool) -> Vec<usize> {
    if query.is_empty() {
        return Vec::new();
    }
    let searchable = if whole_symbol { split_assembly_comment(text).0 } else { text };
    let lowercase = searchable.to_ascii_lowercase();
    let query = query.to_ascii_lowercase();
    lowercase
        .match_indices(&query)
        .filter_map(|(column, _)| {
            let before = searchable[..column].chars().next_back();
            let after = searchable[column + query.len()..].chars().next();
            (!whole_symbol
                || (before.is_none_or(|character| !is_symbol_character(character))
                    && after.is_none_or(|character| !is_symbol_character(character))))
            .then_some(column)
        })
        .collect()
}

fn is_symbol_character(character: char) -> bool {
    character.is_ascii_alphanumeric() || matches!(character, '_' | '.' | ']')
}

fn symbol_at(line: &str, cursor: usize) -> Option<&str> {
    if line.is_empty() {
        return None;
    }
    let bytes = line.as_bytes();
    let mut position = cursor.min(bytes.len());
    if position == bytes.len() || !is_symbol_character(char::from(bytes[position])) {
        position = position.checked_sub(1)?;
    }
    if !is_symbol_character(char::from(bytes[position])) {
        return None;
    }
    let mut start = position;
    while start > 0 && is_symbol_character(char::from(bytes[start - 1])) {
        start -= 1;
    }
    let mut end = position + 1;
    while end < bytes.len() && is_symbol_character(char::from(bytes[end])) {
        end += 1;
    }
    Some(&line[start..end])
}

fn assembly_definition(line: &str) -> Option<&str> {
    if line.starts_with(char::is_whitespace) {
        return None;
    }
    let code = split_assembly_comment(line).0.trim();
    let symbol = code.split_whitespace().next()?.trim_end_matches(':');
    (!symbol.is_empty() && !is_operation(symbol)).then_some(symbol)
}

fn search_preview(line: &str) -> String {
    line.trim().chars().take(48).collect()
}

fn configure_ui_palette(video: &mut Video) {
    video.set_palette(UI_WHITE_COLOR, [255, 255, 255, 255]);
    video.set_palette(UI_ERROR_BACKGROUND, [192, 32, 40, 255]);
    video.set_palette(UI_SUCCESS_BACKGROUND, [32, 80, 192, 255]);
    // Darkened Catppuccin blue/red keep white debugger text readable. These
    // two backgrounds receive the same four-step vertical shading as glyphs.
    video.set_palette(UI_DEBUG_CURRENT_BACKGROUND, [48, 70, 108, 255]);
    video.set_palette(UI_BREAKPOINT_BACKGROUND, [112, 52, 67, 255]);
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
        } else if bytes[index].is_ascii_alphabetic() || matches!(bytes[index], b'_' | b'.' | b']') {
            // Symbols may contain digits. Consume the complete identifier
            // before looking for numeric literals so `P1CTL` remains one
            // label/symbol token instead of highlighting `1C` as hexadecimal.
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'_' | b'.' | b']'))
            {
                index += 1;
            }
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
    fn mouse_click_places_cursor_and_drag_selects_text() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.lines = vec!["ABCDEFGH".to_owned(), "SECOND".to_owned()];

        assert_eq!(
            editor.handle_mouse_press(
                (EDITOR_CODE_START + 1) * GLYPH_WIDTH,
                EDITOR_FIRST_ROW * GLYPH_HEIGHT,
                false,
            ),
            EditorAction::None
        );
        assert_eq!(editor.cursor, Position { line: 0, column: 1 });
        editor.handle_mouse_move(
            (EDITOR_CODE_START + 5) * GLYPH_WIDTH,
            EDITOR_FIRST_ROW * GLYPH_HEIGHT,
        );
        editor.handle_mouse_release();

        assert_eq!(
            editor.selection(),
            Some((Position { line: 0, column: 1 }, Position { line: 0, column: 5 }))
        );
        assert_eq!(editor.selected_text().as_deref(), Some("BCDE"));
    }

    #[test]
    fn mouse_can_open_and_activate_editor_menus() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);

        editor.handle_mouse_press(GLYPH_WIDTH * 13, 0, false);
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::Build, selected: 0 }));
        editor.handle_mouse_move(GLYPH_WIDTH * 13, GLYPH_HEIGHT * 3);
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::Build, selected: 1 }));
        editor.handle_mouse_press(GLYPH_WIDTH * 13, GLYPH_HEIGHT * 3, false);
        assert!(matches!(editor.overlay, Overlay::Building { .. }));
        assert!(editor.build_and_run);
    }

    #[test]
    fn menu_separators_render_and_are_skipped_by_mouse_and_keyboard() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.open_menu(MenuKind::File);

        editor.handle_overlay_key(&Key::Named(NamedKey::ArrowUp), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, selected: 9 }));
        editor.handle_overlay_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, selected: 0 }));

        editor.handle_mouse_press(4 * GLYPH_WIDTH, 4 * GLYPH_HEIGHT, false);
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, .. }));

        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [0; COLUMNS * ROWS];
        let mut backgrounds = [0; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        editor.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut inverse,
            CellStyle { foreground: UI_WHITE_COLOR, background: 0 },
        );
        assert!(
            cells[4 * COLUMNS + 1..4 * COLUMNS + 15].iter().all(|cell| *cell == BOX_HORIZONTAL)
        );
    }

    #[test]
    fn debug_menu_shortcuts_share_one_right_aligned_column() {
        let content_width = menu_width(MenuKind::Debug) - 4;
        for label in menu_labels(MenuKind::Debug).iter().filter(|label| !label.is_empty()) {
            assert_eq!(label.len(), content_width, "misaligned debug menu label: {label}");
            assert_ne!(label.as_bytes().last(), Some(&b' '));
        }
    }

    #[test]
    fn editor_music_radio_shows_status_and_emits_transport_commands() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.set_music_status(Some(MusicStatus {
            filename: "music.nsf".to_owned(),
            title: "TEST ALBUM".to_owned(),
            artist: "COMPOSER".to_owned(),
            track: 2,
            tracks: 8,
            paused: false,
            looping: true,
        }));
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F7),
                PhysicalKey::Code(KeyCode::F7),
                ModifiersState::empty(),
            ),
            EditorAction::Music(MusicCommand::Pause)
        );
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F8),
                PhysicalKey::Code(KeyCode::F8),
                ModifiersState::empty(),
            ),
            EditorAction::Music(MusicCommand::Next)
        );
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F8),
                PhysicalKey::Code(KeyCode::F8),
                ModifiersState::SHIFT,
            ),
            EditorAction::Music(MusicCommand::Previous)
        );
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F8),
                PhysicalKey::Code(KeyCode::F8),
                ModifiersState::SUPER,
            ),
            EditorAction::Music(MusicCommand::ToggleLoop)
        );

        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, false);
        let labels = menu_labels(MenuKind::Music).iter().filter(|label| !label.is_empty());
        assert!(labels.clone().all(|label| label.len() == menu_width(MenuKind::Music) - 4));
        assert_eq!(menu_origin(MenuKind::Music), 27);
        assert_eq!(menu_bar_hit(26), None);
        assert_eq!(menu_bar_hit(27), Some(MenuKind::Music));

        let music_text =
            editor.music_status.as_ref().unwrap().display_marquee(editor.music_marquee_offset);
        let music_start = COLUMNS - music_text.len();
        let first_cell = (0..GLYPH_HEIGHT)
            .flat_map(|y| {
                let start = y * EDITOR_DISPLAY_WIDTH + music_start * GLYPH_WIDTH;
                &video.pixels()[start..start + GLYPH_WIDTH]
            })
            .copied()
            .collect::<Vec<_>>();
        assert!(first_cell.contains(&UI_SUCCESS_BACKGROUND));
        assert!(first_cell.contains(&UI_WHITE_COLOR));
    }

    #[test]
    fn project_browser_expands_directories_and_opens_text_files() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " NOP").unwrap();
        filesystem.borrow_mut().create_directory("source").unwrap();
        filesystem.borrow_mut().write_text("source/defs.inc", "VALUE EQU 1").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        let directory = editor
            .project_entries
            .iter()
            .position(|entry| entry.path.eq_ignore_ascii_case("source"))
            .unwrap();
        editor.activate_project_entry(directory);
        let include = editor
            .project_entries
            .iter()
            .position(|entry| entry.path.eq_ignore_ascii_case("source/defs.inc"))
            .unwrap();
        assert_eq!(editor.project_entries[include].depth, 1);

        editor.activate_project_entry(include);
        assert_eq!(editor.filename.as_deref(), Some("source/defs.inc"));
        assert_eq!(editor.lines, ["VALUE    EQU   1"]);
        assert!(!editor.project_focused);
    }

    #[test]
    fn project_browser_keeps_unsaved_changes_in_their_tab() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.asm", " NOP").unwrap();
        filesystem.borrow_mut().write_text("two.asm", " RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.asm".to_owned()));
        editor.lines = vec!["CHANGED".to_owned()];
        editor.dirty = true;
        let second = editor
            .project_entries
            .iter()
            .position(|entry| entry.path.eq_ignore_ascii_case("two.asm"))
            .unwrap();

        editor.activate_project_entry(second);

        assert_eq!(editor.filename.as_deref(), Some("two.asm"));
        assert_eq!(editor.tabs.len(), 2);
        editor.switch_tab(0);
        assert_eq!(editor.filename.as_deref(), Some("one.asm"));
        assert_eq!(editor.lines, ["CHANGED"]);
        assert!(editor.dirty);
    }

    #[test]
    fn project_browser_replaces_an_unmodified_untitled_tab() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("game.asm", " NOP").unwrap();
        let mut editor = TextEditor::new(filesystem, shared_ui_colors(), None);
        let source = editor
            .project_entries
            .iter()
            .position(|entry| entry.path.eq_ignore_ascii_case("game.asm"))
            .unwrap();

        editor.activate_project_entry(source);

        assert_eq!(editor.tabs.len(), 1);
        assert_eq!(editor.filename.as_deref(), Some("game.asm"));
        assert_eq!(editor.lines, ["         NOP"]);
    }

    #[test]
    fn project_browser_keeps_a_modified_untitled_tab() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("game.txt", "GAME").unwrap();
        let mut editor = TextEditor::new(filesystem, shared_ui_colors(), None);
        editor.insert_text("NOTES");
        let source = editor
            .project_entries
            .iter()
            .position(|entry| entry.path.eq_ignore_ascii_case("game.txt"))
            .unwrap();

        editor.activate_project_entry(source);

        assert_eq!(editor.tabs.len(), 2);
        editor.switch_tab(0);
        assert_eq!(editor.filename, None);
        assert_eq!(editor.lines, ["NOTES"]);
        assert!(editor.dirty);
    }

    #[test]
    fn find_next_previous_and_replace_operations_are_case_insensitive() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.lines = vec!["ONE two one".to_owned(), "TWO".to_owned()];
        editor.last_search = "one".to_owned();

        editor.find_next(true);
        assert_eq!(
            editor.selection(),
            Some((Position { line: 0, column: 0 }, Position { line: 0, column: 3 }))
        );
        editor.find_next(true);
        assert_eq!(
            editor.selection(),
            Some((Position { line: 0, column: 8 }, Position { line: 0, column: 11 }))
        );
        editor.find_next(false);
        assert_eq!(
            editor.selection(),
            Some((Position { line: 0, column: 0 }, Position { line: 0, column: 3 }))
        );

        editor.selection_anchor = None;
        editor.cursor = Position::default();
        editor.last_search = "two".to_owned();
        editor.replace_next("two", "THREE");
        assert_eq!(editor.lines[0], "ONE THREE one");
        assert_eq!(editor.selected_text().as_deref(), Some("TWO"));
        assert_eq!(editor.replace_all("one", "1"), 2);
        assert_eq!(editor.lines, ["1 THREE 1", "TWO"]);
    }

    #[test]
    fn project_search_reads_collapsed_folders_and_click_opens_a_result() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().create_directory("source").unwrap();
        filesystem.borrow_mut().write_text("source/defs.inc", "UNIQUE EQU 1").unwrap();
        let mut editor = TextEditor::new(filesystem, shared_ui_colors(), None);

        editor.show_project_search("unique");
        assert!(matches!(
            editor.overlay,
            Overlay::SearchResults { ref results, .. }
                if results.len() == 1 && results[0].path == "source/defs.inc"
        ));
        editor.handle_mouse_press(
            (SEARCH_RESULTS_X + 3) * GLYPH_WIDTH,
            (SEARCH_RESULTS_Y + 3) * GLYPH_HEIGHT,
            false,
        );

        assert_eq!(editor.filename.as_deref(), Some("source/defs.inc"));
        assert_eq!(editor.selected_text().as_deref(), Some("UNIQUE"));
    }

    #[test]
    fn project_search_uses_unsaved_open_tab_contents() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", "OLD NOP").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));
        editor.lines = vec!["NEWVALUE NOP".to_owned()];
        editor.dirty = true;

        let results = editor.search_project("newvalue", false, false);

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, "main.asm");
    }

    #[test]
    fn symbol_definition_references_and_navigation_history_cross_files() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " LDA VALUE").unwrap();
        filesystem.borrow_mut().create_directory("source").unwrap();
        filesystem.borrow_mut().write_text("source/defs.inc", "VALUE EQU 1").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));
        editor.cursor.column = editor.lines[0].find("VALUE").unwrap();
        let origin = editor.cursor;

        editor.handle_key(
            &Key::Named(NamedKey::F12),
            PhysicalKey::Code(KeyCode::F12),
            ModifiersState::empty(),
        );
        assert_eq!(editor.filename.as_deref(), Some("source/defs.inc"));
        assert_eq!(editor.selected_text().as_deref(), Some("VALUE"));

        editor.navigate_history(false);
        assert_eq!(editor.filename.as_deref(), Some("main.asm"));
        assert_eq!(editor.cursor, origin);
        editor.navigate_history(true);
        assert_eq!(editor.filename.as_deref(), Some("source/defs.inc"));

        editor.overlay = Overlay::None;
        editor.handle_key(
            &Key::Named(NamedKey::F12),
            PhysicalKey::Code(KeyCode::F12),
            ModifiersState::SHIFT,
        );
        assert!(matches!(
            editor.overlay,
            Overlay::SearchResults { ref results, .. } if results.len() == 2
        ));
    }

    #[test]
    fn search_and_go_to_shortcuts_open_the_expected_prompts() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.lines = vec!["FIRST".to_owned(), "SECOND".to_owned()];

        editor.handle_key(
            &key("f"),
            PhysicalKey::Code(KeyCode::KeyF),
            ModifiersState::SUPER | ModifiersState::SHIFT,
        );
        assert!(matches!(editor.overlay, Overlay::SearchPrompt { mode: SearchMode::Project, .. }));

        editor.overlay = Overlay::None;
        editor.handle_key(&key("h"), PhysicalKey::Code(KeyCode::KeyH), ModifiersState::SUPER);
        assert!(matches!(editor.overlay, Overlay::SearchPrompt { mode: SearchMode::Replace, .. }));

        editor.overlay = Overlay::None;
        editor.handle_key(&key("g"), PhysicalKey::Code(KeyCode::KeyG), ModifiersState::SUPER);
        editor.submit_search_prompt(SearchMode::GoToLine, "2".to_owned(), String::new());
        assert_eq!(editor.cursor, Position { line: 1, column: 0 });

        editor.last_search = "first".to_owned();
        editor.selection_anchor = None;
        editor.cursor = Position::default();
        editor.handle_key(
            &Key::Named(NamedKey::F3),
            PhysicalKey::Code(KeyCode::F3),
            ModifiersState::empty(),
        );
        assert_eq!(editor.selected_text().as_deref(), Some("FIRST"));
    }

    #[test]
    fn f5_always_starts_build_and_run_even_when_project_browser_is_focused() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.project_focused = true;

        editor.handle_key(
            &Key::Named(NamedKey::F5),
            PhysicalKey::Code(KeyCode::F5),
            ModifiersState::empty(),
        );

        assert!(editor.build_and_run);
        assert!(matches!(editor.overlay, Overlay::Building { .. }));
    }

    #[test]
    fn f9_and_the_gutter_toggle_source_breakpoints() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " NOP\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F9),
                PhysicalKey::Code(KeyCode::F9),
                ModifiersState::empty(),
            ),
            EditorAction::None
        );
        assert!(editor.line_has_breakpoint(0));

        editor.handle_mouse_press(
            EDITOR_START * GLYPH_WIDTH,
            EDITOR_FIRST_ROW * GLYPH_HEIGHT,
            false,
        );
        assert!(!editor.line_has_breakpoint(0));
    }

    #[test]
    fn debugger_rows_use_catppuccin_blue_and_red_with_white_text() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " NOP\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));
        editor.source_breakpoints.insert(("main.asm".to_owned(), 0));
        editor.debug_location = Some(("main.asm".to_owned(), 1));

        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut video, false);
        let cell_colors = |column: usize, row: usize| {
            (0..GLYPH_HEIGHT)
                .flat_map(|glyph_y| {
                    let start = (row * GLYPH_HEIGHT + glyph_y) * EDITOR_DISPLAY_WIDTH
                        + column * GLYPH_WIDTH;
                    &video.pixels()[start..start + GLYPH_WIDTH]
                })
                .copied()
                .collect::<Vec<_>>()
        };
        let breakpoint = cell_colors(EDITOR_START, EDITOR_FIRST_ROW);
        let executing = cell_colors(EDITOR_START + 1, EDITOR_FIRST_ROW + 1);

        assert!(breakpoint.contains(&UI_BREAKPOINT_BACKGROUND));
        assert!(breakpoint.contains(&UI_WHITE_COLOR));
        assert!(executing.contains(&UI_DEBUG_CURRENT_BACKGROUND));
        assert!(executing.contains(&UI_WHITE_COLOR));
        assert_eq!(video.palette()[UI_DEBUG_CURRENT_BACKGROUND as usize], [48, 70, 108, 255]);
        assert_eq!(video.palette()[UI_BREAKPOINT_BACKGROUND as usize], [112, 52, 67, 255]);

        let background_gradient = |row: usize| {
            let x = (COLUMNS - 1) * GLYPH_WIDTH;
            (0..GLYPH_HEIGHT)
                .map(|glyph_y| {
                    let index = (row * GLYPH_HEIGHT + glyph_y) * EDITOR_DISPLAY_WIDTH + x;
                    video.palette()[video.pixels()[index] as usize]
                })
                .collect::<Vec<_>>()
        };
        let red_gradient = background_gradient(EDITOR_FIRST_ROW);
        let blue_gradient = background_gradient(EDITOR_FIRST_ROW + 1);
        assert_eq!(red_gradient[0], [112, 52, 67, 255]);
        assert_eq!(blue_gradient[0], [48, 70, 108, 255]);
        assert!(red_gradient[7][0] < red_gradient[0][0]);
        assert!(blue_gradient[7][2] < blue_gradient[0][2]);
    }

    #[test]
    fn build_and_run_resolves_source_breakpoints_into_fixed_rom_addresses() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text(
            "fanticon.cfg",
            "TITLE=DEBUG TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=DEBUG.FCN\nSAVE_BANKS=0\nMACHINE=1.0\n",
        ).unwrap();
        filesystem.borrow_mut().write_text(
            "main.asm",
            " FIXED\n ORG $C100\nRESET NOP\nLOOP JMP LOOP\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ",
        ).unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));
        editor.cursor.line = 2;
        editor.toggle_source_breakpoint();
        editor.start_build(true);

        let mut action = EditorAction::None;
        for _ in 0..=BUILD_PROGRESS_FRAMES {
            action = editor.update();
        }
        let EditorAction::Run(launch) = action else { panic!("expected debug launch") };
        assert_eq!(launch.breakpoints, [(SymbolSection::Fixed, 0xc100)]);
        assert!(launch.source_map.iter().any(|entry| {
            entry.source == "main.asm" && entry.line == 3 && entry.address == 0xc100
        }));
        assert!(editor.debug_active);
    }

    #[test]
    fn paused_debug_snapshot_opens_source_and_debug_shortcuts_emit_commands() {
        use fanticon::{
            cartridge::Cartridge, debugger::Debugger, machine::BANK_SIZE, system::FanticonMachine,
        };

        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", "RESET NOP").unwrap();
        let mut editor = TextEditor::new(filesystem, shared_ui_colors(), None);
        editor.debug_source_map = vec![CartridgeSourceMapEntry {
            source: "main.asm".to_owned(),
            line: 1,
            address: 0xc100,
            length: 1,
            section: SymbolSection::Fixed,
        }];
        let mut fixed = [0xff; BANK_SIZE];
        fixed[0x100] = 0xea;
        fixed[0x3ffa..].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        let mut debugger = Debugger::new(FanticonMachine::new(
            Cartridge::new("DEBUG", 1, 0, fixed, Vec::new()).unwrap(),
            None,
        ));
        debugger.step_instruction();
        editor.set_debug_snapshot(debugger.snapshot());

        assert_eq!(editor.filename.as_deref(), Some("main.asm"));
        assert_eq!(editor.debug_location, Some(("main.asm".to_owned(), 0)));
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F10),
                PhysicalKey::Code(KeyCode::F10),
                ModifiersState::empty(),
            ),
            EditorAction::Debug(DebugCommand::StepOver)
        );
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F11),
                PhysicalKey::Code(KeyCode::F11),
                ModifiersState::CONTROL,
            ),
            EditorAction::Debug(DebugCommand::StepCycle)
        );
        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::F5),
                PhysicalKey::Code(KeyCode::F5),
                ModifiersState::empty(),
            ),
            EditorAction::Debug(DebugCommand::Continue)
        );
    }

    #[test]
    fn debugger_watchpoint_and_raster_dialogs_validate_their_inputs() {
        assert_eq!(parse_debug_number("$C020"), Ok(0xc020));
        assert_eq!(parse_debug_number("49184"), Ok(0xc020));
        assert_eq!(parse_raster_breakpoint("199,318"), Ok((199, 318)));
        assert!(parse_raster_breakpoint("999,999").is_err());
        assert!(parse_raster_breakpoint("1,3").is_err());

        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.debug_active = true;
        editor.open_debug_prompt(DebugPromptKind::WriteWatchpoint);
        assert_eq!(
            editor.submit_debug_prompt(DebugPromptKind::WriteWatchpoint, "$2000".to_owned()),
            EditorAction::Debug(DebugCommand::AddWriteWatchpoint(0x2000))
        );
    }

    #[test]
    fn tabs_preserve_independent_editing_cursor_scroll_and_undo_state() {
        let filesystem = shared_filesystem();
        filesystem
            .borrow_mut()
            .write_text("one.txt", &(0..60).map(|line| format!("ONE {line}\n")).collect::<String>())
            .unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.txt".to_owned()));
        editor.cursor = Position { line: 40, column: 3 };
        editor.scroll_line = 30;
        editor.insert_text("!");

        editor.load("two.txt").unwrap();
        editor.cursor.column = 3;
        editor.insert_text("?");
        assert_eq!(editor.tabs.len(), 2);

        editor.switch_tab(0);
        assert_eq!(editor.filename.as_deref(), Some("one.txt"));
        assert_eq!(editor.cursor, Position { line: 40, column: 4 });
        assert_eq!(editor.scroll_line, 30);
        assert!(editor.lines[40].contains("ONE! 40"));
        editor.undo();
        assert_eq!(editor.lines[40], "ONE 40");

        editor.switch_tab(1);
        assert_eq!(editor.lines, ["TWO?"]);
        assert_eq!(editor.cursor, Position { line: 0, column: 4 });
    }

    #[test]
    fn opening_an_existing_document_focuses_its_tab_without_duplicates() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.txt", "ONE").unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.txt".to_owned()));

        editor.load("two.txt").unwrap();
        editor.load("ONE.TXT").unwrap();

        assert_eq!(editor.tabs.len(), 2);
        assert_eq!(editor.active_tab, 0);
        assert_eq!(editor.filename.as_deref(), Some("one.txt"));
    }

    #[test]
    fn tab_shortcuts_cycle_and_dirty_close_requires_a_choice() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.txt", "ONE").unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.txt".to_owned()));
        editor.load("two.txt").unwrap();

        editor.handle_key(
            &Key::Named(NamedKey::Tab),
            PhysicalKey::Code(KeyCode::Tab),
            ModifiersState::SUPER,
        );
        assert_eq!(editor.active_tab, 0);
        editor.insert_text("!");
        editor.request_close_tab(0);
        assert!(matches!(editor.overlay, Overlay::CloseTab { tab: 0 }));

        editor.handle_overlay_key(&key("d"), ModifiersState::empty());
        assert_eq!(editor.tabs.len(), 1);
        assert_eq!(editor.filename.as_deref(), Some("two.txt"));
    }

    #[test]
    fn mouse_selects_and_closes_tabs() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.txt", "ONE").unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.txt".to_owned()));
        editor.load("two.txt").unwrap();

        editor.handle_mouse_press((EDITOR_START + 2) * GLYPH_WIDTH, GLYPH_HEIGHT, false);
        assert_eq!(editor.active_tab, 0);
        editor.handle_mouse_press((EDITOR_START + TAB_WIDTH) * GLYPH_WIDTH, GLYPH_HEIGHT, false);

        assert_eq!(editor.tabs.len(), 1);
        assert_eq!(editor.filename.as_deref(), Some("two.txt"));
    }

    #[test]
    fn tab_strip_separates_the_active_dirty_document_and_close_target() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_document();
        editor.insert_text("CHANGED");
        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [0; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];

        editor.render_tabs(&mut cells, &mut foregrounds, &mut inverse);

        let strip_start = COLUMNS + EDITOR_START;
        let inactive_start = COLUMNS + EDITOR_START + 1;
        let start = inactive_start + TAB_WIDTH;
        assert!(inverse[strip_start]);
        assert!(inverse[inactive_start..inactive_start + TAB_WIDTH].iter().all(|cell| *cell));
        assert!(inverse[start..start + TAB_WIDTH].iter().all(|cell| !*cell));
        assert!(inverse[start + TAB_WIDTH]);
        assert_eq!(cells[start + TAB_WIDTH - 3], b'*');
        assert_eq!(cells[start + TAB_WIDTH - 1], b'X');
        assert!(foregrounds[start..start + TAB_WIDTH].iter().all(|color| *color == UI_WHITE_COLOR));
    }

    #[test]
    fn saving_an_untitled_dirty_tab_from_close_finishes_the_close() {
        let filesystem = shared_filesystem();
        let mut editor = TextEditor::new(filesystem.clone(), shared_ui_colors(), None);
        editor.insert_text("SAVED");
        editor.request_close_tab(0);

        editor.handle_overlay_key(&key("s"), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::Dialog { kind: DialogKind::SaveAs, .. }));
        editor.submit_dialog(DialogKind::SaveAs, "note.txt".to_owned());

        assert_eq!(filesystem.borrow().read_text("note.txt").unwrap(), "SAVED");
        assert_eq!(editor.tabs.len(), 1);
        assert_eq!(editor.filename, None);
        assert_eq!(editor.lines, [""]);
        assert!(!editor.dirty);
    }

    #[test]
    fn save_as_rejects_a_filename_already_open_in_another_tab() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.txt", "ONE").unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("one.txt".to_owned()));
        editor.load("two.txt").unwrap();

        assert_eq!(editor.save_as("ONE.TXT"), Err("FILE IS ALREADY OPEN".to_owned()));
        assert_eq!(editor.filename.as_deref(), Some("two.txt"));
    }

    #[test]
    fn save_all_writes_every_named_dirty_tab() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("one.txt", "ONE").unwrap();
        filesystem.borrow_mut().write_text("two.txt", "TWO").unwrap();
        let mut editor =
            TextEditor::new(filesystem.clone(), shared_ui_colors(), Some("one.txt".to_owned()));
        editor.cursor.column = 3;
        editor.insert_text("!");
        editor.load("two.txt").unwrap();
        editor.cursor.column = 3;
        editor.insert_text("?");

        editor.save_all();

        assert_eq!(filesystem.borrow().read_text("one.txt").unwrap(), "ONE!");
        assert_eq!(filesystem.borrow().read_text("two.txt").unwrap(), "TWO?");
        assert!(editor.tabs.iter().all(|document| !document.dirty));
        assert!(!editor.dirty);
    }

    #[test]
    fn project_build_saves_dirty_include_tabs_before_assembly() {
        let filesystem = shared_filesystem();
        filesystem
            .borrow_mut()
            .write_text("main.asm", " PUT defs.inc\n ORG $8000\n LDA #VALUE")
            .unwrap();
        filesystem.borrow_mut().write_text("defs.inc", "VALUE EQU 1").unwrap();
        let mut editor =
            TextEditor::new(filesystem.clone(), shared_ui_colors(), Some("main.asm".to_owned()));
        editor.load("defs.inc").unwrap();
        editor.lines = vec!["VALUE EQU 2".to_owned()];
        editor.dirty = true;
        editor.switch_tab(0);

        editor.start_build(false);
        finish_pending_build(&mut editor);

        assert_eq!(filesystem.borrow().read_binary("main.bin").unwrap(), [0xa9, 0x02]);
        assert_eq!(filesystem.borrow().read_text("defs.inc").unwrap(), "VALUE    EQU   2");
    }

    #[test]
    fn mouse_wheel_scrolls_document_without_moving_cursor() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.lines = (0..100).map(|line| format!("LINE {line}")).collect();

        editor.handle_mouse_wheel(0.0, -3.0);
        assert_eq!(editor.scroll_line, 3);
        assert_eq!(editor.cursor, Position::default());
        editor.handle_mouse_wheel(0.0, 2.0);
        assert_eq!(editor.scroll_line, 1);
    }

    #[test]
    fn mouse_wheel_accumulates_high_resolution_trackpad_motion() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.lines = (0..100).map(|line| format!("LINE {line}")).collect();

        editor.handle_mouse_wheel(0.0, -0.4);
        editor.handle_mouse_wheel(0.0, -0.4);
        assert_eq!(editor.scroll_line, 0);
        editor.handle_mouse_wheel(0.0, -0.4);
        assert_eq!(editor.scroll_line, 1);
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
    fn blinking_cursor_is_a_white_block_with_black_cell_character() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("note.txt", "A").unwrap();
        let editor = TextEditor::new(filesystem, shared_ui_colors(), Some("note.txt".to_owned()));
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);

        editor.render(&mut video, true);

        let origin_x = EDITOR_CODE_START * GLYPH_WIDTH;
        let origin_y = EDITOR_FIRST_ROW * GLYPH_HEIGHT;
        for (glyph_y, bits) in CHARACTER_ROM[b'A' as usize].iter().copied().enumerate() {
            for glyph_x in 0..GLYPH_WIDTH {
                let pixel = video.pixels()
                    [(origin_y + glyph_y) * EDITOR_DISPLAY_WIDTH + origin_x + glyph_x];
                let is_character = bits & (0x80 >> glyph_x) != 0;
                assert_eq!(pixel, if is_character { 0 } else { UI_WHITE_COLOR });
            }
        }
    }

    #[test]
    fn asm_mode_keeps_ide_chrome_white() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "START NOP").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.open_menu(MenuKind::File);
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);

        editor.render(&mut video, false);

        assert_eq!(video.pixels()[0], UI_WHITE_COLOR);
        let border = CHARACTER_ROM[BOX_TOP_LEFT as usize]
            .iter()
            .enumerate()
            .find_map(|(y, bits)| {
                (0..GLYPH_WIDTH).find(|x| bits & (0x80 >> x) != 0).map(|x| (x, y))
            })
            .unwrap();
        let border_pixel =
            video.pixels()[(GLYPH_HEIGHT + border.1) * EDITOR_DISPLAY_WIDTH + border.0];
        let brightness = [255, 212, 170, 127][(border.1 / 2).min(3)];
        assert_eq!(
            video.palette()[border_pixel as usize][..3],
            [brightness, brightness, brightness]
        );
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
    fn asm_highlighting_does_not_treat_digits_inside_symbols_as_numbers() {
        let label = assembly_syntax_colors("P1CTL    EQU   $C010", ASM_TEXT_COLOR);
        assert!(label[..5].iter().all(|color| *color == ASM_LABEL_COLOR));
        assert!(label[15..20].iter().all(|color| *color == ASM_NUMBER_COLOR));

        let operand = assembly_syntax_colors("         LDA   P1CTL", ASM_TEXT_COLOR);
        assert!(operand[15..20].iter().all(|color| *color == ASM_TEXT_COLOR));
        assert!(!operand.contains(&ASM_NUMBER_COLOR));
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

        editor.start_build(false);
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
    fn build_uses_cartridge_assembler_when_project_manifest_exists() {
        let filesystem = shared_filesystem();
        filesystem
            .borrow_mut()
            .write_text(
                "fanticon.cfg",
                "TITLE=EDITOR TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\n\
                 OUTPUT=TEST.FCN\nSAVE_BANKS=0\nMACHINE=1.0\n",
            )
            .unwrap();
        filesystem
            .borrow_mut()
            .write_text(
                "main.asm",
                " FIXED\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n\
                 ORG $FFFA\n DA NMI,RESET,IRQ",
            )
            .unwrap();
        let mut editor =
            TextEditor::new(filesystem.clone(), shared_ui_colors(), Some("main.asm".to_owned()));

        editor.start_build(false);
        finish_pending_build(&mut editor);

        let bytes = filesystem.borrow().read_binary("test.fcn").unwrap();
        assert!(fanticon::cartridge::Cartridge::from_bytes(&bytes).is_ok());
        assert!(editor.diagnostics.is_empty());
        assert!(matches!(
            editor.overlay,
            Overlay::Message { ref title, .. } if title == "BUILD SUCCESSFUL"
        ));
    }

    #[test]
    fn malformed_manifest_still_selects_cartridge_build() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_binary("fanticon.cfg", &[0xff]).unwrap();
        filesystem.borrow_mut().write_text("main.asm", " FIXED").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        editor.start_build(false);
        finish_pending_build(&mut editor);

        assert!(editor.diagnostics.iter().any(|diagnostic| {
            diagnostic.source.eq_ignore_ascii_case("fanticon.cfg")
                && diagnostic.message == "TEXT FILE IS NOT UTF-8"
        }));
        assert!(
            !editor
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.message.contains("unknown operation 'FIXED'"))
        );
    }

    #[test]
    fn build_errors_select_the_source_location_and_edits_clear_them() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("bad.asm", " ORG $8000\n LDA #").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("bad.asm".to_owned()));

        editor.start_build(false);
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

    #[test]
    fn include_errors_open_the_correct_source_tab() {
        let filesystem = shared_filesystem();
        filesystem
            .borrow_mut()
            .write_text("main.asm", " PUT bad.inc\n ORG $8000\n LDA #VALUE")
            .unwrap();
        filesystem.borrow_mut().write_text("bad.inc", "VALUE EQU").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        editor.start_build(false);
        finish_pending_build(&mut editor);

        assert_eq!(editor.filename.as_deref(), Some("bad.inc"));
        assert_eq!(editor.tabs.len(), 2);
        assert_eq!(editor.cursor.line, 0);
        assert!(
            editor
                .current_diagnostic()
                .is_some_and(|diagnostic| { diagnostic.source.eq_ignore_ascii_case("bad.inc") })
        );
    }
}
