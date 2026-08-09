//! Shared presentation settings for the editor and running cartridges.

use serde::{Deserialize, Serialize};

const SETTINGS_VERSION: u32 = 1;
const MAX_NSF_SCAN_CACHE_ENTRIES: usize = 4_096;
#[cfg(target_arch = "wasm32")]
const WEB_STORAGE_KEY: &str = "fanticon.host-settings.v1";

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum RenderingStyle {
    CleanPixel,
    Vga,
    ArcadeCrt,
    #[default]
    ConsumerCrt,
    Lcd,
    Monochrome,
}

impl RenderingStyle {
    pub const ALL: [Self; 6] = [
        Self::CleanPixel,
        Self::Vga,
        Self::ArcadeCrt,
        Self::ConsumerCrt,
        Self::Lcd,
        Self::Monochrome,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::CleanPixel => "Clean Pixel",
            Self::Vga => "VGA",
            Self::ArcadeCrt => "Arcade CRT",
            Self::ConsumerCrt => "Consumer CRT",
            Self::Lcd => "LCD",
            Self::Monochrome => "Amber Mono",
        }
    }

    pub const fn shader_id(self) -> u32 {
        match self {
            Self::CleanPixel => 0,
            Self::Vga => 1,
            Self::ArcadeCrt => 2,
            Self::ConsumerCrt => 3,
            Self::Lcd => 4,
            Self::Monochrome => 5,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioBufferSize {
    #[default]
    Auto,
    Frames128,
    Frames256,
    Frames512,
    Frames1024,
    Frames2048,
}

impl AudioBufferSize {
    #[cfg(not(target_arch = "wasm32"))]
    pub const ALL: [Self; 6] = [
        Self::Auto,
        Self::Frames128,
        Self::Frames256,
        Self::Frames512,
        Self::Frames1024,
        Self::Frames2048,
    ];

    #[cfg(not(target_arch = "wasm32"))]
    pub const fn label(self) -> &'static str {
        match self {
            Self::Auto => "Auto",
            Self::Frames128 => "128 frames",
            Self::Frames256 => "256 frames",
            Self::Frames512 => "512 frames",
            Self::Frames1024 => "1024 frames",
            Self::Frames2048 => "2048 frames",
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub const fn frames(self) -> Option<u32> {
        match self {
            Self::Auto => None,
            Self::Frames128 => Some(128),
            Self::Frames256 => Some(256),
            Self::Frames512 => Some(512),
            Self::Frames1024 => Some(1024),
            Self::Frames2048 => Some(2048),
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioFilter {
    Crisp,
    #[default]
    Balanced,
    Warm,
    Vintage,
    Minimal,
}

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AudioHighPass {
    Off,
    Hz20,
    #[default]
    Hz60,
    Hz120,
}

impl AudioHighPass {
    pub const ALL: [Self; 4] = [Self::Off, Self::Hz20, Self::Hz60, Self::Hz120];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Off => "Off",
            Self::Hz20 => "20 Hz",
            Self::Hz60 => "60 Hz",
            Self::Hz120 => "120 Hz",
        }
    }

    pub const fn cutoff_hz(self) -> Option<f32> {
        match self {
            Self::Off => None,
            Self::Hz20 => Some(20.0),
            Self::Hz60 => Some(60.0),
            Self::Hz120 => Some(120.0),
        }
    }
}

impl AudioFilter {
    pub const ALL: [Self; 5] =
        [Self::Crisp, Self::Balanced, Self::Warm, Self::Vintage, Self::Minimal];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Crisp => "Crisp",
            Self::Balanced => "Balanced",
            Self::Warm => "Warm",
            Self::Vintage => "Vintage",
            Self::Minimal => "Minimal",
        }
    }

    pub const fn cutoff_hz(self) -> f32 {
        match self {
            Self::Crisp => 18_000.0,
            Self::Balanced => 14_000.0,
            Self::Warm => 10_500.0,
            Self::Vintage => 7_500.0,
            Self::Minimal => 20_000.0,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct GraphicsSettings {
    pub style: RenderingStyle,
    pub effect_strength: f32,
    pub brightness: f32,
    pub integer_scaling: bool,
}

impl Default for GraphicsSettings {
    fn default() -> Self {
        Self {
            style: RenderingStyle::ConsumerCrt,
            effect_strength: 0.82,
            brightness: 1.0,
            integer_scaling: false,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct AudioSettings {
    pub master_volume: f32,
    pub buffer_size: AudioBufferSize,
    pub filter: AudioFilter,
    pub high_pass: AudioHighPass,
    pub stereo_width: f32,
    pub reverb: f32,
    pub mute_when_unfocused: bool,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct MusicPlayerSettings {
    pub folder: String,
    pub excluded: Vec<String>,
    pub shuffle: bool,
    pub repeat: bool,
    pub current: Option<String>,
    pub nsf_scan_cache: Vec<NsfScanCacheEntry>,
}

#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct NsfScanCacheEntry {
    pub id: String,
    pub content_hash: u64,
    pub probe_version: u16,
    pub minimum_seconds: u16,
    pub short: bool,
}

impl Default for MusicPlayerSettings {
    fn default() -> Self {
        Self {
            folder: "/MUSIC".to_owned(),
            excluded: Vec::new(),
            shuffle: false,
            repeat: true,
            current: None,
            nsf_scan_cache: Vec::new(),
        }
    }
}

impl Default for AudioSettings {
    fn default() -> Self {
        Self {
            master_volume: 0.5,
            buffer_size: AudioBufferSize::Auto,
            filter: AudioFilter::Balanced,
            high_pass: AudioHighPass::Hz60,
            stereo_width: 0.5,
            reverb: 0.5,
            mute_when_unfocused: true,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct HostSettings {
    pub version: u32,
    pub graphics: GraphicsSettings,
    pub audio: AudioSettings,
    pub music_player: MusicPlayerSettings,
    pub diagnostics_overlay: bool,
}

impl Default for HostSettings {
    fn default() -> Self {
        Self {
            version: SETTINGS_VERSION,
            graphics: GraphicsSettings::default(),
            audio: AudioSettings::default(),
            music_player: MusicPlayerSettings::default(),
            diagnostics_overlay: false,
        }
    }
}

impl HostSettings {
    pub fn load() -> Self {
        load_source()
            .and_then(|source| serde_json::from_str::<Self>(&source).ok())
            .filter(|settings| settings.version == SETTINGS_VERSION)
            .map(Self::normalized)
            .unwrap_or_default()
    }

    pub fn save(&self) -> Result<(), String> {
        let source = serde_json::to_string_pretty(&self.clone().normalized())
            .map_err(|error| error.to_string())?;
        save_source(&source)
    }

    pub fn normalized(mut self) -> Self {
        self.version = SETTINGS_VERSION;
        self.graphics.effect_strength = self.graphics.effect_strength.clamp(0.0, 1.0);
        self.graphics.brightness = self.graphics.brightness.clamp(0.5, 1.5);
        self.audio.master_volume = self.audio.master_volume.clamp(0.0, 1.0);
        self.audio.stereo_width = self.audio.stereo_width.clamp(0.0, 1.0);
        self.audio.reverb = self.audio.reverb.clamp(0.0, 1.0);
        self.music_player.folder = normalize_music_folder(&self.music_player.folder);
        self.music_player.excluded.sort_by_key(|entry| entry.to_ascii_uppercase());
        self.music_player.excluded.dedup_by(|left, right| left.eq_ignore_ascii_case(right));
        for entry in &mut self.music_player.nsf_scan_cache {
            entry.id = entry.id.to_ascii_uppercase();
        }
        self.music_player.nsf_scan_cache.sort_by_key(|entry| entry.id.clone());
        self.music_player
            .nsf_scan_cache
            .dedup_by(|left, right| left.id.eq_ignore_ascii_case(&right.id));
        if self.music_player.nsf_scan_cache.len() > MAX_NSF_SCAN_CACHE_ENTRIES {
            let excess = self.music_player.nsf_scan_cache.len() - MAX_NSF_SCAN_CACHE_ENTRIES;
            self.music_player.nsf_scan_cache.drain(..excess);
        }
        self
    }
}

fn normalize_music_folder(folder: &str) -> String {
    let mut folder = folder.trim().replace('\\', "/");
    if folder.is_empty() {
        return "/".to_owned();
    }
    if !folder.starts_with('/') {
        folder.insert(0, '/');
    }
    while folder.len() > 1 && folder.ends_with('/') {
        folder.pop();
    }
    folder
}

#[cfg(not(target_arch = "wasm32"))]
fn settings_path() -> Option<std::path::PathBuf> {
    dirs::config_dir().map(|directory| directory.join("Fanticon").join("settings.json"))
}

#[cfg(not(target_arch = "wasm32"))]
fn load_source() -> Option<String> {
    std::fs::read_to_string(settings_path()?).ok()
}

#[cfg(not(target_arch = "wasm32"))]
fn save_source(source: &str) -> Result<(), String> {
    let path =
        settings_path().ok_or_else(|| "platform settings directory is unavailable".to_owned())?;
    let parent = path.parent().ok_or_else(|| "settings path has no parent".to_owned())?;
    std::fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    let temporary = path.with_extension("json.tmp");
    std::fs::write(&temporary, source).map_err(|error| error.to_string())?;
    std::fs::rename(&temporary, &path)
        .or_else(|_| {
            std::fs::remove_file(&path).ok();
            std::fs::rename(&temporary, &path)
        })
        .map_err(|error| error.to_string())
}

#[cfg(target_arch = "wasm32")]
fn load_source() -> Option<String> {
    web_sys::window()?.local_storage().ok()??.get_item(WEB_STORAGE_KEY).ok()?
}

#[cfg(target_arch = "wasm32")]
fn save_source(source: &str) -> Result<(), String> {
    web_sys::window()
        .ok_or_else(|| "browser window is unavailable".to_owned())?
        .local_storage()
        .map_err(|_| "browser local storage is unavailable".to_owned())?
        .ok_or_else(|| "browser local storage is disabled".to_owned())?
        .set_item(WEB_STORAGE_KEY, source)
        .map_err(|_| "could not save browser settings".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn settings_round_trip_and_normalize_ranges() {
        let mut settings = HostSettings::default();
        settings.graphics.style = RenderingStyle::Lcd;
        settings.graphics.effect_strength = 8.0;
        settings.audio.master_volume = -2.0;
        let source = serde_json::to_string(&settings).unwrap();
        let decoded: HostSettings = serde_json::from_str(&source).unwrap();
        let decoded = decoded.normalized();
        assert_eq!(decoded.graphics.style, RenderingStyle::Lcd);
        assert_eq!(decoded.graphics.effect_strength, 1.0);
        assert_eq!(decoded.audio.master_volume, 0.0);
    }

    #[test]
    fn old_documents_gain_new_defaults() {
        let settings: HostSettings = serde_json::from_str("{\"version\":1}").unwrap();
        assert_eq!(settings.graphics, GraphicsSettings::default());
        assert_eq!(settings.audio, AudioSettings::default());
        assert_eq!(settings.music_player, MusicPlayerSettings::default());
    }

    #[test]
    fn music_player_paths_and_exclusions_are_normalized() {
        let mut settings = HostSettings::default();
        settings.music_player.folder = "music\\nes/".to_owned();
        settings.music_player.excluded = vec!["B.NSF#1".to_owned(), "b.nsf#1".to_owned()];
        settings.music_player.nsf_scan_cache = vec![
            NsfScanCacheEntry {
                id: "music/song.nsf#1".to_owned(),
                content_hash: 1,
                probe_version: 1,
                minimum_seconds: 10,
                short: false,
            },
            NsfScanCacheEntry {
                id: "MUSIC/SONG.NSF#1".to_owned(),
                content_hash: 2,
                probe_version: 1,
                minimum_seconds: 10,
                short: true,
            },
        ];
        let settings = settings.normalized();
        assert_eq!(settings.music_player.folder, "/music/nes");
        assert_eq!(settings.music_player.excluded.len(), 1);
        assert_eq!(settings.music_player.nsf_scan_cache.len(), 1);
        assert_eq!(settings.music_player.nsf_scan_cache[0].id, "MUSIC/SONG.NSF#1");
    }
}
