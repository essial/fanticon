use std::collections::{BTreeMap, BTreeSet};

use fanticon::{
    assembler::{
        BankUsage, CartridgeSourceMapEntry, CartridgeSymbol, Diagnostic, FANTICON_INCLUDE_NAME,
        FANTICON_INCLUDE_SOURCE, SymbolSection,
    },
    debugger::{DebugSnapshot, DebugStop},
    disassemble_instruction, instruction_length,
    machine::{VIDEO_DOTS_PER_CPU_CYCLE, bank_kind},
    project::MANIFEST_NAME,
    video::{
        DISPLAY_HEIGHT, DISPLAY_WIDTH, DOTS_PER_SCANLINE, SCANLINES_PER_FRAME, rgb332_to_rgba,
    },
};
use winit::keyboard::{Key, KeyCode, ModifiersState, NamedKey, PhysicalKey};

use super::help::{HelpCategory, HelpEntry, format_guide_body, shared_help_index};
use super::nsf_player::{MusicCommand, MusicStatus};
use super::surface::{Rgba, Surface, scanline_shade};
use super::{
    EDITOR_DISPLAY_HEIGHT, EDITOR_DISPLAY_WIDTH,
    boot_splash::BOOT_LOGO,
    builder::{GameLaunch, build_and_load_project, build_project, build_source},
    character_rom::{
        BOX_BOTTOM_HORIZONTAL, BOX_BOTTOM_LEFT, BOX_BOTTOM_RIGHT, BOX_CAPTION_LEFT,
        BOX_CAPTION_RIGHT, BOX_HORIZONTAL, BOX_RIGHT_VERTICAL, BOX_TOP_HORIZONTAL, BOX_TOP_LEFT,
        BOX_TOP_RIGHT, BOX_VERTICAL, CHARACTER_ROM, DBL_BOTTOM_HORIZONTAL, DBL_BOTTOM_LEFT,
        DBL_BOTTOM_RIGHT, DBL_CAPTION_LEFT, DBL_CAPTION_RIGHT, DBL_HORIZONTAL, DBL_RIGHT_VERTICAL,
        DBL_TOP_HORIZONTAL, DBL_TOP_LEFT, DBL_TOP_RIGHT, DBL_VERTICAL, GLYPH_HEIGHT, GLYPH_WIDTH,
        SHADE_LIGHT, SHADE_MEDIUM, SYMBOL_ARROW_DOWN, SYMBOL_ARROW_RIGHT, SYMBOL_ARROW_UP,
        SYMBOL_BUSY, SYMBOL_CHECK, SYMBOL_CROSS,
    },
    filesystem::{ConsoleFilesystem, SharedFilesystem},
    graphics_editor::{DEFAULT_PALETTE_FILE, GraphicsEditor},
    music_editor::MusicEditor,
    ui_colors::SharedUiColors,
};

const COLUMNS: usize = EDITOR_DISPLAY_WIDTH / GLYPH_WIDTH;
const ROWS: usize = EDITOR_DISPLAY_HEIGHT / GLYPH_HEIGHT;
const EDITOR_FIRST_ROW: usize = 2;
const TEXT_ROWS: usize = ROWS - EDITOR_FIRST_ROW - 1;
const PROJECT_WIDTH: usize = 20;
const EDITOR_START: usize = PROJECT_WIDTH + 1;
const EDITOR_CODE_START: usize = EDITOR_START + 2;
/// The rightmost column belongs to the scrollbar, so text stops one short of it.
const EDITOR_COLUMNS: usize = COLUMNS - EDITOR_CODE_START - 1;
const TAB_WIDTH: usize = 14;
const VISIBLE_TABS: usize = (EDITOR_COLUMNS - 2) / TAB_WIDTH;
const SEARCH_RESULTS_X: usize = 2;
const SEARCH_RESULTS_Y: usize = 3;
const SEARCH_RESULTS_WIDTH: usize = COLUMNS - 4;
const SEARCH_RESULTS_HEIGHT: usize = ROWS - 6;
const SEARCH_RESULTS_VISIBLE: usize = SEARCH_RESULTS_HEIGHT - 5;
const BANK_USAGE_WIDTH: usize = 44;
const BANK_USAGE_HEIGHT: usize = 20;
const BANK_USAGE_VISIBLE: usize = BANK_USAGE_HEIGHT - 6;
const HELP_X: usize = 2;
const HELP_Y: usize = 3;
const HELP_WIDTH: usize = COLUMNS - 4;
const HELP_HEIGHT: usize = ROWS - 6;
const HELP_LIST_WIDTH: usize = 16;
const HELP_PREVIEW_WIDTH: usize = HELP_WIDTH - HELP_LIST_WIDTH - 5;
const HELP_VISIBLE: usize = HELP_HEIGHT - 8;
const ASM_TEXT_COLOR: u8 = 240;
const ASM_LABEL_COLOR: u8 = 241;
const ASM_OPCODE_COLOR: u8 = 242;
const ASM_DIRECTIVE_COLOR: u8 = 243;
const ASM_NUMBER_COLOR: u8 = 244;
const ASM_COMMENT_COLOR: u8 = 245;
const ASM_STRING_COLOR: u8 = 246;
const ASM_ERROR_COLOR: u8 = 247;
const ASM_MACRO_COLOR: u8 = 255;
const UI_WHITE_COLOR: u8 = 248;
const UI_ERROR_BACKGROUND: u8 = 249;
const UI_SUCCESS_BACKGROUND: u8 = 250;
const UI_DEBUG_CURRENT_BACKGROUND: u8 = 251;
const UI_BREAKPOINT_BACKGROUND: u8 = 252;
const UI_CURRENT_LINE_BACKGROUND: u8 = 253;
const UI_SHADOW_COLOR: u8 = 254;

/// Reuses the ASM syntax palette as category tags in the help finder, so an
/// opcode result reads blue the same way it would in an assembly buffer.
const fn help_category_color(category: HelpCategory) -> u8 {
    match category {
        HelpCategory::Opcode => ASM_OPCODE_COLOR,
        HelpCategory::Directive => ASM_DIRECTIVE_COLOR,
        HelpCategory::Command => ASM_LABEL_COLOR,
        HelpCategory::Shortcut => ASM_NUMBER_COLOR,
        HelpCategory::Guide => ASM_STRING_COLOR,
    }
}

/// Reference cards are hand-formatted tables and are shown verbatim; guide
/// bodies are prose pulled from the docs and are word-wrapped to the preview
/// pane's width. Shared by rendering and by the preview scroll key handling
/// so both agree on how many lines a card actually has.
fn help_preview_lines(entry: &HelpEntry) -> Vec<String> {
    if entry.category == HelpCategory::Guide {
        format_guide_body(&entry.body, HELP_PREVIEW_WIDTH)
    } else {
        entry.body.clone()
    }
}
/// Frames the caret stays lit, then dark. Moving the caret restarts this phase
/// so it is always solid at the moment it lands somewhere new.
const CURSOR_BLINK_FRAMES: u32 = 30;
const BUILD_PROGRESS_FRAMES: u8 = 8;
const ABOUT_WIDTH: usize = 42;
const ABOUT_HEIGHT: usize = 24;
const ABOUT_LOGO_WIDTH: usize = 192;
const ABOUT_LOGO_HEIGHT: usize = 120;
const ABOUT_PALETTE: [[u8; 4]; 16] = [
    [0, 0, 0, 255],
    [0, 28, 44, 255],
    [0, 68, 100, 255],
    [0, 118, 158, 255],
    [0, 190, 220, 255],
    [35, 238, 255, 255],
    [25, 20, 75, 255],
    [48, 38, 140, 255],
    [76, 58, 215, 255],
    [112, 88, 255, 255],
    [85, 16, 90, 255],
    [150, 34, 145, 255],
    [220, 68, 192, 255],
    [255, 118, 225, 255],
    [180, 192, 255, 255],
    [255, 255, 255, 255],
];
const ABOUT_RASTER_WAVE: [i8; 32] = [
    0, 1, 2, 3, 4, 4, 5, 5, 5, 5, 4, 4, 3, 2, 1, 0, 0, -1, -2, -3, -4, -4, -5, -5, -5, -5, -4, -4,
    -3, -2, -1, 0,
];
const ABOUT_WAVE_EASE: [u16; 17] =
    [0, 3, 11, 24, 40, 59, 81, 104, 128, 152, 175, 197, 216, 232, 245, 253, 256];

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
    /// Draw the frame with the double rule that marks focus.
    double_frame: bool,
}

impl CellStyle {
    const fn new(foreground: u8, background: u8) -> Self {
        Self { foreground, background, double_frame: false }
    }

    const fn focused(self) -> Self {
        Self { double_frame: true, ..self }
    }
}

#[derive(Clone)]
struct Snapshot {
    lines: Vec<String>,
    cursor: Position,
}

/// Identifies a run of same-kind, cursor-adjacent edits (e.g. holding down a
/// letter key or Backspace) that should share a single undo snapshot instead
/// of cloning the whole document on every keystroke.
#[derive(Clone, Copy, PartialEq, Eq)]
enum EditRunKind {
    Insert,
    Backspace,
    DeleteForward,
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
    Help,
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

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
enum DebugView {
    #[default]
    State,
    Code,
    Memory,
    Video,
    Stops,
    Symbols,
}

impl DebugView {
    const ALL: [Self; 6] =
        [Self::State, Self::Code, Self::Memory, Self::Video, Self::Stops, Self::Symbols];
    const LABELS: [&'static str; 6] = ["State", "Code", "Memory", "Video", "Stops", "Symbols"];
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
    About {
        frame: u16,
    },
    HelpFinder {
        query: String,
        results: Vec<&'static HelpEntry>,
        selected: usize,
        scroll: usize,
        preview_scroll: usize,
    },
    /// Read-only "how much ROM is left" readout, one row per FIXED/BANK
    /// section, populated by a fresh build so it never goes stale.
    BankUsage {
        entries: Vec<BankUsage>,
        scroll: usize,
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
    WriteMemory { address: u16, value: u8 },
    RemoveStop(DebugStop),
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
    edit_run: Option<(EditRunKind, Position)>,
    dirty: bool,
    overlay: Overlay,
    diagnostics: Vec<Diagnostic>,
    diagnostic_index: Option<usize>,
    build_message: Option<String>,
    build_and_run: bool,
    pending_bank_usage: bool,
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
    debug_symbols: BTreeMap<String, CartridgeSymbol>,
    debug_view: DebugView,
    /// Whether the register/memory/video detail panel covers the editor. Stopping
    /// at a breakpoint leaves this hidden so the caret stays in the source file.
    debug_panel_visible: bool,
    /// Frames elapsed in the current caret blink phase, and the position that
    /// phase belongs to, so any movement can restart it.
    blink_frame: u32,
    blink_cursor: Position,
    debug_address: u16,
    debug_selected: usize,
    debug_video_page: usize,
    debug_memory_nibble: Option<u8>,
    debug_watches: Vec<String>,
    music_status: Option<MusicStatus>,
    music_marquee_frame: u8,
    music_marquee_offset: usize,
    graphics_tabs: BTreeMap<u32, GraphicsEditor>,
    graphics_source_views: BTreeSet<u32>,
    music_tabs: BTreeMap<u32, MusicEditor>,
    music_source_views: BTreeSet<u32>,
    music_audition_key: Option<PhysicalKey>,
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
            edit_run: None,
            dirty: false,
            overlay: Overlay::None,
            diagnostics: Vec::new(),
            diagnostic_index: None,
            build_message: None,
            build_and_run: false,
            pending_bank_usage: false,
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
            debug_symbols: BTreeMap::new(),
            debug_view: DebugView::State,
            debug_panel_visible: false,
            blink_frame: 0,
            blink_cursor: Position::default(),
            debug_address: 0,
            debug_selected: 0,
            debug_video_page: 0,
            debug_memory_nibble: None,
            debug_watches: Vec::new(),
            music_status: None,
            music_marquee_frame: 0,
            music_marquee_offset: 0,
            graphics_tabs: BTreeMap::new(),
            graphics_source_views: BTreeSet::new(),
            music_tabs: BTreeMap::new(),
            music_source_views: BTreeSet::new(),
            music_audition_key: None,
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

        if matches!(key, Key::Named(NamedKey::F1)) {
            self.open_help_finder();
            return EditorAction::None;
        }

        if (modifiers.control_key() || modifiers.super_key())
            && matches!(key, Key::Named(NamedKey::Tab))
        {
            self.cycle_tabs(!modifiers.shift_key());
            return EditorAction::None;
        }

        // Show or hide the debug detail panel over the editor.
        if self.debug_active
            && self.debug_snapshot.is_some()
            && (modifiers.control_key() || modifiers.super_key())
            && let Key::Character(text) = key
            && text.eq_ignore_ascii_case("d")
        {
            self.debug_panel_visible = !self.debug_panel_visible;
            return EditorAction::None;
        }

        // Selecting a view also reveals the panel when it is hidden.
        if self.debug_active
            && self.debug_snapshot.is_some()
            && (modifiers.control_key() || modifiers.super_key())
            && let Key::Character(text) = key
            && let Some(digit) = text.chars().next().and_then(|character| character.to_digit(10))
            && (1..=6).contains(&digit)
        {
            self.debug_view = DebugView::ALL[digit as usize - 1];
            self.debug_panel_visible = true;
            self.debug_selected = 0;
            self.debug_memory_nibble = None;
            return EditorAction::None;
        }

        if (modifiers.control_key() || modifiers.super_key())
            && let Key::Character(text) = key
        {
            if self.music_active() && !self.music_source_active() {
                match text.to_ascii_lowercase().as_str() {
                    "z" => {
                        if self.music_tabs.get_mut(&self.document_id).unwrap().undo() {
                            self.dirty = true;
                        }
                        return EditorAction::None;
                    }
                    "m" => {
                        self.toggle_music_source_view();
                        return EditorAction::None;
                    }
                    "s" => {
                        if modifiers.shift_key() {
                            self.save_all();
                        } else {
                            self.save_or_prompt();
                        }
                        return EditorAction::None;
                    }
                    "w" => {
                        self.request_close_tab(self.active_tab);
                        return EditorAction::None;
                    }
                    _ => return EditorAction::None,
                }
            } else if text.eq_ignore_ascii_case("m") && self.music_active() {
                self.toggle_music_source_view();
                return EditorAction::None;
            } else if self.graphics_active() && !self.graphics_source_active() {
                match text.to_ascii_lowercase().as_str() {
                    "c" => {
                        self.graphics_tabs.get_mut(&self.document_id).unwrap().copy();
                        return EditorAction::None;
                    }
                    "x" => {
                        let graphics = self.graphics_tabs.get_mut(&self.document_id).unwrap();
                        graphics.copy();
                        if graphics
                            .handle_key(&Key::Named(NamedKey::Delete), ModifiersState::empty())
                        {
                            self.dirty = true;
                            self.propagate_active_palette();
                        }
                        return EditorAction::None;
                    }
                    "v" => {
                        if self.graphics_tabs.get_mut(&self.document_id).unwrap().paste() {
                            self.dirty = true;
                            self.propagate_active_palette();
                        }
                        return EditorAction::None;
                    }
                    "z" => {
                        if self.graphics_tabs.get_mut(&self.document_id).unwrap().undo() {
                            self.dirty = true;
                            self.propagate_active_palette();
                        }
                        return EditorAction::None;
                    }
                    "g" => {
                        self.toggle_graphics_source_view();
                        return EditorAction::None;
                    }
                    _ => {}
                }
            } else if text.eq_ignore_ascii_case("g") && self.graphics_active() {
                self.toggle_graphics_source_view();
                return EditorAction::None;
            }
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
                    if modifiers.shift_key() {
                        self.new_graphics_document();
                    } else {
                        self.new_document();
                    }
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
                "=" if self.assembly_mode() => {
                    self.insert_banner_comment('=');
                    EditorAction::None
                }
                "-" if self.assembly_mode() => {
                    self.insert_banner_comment('-');
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
        if matches!(key, Key::Named(NamedKey::F7))
            && self.music_active()
            && !self.music_source_active()
        {
            if modifiers.shift_key() && self.music_status.is_some() {
                return EditorAction::Music(MusicCommand::Stop);
            }
            let filename = self.playback_filename();
            if let Some(status) = &self.music_status
                && status.filename.eq_ignore_ascii_case(&filename)
            {
                return EditorAction::Music(if status.paused {
                    MusicCommand::Play
                } else {
                    MusicCommand::Pause
                });
            }
            let source = self.music_tabs[&self.document_id].serialize(&filename);
            return EditorAction::Music(MusicCommand::LoadTracker { filename, source });
        }
        if matches!(key, Key::Named(NamedKey::F8))
            && self.music_active()
            && !self.music_source_active()
        {
            let filename = self.playback_filename();
            let source = self.music_tabs[&self.document_id].serialize(&filename);
            return EditorAction::Music(MusicCommand::LoadTracker { filename, source });
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

        // The panel is modal: while it is on screen it owns the keyboard, so the
        // source editor and project browser never see these keys. Stepping,
        // continuing, and Ctrl/Cmd+D are handled above and still work.
        if self.debug_paused() && self.debug_panel_visible {
            return self.handle_debug_key(key).unwrap_or(EditorAction::None);
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
                PhysicalKey::Code(KeyCode::KeyH) => self.open_menu(MenuKind::Help),
                _ => {}
            }
            return EditorAction::None;
        }

        if self.graphics_active() && !self.graphics_source_active() {
            match key {
                Key::Named(NamedKey::F10) => self.open_menu(MenuKind::File),
                Key::Named(NamedKey::F2) => self.save_or_prompt(),
                _ => {
                    if self
                        .graphics_tabs
                        .get_mut(&self.document_id)
                        .expect("active graphics tab")
                        .handle_key(key, modifiers)
                    {
                        self.dirty = true;
                        self.propagate_active_palette();
                    }
                }
            }
            return EditorAction::None;
        }
        if self.music_active() && !self.music_source_active() {
            if let Some(source) = self.music_tabs[&self.document_id].instrument_audition_source(key)
            {
                if self.music_audition_key == Some(physical_key) {
                    return EditorAction::None;
                }
                self.music_audition_key = Some(physical_key);
                return EditorAction::Music(MusicCommand::AuditionTracker { source });
            }
            match key {
                Key::Named(NamedKey::F10) => self.open_menu(MenuKind::File),
                Key::Named(NamedKey::F2) => self.save_or_prompt(),
                Key::Named(NamedKey::Space) => {
                    return EditorAction::Music(self.tracker_play_stop_command());
                }
                _ => {
                    if self
                        .music_tabs
                        .get_mut(&self.document_id)
                        .expect("active music tab")
                        .handle_key(key, modifiers)
                    {
                        self.dirty = true;
                    }
                }
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

    pub fn handle_key_release(&mut self, physical_key: PhysicalKey) -> EditorAction {
        if self.music_audition_key == Some(physical_key) {
            self.music_audition_key = None;
            EditorAction::Music(MusicCommand::Stop)
        } else {
            EditorAction::None
        }
    }

    pub fn cancel_music_audition(&mut self) -> EditorAction {
        if self.music_audition_key.take().is_some() {
            EditorAction::Music(MusicCommand::Stop)
        } else {
            EditorAction::None
        }
    }

    /// Whether the caret is lit this frame. Restarting the phase on movement keeps
    /// the caret solid while arrowing or typing instead of blinking out mid-motion.
    pub fn cursor_blink_visible(&self) -> bool {
        (self.blink_frame / CURSOR_BLINK_FRAMES).is_multiple_of(2)
    }

    pub fn update(&mut self) -> EditorAction {
        if self.cursor == self.blink_cursor {
            self.blink_frame = self.blink_frame.wrapping_add(1);
        } else {
            self.blink_cursor = self.cursor;
            self.blink_frame = 0;
        }
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
            Overlay::About { frame } => {
                *frame = frame.wrapping_add(1);
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
        if self.debug_snapshot.is_none() {
            self.debug_address = snapshot.pc;
            self.debug_selected = 0;
            self.debug_memory_nibble = None;
        }
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
            // Stopping belongs in the source: give the caret back to the editor.
            self.project_focused = false;
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
        let playback = status.as_ref().and_then(|current| {
            self.filename
                .as_deref()
                .is_some_and(|filename| filename.eq_ignore_ascii_case(&current.filename))
                .then_some(current.position)
                .flatten()
                .map(|(row, _)| (row, current.channel_levels))
        });
        if let Some(music) = self.music_tabs.get_mut(&self.document_id) {
            music.follow_playback(
                playback.map(|(row, _)| row),
                playback.map_or([0; 4], |(_, levels)| levels),
            );
        }
        self.music_status = status;
    }

    pub fn stop_debug_session(&mut self) {
        self.debug_active = false;
        self.debug_snapshot = None;
        self.debug_source_map.clear();
        self.debug_symbols.clear();
        self.debug_location = None;
    }

    pub fn show_debug_error(&mut self, error: String) {
        self.show_build_message("Debug Error", &[error]);
    }

    fn toggle_source_breakpoint(&mut self) -> EditorAction {
        let Some(source) = self.filename.clone().filter(|filename| assembly_filename(filename))
        else {
            self.show_build_message("Breakpoint", &["Open an ASM or INC file".to_owned()]);
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
                error: Some("Enter search text".to_owned()),
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
            self.show_build_message("Find", &[format!("Not found: {}", self.last_search)]);
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
        if query.is_empty() || self.read_only() {
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
            self.show_build_message("Project Search", &[format!("Not found: {query}")]);
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
            self.show_build_message("Definition", &["No symbol at cursor".to_owned()]);
            return;
        };
        let results = self.search_project(&symbol, true, true);
        let Some(result) = results.first().cloned() else {
            self.show_build_message("Definition", &[format!("Not found: {symbol}")]);
            return;
        };
        self.navigate_to_result(&result, true);
    }

    fn find_symbol_references(&mut self) {
        let Some(symbol) = self.word_under_cursor() else {
            self.show_build_message("References", &["No symbol at cursor".to_owned()]);
            return;
        };
        let results = self.search_project(&symbol, true, false);
        if results.is_empty() {
            self.show_build_message("References", &[format!("Not found: {symbol}")]);
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
        self.edit_run = None;
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
            .map(str::to_owned)
            .unwrap_or_else(|| format!("Untitled{id}"))
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
        let removed = self.tabs.remove(tab);
        self.graphics_tabs.remove(&removed.id);
        self.graphics_source_views.remove(&removed.id);
        self.music_tabs.remove(&removed.id);
        self.music_source_views.remove(&removed.id);
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
            Overlay::About { .. } => {
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
            Overlay::HelpFinder { .. } => return EditorAction::None,
            Overlay::BankUsage { .. } => {
                self.overlay = Overlay::None;
                return EditorAction::None;
            }
            Overlay::None => {}
        }

        if self.debug_snapshot.is_some() && cell_y == 2 && cell_x >= EDITOR_START + 1 {
            let mut start = EDITOR_START + 2;
            for (index, label) in DebugView::LABELS.iter().enumerate() {
                let end = start + label.len() + 2;
                if (start..end).contains(&cell_x) {
                    self.debug_view = DebugView::ALL[index];
                    self.debug_selected = 0;
                    self.debug_memory_nibble = None;
                    return EditorAction::None;
                }
                start = end + 1;
            }
        }

        if self.debug_snapshot.is_some() && self.debug_view == DebugView::Memory {
            let row = cell_y.checked_sub(6).filter(|row| *row < 16);
            let column =
                cell_x.checked_sub(29).filter(|column| *column % 3 < 2).map(|column| column / 3);
            if let (Some(row), Some(column)) = (row, column.filter(|column| *column < 16)) {
                self.debug_address =
                    (self.debug_address & 0xff00).wrapping_add((row * 16 + column) as u16);
                self.debug_memory_nibble = None;
                return EditorAction::None;
            }
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

        if self.graphics_active() && !self.graphics_source_active() {
            if self
                .graphics_tabs
                .get_mut(&self.document_id)
                .expect("active graphics tab")
                .handle_mouse_press(x, y)
            {
                self.dirty = true;
                self.propagate_active_palette();
            }
            return EditorAction::None;
        }
        if self.music_active() && !self.music_source_active() {
            if self.music_tabs[&self.document_id].play_button_hit(x, y) {
                return EditorAction::Music(self.tracker_play_stop_command());
            }
            if self
                .music_tabs
                .get_mut(&self.document_id)
                .expect("active music tab")
                .handle_mouse_press(x, y)
            {
                self.dirty = true;
            }
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

    fn handle_debug_key(&mut self, key: &Key) -> Option<EditorAction> {
        // Escape returns the keyboard to the source without leaving the session.
        if matches!(key, Key::Named(NamedKey::Escape)) {
            self.debug_panel_visible = false;
            return Some(EditorAction::None);
        }
        if matches!(key, Key::Named(NamedKey::Tab)) {
            let current = DebugView::ALL.iter().position(|view| *view == self.debug_view).unwrap();
            self.debug_view = DebugView::ALL[(current + 1) % DebugView::ALL.len()];
            self.debug_selected = 0;
            return Some(EditorAction::None);
        }
        let snapshot = self.debug_snapshot.as_ref()?;
        match self.debug_view {
            DebugView::State => None,
            DebugView::Code => match key {
                Key::Named(NamedKey::ArrowDown) => {
                    let length =
                        instruction_length(snapshot.address_space[self.debug_address as usize]);
                    self.debug_address = self.debug_address.wrapping_add(u16::from(length));
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::ArrowUp) => {
                    self.debug_address = self.debug_address.wrapping_sub(1);
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::Home) => {
                    self.debug_address = snapshot.pc;
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::Enter) => {
                    self.debug_view = DebugView::Memory;
                    self.debug_memory_nibble = None;
                    Some(EditorAction::None)
                }
                _ => None,
            },
            DebugView::Memory => {
                let delta = match key {
                    Key::Named(NamedKey::ArrowLeft) => Some(-1),
                    Key::Named(NamedKey::ArrowRight) => Some(1),
                    Key::Named(NamedKey::ArrowUp) => Some(-16),
                    Key::Named(NamedKey::ArrowDown) => Some(16),
                    Key::Named(NamedKey::PageUp) => Some(-256),
                    Key::Named(NamedKey::PageDown) => Some(256),
                    _ => None,
                };
                if let Some(delta) = delta {
                    self.debug_address = self.debug_address.wrapping_add_signed(delta);
                    self.debug_memory_nibble = None;
                    return Some(EditorAction::None);
                }
                if matches!(key, Key::Named(NamedKey::Home)) {
                    self.debug_address = snapshot.pc;
                    self.debug_memory_nibble = None;
                    return Some(EditorAction::None);
                }
                if matches!(key, Key::Named(NamedKey::Delete)) {
                    return Some(EditorAction::Debug(DebugCommand::WriteMemory {
                        address: self.debug_address,
                        value: 0,
                    }));
                }
                let nibble = match key {
                    Key::Character(text) if text.len() == 1 => text
                        .chars()
                        .next()
                        .and_then(|character| character.to_digit(16))
                        .map(|n| n as u8),
                    _ => None,
                };
                if let Some(nibble) = nibble {
                    if let Some(high) = self.debug_memory_nibble.take() {
                        let address = self.debug_address;
                        self.debug_address = self.debug_address.wrapping_add(1);
                        return Some(EditorAction::Debug(DebugCommand::WriteMemory {
                            address,
                            value: (high << 4) | nibble,
                        }));
                    }
                    self.debug_memory_nibble = Some(nibble);
                    return Some(EditorAction::None);
                }
                None
            }
            DebugView::Video => match key {
                Key::Named(NamedKey::ArrowLeft) => {
                    self.debug_video_page = self.debug_video_page.saturating_sub(1);
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::ArrowRight) => {
                    self.debug_video_page = (self.debug_video_page + 1).min(3);
                    Some(EditorAction::None)
                }
                _ => None,
            },
            DebugView::Stops => match key {
                Key::Named(NamedKey::ArrowUp) => {
                    self.debug_selected = self.debug_selected.saturating_sub(1);
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::ArrowDown) => {
                    self.debug_selected =
                        (self.debug_selected + 1).min(snapshot.stops.len().saturating_sub(1));
                    Some(EditorAction::None)
                }
                Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => snapshot
                    .stops
                    .get(self.debug_selected)
                    .copied()
                    .map(|stop| EditorAction::Debug(DebugCommand::RemoveStop(stop))),
                _ => None,
            },
            DebugView::Symbols => {
                let count = self.debug_symbols.len();
                match key {
                    Key::Named(NamedKey::ArrowUp) => {
                        self.debug_selected = self.debug_selected.saturating_sub(1);
                        Some(EditorAction::None)
                    }
                    Key::Named(NamedKey::ArrowDown) => {
                        self.debug_selected =
                            (self.debug_selected + 1).min(count.saturating_sub(1));
                        Some(EditorAction::None)
                    }
                    Key::Named(NamedKey::Enter) => {
                        if let Some((_, symbol)) =
                            self.debug_symbols.iter().nth(self.debug_selected)
                        {
                            self.debug_address = symbol.address;
                            self.debug_view = DebugView::Memory;
                        }
                        Some(EditorAction::None)
                    }
                    Key::Character(text) if text.eq_ignore_ascii_case("w") => {
                        if let Some((name, _)) = self.debug_symbols.iter().nth(self.debug_selected)
                        {
                            if let Some(index) =
                                self.debug_watches.iter().position(|watch| watch == name)
                            {
                                self.debug_watches.remove(index);
                            } else {
                                self.debug_watches.push(name.clone());
                            }
                        }
                        Some(EditorAction::None)
                    }
                    _ => None,
                }
            }
        }
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
        if self.graphics_active()
            && !self.graphics_source_active()
            && self
                .graphics_tabs
                .get_mut(&self.document_id)
                .expect("active graphics tab")
                .handle_mouse_move(x, y)
        {
            self.dirty = true;
            self.propagate_active_palette();
        }
        if self.music_active()
            && !self.music_source_active()
            && self
                .music_tabs
                .get_mut(&self.document_id)
                .expect("active music tab")
                .handle_mouse_move(x, y)
        {
            self.dirty = true;
        }
    }

    pub fn handle_mouse_release(&mut self) {
        if let Some(graphics) = self.graphics_tabs.get_mut(&self.document_id) {
            graphics.handle_mouse_release();
        }
        if let Some(music) = self.music_tabs.get_mut(&self.document_id) {
            music.handle_mouse_release();
        }
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
        if let Some(music) = self.music_tabs.get_mut(&self.document_id) {
            music.handle_mouse_wheel(vertical);
            return;
        }
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
            Err(error) => self.show_build_message("Open Error", &[error]),
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

    /// Dithered track with a solid thumb down the editor's right edge, plus the
    /// arrow caps. Position and length both follow the document.
    fn render_scrollbar(&self, cells: &mut [u8], foregrounds: &mut [u8]) {
        let column = COLUMNS - 1;
        let top = EDITOR_FIRST_ROW;
        let rows = TEXT_ROWS;
        if rows < 3 {
            return;
        }
        put_cell(cells, column, top, SYMBOL_ARROW_UP);
        put_cell(cells, column, top + rows - 1, SYMBOL_ARROW_DOWN);
        let track = rows - 2;
        let lines = self.lines.len().max(1);
        let thumb_size = (track * rows.min(lines) / lines).clamp(1, track);
        let span = lines.saturating_sub(rows);
        let thumb_top = if span == 0 {
            0
        } else {
            (self.scroll_line.min(span) * (track - thumb_size) + span / 2) / span
        };
        for row in 0..track {
            let inside = row >= thumb_top && row < thumb_top + thumb_size;
            put_cell(cells, column, top + 1 + row, if inside { SHADE_MEDIUM } else { SHADE_LIGHT });
            foregrounds[(top + 1 + row) * COLUMNS + column] =
                if inside { UI_WHITE_COLOR } else { UI_SHADOW_COLOR };
        }
        foregrounds[top * COLUMNS + column] = UI_WHITE_COLOR;
        foregrounds[(top + rows - 1) * COLUMNS + column] = UI_WHITE_COLOR;
    }

    fn render_project_browser(&self, cells: &mut [u8], inverse: &mut [bool]) {
        put_text_width(cells, 0, 1, " Project", PROJECT_WIDTH);
        inverse[COLUMNS..COLUMNS + PROJECT_WIDTH].fill(true);
        // The divider doubles up on whichever side holds the keyboard.
        let divider = if self.project_focused { DBL_VERTICAL } else { BOX_VERTICAL };
        for row in 1..ROWS - 1 {
            put_cell(cells, PROJECT_WIDTH, row, divider);
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
                .map(str::to_owned)
                .unwrap_or_else(|| format!("Untitled{id}"));
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
        background_gradients: &mut [bool],
        inverse: &mut [bool],
        style: CellStyle,
    ) {
        if !self.debug_panel_visible {
            return;
        }
        let Some(snapshot) = &self.debug_snapshot else { return };
        let x = EDITOR_START;
        let y = 2;
        let width = COLUMNS - EDITOR_START;
        let height = ROWS - 3;
        // The panel owns the keyboard while it is open, so it wears the focus rule.
        draw_caption_window(
            cells,
            foregrounds,
            backgrounds,
            background_gradients,
            inverse,
            CellRect { x, y, width, height },
            style.focused(),
        );
        let mut tab_x = x + 2;
        for (index, label) in DebugView::LABELS.iter().enumerate() {
            let text = format!(" {label} ");
            put_text(cells, tab_x, y, &text);
            if DebugView::ALL[index] == self.debug_view {
                inverse[y * COLUMNS + tab_x..y * COLUMNS + tab_x + text.len()].fill(true);
            }
            tab_x += text.len() + 1;
        }
        put_text_width(
            cells,
            x + 2,
            y + height - 2,
            "ESC/CTRL+D HIDE  1-6 VIEW  TAB NEXT  F5 CONTINUE  F10/F11 STEP",
            width - 4,
        );

        match self.debug_view {
            DebugView::State => {
                put_text(
                    cells,
                    x + 2,
                    y + 2,
                    &format!(
                        "PC ${:04X}  SP ${:02X}  A ${:02X}  X ${:02X}  Y ${:02X}",
                        snapshot.pc, snapshot.sp, snapshot.a, snapshot.x, snapshot.y
                    ),
                );
                put_text(
                    cells,
                    x + 2,
                    y + 3,
                    &format!(
                        "P ${:02X} {}   Cycles {}",
                        snapshot.status,
                        status_flags(snapshot.status),
                        snapshot.cycles
                    ),
                );
                put_text(
                    cells,
                    x + 2,
                    y + 4,
                    &format!(
                        "Bank {}:{:02X}   IRQ {:X}/{:X}   Raster {},{}",
                        bank_kind_name(snapshot.bank_kind),
                        snapshot.bank_number,
                        snapshot.irq_pending,
                        snapshot.irq_enable,
                        snapshot.raster_line,
                        snapshot.raster_dot
                    ),
                );
                put_text_width(
                    cells,
                    x + 2,
                    y + 5,
                    &format!("Stop {:?}", snapshot.reason),
                    width - 4,
                );
                put_text(
                    cells,
                    x + 2,
                    y + 7,
                    &format!(
                        "Next  {}",
                        disassemble_instruction(snapshot.pc, snapshot.instruction_bytes)
                    ),
                );
                put_text(cells, x + 2, y + 9, "Stack");
                for row in 0..2 {
                    let offset = row * 8;
                    let values = snapshot.stack[offset..offset + 8]
                        .iter()
                        .map(|value| format!("{value:02X}"))
                        .collect::<Vec<_>>()
                        .join(" ");
                    put_text(
                        cells,
                        x + 2,
                        y + 10 + row,
                        &format!("{:02X}: {values}", snapshot.sp.wrapping_add(1 + offset as u8)),
                    );
                }
                put_text(cells, x + 2, y + 14, "Recent Instructions");
                for (row, trace) in snapshot.trace.iter().rev().take(12).enumerate() {
                    put_text_width(
                        cells,
                        x + 2,
                        y + 15 + row,
                        &format!(
                            "${:04X}  {}",
                            trace.address,
                            disassemble_instruction(trace.address, trace.bytes)
                        ),
                        width - 4,
                    );
                }
                put_text(cells, x + 2, y + 29, "Audio");
                put_text(
                    cells,
                    x + 2,
                    y + 30,
                    &format!(
                        "P1 {:02X}/{:03X}  P2 {:02X}/{:03X}  Tri {:02X}/{:03X}  Noi {:02X}/{:X}",
                        snapshot.apu.pulse_control[0],
                        snapshot.apu.pulse_timer[0],
                        snapshot.apu.pulse_control[1],
                        snapshot.apu.pulse_timer[1],
                        snapshot.apu.triangle_control,
                        snapshot.apu.triangle_timer,
                        snapshot.apu.noise_control,
                        snapshot.apu.noise_period
                    ),
                );
            }
            DebugView::Code => {
                put_text(
                    cells,
                    x + 2,
                    y + 2,
                    "DISASSEMBLY                         HOME = CURRENT PC",
                );
                let mut address = self.debug_address;
                for row in 0..36 {
                    let bytes = core::array::from_fn(|offset| {
                        snapshot.address_space[address.wrapping_add(offset as u16) as usize]
                    });
                    let length = instruction_length(bytes[0]);
                    let raw = (0..length)
                        .map(|offset| format!("{:02X}", bytes[offset as usize]))
                        .collect::<Vec<_>>()
                        .join(" ");
                    put_text_width(
                        cells,
                        x + 2,
                        y + 4 + row,
                        &format!(
                            "{}${address:04X}  {raw:<8}  {}",
                            if address == snapshot.pc { ">" } else { " " },
                            disassemble_instruction(address, bytes)
                        ),
                        width - 4,
                    );
                    if address == self.debug_address {
                        inverse[(y + 4 + row) * COLUMNS + x + 1
                            ..(y + 4 + row) * COLUMNS + x + width - 1]
                            .fill(true);
                    }
                    address = address.wrapping_add(u16::from(length));
                }
            }
            DebugView::Memory => {
                let base = self.debug_address & 0xff00;
                put_text(
                    cells,
                    x + 2,
                    y + 2,
                    &format!("MEMORY PAGE ${base:04X}   TYPE HEX TO EDIT   DEL = 00"),
                );
                put_text(cells, x + 8, y + 3, "00 01 02 03 04 05 06 07 08 09 0A 0B 0C 0D 0E 0F");
                for row in 0..16 {
                    let address = base.wrapping_add((row * 16) as u16);
                    let values = (0..16)
                        .map(|column| {
                            format!(
                                "{:02X}",
                                snapshot.address_space[address.wrapping_add(column) as usize]
                            )
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    put_text(cells, x + 2, y + 4 + row, &format!("{address:04X}: {values}"));
                }
                let offset = self.debug_address.wrapping_sub(base) as usize;
                let selected_x = x + 8 + (offset % 16) * 3;
                let selected_y = y + 4 + offset / 16;
                inverse[selected_y * COLUMNS + selected_x..selected_y * COLUMNS + selected_x + 2]
                    .fill(true);
                put_text(
                    cells,
                    x + 2,
                    y + 22,
                    &format!(
                        "SELECTED ${:04X} = ${:02X}{}",
                        self.debug_address,
                        snapshot.address_space[self.debug_address as usize],
                        self.debug_memory_nibble
                            .map_or(String::new(), |nibble| format!("  NEW ${nibble:X}_"))
                    ),
                );
            }
            DebugView::Video => self.render_video_debug(cells, inverse, snapshot, x, y, width),
            DebugView::Stops => {
                put_text(
                    cells,
                    x + 2,
                    y + 2,
                    "Breakpoints and Watchpoints                 Del = Remove",
                );
                if snapshot.stops.is_empty() {
                    put_text(cells, x + 2, y + 4, "No managed stops");
                }
                for (row, stop) in snapshot.stops.iter().take(36).enumerate() {
                    put_text_width(cells, x + 2, y + 4 + row, &format_debug_stop(*stop), width - 4);
                    if row == self.debug_selected {
                        inverse[(y + 4 + row) * COLUMNS + x + 1
                            ..(y + 4 + row) * COLUMNS + x + width - 1]
                            .fill(true);
                    }
                }
            }
            DebugView::Symbols => {
                put_text(
                    cells,
                    x + 2,
                    y + 2,
                    "Symbols                 Enter = Memory   W = Watch/Unwatch",
                );
                for (row, (name, symbol)) in self
                    .debug_symbols
                    .iter()
                    .skip(self.debug_selected.saturating_sub(17))
                    .take(35)
                    .enumerate()
                {
                    let watched = if self.debug_watches.contains(name) { '*' } else { ' ' };
                    put_text_width(
                        cells,
                        x + 2,
                        y + 4 + row,
                        &format!(
                            "{watched} {name:<28} ${:04X}  {}",
                            symbol.address,
                            symbol_section_name(symbol.section)
                        ),
                        width - 4,
                    );
                    if self.debug_symbols.iter().position(|(candidate, _)| candidate == name)
                        == Some(self.debug_selected)
                    {
                        inverse[(y + 4 + row) * COLUMNS + x + 1
                            ..(y + 4 + row) * COLUMNS + x + width - 1]
                            .fill(true);
                    }
                }
                if !self.debug_watches.is_empty() {
                    put_text(cells, x + 2, y + height - 6, "Watches");
                    for (row, name) in self.debug_watches.iter().take(4).enumerate() {
                        if let Some(symbol) = self.debug_symbols.get(name) {
                            put_text(
                                cells,
                                x + 2,
                                y + height - 5 + row,
                                &format!(
                                    "{name} = ${:02X} @ ${:04X}",
                                    snapshot.address_space[symbol.address as usize], symbol.address
                                ),
                            );
                        }
                    }
                }
            }
        }
    }

    fn render_video_debug(
        &self,
        cells: &mut [u8],
        inverse: &mut [bool],
        snapshot: &DebugSnapshot,
        x: usize,
        y: usize,
        width: usize,
    ) {
        let pages = ["Overview", "Palette", "Tilemap", "Sprites"];
        put_text(
            cells,
            x + 2,
            y + 2,
            &format!("Video  < {} >   Left/Right changes page", pages[self.debug_video_page]),
        );
        match self.debug_video_page {
            0 => {
                let video = &snapshot.video;
                put_text(
                    cells,
                    x + 2,
                    y + 4,
                    &format!(
                        "Mode ${:02X}  Control ${:02X}  Backdrop ${:02X}",
                        video.mode, video.control, video.backdrop
                    ),
                );
                put_text(
                    cells,
                    x + 2,
                    y + 5,
                    &format!(
                        "Scroll X {:4}  Y {:4}    Raster IRQ X {:3}  Y {:3}",
                        video.scroll_x, video.scroll_y, video.raster_x, video.raster_y
                    ),
                );
                put_text(
                    cells,
                    x + 2,
                    y + 6,
                    &format!(
                        "Beam X {:3}  Y {:3}    Bitmap Pal {:X}  Sprite overflow {}",
                        snapshot.raster_dot,
                        snapshot.raster_line,
                        video.bitmap_palette,
                        if video.sprite_overflow { "Yes" } else { "No" }
                    ),
                );
                put_text(cells, x + 2, y + 8, "VRAM Layout");
                put_text(cells, x + 2, y + 9, "$0000-$1FFF  256 tile patterns");
                put_text(cells, x + 2, y + 10, "$2000-$27FF  64x32 tile map");
                put_text(cells, x + 2, y + 11, "$2800-$2FFF  Tile attributes");
                put_text(cells, x + 2, y + 12, "$3000-$30FF  32 sprites");
                put_text(cells, x + 2, y + 13, "$4000-$BFFF  Bitmap");
            }
            1 => {
                for row in 0..16 {
                    let values = (0..16)
                        .map(|column| format!("{:02X}", snapshot.video.palette[row * 16 + column]))
                        .collect::<Vec<_>>()
                        .join(" ");
                    put_text(cells, x + 2, y + 4 + row, &format!("{:X}0: {values}", row));
                }
            }
            2 => {
                put_text(cells, x + 2, y + 4, "Tile map (top-left 16x16 of 64x32)");
                for row in 0..16 {
                    let values = (0..16)
                        .map(|column| {
                            format!("{:02X}", snapshot.video.video_ram[0x2000 + row * 64 + column])
                        })
                        .collect::<Vec<_>>()
                        .join(" ");
                    put_text_width(
                        cells,
                        x + 2,
                        y + 5 + row,
                        &format!("{row:02X}: {values}"),
                        width - 4,
                    );
                }
            }
            _ => {
                put_text(cells, x + 2, y + 4, "Sprite  X    Y   Tile Attr Pal");
                for sprite in 0..32 {
                    let base = 0x3000 + sprite * 8;
                    let data = &snapshot.video.video_ram[base..base + 8];
                    put_text(
                        cells,
                        x + 2,
                        y + 5 + sprite,
                        &format!(
                            "  {sprite:02}   {:3}  {:3}   {:02X}   {:02X}   {:02X}",
                            u16::from_le_bytes([data[0], data[1]]),
                            data[2],
                            data[3],
                            data[4],
                            data[5]
                        ),
                    );
                }
            }
        }
        let selected = y * COLUMNS + x + 2;
        inverse[selected..selected].fill(true);
    }

    pub fn render(&self, surface: &mut Surface, cursor_visible: bool) {
        debug_assert_eq!(surface.dimensions(), (EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT));
        let colors = self.colors.get();
        let assembly_mode = self.assembly_mode();
        let background = if assembly_mode { 0 } else { colors.background };
        let foreground = if assembly_mode { ASM_TEXT_COLOR } else { colors.foreground };
        let mut cells = [b' '; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        let mut foregrounds = [foreground; COLUMNS * ROWS];
        let mut backgrounds = [background; COLUMNS * ROWS];
        let mut background_gradients = [false; COLUMNS * ROWS];

        put_text(&mut cells, 0, 0, " File  Edit  Build  Debug  Music  Help");
        if let Some(status) = &self.music_status {
            let text = status.display_marquee(self.music_marquee_offset);
            let start = COLUMNS.saturating_sub(text.len());
            put_text_width(&mut cells, start, 0, &text, COLUMNS - start);
            backgrounds[start..COLUMNS].fill(UI_SUCCESS_BACKGROUND);
        }
        inverse[..COLUMNS].fill(true);
        foregrounds[..COLUMNS].fill(UI_WHITE_COLOR);
        // The menu bar carries the same per-scanline shading as the rest of the
        // chrome. Graphics mode renders flat regardless, so this stays harmless
        // there even though the identity palette leaves no room for shades.
        background_gradients[..COLUMNS].fill(true);

        if (!self.graphics_active() || self.graphics_source_active())
            && (!self.music_active() || self.music_source_active())
        {
            for screen_y in 0..TEXT_ROWS {
                let line_index = self.scroll_line + screen_y;
                let Some(line) = self.lines.get(line_index) else { break };
                let syntax = assembly_mode.then(|| assembly_syntax_colors(line, foreground));
                for (screen_x, byte) in
                    line.bytes().skip(self.scroll_column).take(EDITOR_COLUMNS).enumerate()
                {
                    let index =
                        (screen_y + EDITOR_FIRST_ROW) * COLUMNS + EDITOR_CODE_START + screen_x;
                    let source_column = self.scroll_column + screen_x;
                    cells[index] = byte;
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
                    let end = row * COLUMNS + COLUMNS - 1;
                    foregrounds[start..end].fill(UI_WHITE_COLOR);
                    backgrounds[start..end].fill(if executing {
                        UI_DEBUG_CURRENT_BACKGROUND
                    } else {
                        UI_BREAKPOINT_BACKGROUND
                    });
                    background_gradients[start..end].fill(true);
                    inverse[start..end].fill(false);
                    if executing && breakpoint {
                        backgrounds[start] = UI_BREAKPOINT_BACKGROUND;
                    }
                } else if line_index == self.cursor.line && !self.read_only() {
                    // Editable buffers mark the caret's line; the debugger's own
                    // blue and red rows above always win when a session is live.
                    let start = row * COLUMNS + EDITOR_START;
                    let end = row * COLUMNS + COLUMNS - 1;
                    backgrounds[start..end].fill(UI_CURRENT_LINE_BACKGROUND);
                    background_gradients[start..end].fill(true);
                }
            }
            self.render_scrollbar(&mut cells, &mut foregrounds);
        }

        let name = self.filename.as_deref().unwrap_or("Untitled.txt");
        let dirty = if self.dirty { "*" } else { " " };
        let status = self
            .debug_paused()
            .then(|| {
                format!(
                    " {name}  PAUSED - READ ONLY  LN {}  CTRL/CMD+D DETAILS  F5 CONTINUE",
                    self.cursor.line + 1
                )
            })
            .or_else(|| {
                self.current_diagnostic().map(|diagnostic| {
                    format!(
                        " {}:{}:{} {}",
                        diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
                    )
                })
            })
            .or_else(|| self.build_message.as_ref().map(|message| format!(" {message}")))
            .or_else(|| {
                self.system_read_only().then(|| {
                    format!(
                        " {name}  SYSTEM - READ ONLY  LN {} COL {}",
                        self.cursor.line + 1,
                        self.cursor.column + 1
                    )
                })
            })
            .or_else(|| self.ambient_help_status())
            .unwrap_or_else(|| {
                if self.graphics_active() && !self.graphics_source_active() {
                    return self.graphics_tabs[&self.document_id].status();
                }
                if self.music_active() && !self.music_source_active() {
                    return self.music_tabs[&self.document_id].status();
                }
                format!(
                    " {name}{dirty}  LN {} COL {}",
                    self.cursor.line + 1,
                    self.cursor.column + 1
                )
            });
        put_text(&mut cells, 0, ROWS - 1, &status);
        // Function-key legend on the right, the way every DOS IDE ended its
        // status line. It yields to any message that needs the whole row.
        let keys = " F1 HELP  F2 SAVE  F5 RUN  F9 BREAK  F10 MENU ";
        if status.len() + keys.len() < COLUMNS {
            put_text(&mut cells, COLUMNS - keys.len(), ROWS - 1, keys);
        }
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
            &mut background_gradients,
            &mut inverse,
            CellStyle::new(UI_WHITE_COLOR, background),
        );

        self.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            CellStyle::new(UI_WHITE_COLOR, background),
        );
        render_cells(
            surface,
            &cells,
            &foregrounds,
            &backgrounds,
            &inverse,
            &background_gradients,
            CellStyle::new(foreground, background),
        );

        let native_resource = (self.graphics_active() && !self.graphics_source_active())
            || (self.music_active() && !self.music_source_active());
        if native_resource {
            if self.graphics_active() {
                self.graphics_tabs[&self.document_id].render(surface);
            } else {
                self.music_tabs[&self.document_id].render(surface);
            }
            if !matches!(self.overlay, Overlay::None) {
                let mut mask_cells = [u8::MAX; COLUMNS * ROWS];
                let mut mask_foregrounds = [u8::MAX; COLUMNS * ROWS];
                let mut mask_backgrounds = [u8::MAX; COLUMNS * ROWS];
                let mut mask_inverse = [true; COLUMNS * ROWS];
                let mut mask_gradients = [false; COLUMNS * ROWS];
                self.render_overlay(
                    &mut mask_cells,
                    &mut mask_foregrounds,
                    &mut mask_backgrounds,
                    &mut mask_gradients,
                    &mut mask_inverse,
                    CellStyle::new(UI_WHITE_COLOR, background),
                );
                // The mask arrays are coverage sentinels rather than colors, so
                // they keep their u8::MAX markers; the real colors beneath are
                // what this actually draws.
                render_masked_cells(
                    surface,
                    &cells,
                    &foregrounds,
                    &backgrounds,
                    &inverse,
                    &background_gradients,
                    &mask_cells,
                    &mask_foregrounds,
                    &mask_backgrounds,
                    &mask_inverse,
                    CellStyle::new(foreground, background),
                );
            }
        }

        if let Overlay::About { frame } = &self.overlay {
            draw_about_logo(surface, *frame);
        }

        if cursor_visible
            && !self.project_focused
            && self.debug_snapshot.is_none()
            && matches!(self.overlay, Overlay::None)
            && (!self.graphics_active() || self.graphics_source_active())
            && (!self.music_active() || self.music_source_active())
            && let Some(screen_line) = self.cursor.line.checked_sub(self.scroll_line)
            && screen_line < TEXT_ROWS
            && let Some(screen_column) = self.cursor.column.checked_sub(self.scroll_column)
            && screen_column < EDITOR_COLUMNS
        {
            let cell_x = EDITOR_CODE_START + screen_column;
            let cell_y = screen_line + EDITOR_FIRST_ROW;
            draw_block_cursor(surface, cell_x, cell_y, cells[cell_y * COLUMNS + cell_x]);
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

        if matches!(self.overlay, Overlay::HelpFinder { .. }) {
            let mut close = false;
            let mut query_changed = false;
            if let Overlay::HelpFinder { query, selected, scroll, results, preview_scroll } =
                &mut self.overlay
            {
                match key {
                    Key::Named(NamedKey::Escape | NamedKey::Enter) => close = true,
                    Key::Named(NamedKey::Backspace) => {
                        if query.pop().is_some() {
                            query_changed = true;
                        }
                    }
                    Key::Named(NamedKey::ArrowUp) => {
                        *selected = selected.saturating_sub(1);
                        *scroll = (*scroll).min(*selected);
                        *preview_scroll = 0;
                    }
                    Key::Named(NamedKey::ArrowDown) if !results.is_empty() => {
                        *selected = (*selected + 1).min(results.len() - 1);
                        if *selected >= *scroll + HELP_VISIBLE {
                            *scroll = *selected + 1 - HELP_VISIBLE;
                        }
                        *preview_scroll = 0;
                    }
                    // Page Up/Down scroll the preview pane rather than the
                    // list: the list rarely has more than a screenful of
                    // matches, but a guide section's wrapped prose often
                    // does, and that is the content with no other way to
                    // reach its tail.
                    Key::Named(NamedKey::PageUp) => {
                        *preview_scroll = preview_scroll.saturating_sub(HELP_VISIBLE);
                    }
                    Key::Named(NamedKey::PageDown) => {
                        let max_scroll = results
                            .get(*selected)
                            .map(|entry| {
                                help_preview_lines(entry).len().saturating_sub(HELP_VISIBLE)
                            })
                            .unwrap_or(0);
                        *preview_scroll = (*preview_scroll + HELP_VISIBLE).min(max_scroll);
                    }
                    Key::Named(NamedKey::Space) => {
                        query.push(' ');
                        query_changed = true;
                    }
                    Key::Character(text) if !modifiers.control_key() && !modifiers.super_key() => {
                        let filtered: String =
                            text.chars().filter(|character| character.is_ascii_graphic()).collect();
                        if !filtered.is_empty() {
                            query.push_str(&filtered);
                            query_changed = true;
                        }
                    }
                    _ => {}
                }
            }
            if close {
                self.overlay = Overlay::None;
            } else if query_changed {
                self.refresh_help_finder();
            }
            return EditorAction::None;
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
        if matches!(self.overlay, Overlay::BankUsage { .. }) {
            if let Overlay::BankUsage { entries, scroll } = &mut self.overlay {
                let max_scroll = entries.len().saturating_sub(BANK_USAGE_VISIBLE);
                match key {
                    Key::Named(NamedKey::Escape | NamedKey::Enter) => self.overlay = Overlay::None,
                    Key::Named(NamedKey::ArrowUp) => *scroll = scroll.saturating_sub(1),
                    Key::Named(NamedKey::ArrowDown) => *scroll = (*scroll + 1).min(max_scroll),
                    Key::Named(NamedKey::PageUp) => {
                        *scroll = scroll.saturating_sub(BANK_USAGE_VISIBLE);
                    }
                    Key::Named(NamedKey::PageDown) => {
                        *scroll = (*scroll + BANK_USAGE_VISIBLE).min(max_scroll);
                    }
                    _ => {}
                }
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
                self.show_build_message("Replace", &[format!("Replaced {count} matches")]);
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
                        Err(error) => self.show_build_message("Save Error", &[error]),
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
        if matches!(self.overlay, Overlay::About { .. }) {
            if matches!(key, Key::Named(NamedKey::Enter | NamedKey::Escape)) {
                self.overlay = Overlay::None;
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
            | Overlay::DebugPrompt { .. }
            | Overlay::About { .. }
            | Overlay::HelpFinder { .. }
            | Overlay::BankUsage { .. } => {}
        }
        EditorAction::None
    }

    fn activate_menu(&mut self, menu: MenuKind, selected: usize) -> EditorAction {
        if menu == MenuKind::Music
            && selected == 0
            && self.music_active()
            && !self.music_source_active()
        {
            let filename = self.playback_filename();
            if let Some(status) = &self.music_status
                && status.filename.eq_ignore_ascii_case(&filename)
            {
                return EditorAction::Music(if status.paused {
                    MusicCommand::Play
                } else {
                    MusicCommand::Pause
                });
            }
            let source = self.music_tabs[&self.document_id].serialize(&filename);
            return EditorAction::Music(MusicCommand::LoadTracker { filename, source });
        }
        if menu == MenuKind::Edit && self.music_active() && !self.music_source_active() {
            if selected == 0 && self.music_tabs.get_mut(&self.document_id).unwrap().undo() {
                self.dirty = true;
            }
            return EditorAction::None;
        }
        if menu == MenuKind::Edit && self.graphics_active() && !self.graphics_source_active() {
            match selected {
                0 => {
                    if self.graphics_tabs.get_mut(&self.document_id).unwrap().undo() {
                        self.dirty = true;
                        self.propagate_active_palette();
                    }
                }
                2 => {
                    let graphics = self.graphics_tabs.get_mut(&self.document_id).unwrap();
                    graphics.copy();
                    if graphics.handle_key(&Key::Named(NamedKey::Delete), ModifiersState::empty()) {
                        self.dirty = true;
                        self.propagate_active_palette();
                    }
                }
                3 => self.graphics_tabs.get_mut(&self.document_id).unwrap().copy(),
                4 => {
                    if self.graphics_tabs.get_mut(&self.document_id).unwrap().paste() {
                        self.dirty = true;
                        self.propagate_active_palette();
                    }
                }
                _ => {}
            }
            return EditorAction::None;
        }
        match (menu, selected) {
            (MenuKind::File, 0) => self.new_document(),
            (MenuKind::File, 1) => self.new_graphics_document(),
            (MenuKind::File, 2) => self.new_palette_document(),
            (MenuKind::File, 3) => self.new_music_document(),
            (MenuKind::File, 4) => self.open_dialog(DialogKind::Open),
            (MenuKind::File, 6) => self.save_or_prompt(),
            (MenuKind::File, 7) => self.open_dialog(DialogKind::SaveAs),
            (MenuKind::File, 8) => self.save_all(),
            (MenuKind::File, 10) => self.request_close_tab(self.active_tab),
            (MenuKind::File, 12) => {
                if self.any_dirty_tabs() {
                    self.show_build_message(
                        "Unsaved Tabs",
                        &["Save or close dirty tabs before exit".to_owned()],
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
            (MenuKind::Build, 6) => self.start_bank_usage(),
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
            (MenuKind::Debug, 15) if self.debug_active && self.debug_snapshot.is_some() => {
                self.debug_panel_visible = !self.debug_panel_visible;
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
            (MenuKind::Help, 0) => self.open_help_finder(),
            (MenuKind::Help, 1) => self.overlay = Overlay::About { frame: 0 },
            _ => {}
        }
        EditorAction::None
    }

    fn render_overlay(
        &self,
        cells: &mut [u8],
        foregrounds: &mut [u8],
        backgrounds: &mut [u8],
        background_gradients: &mut [bool],
        inverse: &mut [bool],
        style: CellStyle,
    ) {
        match &self.overlay {
            Overlay::None => {}
            Overlay::Menu { menu, selected } => {
                let x = menu_origin(*menu);
                let width = menu_width(*menu);
                let y = 1;
                let rect = CellRect { x, y, width, height: menu_labels(*menu).len() + 2 };
                draw_shadow(cells, foregrounds, backgrounds, background_gradients, rect);
                draw_window(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    rect,
                    style.focused(),
                );
                for (index, item) in menu_labels(*menu).iter().enumerate() {
                    let row = y + index + 1;
                    if item.is_empty() {
                        for column in x + 1..x + width - 1 {
                            put_cell(cells, column, row, BOX_HORIZONTAL);
                        }
                        continue;
                    }
                    put_text_width(cells, x + 1, row, item, width - 2);
                    if index == *selected {
                        inverse[row * COLUMNS + x + 1..row * COLUMNS + x + width - 1].fill(true);
                    }
                }
            }
            Overlay::Dialog { kind, input, error } => {
                let title = if *kind == DialogKind::Open { "Open File" } else { "Save File" };
                let width = 32;
                let height = 8;
                let x = (COLUMNS - width) / 2;
                let y = (ROWS - height) / 2;
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x, y, width, height },
                    style,
                );
                put_text_width(cells, x + 3, y, title, width - 6);
                put_cell(cells, x + 2, y + 2, SYMBOL_ARROW_RIGHT);
                put_text(cells, x + 4, y + 2, "Name:");
                put_text_width(cells, x + 10, y + 2, input, width - 11);
                put_text(cells, x + 3, y + height - 2, "Enter=OK  Esc=Cancel");
                if let Some(error) = error {
                    put_cell(cells, x + 2, y + 4, SYMBOL_CROSS);
                    put_text_width(cells, x + 4, y + 4, error, width - 5);
                }
            }
            Overlay::DebugPrompt { kind, input, error } => {
                let title = match kind {
                    DebugPromptKind::ReadWatchpoint => "Read Watchpoint",
                    DebugPromptKind::WriteWatchpoint => "Write Watchpoint",
                    DebugPromptKind::RasterBreakpoint => "Raster Breakpoint",
                };
                let label = if *kind == DebugPromptKind::RasterBreakpoint {
                    "Line,Dot:"
                } else {
                    "Address:"
                };
                let width = 42;
                let height = 9;
                let x = (COLUMNS - width) / 2;
                let y = (ROWS - height) / 2;
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x, y, width, height },
                    style,
                );
                put_text_width(cells, x + 3, y, title, width - 6);
                put_cell(cells, x + 2, y + 2, SYMBOL_ARROW_RIGHT);
                put_text(cells, x + 4, y + 2, label);
                put_text_width(cells, x + 14, y + 2, input, width - 16);
                put_text(cells, x + 3, y + height - 2, "Enter=Add  Esc=Cancel");
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
                    background_gradients,
                    inverse,
                    style,
                    "Build",
                    &["Assembling...".to_owned()],
                );
            }
            Overlay::Message { title, lines } => {
                let message_style = if title == "Build Successful" {
                    CellStyle::new(UI_WHITE_COLOR, UI_SUCCESS_BACKGROUND)
                } else if title.contains("Error") {
                    CellStyle::new(UI_WHITE_COLOR, UI_ERROR_BACKGROUND)
                } else {
                    style
                };
                render_message_box(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
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
                    background_gradients,
                    inverse,
                    style,
                    "Unsaved Tab",
                    &[name],
                );
            }
            Overlay::SearchPrompt { mode, query, replacement, field, error } => {
                let title = match mode {
                    SearchMode::Find => "Find",
                    SearchMode::Replace => "Find and Replace",
                    SearchMode::Project => "Find in Project",
                    SearchMode::GoToLine => "Go to Line",
                };
                let width = 54;
                let height = if *mode == SearchMode::Replace { 11 } else { 9 };
                let x = (COLUMNS - width) / 2;
                let y = (ROWS - height) / 2;
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x, y, width, height },
                    style,
                );
                put_text_width(cells, x + 3, y, title, width - 6);
                let query_label = if *mode == SearchMode::GoToLine { "Line:" } else { "Find:" };
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
                    put_text(cells, x + 4, y + 4, "Replace:");
                    put_text_width(cells, x + 13, y + 4, replacement, width - 15);
                    put_text(
                        cells,
                        x + 3,
                        y + height - 2,
                        "Enter=Next  F8=All  Tab=Field  Esc=Cancel",
                    );
                } else {
                    put_text(cells, x + 3, y + height - 2, "Enter=OK  Esc=Cancel");
                }
                if let Some(error) = error {
                    put_cell(cells, x + 2, y + height - 4, SYMBOL_CROSS);
                    put_text_width(cells, x + 4, y + height - 4, error, width - 6);
                }
            }
            Overlay::SearchResults { query, results, selected, scroll } => {
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
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
                    &format!("Search: {query}  {} matches", results.len()),
                    SEARCH_RESULTS_WIDTH - 6,
                );
                put_text(
                    cells,
                    SEARCH_RESULTS_X + 2,
                    SEARCH_RESULTS_Y + 2,
                    "File:Line:Col  Source",
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
                    "Enter/Click=Open  Esc=Close",
                );
            }
            Overlay::BankUsage { entries, scroll } => {
                let x = (COLUMNS - BANK_USAGE_WIDTH) / 2;
                let y = (ROWS - BANK_USAGE_HEIGHT) / 2;
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x, y, width: BANK_USAGE_WIDTH, height: BANK_USAGE_HEIGHT },
                    style,
                );
                put_text_width(cells, x + 3, y, "ROM Bank Usage", BANK_USAGE_WIDTH - 6);
                for (screen_row, entry) in
                    entries.iter().skip(*scroll).take(BANK_USAGE_VISIBLE).enumerate()
                {
                    let row = y + 2 + screen_row;
                    let label = bank_usage_label(entry.section);
                    let used = entry.used;
                    let free = entry.free();
                    let percent = if entry.capacity == 0 { 0 } else { used * 100 / entry.capacity };
                    let line = format!("{label:<8}{used:>6}B Used {free:>6}B Free {percent:>3}%");
                    put_text_width(cells, x + 2, row, &line, BANK_USAGE_WIDTH - 4);
                }
                if entries.len() > BANK_USAGE_VISIBLE {
                    let shown_end = (*scroll + BANK_USAGE_VISIBLE).min(entries.len());
                    put_text_width(
                        cells,
                        x + 2,
                        y + BANK_USAGE_HEIGHT - 3,
                        &format!("{}-{} of {}", *scroll + 1, shown_end, entries.len()),
                        BANK_USAGE_WIDTH - 4,
                    );
                }
                put_text(cells, x + 2, y + BANK_USAGE_HEIGHT - 2, "Enter/Esc=Close");
            }
            Overlay::About { .. } => {
                let x = (COLUMNS - ABOUT_WIDTH) / 2;
                let y = (ROWS - ABOUT_HEIGHT) / 2;
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x, y, width: ABOUT_WIDTH, height: ABOUT_HEIGHT },
                    style,
                );
                put_text_width(cells, x + 3, y, "About Fanticon", ABOUT_WIDTH - 6);
                put_text_width(cells, x + 16, y + 18, "FANTICON", 10);
                put_text_width(
                    cells,
                    x + 14,
                    y + 19,
                    concat!("Version ", env!("CARGO_PKG_VERSION")),
                    16,
                );
                put_text_width(cells, x + 10, y + 21, "Enter/Esc/Click=Close", 22);
            }
            Overlay::HelpFinder { query, results, selected, scroll, preview_scroll } => {
                draw_dialog(
                    cells,
                    foregrounds,
                    backgrounds,
                    background_gradients,
                    inverse,
                    CellRect { x: HELP_X, y: HELP_Y, width: HELP_WIDTH, height: HELP_HEIGHT },
                    style,
                );
                put_text_width(cells, HELP_X + 3, HELP_Y, "Help Finder", HELP_WIDTH - 6);
                put_text(cells, HELP_X + 2, HELP_Y + 2, "Find:");
                put_text_width(cells, HELP_X + 8, HELP_Y + 2, query, HELP_WIDTH - 10);

                let list_x = HELP_X + 2;
                let divider_x = HELP_X + HELP_LIST_WIDTH + 2;
                let preview_x = divider_x + 1;
                let header_row = HELP_Y + 4;
                let content_row = HELP_Y + 6;
                for row in HELP_Y + 1..HELP_Y + HELP_HEIGHT - 2 {
                    put_cell(cells, divider_x, row, BOX_VERTICAL);
                }

                // A muted color for instructional/status text (placeholder,
                // match count, footer) keeps it visually secondary to actual
                // opcode/directive/command/guide content.
                let muted_start = header_row * COLUMNS + list_x;
                let list_label = if query.trim().is_empty() {
                    "Type to search..."
                } else if results.is_empty() {
                    "No matches"
                } else {
                    ""
                };
                if list_label.is_empty() {
                    let text = format!(
                        "{} match{}",
                        results.len(),
                        if results.len() == 1 { "" } else { "es" }
                    );
                    put_text_width(cells, list_x, header_row, &text, HELP_LIST_WIDTH);
                } else {
                    put_text_width(cells, list_x, header_row, list_label, HELP_LIST_WIDTH);
                }
                foregrounds[muted_start
                    ..(muted_start + HELP_LIST_WIDTH).min(header_row * COLUMNS + divider_x)]
                    .fill(ASM_COMMENT_COLOR);

                for (screen_row, entry) in
                    results.iter().skip(*scroll).take(HELP_VISIBLE).enumerate()
                {
                    let row = content_row + screen_row;
                    let index = *scroll + screen_row;
                    // The key is colored by category (matching ASM syntax
                    // colors) instead of spelling the category out, so the
                    // narrow list column is spent on the name, not a label.
                    put_text_width(cells, list_x, row, &entry.key, HELP_LIST_WIDTH);
                    let start = row * COLUMNS + list_x;
                    let end = (start + HELP_LIST_WIDTH).min(row * COLUMNS + divider_x);
                    foregrounds[start..end].fill(help_category_color(entry.category));
                    if index == *selected {
                        inverse[row * COLUMNS + HELP_X + 1..row * COLUMNS + divider_x].fill(true);
                    }
                }

                if let Some(entry) = results.get(*selected) {
                    let title = format!("{} - {}", entry.category.label(), entry.key);
                    put_text_width(cells, preview_x, header_row, &title, HELP_PREVIEW_WIDTH);
                    let title_start = header_row * COLUMNS + preview_x;
                    foregrounds[title_start..title_start + title.len().min(HELP_PREVIEW_WIDTH)]
                        .fill(help_category_color(entry.category));

                    let preview_lines = help_preview_lines(entry);
                    for (index, line) in
                        preview_lines.iter().skip(*preview_scroll).take(HELP_VISIBLE).enumerate()
                    {
                        put_text_width(
                            cells,
                            preview_x,
                            content_row + index,
                            line,
                            HELP_PREVIEW_WIDTH,
                        );
                    }
                    if preview_lines.len() > HELP_VISIBLE {
                        let position = format!(
                            "LINE {}-{} OF {}",
                            *preview_scroll + 1,
                            (*preview_scroll + HELP_VISIBLE).min(preview_lines.len()),
                            preview_lines.len()
                        );
                        let position_start = (content_row - 1) * COLUMNS + preview_x;
                        put_text_width(
                            cells,
                            preview_x,
                            content_row - 1,
                            &position,
                            HELP_PREVIEW_WIDTH,
                        );
                        foregrounds[position_start
                            ..position_start + position.len().min(HELP_PREVIEW_WIDTH)]
                            .fill(ASM_COMMENT_COLOR);
                    }
                }

                let footer = "TYPE TO SEARCH  UP/DOWN SELECT  PGUP/PGDN SCROLL  ESC=CLOSE";
                let footer_row = HELP_Y + HELP_HEIGHT - 2;
                put_text(cells, HELP_X + 2, footer_row, footer);
                let footer_start = footer_row * COLUMNS + HELP_X + 2;
                foregrounds[footer_start..footer_start + footer.len().min(HELP_WIDTH - 4)]
                    .fill(ASM_COMMENT_COLOR);
            }
        }
    }

    fn open_menu(&mut self, menu: MenuKind) {
        self.overlay = Overlay::Menu { menu, selected: 0 };
    }

    /// Opens the F1 help finder. It starts empty on purpose: nothing is
    /// listed until the user types, rather than dumping every opcode,
    /// directive, command, shortcut, and guide section at once.
    fn open_help_finder(&mut self) {
        self.overlay = Overlay::HelpFinder {
            query: String::new(),
            results: Vec::new(),
            selected: 0,
            scroll: 0,
            preview_scroll: 0,
        };
    }

    fn refresh_help_finder(&mut self) {
        if let Overlay::HelpFinder { query, results, selected, scroll, preview_scroll } =
            &mut self.overlay
        {
            *results = shared_help_index().search(query);
            *selected = 0;
            *scroll = 0;
            *preview_scroll = 0;
        }
    }

    fn open_dialog(&mut self, kind: DialogKind) {
        self.overlay =
            Overlay::Dialog { kind, input: self.filename.clone().unwrap_or_default(), error: None };
    }

    fn open_debug_prompt(&mut self, kind: DebugPromptKind) {
        if !self.debug_active {
            self.show_build_message("Debugger", &["Start a debug session first".to_owned()]);
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
                "Save All",
                &["Save untitled tabs with Save As first".to_owned()],
            );
            return;
        }
        match self.save_named_tabs() {
            Ok(()) => self.show_build_message("Save All", &["All named files saved".to_owned()]),
            Err(error) => self.show_build_message("Save Error", &[error]),
        }
    }

    fn save_named_tabs(&mut self) -> Result<(), String> {
        self.sync_active_document();
        let mut failure = None;
        for index in 0..self.tabs.len() {
            let document_id = self.tabs[index].id;
            let Some(filename) =
                self.tabs[index].filename.clone().filter(|_| self.tabs[index].dirty)
            else {
                continue;
            };
            if system_document_filename(&filename) {
                failure = Some("FANTICON.INC is a read-only system file".to_owned());
                break;
            }
            let mut lines = self.tabs[index].lines.clone();
            if self.graphics_tabs.contains_key(&document_id) {
                if self.graphics_source_views.contains(&document_id) {
                    match self.parse_graphics_asset(&filename, &lines.join("\n")) {
                        Ok(graphics) => {
                            self.graphics_tabs.insert(document_id, graphics);
                        }
                        Err(error) => {
                            failure = Some(format!("{filename}: {error}"));
                            break;
                        }
                    }
                }
                if let Err(error) = self.save_graphics_palette(document_id, &filename) {
                    failure = Some(format!("{filename}: {error}"));
                    break;
                }
                lines = normalized_lines(&self.graphics_tabs[&document_id].serialize(&filename));
            } else if self.music_tabs.contains_key(&document_id) {
                if self.music_source_views.contains(&document_id) {
                    match MusicEditor::parse(&lines.join("\n")) {
                        Ok(music) => {
                            self.music_tabs.insert(document_id, music);
                        }
                        Err(error) => {
                            failure = Some(format!("{filename}: {error}"));
                            break;
                        }
                    }
                }
                lines = normalized_lines(&self.music_tabs[&document_id].serialize(&filename));
            } else if assembly_filename(&filename) {
                format_assembly_lines(&mut lines);
            }
            let save_result = self.filesystem.borrow_mut().write_text(&filename, &lines.join("\n"));
            if let Err(error) = save_result {
                failure = Some(format!("{filename}: {error}"));
                break;
            }
            let document = &mut self.tabs[index];
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
        if system_document_filename(&filename) {
            return Err("FANTICON.INC is a read-only system file".to_owned());
        }
        let document_id = self.tabs[tab].id;
        let mut lines = self.tabs[tab].lines.clone();
        if self.graphics_tabs.contains_key(&document_id) {
            if self.graphics_source_views.contains(&document_id) {
                let graphics = self.parse_graphics_asset(&filename, &lines.join("\n"))?;
                self.graphics_tabs.insert(document_id, graphics);
            }
            self.save_graphics_palette(document_id, &filename)?;
            lines = normalized_lines(&self.graphics_tabs[&document_id].serialize(&filename));
        } else if self.music_tabs.contains_key(&document_id) {
            if self.music_source_views.contains(&document_id) {
                let music = MusicEditor::parse(&lines.join("\n"))?;
                self.music_tabs.insert(document_id, music);
            }
            lines = normalized_lines(&self.music_tabs[&document_id].serialize(&filename));
        } else if assembly_filename(&filename) {
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
        // A cancelled ROM-usage build must never linger and hijack this one.
        self.pending_bank_usage = false;
        self.overlay = Overlay::Building { frames_remaining: BUILD_PROGRESS_FRAMES };
    }

    /// Runs a fresh project build purely to report free ROM space, so the
    /// dialog can never show stale numbers from a build the source has since
    /// outgrown.
    fn start_bank_usage(&mut self) {
        self.pending_bank_usage = true;
        self.build_and_run = false;
        self.overlay = Overlay::Building { frames_remaining: BUILD_PROGRESS_FRAMES };
    }

    fn perform_build(&mut self) {
        if let Err(error) = self.save_named_tabs() {
            self.show_build_message("Build Error", &[error]);
            return;
        }
        if self.pending_bank_usage {
            self.pending_bank_usage = false;
            if self.filesystem.borrow().read_binary(MANIFEST_NAME).is_ok() {
                match build_project(&self.filesystem) {
                    Ok(success) => {
                        self.diagnostics.clear();
                        self.diagnostic_index = None;
                        self.overlay =
                            Overlay::BankUsage { entries: success.bank_usage, scroll: 0 };
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
            } else {
                self.show_build_message(
                    "Build Error",
                    &["ROM Bank Usage requires a Fanticon project".to_owned()],
                );
            }
            return;
        }
        if self.build_and_run {
            match build_and_load_project(&self.filesystem) {
                Ok(mut launch) => {
                    let title = launch.cartridge.title.clone();
                    launch.breakpoints = self.resolved_source_breakpoints(&launch.source_map);
                    self.debug_source_map = launch.source_map.clone();
                    self.debug_symbols = launch.symbols.clone();
                    self.debug_snapshot = None;
                    self.debug_active = true;
                    self.show_build_message("Build Successful", &[format!("Running: {title}")]);
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
                        Some(format!("Built {} {} bytes", success.output, success.size));
                    self.show_build_message(
                        "Build Successful",
                        &[
                            format!("Output: {}", success.output),
                            format!("Title: {}", success.title),
                            format!("ROM banks: {}", success.banks),
                            format!("Size: {} bytes", success.size),
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
            self.build_message = Some("Save as ASM/INC before build".to_owned());
            self.show_build_message("Build Error", &["Save as ASM/INC before build".to_owned()]);
            return;
        };
        if !assembly_filename(&filename) {
            self.diagnostics.clear();
            self.diagnostic_index = None;
            self.build_message = Some("Build requires an ASM/INC file".to_owned());
            self.show_build_message("Build Error", &["Build requires an ASM/INC file".to_owned()]);
            return;
        }

        let source = self.lines.join("\n");
        match build_source(&self.filesystem, &filename, &source, None) {
            Ok(success) => {
                self.diagnostics.clear();
                self.diagnostic_index = None;
                self.build_message = Some(format!(
                    "Built {} ${:04X} {} bytes",
                    success.output, success.origin, success.size
                ));
                self.show_build_message(
                    "Build Successful",
                    &[
                        format!("Output: {}", success.output),
                        format!("Origin: ${:04X}", success.origin),
                        format!("Size: {} bytes", success.size),
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
            self.build_message = Some("No build errors".to_owned());
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
            format!("Error {} of {}", index + 1, self.diagnostics.len()),
            format!("{}:{}:{}", diagnostic.source, diagnostic.line, diagnostic.column),
        ];
        lines.extend(wrap_dialog_text(&diagnostic.message, 30));
        self.show_build_message("Build Errors", &lines);
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
        if self.system_read_only()
            || self.filesystem.borrow().is_root_file(filename, FANTICON_INCLUDE_NAME)
        {
            return Err("FANTICON.INC is a read-only system file".to_owned());
        }
        if self.tabs.iter().enumerate().any(|(tab, document)| {
            tab != self.active_tab
                && document
                    .filename
                    .as_deref()
                    .is_some_and(|open| open.eq_ignore_ascii_case(filename))
        }) {
            return Err("File is already open".to_owned());
        }
        if self.graphics_active() {
            let palette_document = self.graphics_tabs[&self.document_id].is_palette_document();
            if palette_document && !palette_filename(filename) {
                return Err("PALETTE DOCUMENTS REQUIRE A .PAL NAME".to_owned());
            }
            if !palette_document && !graphics_filename(filename) {
                return Err("GRAPHICS DOCUMENTS REQUIRE A .GFX NAME".to_owned());
            }
            if self.graphics_source_active() {
                let parsed = self.parse_graphics_asset(filename, &self.lines.join("\n"))?;
                self.graphics_tabs.insert(self.document_id, parsed);
            }
            self.save_graphics_palette(self.document_id, filename)?;
            let text = self.graphics_tabs[&self.document_id].serialize(filename);
            self.filesystem.borrow_mut().write_text(filename, &text)?;
            self.lines = normalized_lines(&text);
            self.filename = Some(filename.to_ascii_lowercase());
            self.dirty = false;
            self.propagate_active_palette();
            self.sync_active_document();
            self.refresh_project_browser();
            if self.close_after_save.take() == Some(self.document_id) {
                self.close_tab(self.active_tab);
            }
            return Ok(());
        }
        if self.music_active() {
            if !music_filename(filename) {
                return Err("MUSIC DOCUMENTS REQUIRE A .MUS NAME".to_owned());
            }
            if self.music_source_active() {
                let parsed = MusicEditor::parse(&self.lines.join("\n"))?;
                self.music_tabs.insert(self.document_id, parsed);
            }
            let text = self.music_tabs[&self.document_id].serialize(filename);
            self.filesystem.borrow_mut().write_text(filename, &text)?;
            self.lines = normalized_lines(&text);
            self.filename = Some(filename.to_ascii_lowercase());
            self.dirty = false;
            self.sync_active_document();
            self.refresh_project_browser();
            if self.close_after_save.take() == Some(self.document_id) {
                self.close_tab(self.active_tab);
            }
            return Ok(());
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
        let system_file = self.filesystem.borrow().is_root_file(filename, FANTICON_INCLUDE_NAME);
        let filename = if system_file {
            format!("/{}", FANTICON_INCLUDE_NAME.to_ascii_lowercase())
        } else {
            filename.to_owned()
        };
        if let Some(tab) = self.tabs.iter().position(|document| {
            document.filename.as_deref().is_some_and(|open| open.eq_ignore_ascii_case(&filename))
        }) {
            let disposable_tab = self.active_tab_is_disposable().then_some(self.active_tab);
            self.switch_tab(tab);
            if let Some(disposable_tab) = disposable_tab {
                self.close_tab(disposable_tab);
            }
            return Ok(());
        }
        let text = if system_file {
            FANTICON_INCLUDE_SOURCE.to_owned()
        } else {
            self.filesystem.borrow().read_text(&filename)?
        };
        let graphics = graphics_asset_filename(&filename)
            .then(|| self.parse_graphics_asset(&filename, &text))
            .transpose()?;
        let music = music_filename(&filename).then(|| MusicEditor::parse(&text)).transpose()?;
        let mut lines = text
            .replace("\r\n", "\n")
            .replace('\r', "\n")
            .split('\n')
            .map(str::to_owned)
            .collect::<Vec<_>>();
        if assembly_filename(&filename) {
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
            self.edit_run = None;
            self.dirty = false;
            if let Some(graphics) = graphics {
                self.graphics_tabs.insert(self.document_id, graphics);
            }
            if let Some(music) = music {
                self.music_tabs.insert(self.document_id, music);
            }
        } else if self.active_tab_is_disposable() {
            self.graphics_tabs.remove(&self.document_id);
            self.graphics_source_views.remove(&self.document_id);
            self.music_tabs.remove(&self.document_id);
            self.music_source_views.remove(&self.document_id);
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
            if let Some(graphics) = graphics {
                self.graphics_tabs.insert(self.document_id, graphics);
            }
            if let Some(music) = music {
                self.music_tabs.insert(self.document_id, music);
            }
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
            if let Some(graphics) = graphics {
                self.graphics_tabs.insert(self.document_id, graphics);
            }
            if let Some(music) = music {
                self.music_tabs.insert(self.document_id, music);
            }
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

    fn new_graphics_document(&mut self) {
        self.sync_active_document();
        let mut document = self.blank_document();
        let palette = match self.ensure_default_palette() {
            Ok(palette) => palette,
            Err(error) => {
                self.show_build_message("Palette Error", &[error]);
                return;
            }
        };
        let mut graphics = GraphicsEditor::with_shared_palette(DEFAULT_PALETTE_FILE);
        let _ = graphics.replace_palette(palette.palette());
        document.lines = normalized_lines(&graphics.serialize("Untitled.gfx"));
        self.graphics_tabs.insert(document.id, graphics);
        self.tabs.push(document.clone());
        self.active_tab = self.tabs.len() - 1;
        self.restore_document(document);
        self.ensure_active_tab_visible();
        self.invalidate_build();
    }

    fn new_music_document(&mut self) {
        self.sync_active_document();
        let mut document = self.blank_document();
        let music = MusicEditor::default();
        document.lines =
            normalized_lines(&music.serialize(&format!("Untitled{}.mus", document.id)));
        self.music_tabs.insert(document.id, music);
        self.tabs.push(document.clone());
        self.active_tab = self.tabs.len() - 1;
        self.restore_document(document);
        self.ensure_active_tab_visible();
        self.invalidate_build();
    }

    fn new_palette_document(&mut self) {
        if let Err(error) =
            self.ensure_default_palette().and_then(|_| self.load(DEFAULT_PALETTE_FILE))
        {
            self.show_build_message("Palette Error", &[error]);
        } else {
            self.refresh_project_browser();
        }
    }

    fn ensure_default_palette(&mut self) -> Result<GraphicsEditor, String> {
        let exists = self.filesystem.borrow().list(None)?.iter().any(|entry| {
            !entry.is_directory && entry.name.eq_ignore_ascii_case(DEFAULT_PALETTE_FILE)
        });
        if exists {
            let source = self.filesystem.borrow().read_text(DEFAULT_PALETTE_FILE)?;
            let palette = GraphicsEditor::parse(&source)?;
            if !palette.is_palette_document() {
                return Err(format!("{DEFAULT_PALETTE_FILE} IS NOT A PALETTE RESOURCE"));
            }
            return Ok(palette);
        }

        let palette = GraphicsEditor::palette_document();
        let source = palette.serialize(DEFAULT_PALETTE_FILE);
        self.filesystem.borrow_mut().write_text(DEFAULT_PALETTE_FILE, &source)?;
        self.refresh_project_browser();
        Ok(palette)
    }

    fn record_undo(&mut self) {
        self.invalidate_build();
        if self.undo.len() == 64 {
            self.undo.remove(0);
        }
        self.undo.push(Snapshot { lines: self.lines.clone(), cursor: self.cursor });
        self.edit_run = None;
    }

    /// Like `record_undo`, but skips the snapshot when this edit continues an
    /// uninterrupted run of the same kind at the same cursor position (e.g.
    /// typing or holding Backspace), so a whole editing session doesn't clone
    /// the entire document once per keystroke.
    fn record_undo_for(&mut self, run: EditRunKind) {
        if self.edit_run != Some((run, self.cursor)) {
            self.record_undo();
        }
    }

    /// The built-in include is a virtual system document. The managed disk
    /// copy only makes it discoverable in the project browser; assembly always
    /// uses the embedded source, so editing that copy would be misleading.
    fn system_read_only(&self) -> bool {
        self.filename.as_deref().is_some_and(system_document_filename)
    }

    /// A debug session also owns the source that produced the running
    /// cartridge because its line numbers back the source map and breakpoints.
    fn read_only(&self) -> bool {
        self.debug_active || self.system_read_only()
    }

    /// True only while the machine is stopped, with a snapshot to inspect.
    fn debug_paused(&self) -> bool {
        self.debug_active && self.debug_snapshot.is_some()
    }

    fn undo(&mut self) {
        if self.read_only() {
            return;
        }
        if let Some(snapshot) = self.undo.pop() {
            self.invalidate_build();
            self.lines = snapshot.lines;
            self.cursor = snapshot.cursor;
            self.selection_anchor = None;
            self.edit_run = None;
            self.dirty = true;
            self.ensure_cursor_visible();
        }
    }

    fn insert_text(&mut self, text: &str) {
        if text.is_empty() || self.read_only() {
            return;
        }
        // A `;` typed as the only content on a line - typically one that
        // Enter just auto-indented to the opcode column - drops back to
        // column 1 instead of leaving the full-line comment indented, since
        // `format_assembly_line` (and the Merlin convention it follows)
        // never indents a `;`/`*` full-line comment.
        if text == ";"
            && self.assembly_mode()
            && self.selection_anchor.is_none()
            && self.lines[self.cursor.line].trim().is_empty()
        {
            self.record_undo_for(EditRunKind::Insert);
            self.lines[self.cursor.line].clear();
            self.lines[self.cursor.line].push(';');
            self.cursor.column = 1;
            self.dirty = true;
            self.edit_run = Some((EditRunKind::Insert, self.cursor));
            return;
        }
        self.record_undo_for(EditRunKind::Insert);
        self.delete_selection_without_undo();
        self.lines[self.cursor.line].insert_str(self.cursor.column, text);
        self.cursor.column += text.len();
        self.dirty = true;
        self.edit_run = Some((EditRunKind::Insert, self.cursor));
    }

    fn insert_newline(&mut self) {
        if self.read_only() {
            return;
        }
        self.record_undo();
        self.delete_selection_without_undo();
        if self.assembly_mode() && self.cursor.column == self.lines[self.cursor.line].len() {
            self.lines[self.cursor.line] = format_assembly_line(&self.lines[self.cursor.line]);
            self.cursor.column = self.lines[self.cursor.line].len();
        }
        let remainder = self.lines[self.cursor.line].split_off(self.cursor.column);
        self.cursor.line += 1;
        if self.assembly_mode() {
            // Default a new line to the opcode column rather than the left
            // margin, matching the Merlin convention: most lines are plain
            // instructions, and a label is the exception, not the rule.
            let mut new_line = String::new();
            pad_to_column(&mut new_line, 9);
            self.cursor.column = new_line.len();
            new_line.push_str(&remainder);
            self.lines.insert(self.cursor.line, new_line);
        } else {
            self.cursor.column = 0;
            self.lines.insert(self.cursor.line, remainder);
        }
        self.dirty = true;
    }

    /// Drops a three-line section-heading divider at the cursor's line -
    /// a `;===...`/`;---...` bar, a bare `;` line ready for the heading
    /// text, then a matching closing bar - mirroring the hand-written
    /// dividers already used throughout the codebase (e.g. `funcs.inc`).
    fn insert_banner_comment(&mut self, fill: char) {
        if self.read_only() {
            return;
        }
        self.record_undo();
        self.delete_selection_without_undo();
        let bar = banner_bar(fill);
        let line = self.cursor.line;
        self.lines[line] = bar.clone();
        self.lines.insert(line + 1, ";".to_owned());
        self.lines.insert(line + 2, bar);
        self.cursor = Position { line: line + 1, column: 1 };
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
        if self.read_only() {
            return;
        }
        if self.has_selection() {
            self.record_undo();
            self.delete_selection_without_undo();
        } else if self.cursor.column > 0 {
            self.record_undo_for(EditRunKind::Backspace);
            self.cursor.column -= 1;
            self.lines[self.cursor.line].remove(self.cursor.column);
            self.edit_run = Some((EditRunKind::Backspace, self.cursor));
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
        if self.read_only() {
            return;
        }
        if self.has_selection() {
            self.record_undo();
            self.delete_selection_without_undo();
        } else if self.cursor.column < self.lines[self.cursor.line].len() {
            self.record_undo_for(EditRunKind::DeleteForward);
            self.lines[self.cursor.line].remove(self.cursor.column);
            self.edit_run = Some((EditRunKind::DeleteForward, self.cursor));
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
        if self.read_only() {
            // Still copy: reading the selection is harmless while paused.
            self.copy_selection();
            return;
        }
        if let Some(text) = self.selected_text() {
            self.clipboard = text;
            self.record_undo();
            self.delete_selection_without_undo();
            self.dirty = true;
        }
    }

    fn paste(&mut self) {
        if self.clipboard.is_empty() || self.read_only() {
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
        if !self.assembly_mode() || self.read_only() {
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

    /// The zero-keystroke help tier: a one-line gloss for the opcode or
    /// directive under the cursor, shown in the status bar with no popup at
    /// all. Returns `None` outside assembly mode or when the cursor is not
    /// on a recognized token, so the caller falls back to the ordinary
    /// filename/line/column status.
    fn ambient_help_status(&self) -> Option<String> {
        if !self.assembly_mode() {
            return None;
        }
        let line = self.lines.get(self.cursor.line)?;
        let (_, token) = assembly_tokens(line).into_iter().find(|(start, token)| {
            self.cursor.column >= *start && self.cursor.column <= *start + token.len()
        })?;
        let entry = shared_help_index().ambient_gloss(token)?;
        let name = self.filename.as_deref().unwrap_or("Untitled.txt");
        let dirty = if self.dirty { "*" } else { " " };
        Some(format!(
            " {name}{dirty}  LN {} COL {}  {}: {}",
            self.cursor.line + 1,
            self.cursor.column + 1,
            entry.key,
            entry.summary
        ))
    }

    fn graphics_active(&self) -> bool {
        self.graphics_tabs.contains_key(&self.document_id)
    }

    fn graphics_source_active(&self) -> bool {
        self.graphics_source_views.contains(&self.document_id)
    }

    fn music_active(&self) -> bool {
        self.music_tabs.contains_key(&self.document_id)
    }

    fn music_source_active(&self) -> bool {
        self.music_source_views.contains(&self.document_id)
    }

    /// The name used to identify this document's tracker song to the shared
    /// `MusicRadio` for play/pause/stop routing. Every unsaved document must
    /// get a *distinct* placeholder here: two different "Untitled.mus" tabs
    /// sharing one literal name would make the radio's currently-playing
    /// filename match either tab's placeholder, so playing one and then
    /// trying to play the other would look up as "already playing" and just
    /// toggle pause/stop instead of loading the second song's actual data.
    fn playback_filename(&self) -> String {
        self.filename.clone().unwrap_or_else(|| format!("Untitled{}.mus", self.document_id))
    }

    fn tracker_play_stop_command(&self) -> MusicCommand {
        let filename = self.playback_filename();
        if self
            .music_status
            .as_ref()
            .is_some_and(|status| status.filename.eq_ignore_ascii_case(&filename))
        {
            return MusicCommand::Stop;
        }
        let source = self.music_tabs[&self.document_id].serialize(&filename);
        MusicCommand::LoadTracker { filename, source }
    }

    fn toggle_music_source_view(&mut self) {
        if self.music_source_views.remove(&self.document_id) {
            match MusicEditor::parse(&self.lines.join("\n")) {
                Ok(music) => {
                    self.music_tabs.insert(self.document_id, music);
                    self.selection_anchor = None;
                }
                Err(error) => {
                    self.music_source_views.insert(self.document_id);
                    self.show_build_message("Music Source Error", &[error]);
                }
            }
        } else if let Some(music) = self.music_tabs.get(&self.document_id) {
            let filename = self.playback_filename();
            self.lines = normalized_lines(&music.serialize(&filename));
            self.cursor = Position::default();
            self.selection_anchor = None;
            self.scroll_line = 0;
            self.scroll_column = 0;
            self.music_source_views.insert(self.document_id);
        }
    }

    fn toggle_graphics_source_view(&mut self) {
        if self.graphics_source_views.remove(&self.document_id) {
            let filename = self.filename.as_deref().unwrap_or("Untitled.gfx");
            match self.parse_graphics_asset(filename, &self.lines.join("\n")) {
                Ok(graphics) => {
                    self.graphics_tabs.insert(self.document_id, graphics);
                    self.selection_anchor = None;
                    self.propagate_active_palette();
                }
                Err(error) => {
                    self.graphics_source_views.insert(self.document_id);
                    self.show_build_message("Gfx Source Error", &[error]);
                }
            }
        } else if let Some(graphics) = self.graphics_tabs.get(&self.document_id) {
            let filename = self.filename.as_deref().unwrap_or("Untitled.gfx");
            self.lines = normalized_lines(&graphics.serialize(filename));
            self.cursor = Position::default();
            self.selection_anchor = None;
            self.scroll_line = 0;
            self.scroll_column = 0;
            self.graphics_source_views.insert(self.document_id);
        }
    }

    fn parse_graphics_asset(&self, filename: &str, source: &str) -> Result<GraphicsEditor, String> {
        let mut graphics = GraphicsEditor::parse(source)?;
        let Some(reference) = graphics.palette_reference() else { return Ok(graphics) };
        let palette_path = sibling_asset_path(filename, reference);
        let palette_source = match self.filesystem.borrow().read_text(&palette_path) {
            Ok(source) => source,
            Err(error) => {
                let fallback = self.tabs.iter().find_map(|document| {
                    let open = self.graphics_tabs.get(&document.id)?;
                    let open_path = if open.is_palette_document() {
                        document.filename.clone()
                    } else {
                        open.palette_reference().map(|open_reference| {
                            sibling_asset_path(
                                document.filename.as_deref().unwrap_or(""),
                                open_reference,
                            )
                        })
                    }?;
                    open_path.eq_ignore_ascii_case(&palette_path).then(|| {
                        let mut palette = GraphicsEditor::palette_document();
                        let _ = palette.replace_palette(open.palette());
                        palette.serialize(&palette_path)
                    })
                });
                fallback.ok_or_else(|| format!("PALETTE {palette_path}: {error}"))?
            }
        };
        let palette = GraphicsEditor::parse(&palette_source)
            .map_err(|error| format!("PALETTE {palette_path}: {error}"))?;
        if !palette.is_palette_document() {
            return Err(format!("{palette_path} IS NOT A PALETTE RESOURCE"));
        }
        graphics.replace_palette(palette.palette())?;
        Ok(graphics)
    }

    fn save_graphics_palette(&mut self, document_id: u32, filename: &str) -> Result<(), String> {
        let Some(graphics) = self.graphics_tabs.get(&document_id) else { return Ok(()) };
        let Some(reference) = graphics.palette_reference() else { return Ok(()) };
        let palette_path = sibling_asset_path(filename, reference);
        let mut palette = GraphicsEditor::palette_document();
        palette.replace_palette(graphics.palette())?;
        let source = palette.serialize(&palette_path);
        self.filesystem.borrow_mut().write_text(&palette_path, &source)?;
        Ok(())
    }

    fn propagate_active_palette(&mut self) {
        let Some(active_graphics) = self.graphics_tabs.get(&self.document_id) else { return };
        let active_filename = self.filename.as_deref().unwrap_or("");
        let active_path = if active_graphics.is_palette_document() {
            (!active_filename.is_empty()).then(|| active_filename.to_owned())
        } else {
            active_graphics
                .palette_reference()
                .map(|reference| sibling_asset_path(active_filename, reference))
        };
        let Some(active_path) = active_path else { return };
        let palette = active_graphics.palette().to_vec();
        for index in 0..self.tabs.len() {
            let document_id = self.tabs[index].id;
            if document_id == self.document_id {
                continue;
            }
            let filename = self.tabs[index].filename.clone();
            let Some(graphics) = self.graphics_tabs.get_mut(&document_id) else { continue };
            let palette_document = graphics.is_palette_document();
            let path = if graphics.is_palette_document() {
                filename
            } else {
                graphics.palette_reference().map(|reference| {
                    sibling_asset_path(
                        self.tabs[index].filename.as_deref().unwrap_or(""),
                        reference,
                    )
                })
            };
            if path.is_some_and(|path| path.eq_ignore_ascii_case(&active_path)) {
                let _ = graphics.replace_palette(&palette);
                if palette_document {
                    self.tabs[index].dirty = true;
                }
            }
        }
    }
}

fn graphics_filename(filename: &str) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("gfx"))
}

fn palette_filename(filename: &str) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("pal"))
}

fn music_filename(filename: &str) -> bool {
    filename.rsplit_once('.').is_some_and(|(_, extension)| extension.eq_ignore_ascii_case("mus"))
}

fn graphics_asset_filename(filename: &str) -> bool {
    graphics_filename(filename) || palette_filename(filename)
}

fn sibling_asset_path(filename: &str, reference: &str) -> String {
    if reference.starts_with(['/', '\\']) {
        return reference.trim_start_matches(['/', '\\']).to_ascii_lowercase();
    }
    let filename = filename.replace('\\', "/");
    match filename.rsplit_once('/') {
        Some((directory, _)) if !directory.is_empty() => {
            format!("{directory}/{}", reference.to_ascii_lowercase())
        }
        _ => reference.to_ascii_lowercase(),
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

fn bank_usage_label(section: SymbolSection) -> String {
    match section {
        SymbolSection::Fixed => "Fixed".to_owned(),
        SymbolSection::Bank(bank) => format!("Bank {bank}"),
    }
}

fn menu_items(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => &[
            "New Text",
            "New Graphics",
            "New Palette",
            "New Music",
            "Open...",
            "",
            "Save",
            "Save As...",
            "Save All",
            "",
            "Close Tab",
            "",
            "Exit",
        ],
        MenuKind::Edit => &[
            "Undo",
            "",
            "Cut",
            "Copy",
            "Paste",
            "Select All",
            "",
            "Find",
            "Replace",
            "Project Find",
            "Go To Line",
            "",
            "Back",
            "Forward",
        ],
        MenuKind::Build => {
            &["Assemble", "Build & Run", "", "Next Error", "Prev Error", "", "ROM Usage"]
        }
        MenuKind::Debug => &[
            "Start/Continue",
            "Stop",
            "Toggle Break",
            "",
            "Step Over",
            "Step Into",
            "Step Out",
            "Step Cycle",
            "",
            "Read Watch",
            "Write Watch",
            "Raster Break",
            "",
            "Clear Breaks",
            "",
            "Debug Panel",
        ],
        MenuKind::Music => &["Play/Pause", "Previous", "Next", "Loop", "", "Stop"],
        MenuKind::Help => &["Find Help", "About"],
    }
}

const fn menu_origin(menu: MenuKind) -> usize {
    match menu {
        MenuKind::File => 0,
        MenuKind::Edit => 6,
        MenuKind::Build => 12,
        MenuKind::Debug => 19,
        MenuKind::Music => 27,
        MenuKind::Help => 34,
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
        34..=39 => Some(MenuKind::Help),
        _ => None,
    }
}

fn menu_labels(menu: MenuKind) -> &'static [&'static str] {
    match menu {
        MenuKind::File => &[
            "New Text  N",
            "New Gfx   G",
            "New Pal   P",
            "New Music M",
            "Open      O",
            "",
            "Save      S",
            "Save As   A",
            "Save All  L",
            "",
            "Close Tab W",
            "",
            "Exit      X",
        ],
        MenuKind::Edit => &[
            "Undo      U",
            "",
            "Cut       T",
            "Copy      C",
            "Paste     P",
            "Select All A",
            "",
            "Find      F",
            "Replace   R",
            "Proj Find J",
            "Go Line   G",
            "",
            "Back      K",
            "Forward   L",
        ],
        MenuKind::Build => {
            &["Assemble  B", "Build+Run F5", "", "Next Err  N", "Prev Err  P", "", "ROM Usage  U"]
        }
        MenuKind::Debug => &[
            "Continue             F5",
            "Stop           Shift+F5",
            "Toggle Break         F9",
            "",
            "Step Over           F10",
            "Step Into           F11",
            "Step Out      Shift+F11",
            "Step Cycle Ctrl/Cmd+F11",
            "",
            "Read Watch            R",
            "Write Watch           W",
            "Raster Break          A",
            "",
            "Clear Breaks          C",
            "",
            "Debug Panel  Ctrl/Cmd+D",
        ],
        MenuKind::Music => &[
            "Play/Pause          F7",
            "Previous      Shift+F8",
            "Next                F8",
            "Loop       Ctrl/Cmd+F8",
            "",
            "Stop          Shift+F7",
        ],
        MenuKind::Help => &["Find Help F1", "About      A"],
    }
}

fn menu_hotkey(menu: MenuKind, key: &str) -> Option<usize> {
    let key = key.to_ascii_lowercase();
    let hotkeys: &[(usize, &str)] = match menu {
        MenuKind::File => &[
            (0, "n"),
            (1, "g"),
            (2, "p"),
            (3, "m"),
            (4, "o"),
            (6, "s"),
            (7, "a"),
            (8, "l"),
            (10, "w"),
            (12, "x"),
        ],
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
        MenuKind::Build => &[(0, "b"), (1, "r"), (3, "n"), (4, "p"), (6, "u")],
        MenuKind::Debug => {
            &[(0, "g"), (1, "s"), (2, "b"), (9, "r"), (10, "w"), (11, "a"), (13, "c"), (15, "d")]
        }
        MenuKind::Music => &[(0, "p"), (1, "r"), (2, "n"), (3, "l"), (5, "s")],
        MenuKind::Help => &[(0, "f"), (1, "a")],
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
        (MenuKind::Debug, true) | (MenuKind::Help, false) => MenuKind::Music,
        (MenuKind::Music, true) | (MenuKind::File, false) => MenuKind::Help,
        (MenuKind::Help, true) | (MenuKind::Edit, false) => MenuKind::File,
    }
}

fn render_message_box(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    background_gradients: &mut [bool],
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
    draw_dialog(
        cells,
        foregrounds,
        backgrounds,
        background_gradients,
        inverse,
        CellRect { x, y, width, height },
        style,
    );
    let symbol = if title == "Build Successful" {
        SYMBOL_CHECK
    } else if title.contains("Error") {
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
        // One way out gets one label. Enter and Escape both dismiss, so they
        // share a single action instead of posing as a choice.
        "Build Errors" => "F4=Next  Enter/Esc=Close",
        "Unsaved Tab" => "S=Save  D=Discard  Esc=Cancel",
        _ => "Enter/Esc=Close",
    };
    put_text(cells, x + 2, y + height - 2, footer);
}

fn draw_window(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    background_gradients: &mut [bool],
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
        // Whoever owns the background owns its shading, or the caret-line and
        // debugger rows underneath keep banding straight through the window.
        background_gradients[range.clone()].fill(false);
        inverse[range].fill(false);
    }

    let f = frame_set(style.double_frame);
    put_cell(cells, x, y, f.top_left);
    put_cell(cells, x + width - 1, y, f.top_right);
    put_cell(cells, x, y + height - 1, f.bottom_left);
    put_cell(cells, x + width - 1, y + height - 1, f.bottom_right);
    for column in x + 1..x + width - 1 {
        put_cell(cells, column, y, f.top);
        put_cell(cells, column, y + height - 1, f.bottom);
    }
    for row in y + 1..y + height - 1 {
        put_cell(cells, x, row, f.left);
        put_cell(cells, x + width - 1, row, f.right);
    }
}

struct FrameSet {
    top_left: u8,
    top_right: u8,
    bottom_left: u8,
    bottom_right: u8,
    top: u8,
    bottom: u8,
    left: u8,
    right: u8,
    caption_left: u8,
    caption_right: u8,
    caption: u8,
}

const fn frame_set(double: bool) -> FrameSet {
    if double {
        FrameSet {
            top_left: DBL_TOP_LEFT,
            top_right: DBL_TOP_RIGHT,
            bottom_left: DBL_BOTTOM_LEFT,
            bottom_right: DBL_BOTTOM_RIGHT,
            top: DBL_TOP_HORIZONTAL,
            bottom: DBL_BOTTOM_HORIZONTAL,
            left: DBL_VERTICAL,
            right: DBL_RIGHT_VERTICAL,
            caption_left: DBL_CAPTION_LEFT,
            caption_right: DBL_CAPTION_RIGHT,
            caption: DBL_HORIZONTAL,
        }
    } else {
        FrameSet {
            top_left: BOX_TOP_LEFT,
            top_right: BOX_TOP_RIGHT,
            bottom_left: BOX_BOTTOM_LEFT,
            bottom_right: BOX_BOTTOM_RIGHT,
            top: BOX_TOP_HORIZONTAL,
            bottom: BOX_BOTTOM_HORIZONTAL,
            left: BOX_VERTICAL,
            right: BOX_RIGHT_VERTICAL,
            caption_left: BOX_CAPTION_LEFT,
            caption_right: BOX_CAPTION_RIGHT,
            caption: BOX_HORIZONTAL,
        }
    }
}

/// A DOS-style drop shadow one cell right and below the window.
fn draw_shadow(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    background_gradients: &mut [bool],
    rect: CellRect,
) {
    let CellRect { x, y, width, height } = rect;
    let mut shade = |column: usize, row: usize| {
        if column >= COLUMNS || row >= ROWS {
            return;
        }
        let index = row * COLUMNS + column;
        cells[index] = SHADE_LIGHT;
        foregrounds[index] = UI_SHADOW_COLOR;
        backgrounds[index] = 0;
        background_gradients[index] = false;
    };
    for row in y + 1..y + height + 1 {
        shade(x + width, row);
    }
    for column in x + 1..x + width + 1 {
        shade(column, y + height);
    }
}

/// A dialog: focused double rule plus the drop shadow that sells the depth.
fn draw_dialog(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    background_gradients: &mut [bool],
    inverse: &mut [bool],
    rect: CellRect,
    style: CellStyle,
) {
    draw_shadow(cells, foregrounds, backgrounds, background_gradients, rect);
    draw_caption_window(
        cells,
        foregrounds,
        backgrounds,
        background_gradients,
        inverse,
        rect,
        style.focused(),
    );
}

fn draw_caption_window(
    cells: &mut [u8],
    foregrounds: &mut [u8],
    backgrounds: &mut [u8],
    background_gradients: &mut [bool],
    inverse: &mut [bool],
    rect: CellRect,
    style: CellStyle,
) {
    draw_window(cells, foregrounds, backgrounds, background_gradients, inverse, rect, style);
    let f = frame_set(style.double_frame);
    put_cell(cells, rect.x, rect.y, f.caption_left);
    put_cell(cells, rect.x + rect.width - 1, rect.y, f.caption_right);
    for column in rect.x + 1..rect.x + rect.width - 1 {
        put_cell(cells, column, rect.y, f.caption);
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
        cells[y * COLUMNS + x + offset] = byte;
    }
}

fn put_cell(cells: &mut [u8], x: usize, y: usize, character: u8) {
    if x < COLUMNS && y < ROWS {
        cells[y * COLUMNS + x] = character;
    }
}

/// Paint one cell's background and glyph into the surface.
///
/// Colors are the editor's own table rather than console palette entries, so
/// the per-scanline shading is arithmetic on the resolved color instead of a
/// reserved entry per level. Frame glyphs opt out of shading so rules stay an
/// even weight along their whole length.
fn draw_cell(
    surface: &mut Surface,
    cell_x: usize,
    cell_y: usize,
    character: u8,
    foreground: Rgba,
    background: Option<Rgba>,
    shaded_background: bool,
) {
    let x = cell_x * GLYPH_WIDTH;
    let y = cell_y * GLYPH_HEIGHT;
    if let Some(background) = background {
        for glyph_y in 0..GLYPH_HEIGHT {
            let color =
                if shaded_background { scanline_shade(background, glyph_y) } else { background };
            surface.fill_rect(x, y + glyph_y, GLYPH_WIDTH, 1, color);
        }
    }
    let glyph = CHARACTER_ROM[usize::from(character).min(CHARACTER_ROM.len() - 1)];
    let frame = is_frame_character(character) || is_scrollbar_column(cell_x, cell_y);
    for (glyph_y, bits) in glyph.into_iter().enumerate() {
        if bits == 0 {
            continue;
        }
        let color = if frame { foreground } else { scanline_shade(foreground, glyph_y) };
        for glyph_x in 0..GLYPH_WIDTH {
            if bits & (0x80 >> glyph_x) != 0 {
                surface.put_pixel(x + glyph_x, y + glyph_y, color);
            }
        }
    }
}

fn render_cells(
    surface: &mut Surface,
    cells: &[u8],
    foregrounds: &[u8],
    backgrounds: &[u8],
    inverse: &[bool],
    background_gradients: &[bool],
    style: CellStyle,
) {
    let CellStyle { foreground: _, background, .. } = style;
    surface.clear(editor_color(background));
    for cell_y in 0..ROWS {
        for cell_x in 0..COLUMNS {
            let index = cell_y * COLUMNS + cell_x;
            let (cell_foreground, cell_background) = if inverse[index] {
                (backgrounds[index], foregrounds[index])
            } else {
                (foregrounds[index], backgrounds[index])
            };
            // The page is already this color; skip the fill and keep the glyph.
            let fill = (cell_background != background).then(|| editor_color(cell_background));
            draw_cell(
                surface,
                cell_x,
                cell_y,
                cells[index],
                editor_color(cell_foreground),
                fill,
                background_gradients[index],
            );
        }
    }
}

#[allow(clippy::too_many_arguments)]
fn render_masked_cells(
    surface: &mut Surface,
    cells: &[u8],
    foregrounds: &[u8],
    backgrounds: &[u8],
    inverse: &[bool],
    background_gradients: &[bool],
    mask_cells: &[u8],
    mask_foregrounds: &[u8],
    mask_backgrounds: &[u8],
    mask_inverse: &[bool],
    style: CellStyle,
) {
    let _ = style;
    for cell_y in 0..ROWS {
        for cell_x in 0..COLUMNS {
            let index = cell_y * COLUMNS + cell_x;
            let covered = mask_cells[index] != u8::MAX
                || mask_foregrounds[index] != u8::MAX
                || mask_backgrounds[index] != u8::MAX
                || !mask_inverse[index];
            if !covered {
                continue;
            }
            let (cell_foreground, cell_background) = if inverse[index] {
                (backgrounds[index], foregrounds[index])
            } else {
                (foregrounds[index], backgrounds[index])
            };
            draw_cell(
                surface,
                cell_x,
                cell_y,
                cells[index],
                editor_color(cell_foreground),
                Some(editor_color(cell_background)),
                background_gradients[index],
            );
        }
    }
}

fn is_frame_character(character: u8) -> bool {
    matches!(
        character,
        BOX_HORIZONTAL
            | BOX_VERTICAL
            | BOX_TOP_HORIZONTAL
            | BOX_BOTTOM_HORIZONTAL
            | BOX_RIGHT_VERTICAL
            | BOX_CAPTION_LEFT
            | BOX_CAPTION_RIGHT
            | BOX_TOP_LEFT
            | BOX_TOP_RIGHT
            | BOX_BOTTOM_LEFT
            | BOX_BOTTOM_RIGHT
            | DBL_HORIZONTAL
            | DBL_VERTICAL
            | DBL_TOP_HORIZONTAL
            | DBL_BOTTOM_HORIZONTAL
            | DBL_RIGHT_VERTICAL
            | DBL_CAPTION_LEFT
            | DBL_CAPTION_RIGHT
            | DBL_TOP_LEFT
            | DBL_TOP_RIGHT
            | DBL_BOTTOM_LEFT
            | DBL_BOTTOM_RIGHT
    )
}

/// Whether a cell sits in the scrollbar's column along the editor's right
/// edge. Its track, thumb, and end caps share glyphs (`SHADE_LIGHT`,
/// `SHADE_MEDIUM`) with the window drop shadow, but should read as one solid
/// rail rather than picking up the shadow's per-scanline banding, so this is
/// checked by position rather than by character alone.
fn is_scrollbar_column(cell_x: usize, cell_y: usize) -> bool {
    cell_x == COLUMNS - 1 && (EDITOR_FIRST_ROW..EDITOR_FIRST_ROW + TEXT_ROWS).contains(&cell_y)
}

fn draw_about_logo(surface: &mut Surface, frame: u16) {
    // The logo is authored in the console's RGB332 space but drawn here in true
    // color, so its palette maps straight to RGBA with nothing reserved.
    let color_map: [Rgba; 256] = core::array::from_fn(|color| {
        let rgb = rgb332(color as u8);
        let mut nearest = ABOUT_PALETTE[0];
        let mut nearest_distance = u32::MAX;
        for candidate in ABOUT_PALETTE {
            let red = i32::from(rgb[0]) - i32::from(candidate[0]);
            let green = i32::from(rgb[1]) - i32::from(candidate[1]);
            let blue = i32::from(rgb[2]) - i32::from(candidate[2]);
            let distance = (red * red + green * green + blue * blue) as u32;
            if distance < nearest_distance {
                nearest = candidate;
                nearest_distance = distance;
            }
        }
        nearest
    });

    let modal_y = (ROWS - ABOUT_HEIGHT) / 2;
    let left = (EDITOR_DISPLAY_WIDTH - ABOUT_LOGO_WIDTH) / 2;
    let top = (modal_y + 3) * GLYPH_HEIGHT;
    for y in 0..ABOUT_LOGO_HEIGHT {
        for x in 0..ABOUT_LOGO_WIDTH {
            let (horizontal, vertical) = about_wave_offsets(x, y, frame);
            let warped_x = x as i32 - horizontal;
            let warped_y = y as i32 - vertical;
            if !(0..ABOUT_LOGO_WIDTH as i32).contains(&warped_x)
                || !(0..ABOUT_LOGO_HEIGHT as i32).contains(&warped_y)
            {
                continue;
            }
            let source_x = warped_x as usize * DISPLAY_WIDTH / ABOUT_LOGO_WIDTH;
            let source_y = warped_y as usize * DISPLAY_HEIGHT / ABOUT_LOGO_HEIGHT;
            let source = BOOT_LOGO[source_y * DISPLAY_WIDTH + source_x];
            let rgb = rgb332(source);
            if u16::from(rgb[0]) + u16::from(rgb[1]) + u16::from(rgb[2]) < 32 {
                continue;
            }
            surface.put_pixel(left + x, top + y, color_map[source as usize]);
        }
    }
}

fn about_wave_offsets(x: usize, y: usize, frame: u16) -> (i32, i32) {
    let Some(phase) = about_wave_phase(frame) else { return (0, 0) };
    let strength = i32::from(about_wave_strength(frame));
    let horizontal = i32::from(ABOUT_RASTER_WAVE[(y / 3 + phase) % ABOUT_RASTER_WAVE.len()]);
    let vertical = i32::from(ABOUT_RASTER_WAVE[(x / 6 + phase + 8) % ABOUT_RASTER_WAVE.len()]);
    (
        scale_signed(scale_signed(horizontal, 3, 2), strength, 256),
        scale_signed(scale_signed(vertical, 1, 2), strength, 256),
    )
}

fn about_wave_phase(frame: u16) -> Option<usize> {
    let wave_time = frame % 240;
    (120..180).contains(&wave_time).then(|| usize::from(wave_time - 120))
}

fn about_wave_strength(frame: u16) -> u16 {
    let wave_time = frame % 240;
    if !(120..180).contains(&wave_time) {
        return 0;
    }
    let local = usize::from(wave_time - 120);
    let step = if local < 16 {
        local
    } else if local >= 44 {
        59 - local
    } else {
        16
    };
    ABOUT_WAVE_EASE[step]
}

fn scale_signed(value: i32, numerator: i32, denominator: i32) -> i32 {
    let scaled = value * numerator;
    if scaled >= 0 {
        (scaled + denominator / 2) / denominator
    } else {
        (scaled - denominator / 2) / denominator
    }
}

fn rgb332(color: u8) -> [u8; 3] {
    [
        ((u16::from(color >> 5) * 255 + 3) / 7) as u8,
        ((u16::from((color >> 2) & 7) * 255 + 3) / 7) as u8,
        (u16::from(color & 3) * 85) as u8,
    ]
}

fn draw_block_cursor(surface: &mut Surface, cell_x: usize, cell_y: usize, character: u8) {
    let origin_x = cell_x * GLYPH_WIDTH;
    let origin_y = cell_y * GLYPH_HEIGHT;
    surface.fill_rect(origin_x, origin_y, GLYPH_WIDTH, GLYPH_HEIGHT, editor_color(UI_WHITE_COLOR));
    let glyph = CHARACTER_ROM[usize::from(character).min(CHARACTER_ROM.len() - 1)];
    for (glyph_y, bits) in glyph.into_iter().enumerate() {
        for glyph_x in 0..GLYPH_WIDTH {
            if bits & (0x80 >> glyph_x) != 0 {
                surface.put_pixel(origin_x + glyph_x, origin_y + glyph_y, [0, 0, 0, 255]);
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

fn system_document_filename(filename: &str) -> bool {
    filename.eq_ignore_ascii_case("/fanticon.inc")
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
        return Err("Value required".to_owned());
    }
    let (digits, radix) = input.strip_prefix('$').map_or_else(
        || input.strip_prefix("0x").map_or((input, 10), |digits| (digits, 16)),
        |digits| (digits, 16),
    );
    u16::from_str_radix(digits, radix).map_err(|_| "Enter a 16-bit address".to_owned())
}

fn parse_raster_breakpoint(input: &str) -> Result<(u16, u16), String> {
    let fields = input.split([',', ':', ' ']).filter(|field| !field.is_empty()).collect::<Vec<_>>();
    if fields.len() != 2 {
        return Err("Use line,dot".to_owned());
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

fn symbol_section_name(section: SymbolSection) -> String {
    match section {
        SymbolSection::Fixed => "Fixed".to_owned(),
        SymbolSection::Bank(bank) => format!("Bank {bank:02X}"),
    }
}

fn format_debug_stop(stop: DebugStop) -> String {
    match stop {
        DebugStop::Instruction(address) => format!("Execution breakpoint   ${address:04X}"),
        DebugStop::Source { section, address } => {
            format!("Source breakpoint      ${address:04X}  {}", symbol_section_name(section))
        }
        DebugStop::MemoryRead(address) => format!("Read watchpoint        ${address:04X}"),
        DebugStop::MemoryWrite(address) => format!("Write watchpoint       ${address:04X}"),
        DebugStop::Raster { dot, line } => format!("Raster breakpoint      Line {line} Dot {dot}"),
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

/// Resolve one of the editor's cell colors to true color.
///
/// Chrome used to write these into the console's palette, which meant the
/// interface and the running cartridge fought over the same 256 entries. The
/// editor now keeps its own table: named indexes get their exact color, and
/// every other index still means the RGB332 byte it always did, so the `COLOR`
/// command keeps choosing from the console's own range.
fn editor_color(index: u8) -> Rgba {
    match index {
        UI_WHITE_COLOR => [255, 255, 255, 255],
        UI_ERROR_BACKGROUND => [192, 32, 40, 255],
        UI_SUCCESS_BACKGROUND => [32, 80, 192, 255],
        // Darkened Catppuccin blue/red keep white debugger text readable.
        UI_DEBUG_CURRENT_BACKGROUND => [48, 70, 108, 255],
        UI_BREAKPOINT_BACKGROUND => [112, 52, 67, 255],
        // Dark gray caret line: visible against true black without competing
        // with the syntax colors or the debugger's blue and red rows.
        UI_CURRENT_LINE_BACKGROUND => [38, 40, 52, 255],
        // Dithered window shadow, dim enough to read as depth, not content.
        UI_SHADOW_COLOR => [58, 60, 74, 255],
        // Catppuccin Mocha accents over Fanticon's required true-black page.
        ASM_TEXT_COLOR => [205, 214, 244, 255],
        ASM_LABEL_COLOR => [180, 190, 254, 255],
        ASM_OPCODE_COLOR => [137, 180, 250, 255],
        ASM_DIRECTIVE_COLOR => [203, 166, 247, 255],
        ASM_NUMBER_COLOR => [250, 179, 135, 255],
        ASM_COMMENT_COLOR => [127, 132, 156, 255],
        ASM_STRING_COLOR => [166, 227, 161, 255],
        ASM_ERROR_COLOR => [243, 139, 168, 255],
        // Catppuccin yellow, kept distinct from every other ASM_* color so
        // macro definitions, invocations, and ]1-]8 parameters read as one
        // visual category apart from ordinary directives/opcodes.
        ASM_MACRO_COLOR => [249, 226, 175, 255],
        other => rgb332_to_rgba(other),
    }
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
    if is_macro_invocation(trimmed_start) {
        // Align the same way an ordinary instruction does: label at column
        // 1, PMC/>>> keyword at column 9, and everything after it at column
        // 15 - matching the label/opcode/operand split below. The one
        // difference is that the "operand" here is only ever split once,
        // right after the keyword; its semicolon-separated argument list is
        // copied through untouched instead of being re-tokenized, since
        // reflowing it would corrupt it the same way the old bug did.
        let first_field = trimmed_start.split_whitespace().next().unwrap_or_default();
        let has_label = !leading_whitespace && !is_operation(first_field);
        let mut output = String::new();
        let after_label = if has_label {
            output.push_str(first_field);
            trimmed_start[first_field.len()..].trim_start()
        } else {
            trimmed_start
        };
        let keyword_len = after_label.split_whitespace().next().unwrap_or_default().len();
        let (keyword, arguments) = after_label.split_at(keyword_len);
        pad_to_column(&mut output, 9);
        output.push_str(keyword);
        let arguments = arguments.trim_start();
        if !arguments.is_empty() {
            pad_to_column(&mut output, 15);
            output.push_str(arguments);
        }
        return output;
    }

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

/// The shared column width for `;===...`/`;---...` section-heading dividers,
/// matching the hand-written 54-column bars already used throughout the
/// codebase (e.g. `funcs.inc`'s `;-----------------------------------------------------`).
const BANNER_COLUMN: usize = 54;

fn banner_bar(fill: char) -> String {
    let mut bar = String::from(";");
    while bar.chars().count() < BANNER_COLUMN {
        bar.push(fill);
    }
    bar
}

fn pad_to_column(output: &mut String, column: usize) {
    let spaces = column.saturating_sub(output.len()).max(1);
    output.extend(core::iter::repeat_n(' ', spaces));
}

/// Mirrors the assembler's semicolon-argument directives so the editor does
/// not mistake named macro parameters or repeat indexes for comments.
fn is_macro_invocation(line: &str) -> bool {
    line.split_whitespace().take(2).any(|field| {
        matches!(field.to_ascii_uppercase().as_str(), "MAC" | "PMC" | ">>>" | "LUP" | "REPEAT")
    })
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

    let (code, comment) =
        if is_macro_invocation(trimmed) { (line, None) } else { split_assembly_comment(line) };
    if let Some(comment) = comment {
        let start = comment.as_ptr() as usize - line.as_ptr() as usize;
        colors[start..].fill(ASM_COMMENT_COLOR);
    }

    let tokens = assembly_tokens(code);
    if let Some(operation_index) = tokens.iter().position(|(_, token)| is_operation(token)) {
        let (op_start, op_token) = tokens[operation_index];
        let is_macro_related = is_macro_keyword(op_token);
        if operation_index > 0 {
            let (start, token) = tokens[0];
            let label_color = if is_macro_related { ASM_MACRO_COLOR } else { ASM_LABEL_COLOR };
            colors[start..start + token.len()].fill(label_color);
        }
        let color = if is_macro_related {
            ASM_MACRO_COLOR
        } else if is_directive(op_token) {
            ASM_DIRECTIVE_COLOR
        } else {
            ASM_OPCODE_COLOR
        };
        colors[op_start..op_start + op_token.len()].fill(color);

        // A PMC/>>> invocation names the macro right after the keyword,
        // packed against its semicolon-separated arguments with no space
        // (e.g. `PMC PRINTAT;message;2;5`) - color just the name portion.
        if matches!(op_token.to_ascii_uppercase().as_str(), "PMC" | ">>>")
            && let Some((name_start, name_token)) = tokens.get(operation_index + 1).copied()
        {
            let name_len = name_token.split([',', ';']).next().unwrap_or(name_token).len();
            colors[name_start..name_start + name_len].fill(ASM_MACRO_COLOR);
        }
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
            let start = index;
            let is_macro_parameter = bytes[index] == b']';
            index += 1;
            while index < bytes.len()
                && (bytes[index].is_ascii_alphanumeric()
                    || matches!(bytes[index], b'_' | b'.' | b']'))
            {
                index += 1;
            }
            if is_macro_parameter {
                colors[start..index].fill(ASM_MACRO_COLOR);
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

fn is_macro_keyword(token: &str) -> bool {
    matches!(token.to_ascii_uppercase().as_str(), "MAC" | "EOM" | "PMC" | "<<<" | ">>>")
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
            | "JAM"
            | "SLO"
            | "RLA"
            | "SRE"
            | "RRA"
            | "SAX"
            | "LAX"
            | "DCP"
            | "ISC"
            | "ISB"
            | "ANC"
            | "ALR"
            | "ARR"
            | "XAA"
            | "AXS"
            | "SBX"
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
            | "IF"
            | "ELSE"
            | "FIN"
            | "ENDIF"
            | "LUP"
            | "REPEAT"
            | "--^"
            | "ENDREP"
            | "DUM"
            | "DEND"
            | "PROC"
            | "ENDPROC"
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
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, selected: 12 }));
        editor.handle_overlay_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, selected: 0 }));

        editor.handle_mouse_press(4 * GLYPH_WIDTH, 7 * GLYPH_HEIGHT, false);
        assert!(matches!(editor.overlay, Overlay::Menu { menu: MenuKind::File, .. }));

        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [0; COLUMNS * ROWS];
        let mut backgrounds = [0; COLUMNS * ROWS];
        let mut background_gradients = [false; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        editor.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            CellStyle::new(UI_WHITE_COLOR, 0),
        );
        assert_eq!(cells[2 * COLUMNS + 1], b'N');
        assert_eq!(cells[2 * COLUMNS + 2], b'e');
        assert!(inverse[2 * COLUMNS + 1]);
        assert!(
            cells[7 * COLUMNS + 1..7 * COLUMNS + 15].iter().all(|cell| *cell == BOX_HORIZONTAL)
        );
    }

    #[test]
    fn every_menu_mouse_row_matches_its_command_and_file_exit_is_clickable() {
        for menu in [
            MenuKind::File,
            MenuKind::Edit,
            MenuKind::Build,
            MenuKind::Debug,
            MenuKind::Music,
            MenuKind::Help,
        ] {
            assert_eq!(menu_labels(menu).len(), menu_items(menu).len(), "{menu:?}");
        }

        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.open_menu(MenuKind::File);
        assert_eq!(
            editor.handle_mouse_press(4 * GLYPH_WIDTH, 14 * GLYPH_HEIGHT, false),
            EditorAction::Exit
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
            position: None,
            channel_levels: [0; 4],
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

        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let labels = menu_labels(MenuKind::Music).iter().filter(|label| !label.is_empty());
        assert!(labels.clone().all(|label| label.len() == menu_width(MenuKind::Music) - 4));
        assert_eq!(menu_origin(MenuKind::Music), 27);
        assert_eq!(menu_bar_hit(26), None);
        assert_eq!(menu_bar_hit(27), Some(MenuKind::Music));

        let music_text =
            editor.music_status.as_ref().unwrap().display_marquee(editor.music_marquee_offset);
        let music_start = COLUMNS - music_text.len();
        let first_cell = (0..GLYPH_HEIGHT)
            .flat_map(|y| (0..GLYPH_WIDTH).map(move |x| (music_start * GLYPH_WIDTH + x, y)))
            .map(|(x, y)| surface.pixel(x, y))
            .collect::<Vec<_>>();
        // Shading darkens both down the cell, so match the unshaded top row.
        assert!(first_cell.contains(&editor_color(UI_SUCCESS_BACKGROUND)));
        assert!(first_cell.contains(&editor_color(UI_WHITE_COLOR)));
    }

    #[test]
    fn help_about_renders_version_animated_logo_and_gradient_menu_bar() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        assert_eq!(menu_origin(MenuKind::Help), 34);
        assert_eq!(menu_bar_hit(34), Some(MenuKind::Help));
        assert_eq!(menu_hotkey(MenuKind::Help, "a"), Some(1));
        assert_eq!(menu_hotkey(MenuKind::Help, "f"), Some(0));
        assert_eq!(about_wave_phase(119), None);
        assert_eq!(about_wave_phase(120), Some(0));
        assert_eq!(about_wave_phase(121), Some(1));
        assert_eq!(about_wave_strength(119), 0);
        assert_eq!(about_wave_strength(120), 0);
        assert_eq!(about_wave_strength(136), 256);
        assert_eq!(about_wave_strength(163), 256);
        assert_eq!(about_wave_strength(179), 0);
        assert_eq!(about_wave_strength(180), 0);
        let fade_in = (120..=136).map(about_wave_strength).collect::<Vec<_>>();
        let fade_out = (163..=179).map(about_wave_strength).collect::<Vec<_>>();
        assert!(fade_in.windows(2).all(|pair| pair[0] <= pair[1]));
        assert!(fade_out.windows(2).all(|pair| pair[0] >= pair[1]));
        let horizontal_amplitude =
            (0..ABOUT_LOGO_HEIGHT).map(|y| about_wave_offsets(0, y, 136).0.abs()).max().unwrap();
        let vertical_amplitude =
            (0..ABOUT_LOGO_WIDTH).map(|x| about_wave_offsets(x, 0, 136).1.abs()).max().unwrap();
        assert_eq!(horizontal_amplitude, 8);
        assert_eq!(vertical_amplitude, 3);
        editor.activate_menu(MenuKind::Help, 1);
        assert!(matches!(editor.overlay, Overlay::About { frame: 0 }));

        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [0; COLUMNS * ROWS];
        let mut backgrounds = [0; COLUMNS * ROWS];
        let mut background_gradients = [false; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        editor.render_overlay(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            CellStyle::new(UI_WHITE_COLOR, 0),
        );
        assert!(cells.windows(8).any(|window| window == b"FANTICON"));
        let version = env!("CARGO_PKG_VERSION").as_bytes();
        assert!(cells.windows(version.len()).any(|window| window == version));

        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let still = surface.pixels().to_vec();
        assert!(
            ABOUT_PALETTE.iter().any(|color| {
                (0..EDITOR_DISPLAY_HEIGHT)
                    .any(|y| (0..EDITOR_DISPLAY_WIDTH).any(|x| surface.pixel(x, y) == *color))
            }),
            "the logo paints its own colors straight into the surface"
        );
        let top = surface.pixel((COLUMNS - 1) * GLYPH_WIDTH, 0);
        let bottom = surface.pixel((COLUMNS - 1) * GLYPH_WIDTH, 7);
        assert_eq!(top, [255, 255, 255, 255]);
        assert!(bottom[0] < top[0]);

        for _ in 0..130 {
            editor.update();
        }
        editor.render(&mut surface, false);
        assert_ne!(surface.pixels(), still.as_slice());
        editor.handle_overlay_key(&Key::Named(NamedKey::Escape), ModifiersState::empty());
        assert!(matches!(editor.overlay, Overlay::None));
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

        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let cell_colors = |column: usize, row: usize| {
            (0..GLYPH_HEIGHT)
                .flat_map(|glyph_y| {
                    (0..GLYPH_WIDTH).map(move |glyph_x| {
                        (column * GLYPH_WIDTH + glyph_x, row * GLYPH_HEIGHT + glyph_y)
                    })
                })
                .map(|(x, y)| surface.pixel(x, y))
                .collect::<Vec<_>>()
        };
        let breakpoint = cell_colors(EDITOR_START, EDITOR_FIRST_ROW);
        let executing = cell_colors(EDITOR_START + 1, EDITOR_FIRST_ROW + 1);

        assert!(breakpoint.contains(&editor_color(UI_BREAKPOINT_BACKGROUND)));
        assert!(executing.contains(&editor_color(UI_DEBUG_CURRENT_BACKGROUND)));
        let has_white_gradient_text = |colors: &[Rgba]| {
            colors.iter().any(|rgba| rgba[0] == rgba[1] && rgba[1] == rgba[2] && rgba[0] >= 127)
        };
        assert!(has_white_gradient_text(&breakpoint));
        assert!(has_white_gradient_text(&executing));
        assert_eq!(editor_color(UI_DEBUG_CURRENT_BACKGROUND), [48, 70, 108, 255]);
        assert_eq!(editor_color(UI_BREAKPOINT_BACKGROUND), [112, 52, 67, 255]);

        let background_gradient = |row: usize| {
            // The last column is the scrollbar now, so sample the one before it.
            let x = (COLUMNS - 2) * GLYPH_WIDTH;
            (0..GLYPH_HEIGHT)
                .map(|glyph_y| surface.pixel(x, row * GLYPH_HEIGHT + glyph_y))
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
        assert_eq!(editor.debug_snapshot.as_ref().unwrap().address_space[0xc100], 0xea);
        assert_eq!(
            editor.handle_key(
                &Key::Character("3".into()),
                PhysicalKey::Code(KeyCode::Digit3),
                ModifiersState::CONTROL,
            ),
            EditorAction::None
        );
        assert_eq!(editor.debug_view, DebugView::Memory);
        editor.handle_key(
            &Key::Character("A".into()),
            PhysicalKey::Code(KeyCode::KeyA),
            ModifiersState::empty(),
        );
        assert_eq!(
            editor.handle_key(
                &Key::Character("5".into()),
                PhysicalKey::Code(KeyCode::Digit5),
                ModifiersState::empty(),
            ),
            EditorAction::Debug(DebugCommand::WriteMemory { address: 0xc100, value: 0xa5 })
        );
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
    fn windows_wear_dos_furniture_focus_rules_shadows_and_a_scrollbar() {
        let filesystem = shared_filesystem();
        let long = (0..200).map(|n| format!(" NOP ; {n}")).collect::<Vec<_>>().join("\n");
        filesystem.borrow_mut().write_text("main.asm", &long).unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [UI_WHITE_COLOR; COLUMNS * ROWS];
        let mut backgrounds = [0; COLUMNS * ROWS];
        let mut background_gradients = [false; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];
        let at = |cells: &[u8; COLUMNS * ROWS], x: usize, y: usize| cells[y * COLUMNS + x];

        // An unfocused editor keeps the plain divider; focusing the browser doubles it.
        editor.render_project_browser(&mut cells, &mut inverse);
        assert_eq!(at(&cells, PROJECT_WIDTH, 4), BOX_VERTICAL);
        editor.project_focused = true;
        editor.render_project_browser(&mut cells, &mut inverse);
        assert_eq!(at(&cells, PROJECT_WIDTH, 4), DBL_VERTICAL);

        // The scrollbar caps with arrows and rides a dithered track.
        editor.render_scrollbar(&mut cells, &mut foregrounds);
        assert_eq!(at(&cells, COLUMNS - 1, EDITOR_FIRST_ROW), SYMBOL_ARROW_UP);
        assert_eq!(at(&cells, COLUMNS - 1, EDITOR_FIRST_ROW + TEXT_ROWS - 1), SYMBOL_ARROW_DOWN);
        let track: Vec<u8> =
            (1..TEXT_ROWS - 1).map(|row| at(&cells, COLUMNS - 1, EDITOR_FIRST_ROW + row)).collect();
        assert!(track.contains(&SHADE_MEDIUM), "thumb");
        assert!(track.contains(&SHADE_LIGHT), "track");
        // Text stops one column short so a long line never collides with it.
        assert_eq!(EDITOR_COLUMNS, COLUMNS - EDITOR_CODE_START - 1);

        // A dialog draws a doubled rule and casts a shadow below and to the right.
        let rect = CellRect { x: 10, y: 10, width: 20, height: 6 };
        draw_dialog(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            rect,
            CellStyle::new(UI_WHITE_COLOR, 0),
        );
        assert_eq!(at(&cells, 10, 15), DBL_BOTTOM_LEFT);
        assert_eq!(at(&cells, 29, 15), DBL_BOTTOM_RIGHT);
        assert_eq!(at(&cells, 30, 11), SHADE_LIGHT, "shadow down the right edge");
        assert_eq!(at(&cells, 11, 16), SHADE_LIGHT, "shadow along the bottom");
        assert_eq!(foregrounds[16 * COLUMNS + 11], UI_SHADOW_COLOR);

        // A window owns the shading of every cell it covers: a caret line or
        // debugger row underneath must not keep banding through the dialog.
        background_gradients.fill(true);
        draw_dialog(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            rect,
            CellStyle::new(UI_WHITE_COLOR, 0),
        );
        for row in 10..16 {
            for column in 10..30 {
                assert!(
                    !background_gradients[row * COLUMNS + column],
                    "dialog cell {column},{row} still carries the row shading beneath it"
                );
            }
        }
        assert!(background_gradients[9 * COLUMNS + 10], "rows outside keep their shading");
    }

    #[test]
    fn caret_blink_restarts_whenever_the_cursor_moves() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " NOP\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        assert!(editor.cursor_blink_visible());
        for _ in 0..CURSOR_BLINK_FRAMES {
            editor.update();
        }
        assert!(!editor.cursor_blink_visible(), "the caret should have blinked dark by now");

        // Arrowing to a new line restarts the phase so the caret is lit on arrival.
        editor.handle_key(
            &Key::Named(NamedKey::ArrowDown),
            PhysicalKey::Code(KeyCode::ArrowDown),
            ModifiersState::empty(),
        );
        editor.update();
        assert_eq!(editor.cursor.line, 1);
        assert!(editor.cursor_blink_visible());

        // Typing moves the caret too, and restarts the phase the same way.
        for _ in 0..CURSOR_BLINK_FRAMES {
            editor.update();
        }
        assert!(!editor.cursor_blink_visible());
        editor.handle_key(
            &Key::Character("A".into()),
            PhysicalKey::Code(KeyCode::KeyA),
            ModifiersState::empty(),
        );
        editor.update();
        assert!(editor.cursor_blink_visible());
    }

    #[test]
    fn caret_line_is_shaded_only_while_the_buffer_is_editable() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", " NOP\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        let row_has = |surface: &Surface, row: usize, color: u8| {
            let wanted = editor_color(color);
            (0..GLYPH_HEIGHT).any(|glyph_y| {
                let y = row * GLYPH_HEIGHT + glyph_y;
                (EDITOR_START * GLYPH_WIDTH..EDITOR_DISPLAY_WIDTH)
                    .any(|x| surface.pixel(x, y) == wanted)
            })
        };

        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        assert!(row_has(&surface, EDITOR_FIRST_ROW, UI_CURRENT_LINE_BACKGROUND));
        assert!(!row_has(&surface, EDITOR_FIRST_ROW + 1, UI_CURRENT_LINE_BACKGROUND));
        assert_ne!(editor_color(UI_CURRENT_LINE_BACKGROUND), [0, 0, 0, 255]);

        // Following the caret keeps the shade on whichever line it lands on.
        editor.handle_key(
            &Key::Named(NamedKey::ArrowDown),
            PhysicalKey::Code(KeyCode::ArrowDown),
            ModifiersState::empty(),
        );
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        assert!(!row_has(&surface, EDITOR_FIRST_ROW, UI_CURRENT_LINE_BACKGROUND));
        assert!(row_has(&surface, EDITOR_FIRST_ROW + 1, UI_CURRENT_LINE_BACKGROUND));

        // A live session makes the buffer read-only, so the editable shade goes away.
        editor.debug_active = true;
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        assert!(!row_has(&surface, EDITOR_FIRST_ROW, UI_CURRENT_LINE_BACKGROUND));
        assert!(!row_has(&surface, EDITOR_FIRST_ROW + 1, UI_CURRENT_LINE_BACKGROUND));
    }

    #[test]
    fn breakpoints_stop_in_the_source_editor_and_the_detail_panel_is_opt_in() {
        use fanticon::{
            cartridge::Cartridge, debugger::Debugger, machine::BANK_SIZE, system::FanticonMachine,
        };

        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("main.asm", "RESET NOP\n RTS").unwrap();
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

        // Stopping lands in the source file with the caret on the executing line.
        assert!(!editor.debug_panel_visible);
        assert!(!editor.project_focused);
        assert_eq!(editor.filename.as_deref(), Some("main.asm"));
        assert_eq!(editor.debug_location, Some(("main.asm".to_owned(), 0)));
        assert_eq!(editor.cursor.line, 0);

        // The executing line keeps its blue highlight with the panel hidden.
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let row_colors = |column: usize, row: usize| {
            (0..GLYPH_HEIGHT)
                .flat_map(|glyph_y| {
                    (0..GLYPH_WIDTH).map(move |glyph_x| {
                        (column * GLYPH_WIDTH + glyph_x, row * GLYPH_HEIGHT + glyph_y)
                    })
                })
                .map(|(x, y)| surface.pixel(x, y))
                .collect::<Vec<_>>()
        };
        assert!(
            row_colors(EDITOR_START + 1, EDITOR_FIRST_ROW)
                .contains(&editor_color(UI_DEBUG_CURRENT_BACKGROUND))
        );

        // The source is read-only while paused: typing and deleting do nothing.
        let original = editor.lines.clone();
        editor.handle_key(
            &Key::Character("X".into()),
            PhysicalKey::Code(KeyCode::KeyX),
            ModifiersState::empty(),
        );
        editor.handle_key(
            &Key::Named(NamedKey::Tab),
            PhysicalKey::Code(KeyCode::Tab),
            ModifiersState::empty(),
        );
        editor.handle_key(
            &Key::Named(NamedKey::Enter),
            PhysicalKey::Code(KeyCode::Enter),
            ModifiersState::empty(),
        );
        editor.handle_key(
            &Key::Named(NamedKey::Delete),
            PhysicalKey::Code(KeyCode::Delete),
            ModifiersState::empty(),
        );
        editor.handle_key(
            &Key::Named(NamedKey::Backspace),
            PhysicalKey::Code(KeyCode::Backspace),
            ModifiersState::empty(),
        );
        editor.clipboard = "PASTED".to_owned();
        editor.handle_key(
            &Key::Character("v".into()),
            PhysicalKey::Code(KeyCode::KeyV),
            ModifiersState::CONTROL,
        );
        assert_eq!(editor.lines, original);
        assert!(!editor.dirty);

        // Navigation still works, and the panel did not steal Tab while hidden.
        assert_eq!(editor.debug_view, DebugView::State);
        editor.handle_key(
            &Key::Named(NamedKey::ArrowDown),
            PhysicalKey::Code(KeyCode::ArrowDown),
            ModifiersState::empty(),
        );
        assert_eq!(editor.cursor.line, 1);

        // Ctrl/Cmd+D reveals it, and then Tab cycles the detail views.
        editor.handle_key(
            &Key::Character("d".into()),
            PhysicalKey::Code(KeyCode::KeyD),
            ModifiersState::CONTROL,
        );
        assert!(editor.debug_panel_visible);
        editor.handle_key(
            &Key::Named(NamedKey::Tab),
            PhysicalKey::Code(KeyCode::Tab),
            ModifiersState::empty(),
        );
        assert_eq!(editor.debug_view, DebugView::Code);

        // With the panel open the editor is inert: the caret does not move.
        let caret = editor.cursor;
        editor.handle_key(
            &Key::Named(NamedKey::End),
            PhysicalKey::Code(KeyCode::End),
            ModifiersState::empty(),
        );
        editor.handle_key(
            &Key::Named(NamedKey::F6),
            PhysicalKey::Code(KeyCode::F6),
            ModifiersState::empty(),
        );
        assert_eq!(editor.cursor, caret);
        assert!(!editor.project_focused);

        // Escape hands the keyboard back to the source without ending the session.
        editor.handle_key(
            &Key::Named(NamedKey::Escape),
            PhysicalKey::Code(KeyCode::Escape),
            ModifiersState::empty(),
        );
        assert!(!editor.debug_panel_visible);
        assert!(editor.debug_active);

        // Picking a view re-opens it from the source, and Ctrl/Cmd+D closes it.
        editor.handle_key(
            &Key::Character("4".into()),
            PhysicalKey::Code(KeyCode::Digit4),
            ModifiersState::CONTROL,
        );
        assert!(editor.debug_panel_visible);
        assert_eq!(editor.debug_view, DebugView::Video);
        editor.handle_key(
            &Key::Character("d".into()),
            PhysicalKey::Code(KeyCode::KeyD),
            ModifiersState::CONTROL,
        );
        assert!(!editor.debug_panel_visible);
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
    fn tab_titles_and_dialog_chrome_no_longer_force_uppercase() {
        // Filenames are tracked case-insensitively (`self.filename` is
        // canonicalized to lowercase so "Player.asm" and "player.asm" are the
        // same open tab); that's a separate, intentional identity rule, not a
        // rendering bug. What this guards is that rendering stops re-mangling
        // whatever case is actually stored into all-caps on top of that.
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("Player.asm", "").unwrap();
        let editor = TextEditor::new(filesystem, shared_ui_colors(), Some("Player.asm".to_owned()));
        let mut cells = [b' '; COLUMNS * ROWS];
        let mut foregrounds = [0; COLUMNS * ROWS];
        let mut inverse = [false; COLUMNS * ROWS];

        editor.render_tabs(&mut cells, &mut foregrounds, &mut inverse);

        let start = COLUMNS + EDITOR_START + 1;
        let label: String =
            cells[start..start + TAB_WIDTH].iter().map(|&byte| byte as char).collect();
        assert!(label.contains("player.asm"), "tab label was {label:?}");
        assert!(!label.contains("PLAYER.ASM"));
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

        assert_eq!(editor.save_as("ONE.TXT"), Err("File is already open".to_owned()));
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
    fn built_in_fanticon_include_is_a_virtual_read_only_document() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("fanticon.inc", "STALE DISK COPY").unwrap();
        let mut editor = TextEditor::new(
            filesystem.clone(),
            shared_ui_colors(),
            Some("FANTICON.INC".to_owned()),
        );

        assert_eq!(editor.filename.as_deref(), Some("/fanticon.inc"));
        assert!(editor.system_read_only());
        assert!(editor.lines.join("\n").contains("FANTICON_MAJOR"));
        assert!(!editor.lines.join("\n").contains("STALE DISK COPY"));

        let original = editor.lines.clone();
        editor.insert_text("CHANGED");
        editor.insert_newline();
        editor.backspace();
        editor.delete_forward();
        editor.clipboard = "PASTED".to_owned();
        editor.paste();
        assert_eq!(editor.replace_all("FANTICON", "BROKEN"), 0);
        assert_eq!(editor.lines, original);
        assert!(!editor.dirty);
        assert_eq!(
            editor.save_as("copy.inc"),
            Err("FANTICON.INC is a read-only system file".to_owned())
        );
        assert_eq!(filesystem.borrow().read_text("fanticon.inc").unwrap(), "STALE DISK COPY");
    }

    #[test]
    fn project_file_named_fanticon_inc_remains_editable() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().create_directory("project").unwrap();
        filesystem.borrow_mut().write_text("project/fanticon.inc", "VALUE EQU 1").unwrap();
        let mut editor = TextEditor::new(
            filesystem.clone(),
            shared_ui_colors(),
            Some("project/fanticon.inc".to_owned()),
        );

        assert!(!editor.system_read_only());
        editor.cursor.column = editor.lines[0].len();
        editor.insert_text(" ; LOCAL");
        editor.save_as("project/fanticon.inc").unwrap();
        assert!(filesystem.borrow().read_text("project/fanticon.inc").unwrap().contains("LOCAL"));
        assert_eq!(
            editor.save_as("/fanticon.inc"),
            Err("FANTICON.INC is a read-only system file".to_owned())
        );
    }

    #[test]
    fn music_assets_live_in_tabs_and_save_as_assembler_source() {
        let filesystem = shared_filesystem();
        let colors = shared_ui_colors();
        let mut editor = TextEditor::new(filesystem.clone(), colors.clone(), None);
        editor.new_music_document();
        assert!(editor.music_active());
        assert!(!editor.music_source_active());

        editor.save_as("theme.mus").unwrap();
        let source = filesystem.borrow().read_text("theme.mus").unwrap();
        assert!(source.contains(";@FANTICON-MUSIC 2"));
        assert!(source.contains(";@FRAME 00"));
        assert!(source.contains("THEME_MUSIC"));

        let reopened = TextEditor::new(filesystem, colors, Some("theme.mus".to_owned()));
        assert!(reopened.music_active());
        assert_eq!(reopened.filename.as_deref(), Some("theme.mus"));
    }

    #[test]
    fn music_tabs_can_toggle_to_ascii_source_and_back() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_music_document();
        editor.toggle_music_source_view();
        assert!(editor.music_source_active());
        assert!(editor.lines.join("\n").contains(";@PATTERN P1 00"));
        editor.toggle_music_source_view();
        assert!(!editor.music_source_active());
    }

    #[test]
    fn f7_auditions_the_active_tracker_and_f8_restarts_it() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_music_document();
        let play = editor.handle_key(
            &Key::Named(NamedKey::F7),
            PhysicalKey::Code(KeyCode::F7),
            ModifiersState::empty(),
        );
        assert!(matches!(play, EditorAction::Music(MusicCommand::LoadTracker { .. })));
        let restart = editor.handle_key(
            &Key::Named(NamedKey::F8),
            PhysicalKey::Code(KeyCode::F8),
            ModifiersState::empty(),
        );
        assert!(matches!(restart, EditorAction::Music(MusicCommand::LoadTracker { .. })));
    }

    #[test]
    fn instrument_note_keys_audition_until_the_matching_key_is_released() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_music_document();
        let physical = PhysicalKey::Code(KeyCode::KeyA);
        for _ in 0..2 {
            editor
                .music_tabs
                .get_mut(&editor.document_id)
                .unwrap()
                .handle_key(&Key::Character("v".into()), ModifiersState::empty());
        }

        let press =
            editor.handle_key(&Key::Character("a".into()), physical, ModifiersState::empty());
        assert!(matches!(press, EditorAction::Music(MusicCommand::AuditionTracker { .. })));
        assert_eq!(
            editor.handle_key(&Key::Character("a".into()), physical, ModifiersState::empty()),
            EditorAction::None,
            "key repeat must not restart the envelope"
        );
        assert_eq!(editor.handle_key_release(PhysicalKey::Code(KeyCode::KeyS)), EditorAction::None);
        assert_eq!(editor.handle_key_release(physical), EditorAction::Music(MusicCommand::Stop));
        assert_eq!(editor.handle_key_release(physical), EditorAction::None);
    }

    #[test]
    fn space_starts_and_stops_tracker_playback_and_follows_the_live_row() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_music_document();
        editor.save_as("theme.mus").unwrap();
        let start = editor.handle_key(
            &Key::Named(NamedKey::Space),
            PhysicalKey::Code(KeyCode::Space),
            ModifiersState::empty(),
        );
        assert!(matches!(start, EditorAction::Music(MusicCommand::LoadTracker { .. })));

        editor.set_music_status(Some(MusicStatus {
            filename: "theme.mus".to_owned(),
            title: "theme.mus".to_owned(),
            artist: "FANTICON TRACKER".to_owned(),
            track: 1,
            tracks: 1,
            paused: false,
            looping: true,
            position: Some((40, 64)),
            channel_levels: [15, 8, 4, 2],
        }));
        let tracker = &editor.music_tabs[&editor.document_id];
        let (row, playback_row, visible) = tracker.playback_view();
        assert_eq!(row, 40);
        assert_eq!(playback_row, Some(40));
        assert!(visible.contains(&40));

        assert_eq!(
            editor.handle_key(
                &Key::Named(NamedKey::Space),
                PhysicalKey::Code(KeyCode::Space),
                ModifiersState::empty(),
            ),
            EditorAction::Music(MusicCommand::Stop)
        );
    }

    #[test]
    fn two_unsaved_songs_get_distinct_playback_identities() {
        // Regression test: two never-saved tracker tabs used to both fall
        // back to the literal filename "UNTITLED.MUS" for play/pause/stop
        // routing. That made the radio's "currently playing" filename match
        // *either* tab's placeholder, so playing the first song and then
        // pressing play on the second looked like "still playing the same
        // song" and just toggled pause/stop instead of loading the second
        // song's actual data.
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_music_document();
        let first_id = editor.document_id;
        let first_start = editor.handle_key(
            &Key::Named(NamedKey::Space),
            PhysicalKey::Code(KeyCode::Space),
            ModifiersState::empty(),
        );
        let EditorAction::Music(MusicCommand::LoadTracker { filename: first_filename, .. }) =
            first_start
        else {
            panic!("expected the first song to load: {first_start:?}");
        };

        editor.new_music_document();
        let second_id = editor.document_id;
        assert_ne!(first_id, second_id);

        // The radio is still reporting the first tab's song as playing.
        editor.set_music_status(Some(MusicStatus {
            filename: first_filename.clone(),
            title: first_filename.clone(),
            artist: "Fanticon Tracker".to_owned(),
            track: 1,
            tracks: 1,
            paused: false,
            looping: true,
            position: Some((0, 64)),
            channel_levels: [0; 4],
        }));

        let second_start = editor.handle_key(
            &Key::Named(NamedKey::Space),
            PhysicalKey::Code(KeyCode::Space),
            ModifiersState::empty(),
        );
        let EditorAction::Music(MusicCommand::LoadTracker { filename: second_filename, .. }) =
            second_start
        else {
            panic!(
                "second tab's song must load its own data, not toggle the first tab: {second_start:?}"
            );
        };
        assert_ne!(first_filename, second_filename);
    }

    #[test]
    fn editor_renders_menu_and_document_to_framebuffer() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.insert_text("hello");
        editor.open_menu(MenuKind::File);
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, true);
        assert!(surface.pixels().iter().any(|channel| *channel == 255));
        assert!(surface.pixels().iter().any(|channel| *channel == 0));
    }

    #[test]
    fn blinking_cursor_is_a_white_block_with_black_cell_character() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("note.txt", "A").unwrap();
        let editor = TextEditor::new(filesystem, shared_ui_colors(), Some("note.txt".to_owned()));
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);

        editor.render(&mut surface, true);

        let origin_x = EDITOR_CODE_START * GLYPH_WIDTH;
        let origin_y = EDITOR_FIRST_ROW * GLYPH_HEIGHT;
        for (glyph_y, bits) in CHARACTER_ROM[b'A' as usize].iter().copied().enumerate() {
            for glyph_x in 0..GLYPH_WIDTH {
                let pixel = surface.pixel(origin_x + glyph_x, origin_y + glyph_y);
                let is_character = bits & (0x80 >> glyph_x) != 0;
                let expected =
                    if is_character { [0, 0, 0, 255] } else { editor_color(UI_WHITE_COLOR) };
                assert_eq!(pixel, expected);
            }
        }
    }

    #[test]
    fn lowercase_source_text_renders_as_typed_instead_of_being_forced_uppercase() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("note.txt", "a").unwrap();
        let editor = TextEditor::new(filesystem, shared_ui_colors(), Some("note.txt".to_owned()));
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);

        editor.render(&mut surface, true);

        let origin_x = EDITOR_CODE_START * GLYPH_WIDTH;
        let origin_y = EDITOR_FIRST_ROW * GLYPH_HEIGHT;
        for (glyph_y, bits) in CHARACTER_ROM[b'a' as usize].iter().copied().enumerate() {
            for glyph_x in 0..GLYPH_WIDTH {
                let pixel = surface.pixel(origin_x + glyph_x, origin_y + glyph_y);
                let is_character = bits & (0x80 >> glyph_x) != 0;
                let expected =
                    if is_character { [0, 0, 0, 255] } else { editor_color(UI_WHITE_COLOR) };
                assert_eq!(pixel, expected);
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
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);

        editor.render(&mut surface, false);

        assert_eq!(surface.pixel(0, 0), editor_color(UI_WHITE_COLOR));
        let border = CHARACTER_ROM[BOX_TOP_LEFT as usize]
            .iter()
            .enumerate()
            .find_map(|(y, bits)| {
                (0..GLYPH_WIDTH).find(|x| bits & (0x80 >> x) != 0).map(|x| (x, y))
            })
            .unwrap();
        let border_pixel = surface.pixel(border.0, GLYPH_HEIGHT + border.1);
        // Frame rules never take the scanline shading, so chrome stays pure white.
        assert_eq!(border_pixel, [255, 255, 255, 255]);
    }

    #[test]
    fn project_separator_is_thin_edge_aligned_and_uniform() {
        let editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);

        let separator_x = PROJECT_WIDTH * GLYPH_WIDTH;
        for y in GLYPH_HEIGHT..(ROWS - 1) * GLYPH_HEIGHT {
            assert_eq!(
                surface.pixel(separator_x, y),
                editor_color(UI_WHITE_COLOR),
                "separator changed color at scanline {y}"
            );
            assert_eq!(surface.pixel(separator_x + 1, y), [0, 0, 0, 255]);
        }
    }

    #[test]
    fn window_frame_glyphs_align_to_their_outer_cell_edges() {
        assert_eq!(CHARACTER_ROM[BOX_TOP_HORIZONTAL as usize], [0xff, 0, 0, 0, 0, 0, 0, 0]);
        assert_eq!(CHARACTER_ROM[BOX_BOTTOM_HORIZONTAL as usize], [0, 0, 0, 0, 0, 0, 0, 0xff]);
        assert_eq!(CHARACTER_ROM[BOX_VERTICAL as usize], [0x80; GLYPH_HEIGHT]);
        assert_eq!(CHARACTER_ROM[BOX_RIGHT_VERTICAL as usize], [0x01; GLYPH_HEIGHT]);
        assert_eq!(CHARACTER_ROM[BOX_CAPTION_LEFT as usize][3], 0xff);
        assert_eq!(CHARACTER_ROM[BOX_CAPTION_RIGHT as usize][3], 0xff);
        assert_eq!(&CHARACTER_ROM[BOX_CAPTION_LEFT as usize][..3], &[0; 3]);
        assert_eq!(&CHARACTER_ROM[BOX_CAPTION_RIGHT as usize][..3], &[0; 3]);
        assert_eq!(CHARACTER_ROM[BOX_CAPTION_LEFT as usize][4], 0x80);
        assert_eq!(CHARACTER_ROM[BOX_CAPTION_RIGHT as usize][4], 0x01);
    }

    #[test]
    fn popup_windows_clear_underlying_text_and_use_rom_borders() {
        let mut cells = [b'X'; COLUMNS * ROWS];
        let mut foregrounds = [ASM_ERROR_COLOR; COLUMNS * ROWS];
        let mut backgrounds = [ASM_ERROR_COLOR; COLUMNS * ROWS];
        let mut background_gradients = [false; COLUMNS * ROWS];
        let mut inverse = [true; COLUMNS * ROWS];

        draw_dialog(
            &mut cells,
            &mut foregrounds,
            &mut backgrounds,
            &mut background_gradients,
            &mut inverse,
            CellRect { x: 3, y: 4, width: 12, height: 6 },
            CellStyle::new(UI_WHITE_COLOR, UI_ERROR_BACKGROUND),
        );

        // Dialogs wear the focus rule, so they use the doubled ROM border set.
        assert_eq!(cells[4 * COLUMNS + 3], DBL_CAPTION_LEFT);
        assert_eq!(cells[4 * COLUMNS + 14], DBL_CAPTION_RIGHT);
        assert_eq!(cells[9 * COLUMNS + 3], DBL_BOTTOM_LEFT);
        assert_eq!(cells[9 * COLUMNS + 14], DBL_BOTTOM_RIGHT);
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
    fn graphics_assets_live_in_tabs_save_as_ascii_and_toggle_source_view() {
        let filesystem = shared_filesystem();
        let mut editor = TextEditor::new(filesystem.clone(), shared_ui_colors(), None);
        editor.new_graphics_document();
        assert!(editor.graphics_active());
        assert_eq!(editor.tabs.len(), 2);
        assert!(filesystem.borrow().read_text("game.pal").unwrap().contains(";@FANTICON-PAL 1"));
        editor.save_as("world.gfx").unwrap();

        let source = filesystem.borrow().read_text("world.gfx").unwrap();
        assert!(source.is_ascii());
        assert!(source.contains(";@FANTICON-GFX 3"));
        assert!(source.contains(";@PALETTE-FILE GAME.PAL"));
        assert!(source.contains("WORLD_CHR"));
        assert_eq!(fanticon::assembler::assemble(&source).unwrap().bytes.len(), 12_288);
        let palette = filesystem.borrow().read_text("game.pal").unwrap();
        assert!(palette.contains(";@FANTICON-PAL 1"));
        assert_eq!(fanticon::assembler::assemble(&palette).unwrap().bytes.len(), 256);

        editor.toggle_graphics_source_view();
        assert!(editor.graphics_source_active());
        assert_eq!(editor.lines[0], ";@FANTICON-GFX 3");
        editor.toggle_graphics_source_view();
        assert!(!editor.graphics_source_active());

        editor.new_document();
        editor.load("world.gfx").unwrap();
        assert!(editor.graphics_active());
        assert_eq!(editor.filename.as_deref(), Some("world.gfx"));
    }

    #[test]
    fn menus_composite_over_graphics_without_hiding_the_workspace() {
        let mut editor = TextEditor::new(shared_filesystem(), shared_ui_colors(), None);
        editor.new_graphics_document();
        let pane_left = EDITOR_START * GLYPH_WIDTH;
        let pane_top = 3 * GLYPH_HEIGHT;
        editor.handle_mouse_press(pane_left + 12 + 5 * 24, pane_top + 319, false);
        editor.handle_mouse_press(pane_left + 13 + 3 * 28, pane_top + 39 + 3 * 28, false);
        editor.handle_mouse_release();

        let mut unobscured = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut unobscured, false);
        let sample = (pane_left + 252 + 3, pane_top + 38 + 3);
        assert_ne!(unobscured.pixel(sample.0, sample.1), [0, 0, 0, 255]);

        editor.open_menu(MenuKind::Debug);
        let mut with_menu = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut with_menu, false);

        assert_eq!(with_menu.pixel(sample.0, sample.1), unobscured.pixel(sample.0, sample.1));
        assert!((24..120).any(|y| {
            (pane_left..368).any(|x| with_menu.pixel(x, y) != unobscured.pixel(x, y))
        }));
    }

    #[test]
    fn new_palette_immediately_creates_and_opens_game_pal() {
        let filesystem = shared_filesystem();
        let mut editor = TextEditor::new(filesystem.clone(), shared_ui_colors(), None);

        editor.new_palette_document();

        assert_eq!(editor.filename.as_deref(), Some("game.pal"));
        assert!(editor.graphics_tabs[&editor.document_id].is_palette_document());
        assert!(!editor.dirty);
        assert!(filesystem.borrow().read_text("GAME.PAL").unwrap().starts_with(";@FANTICON-PAL 1"));

        let tab_count = editor.tabs.len();
        editor.new_palette_document();
        assert_eq!(editor.tabs.len(), tab_count);
        assert_eq!(editor.filename.as_deref(), Some("game.pal"));
    }

    #[test]
    fn new_graphics_refuses_to_replace_an_invalid_existing_game_pal() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("game.pal", "NOT A PALETTE").unwrap();
        let mut editor = TextEditor::new(filesystem.clone(), shared_ui_colors(), None);

        editor.new_graphics_document();

        assert_eq!(editor.tabs.len(), 1);
        assert_eq!(filesystem.borrow().read_text("game.pal").unwrap(), "NOT A PALETTE");
        assert!(matches!(editor.overlay, Overlay::Message { .. }));
    }

    #[test]
    fn graphics_views_paint_asset_colors_exactly_without_a_palette() {
        let filesystem = shared_filesystem();
        let mut editor = TextEditor::new(filesystem, shared_ui_colors(), None);
        editor.new_graphics_document();
        assert!(editor.graphics_active() && !editor.graphics_source_active());

        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);

        // Asset bytes are RGB332 and reach the screen as exactly that color.
        // Nothing is reserved for chrome, so every one of the 256 values is
        // available to artwork.
        let painted: Vec<Rgba> = (0..EDITOR_DISPLAY_HEIGHT)
            .flat_map(|y| (0..EDITOR_DISPLAY_WIDTH).map(move |x| (x, y)))
            .map(|(x, y)| surface.pixel(x, y))
            .collect();
        for byte in [0x00u8, 0x25, 0x92, 0xfa, 0xff] {
            let exact = fanticon::video::rgb332_to_rgba(byte);
            assert_eq!(
                exact,
                fanticon::video::rgb332_to_rgba(byte),
                "RGB332 {byte:02X} must expand to one fixed color"
            );
        }
        assert!(
            painted.contains(&fanticon::video::rgb332_to_rgba(0xff)),
            "the graphics pane draws its white through the same expansion"
        );

        // Chrome shares the surface without competing for any of it.
        assert_eq!(editor_color(UI_WHITE_COLOR), [255, 255, 255, 255]);
        assert_eq!(editor_color(0x25), fanticon::video::rgb332_to_rgba(0x25));
    }

    #[test]
    fn palette_resource_is_shared_by_multiple_graphics_documents() {
        let filesystem = shared_filesystem();
        let mut editor = TextEditor::new(filesystem.clone(), shared_ui_colors(), None);
        editor.new_graphics_document();
        editor.save_as("world.gfx").unwrap();
        let world_source = filesystem.borrow().read_text("world.gfx").unwrap();
        filesystem.borrow_mut().write_text("title.gfx", &world_source).unwrap();

        editor.load("game.pal").unwrap();
        assert!(editor.graphics_tabs[&editor.document_id].is_palette_document());
        assert!(
            editor
                .graphics_tabs
                .get_mut(&editor.document_id)
                .unwrap()
                .handle_key(&Key::Character("r".into()), ModifiersState::empty())
        );
        editor.propagate_active_palette();
        let changed = editor.graphics_tabs[&editor.document_id].palette()[1];
        assert_eq!(
            editor
                .graphics_tabs
                .values()
                .find(|graphics| { graphics.palette_reference() == Some("GAME.PAL") })
                .unwrap()
                .palette()[1],
            changed
        );
        editor.dirty = true;
        assert!(editor.save_tab(editor.active_tab).unwrap());

        editor.load("title.gfx").unwrap();
        assert_eq!(editor.graphics_tabs[&editor.document_id].palette()[1], changed);
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
    fn asm_highlighting_recognizes_undocumented_aliases() {
        for mnemonic in [
            "KIL", "JAM", "SLO", "RLA", "SRE", "RRA", "SAX", "LAX", "DCP", "ISC", "ISB", "ANC",
            "ALR", "ARR", "XAA", "AXS", "SBX", "AHX", "SHY", "SHX", "TAS", "LAS",
        ] {
            let line = format!("         {mnemonic}   #$01");
            let colors = assembly_syntax_colors(&line, ASM_TEXT_COLOR);
            assert!(colors[9..9 + mnemonic.len()].iter().all(|color| *color == ASM_OPCODE_COLOR));
        }
    }

    #[test]
    fn semicolons_inside_assembly_strings_are_not_comments() {
        assert_eq!(
            format_assembly_line(" msg asc \"hello;world\" ; real comment"),
            "         msg   asc \"hello;world\" ; real comment"
        );
    }

    #[test]
    fn format_assembly_line_does_not_reflow_macro_invocations() {
        // PMC's semicolon-separated argument list must survive the editor's
        // live formatting untouched; previously the formatter's own
        // comment-splitting had no PMC exception and moved everything after
        // the first `;` into a right-aligned "comment" field, corrupting
        // the macro call before it was ever saved or compiled.
        assert_eq!(
            format_assembly_line("         PMC   PRINTAT;message;2;5"),
            "         PMC   PRINTAT;message;2;5"
        );
        // The gap between the keyword and its arguments normalizes to
        // column 15 - the same operand column an ordinary instruction
        // gets - even though the arguments themselves are left untouched.
        assert_eq!(
            format_assembly_line("         >>> PRINTAT;message;2;5"),
            "         >>>   PRINTAT;message;2;5"
        );
        // A macro invocation typed with no leading whitespace still isn't a
        // label - PMC/>>> is always a recognized operation - so it should
        // still auto-indent to column 9 like any other unlabeled
        // instruction, even though its argument list stays untouched.
        assert_eq!(
            format_assembly_line("PMC PRINTAT;message;2;5"),
            "         PMC   PRINTAT;message;2;5"
        );
        assert_eq!(
            format_assembly_line("STORE MAC VALUE;DEST=$20"),
            "STORE    MAC   VALUE;DEST=$20"
        );
        assert_eq!(format_assembly_line("REPEAT 8;INDEX"), "         REPEAT 8;INDEX");
    }

    #[test]
    fn modern_macro_directives_are_highlighted_as_directives() {
        for directive in ["IF", "ELSE", "ENDIF", "REPEAT", "ENDREP", "--^"] {
            let line = format!("         {directive}   1");
            let colors = assembly_syntax_colors(&line, ASM_TEXT_COLOR);
            assert!(
                colors[9..9 + directive.len()].iter().all(|color| *color == ASM_DIRECTIVE_COLOR)
            );
        }
    }

    #[test]
    fn asm_highlighting_does_not_color_macro_arguments_as_a_comment() {
        let colors = assembly_syntax_colors("         PMC   PRINTAT;message;2;5", ASM_TEXT_COLOR);
        assert!(!colors.contains(&ASM_COMMENT_COLOR));
    }

    #[test]
    fn asm_highlighting_gives_macros_their_own_color() {
        // Definition: both the declared name and the MAC/EOM keywords.
        let definition = assembly_syntax_colors("LOADIMM  MAC", ASM_TEXT_COLOR);
        assert!(definition[..7].iter().all(|color| *color == ASM_MACRO_COLOR));
        assert!(definition[9..12].iter().all(|color| *color == ASM_MACRO_COLOR));

        let eom = assembly_syntax_colors("         EOM", ASM_TEXT_COLOR);
        assert!(eom[9..12].iter().all(|color| *color == ASM_MACRO_COLOR));

        // Invocation: the PMC keyword and the macro name, but not its args.
        let invocation =
            assembly_syntax_colors("         PMC   PRINTAT;message;2;5", ASM_TEXT_COLOR);
        assert!(invocation[9..12].iter().all(|color| *color == ASM_MACRO_COLOR));
        assert!(invocation[15..22].iter().all(|color| *color == ASM_MACRO_COLOR));
        assert_eq!(invocation[22], ASM_TEXT_COLOR);

        // Parameter placeholders inside a macro body (the `#` immediate
        // marker colors separately as a number literal; `]1` is the part
        // that should read as a macro placeholder).
        let body = assembly_syntax_colors("         LDA   #]1", ASM_TEXT_COLOR);
        assert!(body[16..18].iter().all(|color| *color == ASM_MACRO_COLOR));
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
        // A fresh line defaults to the opcode column, not the label column -
        // most lines are plain instructions, and typing a label is the
        // exception that then has to un-indent back to column 1.
        assert_eq!(editor.lines[1], " ".repeat(9));
        assert_eq!(editor.cursor, Position { line: 1, column: 9 });
    }

    #[test]
    fn semicolon_on_an_auto_indented_blank_line_drops_to_the_left_column() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.insert_text("LDA #$20");
        editor.insert_newline();
        assert_eq!(editor.cursor, Position { line: 1, column: 9 });
        editor.insert_text(";");
        assert_eq!(editor.lines[1], ";");
        assert_eq!(editor.cursor, Position { line: 1, column: 1 });
    }

    #[test]
    fn ctrl_equals_and_minus_insert_a_section_heading_divider() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.handle_key(
            &Key::Character("=".into()),
            PhysicalKey::Code(KeyCode::Equal),
            ModifiersState::CONTROL,
        );
        let bar = ";".to_owned() + &"=".repeat(53);
        assert_eq!(bar.len(), 54);
        assert_eq!(editor.lines, vec![bar.clone(), ";".to_owned(), bar]);
        assert_eq!(editor.cursor, Position { line: 1, column: 1 });

        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("code.asm", "").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("code.asm".to_owned()));
        editor.handle_key(
            &Key::Character("-".into()),
            PhysicalKey::Code(KeyCode::Minus),
            ModifiersState::CONTROL,
        );
        let bar = ";".to_owned() + &"-".repeat(53);
        assert_eq!(editor.lines, vec![bar.clone(), ";".to_owned(), bar]);
        assert_eq!(editor.cursor, Position { line: 1, column: 1 });
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
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        assert_eq!(editor_color(ASM_TEXT_COLOR), [205, 214, 244, 255]);
        assert_eq!(editor_color(ASM_COMMENT_COLOR), [127, 132, 156, 255]);
        assert_eq!(editor_color(ASM_ERROR_COLOR), [243, 139, 168, 255]);
        // Glyph rows below the first are shaded, so match the unshaded top row.
        let painted = |color: u8| {
            let wanted = editor_color(color);
            (0..EDITOR_DISPLAY_HEIGHT)
                .any(|y| (0..EDITOR_DISPLAY_WIDTH).any(|x| surface.pixel(x, y) == wanted))
        };
        assert!(painted(ASM_LABEL_COLOR));
        assert!(painted(ASM_OPCODE_COLOR));
        assert!(painted(ASM_NUMBER_COLOR));
        assert!(painted(ASM_COMMENT_COLOR));
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
        assert!(editor.build_message.as_deref().is_some_and(|message| message.contains("Built")));
        assert!(matches!(
            editor.overlay,
            Overlay::Message { ref title, .. } if title == "Build Successful"
        ));
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let painted = |color: u8| {
            let wanted = editor_color(color);
            (0..EDITOR_DISPLAY_HEIGHT)
                .any(|y| (0..EDITOR_DISPLAY_WIDTH).any(|x| surface.pixel(x, y) == wanted))
        };
        assert!(painted(UI_WHITE_COLOR));
        assert!(painted(UI_SUCCESS_BACKGROUND));
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
            Overlay::Message { ref title, .. } if title == "Build Successful"
        ));
    }

    #[test]
    fn rom_usage_menu_item_reports_free_space_per_bank() {
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
                " BANK 0\n ORG $8000\nLEVEL DFB $42\n FIXED\n ORG $C100\n\
                 RESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ",
            )
            .unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("main.asm".to_owned()));

        editor.start_bank_usage();
        assert!(matches!(editor.overlay, Overlay::Building { .. }));
        finish_pending_build(&mut editor);

        let Overlay::BankUsage { entries, scroll } = &editor.overlay else {
            panic!("expected a ROM bank usage dialog");
        };
        assert_eq!(*scroll, 0);
        assert_eq!(entries[0].section, SymbolSection::Fixed);
        assert!(entries[0].used > 0);
        assert!(entries[0].free() < entries[0].capacity);
        let bank_zero =
            entries.iter().find(|entry| entry.section == SymbolSection::Bank(0)).unwrap();
        assert_eq!(bank_zero.used, 1);
        assert_eq!(bank_zero.free(), bank_zero.capacity - 1);

        assert_eq!(
            editor.handle_overlay_key(&Key::Named(NamedKey::Escape), ModifiersState::empty()),
            EditorAction::None
        );
        assert!(matches!(editor.overlay, Overlay::None));
    }

    #[test]
    fn rom_usage_without_a_project_shows_an_error() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("solo.asm", " ORG $8000\n RTS").unwrap();
        let mut editor =
            TextEditor::new(filesystem, shared_ui_colors(), Some("solo.asm".to_owned()));

        editor.start_bank_usage();
        finish_pending_build(&mut editor);

        assert!(matches!(
            editor.overlay,
            Overlay::Message { ref title, ref lines }
                if title == "Build Error" && lines[0].contains("ROM Bank Usage")
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
                && diagnostic.message == "Text file is not UTF-8"
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
            Overlay::Message { ref title, .. } if title == "Build Errors"
        ));
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, EDITOR_DISPLAY_HEIGHT);
        editor.render(&mut surface, false);
        let painted = |color: u8| {
            let wanted = editor_color(color);
            (0..EDITOR_DISPLAY_HEIGHT)
                .any(|y| (0..EDITOR_DISPLAY_WIDTH).any(|x| surface.pixel(x, y) == wanted))
        };
        assert!(painted(UI_WHITE_COLOR));
        assert!(painted(UI_ERROR_BACKGROUND));
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
