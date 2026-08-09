//! Keyboard/controller-friendly presentation settings screen.

use winit::keyboard::{Key, NamedKey};

use super::{AudioBufferSize, AudioFilter, AudioHighPass, HostSettings, RenderingStyle, Surface};

const ROWS: usize = 14;
const PANEL_X: usize = 88;
const PANEL_Y: usize = 28;
const PANEL_WIDTH: usize = 464;
const PANEL_HEIGHT: usize = 330;
const ROW_START: usize = 38;
const ROW_STEP: usize = 23;
const PREVIEW_X: usize = 146;
const PREVIEW_Y: usize = 362;
const PREVIEW_WIDTH: usize = 54;
const PREVIEW_GAP: usize = 4;
const WHITE: [u8; 4] = [238, 238, 238, 255];
const BLACK: [u8; 4] = [8, 8, 12, 255];
const BLUE: [u8; 4] = [40, 85, 180, 255];
const CYAN: [u8; 4] = [70, 220, 235, 255];
const GRAY: [u8; 4] = [85, 90, 105, 255];

#[derive(Clone, Debug, PartialEq)]
pub enum SettingsMenuAction {
    None,
    Changed(HostSettings),
    Close,
}

pub struct SettingsMenu {
    settings: HostSettings,
    cursor: usize,
    in_game: bool,
    save_status: Option<bool>,
}

impl SettingsMenu {
    pub fn new(settings: HostSettings, in_game: bool) -> Self {
        Self { settings, cursor: 0, in_game, save_status: None }
    }

    pub fn handle_key(&mut self, key: &Key) -> SettingsMenuAction {
        match key {
            Key::Named(NamedKey::Escape | NamedKey::F10) => return SettingsMenuAction::Close,
            Key::Named(NamedKey::ArrowUp) => self.cursor = (self.cursor + ROWS - 1) % ROWS,
            Key::Named(NamedKey::ArrowDown) => self.cursor = (self.cursor + 1) % ROWS,
            Key::Named(NamedKey::ArrowLeft) => return self.change(-1),
            Key::Named(NamedKey::ArrowRight) => return self.change(1),
            Key::Named(NamedKey::Enter) if self.cursor == ROWS - 2 => {
                self.settings = HostSettings::default();
                return SettingsMenuAction::Changed(self.settings.clone());
            }
            Key::Named(NamedKey::Enter) if self.cursor == ROWS - 1 => {
                return SettingsMenuAction::Close;
            }
            _ => {}
        }
        SettingsMenuAction::None
    }

    pub fn handle_controller(&mut self, pressed: u8) -> SettingsMenuAction {
        use fanticon::system::ControllerState;
        if pressed & ControllerState::UP != 0 {
            return self.handle_key(&Key::Named(NamedKey::ArrowUp));
        }
        if pressed & ControllerState::DOWN != 0 {
            return self.handle_key(&Key::Named(NamedKey::ArrowDown));
        }
        if pressed & ControllerState::LEFT != 0 {
            return self.handle_key(&Key::Named(NamedKey::ArrowLeft));
        }
        if pressed & ControllerState::RIGHT != 0 {
            return self.handle_key(&Key::Named(NamedKey::ArrowRight));
        }
        if pressed & (ControllerState::A | ControllerState::START) != 0 {
            return self.handle_key(&Key::Named(NamedKey::Enter));
        }
        if pressed & ControllerState::B != 0 {
            return SettingsMenuAction::Close;
        }
        SettingsMenuAction::None
    }

    fn change(&mut self, direction: i32) -> SettingsMenuAction {
        match self.cursor {
            0 => cycle(&mut self.settings.graphics.style, &RenderingStyle::ALL, direction),
            1 => adjust(&mut self.settings.graphics.effect_strength, direction, 0.1),
            2 => adjust(&mut self.settings.graphics.brightness, direction, 0.05),
            3 => self.settings.graphics.integer_scaling = !self.settings.graphics.integer_scaling,
            4 => adjust(&mut self.settings.audio.master_volume, direction, 0.05),
            5 => {
                #[cfg(not(target_arch = "wasm32"))]
                cycle(&mut self.settings.audio.buffer_size, &AudioBufferSize::ALL, direction);
                #[cfg(target_arch = "wasm32")]
                return SettingsMenuAction::None;
            }
            6 => cycle(&mut self.settings.audio.filter, &AudioFilter::ALL, direction),
            7 => cycle(&mut self.settings.audio.high_pass, &AudioHighPass::ALL, direction),
            8 => adjust(&mut self.settings.audio.stereo_width, direction, 0.1),
            9 => adjust(&mut self.settings.audio.reverb, direction, 0.1),
            10 => {
                self.settings.audio.mute_when_unfocused = !self.settings.audio.mute_when_unfocused;
            }
            11 => self.settings.diagnostics_overlay = !self.settings.diagnostics_overlay,
            _ => return SettingsMenuAction::None,
        }
        self.settings = self.settings.clone().normalized();
        SettingsMenuAction::Changed(self.settings.clone())
    }

    pub fn handle_mouse_move(&mut self, x: usize, y: usize) {
        if let Some(row) = row_at(x, y) {
            self.cursor = row;
        } else if preview_at(x, y).is_some() {
            self.cursor = 0;
        }
    }

    pub fn handle_mouse_press(&mut self, x: usize, y: usize) -> SettingsMenuAction {
        if let Some(style) = preview_at(x, y) {
            self.cursor = 0;
            self.settings.graphics.style = RenderingStyle::ALL[style];
            return SettingsMenuAction::Changed(self.settings.clone());
        }
        let Some(row) = row_at(x, y) else { return SettingsMenuAction::None };
        self.cursor = row;
        if row >= ROWS - 2 { self.handle_key(&Key::Named(NamedKey::Enter)) } else { self.change(1) }
    }

    pub fn set_save_status(&mut self, saved: bool) {
        self.save_status = Some(saved);
    }

    pub fn render(&self, surface: &mut Surface) {
        surface.resize(640, 400);
        surface.clear(BLACK);
        surface.fill_rect(0, 0, 640, 24, BLUE);
        surface.draw_text(
            16,
            8,
            if self.in_game { "FANTICON SYSTEM MENU" } else { "FANTICON SETTINGS" },
            WHITE,
            Some(BLUE),
        );
        let rows = [
            ("Rendering style", self.settings.graphics.style.label().to_owned()),
            ("Effect strength", percent(self.settings.graphics.effect_strength)),
            ("Brightness", percent(self.settings.graphics.brightness)),
            (
                "Integer scaling",
                if self.settings.graphics.integer_scaling { "On" } else { "Off" }.to_owned(),
            ),
            ("Master volume", percent(self.settings.audio.master_volume)),
            ("Audio buffer", audio_buffer_label(self.settings.audio.buffer_size)),
            ("Audio filtering", self.settings.audio.filter.label().to_owned()),
            ("High-pass filter", self.settings.audio.high_pass.label().to_owned()),
            ("Stereo width", percent(self.settings.audio.stereo_width)),
            ("Reverb", percent(self.settings.audio.reverb)),
            (
                "Mute when unfocused",
                if self.settings.audio.mute_when_unfocused { "On" } else { "Off" }.to_owned(),
            ),
            (
                "Diagnostics overlay",
                if self.settings.diagnostics_overlay { "On" } else { "Off" }.to_owned(),
            ),
            ("Restore defaults", "Enter".to_owned()),
            (if self.in_game { "Resume game" } else { "Close settings" }, "Enter".to_owned()),
        ];
        surface.fill_rect(PANEL_X, PANEL_Y, PANEL_WIDTH, PANEL_HEIGHT, [18, 20, 30, 255]);
        surface.stroke_rect(PANEL_X, PANEL_Y, PANEL_WIDTH, PANEL_HEIGHT, CYAN);
        for (index, (label, value)) in rows.iter().enumerate() {
            let row_y = ROW_START + index * ROW_STEP;
            let selected = index == self.cursor;
            let background = if selected { BLUE } else { [18, 20, 30, 255] };
            surface.fill_rect(PANEL_X + 8, row_y - 4, PANEL_WIDTH - 16, 16, background);
            surface.draw_text(PANEL_X + 16, row_y, label, WHITE, Some(background));
            let value_x = PANEL_X + PANEL_WIDTH - 24 - value.len() * 8;
            surface.draw_text(
                value_x,
                row_y,
                value,
                if selected { WHITE } else { CYAN },
                Some(background),
            );
        }
        render_style_previews(surface, self.settings.graphics.style);
        let footer = match self.save_status {
            Some(true) => "SAVED   CLICK/ARROWS CHANGE   ENTER SELECT   F10/ESC CLOSE",
            Some(false) => "SAVE FAILED   CLICK/ARROWS CHANGE   F10/ESC CLOSE",
            None => "CLICK/ARROWS CHANGE   ENTER SELECT   F10/ESC CLOSE",
        };
        surface.draw_text(16, 388, footer, WHITE, Some(GRAY));
    }
}

fn row_at(x: usize, y: usize) -> Option<usize> {
    if !(PANEL_X + 8..PANEL_X + PANEL_WIDTH - 8).contains(&x) || y + 4 < ROW_START {
        return None;
    }
    let row = (y + 4 - ROW_START) / ROW_STEP;
    (row < ROWS && y < ROW_START + row * ROW_STEP + 12).then_some(row)
}

fn preview_at(x: usize, y: usize) -> Option<usize> {
    if !(PREVIEW_Y..PREVIEW_Y + 14).contains(&y) || x < PREVIEW_X {
        return None;
    }
    let slot = (x - PREVIEW_X) / (PREVIEW_WIDTH + PREVIEW_GAP);
    let within = (x - PREVIEW_X) % (PREVIEW_WIDTH + PREVIEW_GAP);
    (slot < RenderingStyle::ALL.len() && within < PREVIEW_WIDTH).then_some(slot)
}

fn render_style_previews(surface: &mut Surface, selected: RenderingStyle) {
    let colors = [
        [90, 160, 235, 255],
        [80, 115, 210, 255],
        [235, 70, 150, 255],
        [180, 95, 70, 255],
        [95, 190, 175, 255],
        [205, 125, 35, 255],
    ];
    for (index, style) in RenderingStyle::ALL.iter().enumerate() {
        let x = PREVIEW_X + index * (PREVIEW_WIDTH + PREVIEW_GAP);
        surface.fill_rect(x, PREVIEW_Y, PREVIEW_WIDTH, 14, [12, 14, 20, 255]);
        surface.fill_rect(x + 2, PREVIEW_Y + 2, PREVIEW_WIDTH - 4, 10, colors[index]);
        if index != 0 {
            for line in (PREVIEW_Y + 3..PREVIEW_Y + 12).step_by(if index == 4 { 3 } else { 2 }) {
                surface.fill_rect(x + 2, line, PREVIEW_WIDTH - 4, 1, [20, 24, 32, 255]);
            }
        }
        surface.stroke_rect(
            x,
            PREVIEW_Y,
            PREVIEW_WIDTH,
            14,
            if *style == selected { CYAN } else { GRAY },
        );
    }
}

fn audio_buffer_label(buffer: AudioBufferSize) -> String {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = buffer;
        "Browser managed".to_owned()
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        buffer.label().to_owned()
    }
}

fn percent(value: f32) -> String {
    format!("{}%", (value * 100.0).round() as i32)
}

fn adjust(value: &mut f32, direction: i32, step: f32) {
    *value += direction.signum() as f32 * step;
}

fn cycle<T: Copy + PartialEq>(value: &mut T, values: &[T], direction: i32) {
    let current = values.iter().position(|candidate| candidate == value).unwrap_or(0);
    let next = if direction < 0 {
        (current + values.len() - 1) % values.len()
    } else {
        (current + 1) % values.len()
    };
    *value = values[next];
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn menu_changes_values_and_reset_restores_defaults() {
        let mut menu = SettingsMenu::new(HostSettings::default(), true);
        assert!(matches!(
            menu.handle_key(&Key::Named(NamedKey::ArrowRight)),
            SettingsMenuAction::Changed(_)
        ));
        assert_ne!(menu.settings.graphics.style, RenderingStyle::ConsumerCrt);
        menu.cursor = ROWS - 2;
        menu.handle_key(&Key::Named(NamedKey::Enter));
        assert_eq!(menu.settings, HostSettings::default());
    }

    #[test]
    fn menu_renders_full_settings_surface() {
        let menu = SettingsMenu::new(HostSettings::default(), false);
        let mut surface = Surface::new(1, 1);
        menu.render(&mut surface);
        assert_eq!(surface.dimensions(), (640, 400));
        assert!(surface.pixels().iter().any(|byte| *byte != 0));
    }

    #[test]
    fn audio_rows_are_independent_and_cover_device_preferences() {
        let mut menu = SettingsMenu::new(HostSettings::default(), true);
        menu.cursor = 5;
        menu.change(1);
        assert_eq!(menu.settings.audio.buffer_size, AudioBufferSize::Frames128);
        menu.cursor = 6;
        menu.change(1);
        assert_eq!(menu.settings.audio.filter, AudioFilter::Warm);

        menu.cursor = 7;
        menu.change(-1);
        assert_eq!(menu.settings.audio.high_pass, AudioHighPass::Hz20);
        let initial_reverb = menu.settings.audio.reverb;
        menu.cursor = 8;
        menu.change(-1);
        assert_eq!(menu.settings.audio.reverb, initial_reverb);
        let initial_width = menu.settings.audio.stereo_width;
        menu.cursor = 9;
        menu.change(-1);
        assert_eq!(menu.settings.audio.stereo_width, initial_width);

        menu.cursor = 10;
        menu.change(1);
        assert!(!menu.settings.audio.mute_when_unfocused);
    }

    #[test]
    fn mouse_selects_rows_styles_and_diagnostics() {
        let mut menu = SettingsMenu::new(HostSettings::default(), false);
        let diagnostics_y = ROW_START + 11 * ROW_STEP;
        menu.handle_mouse_move(PANEL_X + 20, diagnostics_y);
        assert_eq!(menu.cursor, 11);
        assert!(matches!(
            menu.handle_mouse_press(PANEL_X + 20, diagnostics_y),
            SettingsMenuAction::Changed(_)
        ));
        assert!(menu.settings.diagnostics_overlay);

        let arcade_x = PREVIEW_X + 2 * (PREVIEW_WIDTH + PREVIEW_GAP) + 4;
        menu.handle_mouse_press(arcade_x, PREVIEW_Y + 4);
        assert_eq!(menu.settings.graphics.style, RenderingStyle::ArcadeCrt);
    }

    #[test]
    fn save_status_is_rendered_without_changing_menu_state() {
        let mut menu = SettingsMenu::new(HostSettings::default(), true);
        menu.set_save_status(true);
        let mut surface = Surface::new(640, 400);
        menu.render(&mut surface);
        assert!(surface.pixels().iter().any(|byte| *byte != 0));
        assert_eq!(menu.cursor, 0);
    }
}
