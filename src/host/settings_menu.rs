//! Keyboard/controller-friendly presentation settings screen.

use winit::keyboard::{Key, NamedKey};

use super::{AudioBufferSize, AudioFilter, AudioHighPass, HostSettings, RenderingStyle, Surface};

const ROWS: usize = 13;
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
}

impl SettingsMenu {
    pub fn new(settings: HostSettings, in_game: bool) -> Self {
        Self { settings, cursor: 0, in_game }
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
            5 => cycle(&mut self.settings.audio.buffer_size, &AudioBufferSize::ALL, direction),
            6 => cycle(&mut self.settings.audio.filter, &AudioFilter::ALL, direction),
            7 => cycle(&mut self.settings.audio.high_pass, &AudioHighPass::ALL, direction),
            8 => adjust(&mut self.settings.audio.stereo_width, direction, 0.1),
            9 => adjust(&mut self.settings.audio.reverb, direction, 0.1),
            10 => {
                self.settings.audio.mute_when_unfocused = !self.settings.audio.mute_when_unfocused;
            }
            _ => return SettingsMenuAction::None,
        }
        self.settings = self.settings.clone().normalized();
        SettingsMenuAction::Changed(self.settings.clone())
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
        surface.draw_text(
            16,
            376,
            "ARROWS CHANGE   ENTER SELECT   F10/ESC CLOSE",
            WHITE,
            Some(GRAY),
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
            ("Audio buffer", self.settings.audio.buffer_size.label().to_owned()),
            ("Audio filtering", self.settings.audio.filter.label().to_owned()),
            ("High-pass filter", self.settings.audio.high_pass.label().to_owned()),
            ("Stereo width", percent(self.settings.audio.stereo_width)),
            ("Reverb", percent(self.settings.audio.reverb)),
            (
                "Mute when unfocused",
                if self.settings.audio.mute_when_unfocused { "On" } else { "Off" }.to_owned(),
            ),
            ("Restore defaults", "Enter".to_owned()),
            (if self.in_game { "Resume game" } else { "Close settings" }, "Enter".to_owned()),
        ];
        let x = 88;
        let y = 40;
        let width = 464;
        surface.fill_rect(x, y, width, 320, [18, 20, 30, 255]);
        surface.stroke_rect(x, y, width, 320, CYAN);
        for (index, (label, value)) in rows.iter().enumerate() {
            let row_y = y + 14 + index * 22;
            let selected = index == self.cursor;
            let background = if selected { BLUE } else { [18, 20, 30, 255] };
            surface.fill_rect(x + 8, row_y - 4, width - 16, 16, background);
            surface.draw_text(x + 16, row_y, label, WHITE, Some(background));
            let value_x = x + width - 24 - value.len() * 8;
            surface.draw_text(
                value_x,
                row_y,
                value,
                if selected { WHITE } else { CYAN },
                Some(background),
            );
        }
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
}
