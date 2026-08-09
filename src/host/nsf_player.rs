use std::collections::HashMap;

use fanticon::{
    Bus, Cpu, Status,
    audio::{NOISE_PERIODS, PULSE_DUTY_TABLE, TRIANGLE_SEQUENCE, mix_sample, step_noise_lfsr},
    machine::{CPU_CLOCK_HZ, CPU_CYCLES_PER_FRAME},
};

use super::music_editor::MusicEditor;

const NSF_HEADER_SIZE: usize = 0x80;
const NTSC_CPU_HZ: u32 = 1_789_773;
const PAL_CPU_HZ: u32 = 1_662_607;
const DEFAULT_NTSC_SPEED_US: u16 = 16_639;
const DEFAULT_PAL_SPEED_US: u16 = 19_997;
const RETURN_PC: u16 = 0x5000;
const NES_NOISE_PERIODS: [u16; 16] =
    [4, 8, 16, 32, 64, 96, 128, 160, 202, 254, 380, 508, 762, 1_016, 2_034, 4_068];
const NES_LENGTH_TABLE: [u8; 32] = [
    10, 254, 20, 2, 40, 4, 80, 6, 160, 8, 60, 10, 14, 12, 26, 14, 12, 16, 24, 18, 48, 20, 96, 22,
    192, 24, 72, 26, 16, 28, 32, 30,
];

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum MusicCommand {
    Load { filename: String, bytes: Vec<u8>, track: u8 },
    LoadTracker { filename: String, source: String },
    AuditionTracker { source: String },
    LoadPlaylistNsf { filename: String, bytes: Vec<u8>, track: u8 },
    LoadPlaylistTracker { filename: String, source: String },
    Play,
    Pause,
    Stop,
    Next,
    Previous,
    ToggleLoop,
    Status,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MusicStatus {
    pub filename: String,
    pub title: String,
    pub artist: String,
    pub track: u8,
    pub tracks: u8,
    pub paused: bool,
    pub looping: bool,
    pub position: Option<(usize, usize)>,
    pub channel_levels: [u8; 4],
}

impl MusicStatus {
    pub fn display_marquee(&self, offset: usize) -> String {
        let state = if self.paused { "||" } else { ">" };
        let passes = if self.looping { "2X" } else { "1X" };
        let format =
            if self.filename.to_ascii_lowercase().ends_with(".mus") { "MUS" } else { "NSF" };
        format!(
            "{format} {state} {}/{} {passes} [{}]",
            self.track,
            self.tracks,
            marquee_text(&self.title, offset, 6)
        )
    }
}

pub struct MusicFrame<'a> {
    pub samples: &'a [u16],
    pub source_rate: u32,
}

pub struct NsfImport {
    pub source: String,
    pub captured_frames: usize,
    pub dpcm_omitted: bool,
}

pub fn import_nsf_to_mus(
    bytes: &[u8],
    track: u8,
    output_filename: &str,
) -> Result<NsfImport, String> {
    let mut player = NsfPlayer::new(bytes, track)?;
    player.set_loop_limit(1);
    let mut frames = Vec::with_capacity(60 * 30);
    for _ in 0..60 * 600 {
        player.render_capture_frame();
        frames.push(player.bus.apu.capture_frame());
        if player.finished {
            break;
        }
    }
    if !player.loop_detected {
        return Err("NSF LOOP WAS NOT DETECTED WITHIN TEN MINUTES".to_owned());
    }
    let first_audio = frames
        .iter()
        .position(|frame| [frame[0], frame[4], frame[8], frame[12]].iter().any(|value| *value != 0))
        .ok_or_else(|| "NSF DID NOT PRODUCE SUPPORTED FOUR-CHANNEL AUDIO".to_owned())?;
    let loop_frame = ((player.loop_start_cycle.saturating_mul(60)
        + u64::from(player.image.cpu_rate) / 2)
        / u64::from(player.image.cpu_rate)) as usize;
    if first_audio != 0 {
        frames.drain(..first_audio);
    }
    let source = MusicEditor::captured_frames_to_source(
        output_filename,
        &frames,
        player.image.cpu_rate,
        loop_frame.saturating_sub(first_audio),
    )?;
    Ok(NsfImport { source, captured_frames: frames.len(), dpcm_omitted: player.bus.apu.dpcm_used })
}

pub struct MusicRadio {
    player: Option<NsfPlayer>,
    tracker: Option<TrackerPlayer>,
    filename: String,
    bytes: Vec<u8>,
    paused: bool,
    looping: bool,
    advance_pending: bool,
    playlist_item: bool,
}

impl MusicRadio {
    pub fn new() -> Self {
        Self {
            player: None,
            tracker: None,
            filename: String::new(),
            bytes: Vec::new(),
            paused: false,
            looping: true,
            advance_pending: false,
            playlist_item: false,
        }
    }

    pub fn apply(&mut self, command: MusicCommand) -> Result<String, String> {
        match command {
            MusicCommand::Load { filename, bytes, track } => {
                let mut player = NsfPlayer::new(&bytes, track)?;
                player.set_loop_limit(if self.looping { 2 } else { 1 });
                self.filename = filename;
                self.bytes = bytes;
                self.paused = false;
                self.advance_pending = false;
                self.playlist_item = false;
                self.player = Some(player);
                self.tracker = None;
                Ok(self.description("PLAYING"))
            }
            MusicCommand::LoadTracker { filename, source } => {
                let song = MusicEditor::compile(&source)?;
                self.filename = filename;
                self.bytes.clear();
                self.paused = false;
                self.advance_pending = false;
                self.playlist_item = false;
                self.player = None;
                self.tracker = Some(TrackerPlayer::new(song, self.looping));
                Ok(self.description("PLAYING"))
            }
            MusicCommand::AuditionTracker { source } => {
                let song = MusicEditor::compile(&source)?;
                self.filename = "AUDITION.MUS".to_owned();
                self.bytes.clear();
                self.paused = false;
                self.advance_pending = false;
                self.playlist_item = false;
                self.player = None;
                self.tracker = Some(TrackerPlayer::new(song, false));
                Ok("INSTRUMENT AUDITION".to_owned())
            }
            MusicCommand::LoadPlaylistNsf { filename, bytes, track } => {
                let mut player = NsfPlayer::new(&bytes, track)?;
                player.set_loop_limit(1);
                self.filename = filename;
                self.bytes = bytes;
                self.paused = false;
                self.advance_pending = false;
                self.playlist_item = true;
                self.player = Some(player);
                self.tracker = None;
                Ok(self.description("PLAYLIST"))
            }
            MusicCommand::LoadPlaylistTracker { filename, source } => {
                let song = MusicEditor::compile(&source)?;
                self.filename = filename;
                self.bytes.clear();
                self.paused = false;
                self.advance_pending = false;
                self.playlist_item = true;
                self.player = None;
                self.tracker = Some(TrackerPlayer::new(song, false));
                Ok(self.description("PLAYLIST"))
            }
            MusicCommand::Play => {
                self.require_music()?;
                self.paused = false;
                self.advance_pending = false;
                Ok(self.description("PLAYING"))
            }
            MusicCommand::Pause => {
                self.require_music()?;
                self.paused = true;
                Ok(self.description("PAUSED"))
            }
            MusicCommand::Stop => {
                self.require_music()?;
                let tracker = self.tracker.is_some();
                self.player = None;
                self.tracker = None;
                self.bytes.clear();
                self.filename.clear();
                self.paused = false;
                self.playlist_item = false;
                Ok(format!("{} STOPPED", if tracker { "MUSIC" } else { "NSF" }))
            }
            MusicCommand::Next => self.change_track(true),
            MusicCommand::Previous => self.change_track(false),
            MusicCommand::ToggleLoop => {
                self.require_music()?;
                self.looping = !self.looping;
                if let Some(player) = &mut self.player {
                    player.set_loop_limit(if self.looping { 2 } else { 1 });
                }
                if let Some(tracker) = &mut self.tracker {
                    tracker.looping = self.looping;
                }
                Ok(format!("NSF LOOP {}", if self.looping { "ON" } else { "OFF" }))
            }
            MusicCommand::Status => {
                self.require_music()?;
                Ok(self.description(if self.paused { "PAUSED" } else { "PLAYING" }))
            }
        }
    }

    pub fn status(&self) -> Option<MusicStatus> {
        if self.tracker.is_some() {
            return Some(MusicStatus {
                filename: self.filename.clone(),
                title: self.filename.clone(),
                artist: "FANTICON TRACKER".to_owned(),
                track: 1,
                tracks: 1,
                paused: self.paused,
                looping: self.looping && !self.playlist_item,
                position: self
                    .tracker
                    .as_ref()
                    .map(|tracker| (tracker.playing_row, tracker.tracker_rows)),
                channel_levels: if self.paused {
                    [0; 4]
                } else {
                    self.tracker.as_ref().map_or([0; 4], TrackerPlayer::channel_levels)
                },
            });
        }
        let player = self.player.as_ref()?;
        Some(MusicStatus {
            filename: self.filename.clone(),
            title: player.image.title.clone(),
            artist: player.image.artist.clone(),
            track: player.track,
            tracks: player.image.songs,
            paused: self.paused,
            looping: self.looping && !self.playlist_item,
            position: None,
            channel_levels: [0; 4],
        })
    }

    pub fn render_frame(&mut self) -> Option<MusicFrame<'_>> {
        if self.paused {
            return None;
        }
        if self.tracker.is_some() {
            return self.render_tracker_frame();
        }
        if self.advance_pending {
            self.advance_automatically();
        }
        if self.paused {
            return None;
        }
        let player = self.player.as_mut()?;
        player.render_frame();
        self.advance_pending = player.finished;
        Some(MusicFrame { samples: &player.samples, source_rate: player.image.cpu_rate })
    }

    fn render_tracker_frame(&mut self) -> Option<MusicFrame<'_>> {
        let finished = if let Some(tracker) = &mut self.tracker {
            tracker.render_frame();
            tracker.finished
        } else {
            return None;
        };
        if finished {
            self.tracker = None;
            self.filename.clear();
            self.playlist_item = false;
            return None;
        }
        self.tracker
            .as_ref()
            .map(|tracker| MusicFrame { samples: &tracker.samples, source_rate: CPU_CLOCK_HZ })
    }

    fn change_track(&mut self, forward: bool) -> Result<String, String> {
        if let Some(tracker) = &mut self.tracker {
            tracker.restart();
            self.paused = false;
            return Ok(self.description("RESTARTED"));
        }
        let player = self.require_player()?;
        let current = player.track;
        let tracks = player.image.songs;
        let next = if forward {
            if current < tracks {
                current + 1
            } else if self.looping {
                1
            } else {
                current
            }
        } else if current > 1 {
            current - 1
        } else if self.looping {
            tracks
        } else {
            current
        };
        if next != current {
            let mut player = NsfPlayer::new(&self.bytes, next)?;
            player.set_loop_limit(if self.looping { 2 } else { 1 });
            self.player = Some(player);
        }
        self.paused = false;
        self.advance_pending = false;
        Ok(self.description("PLAYING"))
    }

    fn advance_automatically(&mut self) {
        self.advance_pending = false;
        if self.playlist_item {
            self.player = None;
            self.bytes.clear();
            self.filename.clear();
            self.playlist_item = false;
            return;
        }
        let Some(player) = &self.player else { return };
        let next = if player.track < player.image.songs {
            Some(player.track + 1)
        } else if self.looping {
            Some(1)
        } else {
            None
        };
        match next.and_then(|track| NsfPlayer::new(&self.bytes, track).ok()) {
            Some(mut player) => {
                player.set_loop_limit(if self.looping { 2 } else { 1 });
                self.player = Some(player);
            }
            None => {
                self.player = None;
                self.bytes.clear();
                self.filename.clear();
            }
        }
    }

    fn require_player(&self) -> Result<&NsfPlayer, String> {
        self.player.as_ref().ok_or_else(|| "NO MUSIC IS LOADED".to_owned())
    }

    fn require_music(&self) -> Result<(), String> {
        if self.player.is_some() || self.tracker.is_some() {
            Ok(())
        } else {
            Err("NO MUSIC IS LOADED".to_owned())
        }
    }

    fn description(&self, state: &str) -> String {
        self.status().map_or_else(
            || "NO NSF IS LOADED".to_owned(),
            |status| {
                let artist = if status.artist.is_empty() {
                    String::new()
                } else {
                    format!(" BY {}", status.artist)
                };
                format!(
                    "{state} {} TRACK {}/{}: {}{artist}",
                    status.filename, status.track, status.tracks, status.title
                )
            },
        )
    }
}

pub fn nsf_track_count(bytes: &[u8]) -> Result<u8, String> {
    Ok(NsfImage::parse(bytes)?.songs)
}

pub(crate) struct NsfTrackProbe {
    player: NsfPlayer,
    elapsed_frames: usize,
    minimum_frames: usize,
}

impl NsfTrackProbe {
    pub fn new(bytes: &[u8], track: u8, minimum_seconds: usize) -> Result<Self, String> {
        let mut player = NsfPlayer::new(bytes, track)?;
        player.set_loop_limit(1);
        Ok(Self { player, elapsed_frames: 0, minimum_frames: minimum_seconds.saturating_mul(60) })
    }

    /// Advances a bounded amount of emulated playback. `Some(true)` means the
    /// track proved shorter than the threshold, `Some(false)` means it should be
    /// retained, and `None` means more background work remains.
    pub fn step(&mut self, frame_budget: usize) -> Option<bool> {
        if self.minimum_frames == 0 {
            return Some(false);
        }
        for _ in 0..frame_budget {
            self.player.render_capture_frame();
            self.elapsed_frames += 1;
            if self.player.finished {
                return Some(self.elapsed_frames < self.minimum_frames);
            }
            if self.elapsed_frames >= self.minimum_frames {
                return Some(false);
            }
        }
        None
    }
}

#[derive(Clone, Copy, Default)]
struct TrackerPulse {
    control: u8,
    timer: u16,
    divider: u16,
    phase: u8,
}

#[derive(Clone, Copy, Default)]
struct TrackerTriangle {
    enabled: bool,
    timer: u16,
    divider: u16,
    phase: u8,
}

#[derive(Clone, Copy)]
struct TrackerNoise {
    control: u8,
    period: u8,
    divider: u16,
    lfsr: u16,
}

impl Default for TrackerNoise {
    fn default() -> Self {
        Self { control: 0, period: 0, divider: 0, lfsr: 1 }
    }
}

struct TrackerPlayer {
    frames: Vec<[u8; 16]>,
    tracker_rows: usize,
    ticks_per_row: u8,
    loop_frame: usize,
    frame: usize,
    playing_row: usize,
    looping: bool,
    finished: bool,
    cycle: u64,
    pulse: [TrackerPulse; 2],
    triangle: TrackerTriangle,
    noise: TrackerNoise,
    samples: Vec<u16>,
}

impl TrackerPlayer {
    fn new(song: super::music_editor::CompiledSong, looping: bool) -> Self {
        Self {
            frames: song.frames,
            tracker_rows: song.tracker_rows,
            ticks_per_row: song.ticks_per_row,
            loop_frame: song.loop_frame,
            frame: 0,
            playing_row: 0,
            looping,
            finished: false,
            cycle: 0,
            pulse: [TrackerPulse::default(); 2],
            triangle: TrackerTriangle::default(),
            noise: TrackerNoise::default(),
            samples: Vec::with_capacity(CPU_CYCLES_PER_FRAME as usize),
        }
    }

    fn restart(&mut self) {
        self.frame = 0;
        self.playing_row = 0;
        self.finished = false;
        self.pulse = [TrackerPulse::default(); 2];
        self.triangle = TrackerTriangle::default();
        self.noise = TrackerNoise::default();
    }

    fn channel_levels(&self) -> [u8; 4] {
        [
            if self.pulse[0].control & 0x80 != 0 { self.pulse[0].control & 15 } else { 0 },
            if self.pulse[1].control & 0x80 != 0 { self.pulse[1].control & 15 } else { 0 },
            u8::from(self.triangle.enabled) * 15,
            if self.noise.control & 0x80 != 0 { self.noise.control & 15 } else { 0 },
        ]
    }

    fn render_frame(&mut self) {
        self.apply_frame();
        self.samples.clear();
        for _ in 0..CPU_CYCLES_PER_FRAME {
            self.tick();
            let p1 = tracker_pulse_level(self.pulse[0]);
            let p2 = tracker_pulse_level(self.pulse[1]);
            let tri = if self.triangle.enabled {
                TRIANGLE_SEQUENCE[self.triangle.phase as usize]
            } else {
                0
            };
            let noise = if self.noise.control & 0x80 != 0 && self.noise.lfsr & 1 == 0 {
                self.noise.control & 15
            } else {
                0
            };
            self.samples.push(mix_sample(p1, p2, tri, noise, 15));
        }
    }

    fn apply_frame(&mut self) {
        let Some(frame) = self.frames.get(self.frame).copied() else {
            self.finished = true;
            return;
        };
        self.playing_row = self.frame / usize::from(self.ticks_per_row);
        for voice in 0..2 {
            let base = voice * 4;
            let timer = u16::from_le_bytes([frame[base + 1], frame[base + 2]]);
            self.pulse[voice].control = frame[base];
            self.pulse[voice].timer = timer;
            if frame[base + 3] != 0 {
                self.pulse[voice].divider = timer;
                self.pulse[voice].phase = 0;
            }
        }
        let timer = u16::from_le_bytes([frame[9], frame[10]]);
        self.triangle.enabled = frame[8] != 0;
        self.triangle.timer = timer;
        if frame[11] != 0 {
            self.triangle.divider = timer;
            self.triangle.phase = 0;
        }
        self.noise.control = frame[12];
        self.noise.period = frame[13] & 15;
        if frame[15] != 0 {
            self.noise.divider = 0;
            self.noise.lfsr = 1;
        }
        self.frame += 1;
        if self.frame == self.frames.len() && self.looping {
            self.frame = self.loop_frame.min(self.frames.len() - 1);
        }
    }

    fn tick(&mut self) {
        let even = self.cycle.is_multiple_of(2);
        for pulse in &mut self.pulse {
            if even {
                if pulse.divider == 0 {
                    pulse.phase = (pulse.phase + 1) & 7;
                    pulse.divider = pulse.timer;
                } else {
                    pulse.divider -= 1;
                }
            }
        }
        if self.triangle.divider == 0 {
            self.triangle.phase = (self.triangle.phase + 1) & 31;
            self.triangle.divider = self.triangle.timer;
        } else {
            self.triangle.divider -= 1;
        }
        if self.noise.divider == 0 {
            self.noise.lfsr = step_noise_lfsr(self.noise.lfsr, self.noise.control & 0x40 != 0);
            self.noise.divider = NOISE_PERIODS[self.noise.period as usize] - 1;
        } else {
            self.noise.divider -= 1;
        }
        self.cycle = self.cycle.wrapping_add(1);
    }
}

fn tracker_pulse_level(pulse: TrackerPulse) -> u8 {
    if pulse.control & 0x80 == 0 {
        0
    } else {
        PULSE_DUTY_TABLE[((pulse.control >> 5) & 3) as usize][pulse.phase as usize]
            * (pulse.control & 15)
    }
}

#[derive(Clone, Debug)]
struct NsfImage {
    songs: u8,
    start_song: u8,
    load_address: u16,
    init_address: u16,
    play_address: u16,
    title: String,
    artist: String,
    cpu_rate: u32,
    speed_us: u16,
    region_x: u8,
    initial_banks: [u8; 8],
    banked: bool,
    payload: Vec<u8>,
}

impl NsfImage {
    fn parse(bytes: &[u8]) -> Result<Self, String> {
        if bytes.len() < NSF_HEADER_SIZE || &bytes[..5] != b"NESM\x1A" {
            return Err("NOT AN NSF FILE".to_owned());
        }
        if bytes[5] != 1 {
            return Err(format!("NSF VERSION {} IS NOT SUPPORTED", bytes[5]));
        }
        let songs = bytes[6];
        let start_song = bytes[7];
        if songs == 0 || start_song == 0 || start_song > songs {
            return Err("INVALID NSF TRACK TABLE".to_owned());
        }
        let word = |offset: usize| u16::from_le_bytes([bytes[offset], bytes[offset + 1]]);
        let load_address = word(0x08);
        let init_address = word(0x0a);
        let play_address = word(0x0c);
        if load_address < 0x6000 || init_address < 0x6000 || play_address < 0x6000 {
            return Err("NSF LOAD, INIT, AND PLAY MUST BE AT $6000 OR ABOVE".to_owned());
        }
        let expansion = bytes[0x7b];
        if expansion != 0 {
            return Err(format!("NSF EXPANSION AUDIO ${expansion:02X} IS NOT SUPPORTED"));
        }
        let region = bytes[0x7a] & 3;
        let pal = region == 1;
        let cpu_rate = if pal { PAL_CPU_HZ } else { NTSC_CPU_HZ };
        let declared_speed = word(if pal { 0x78 } else { 0x6e });
        let speed_us = if declared_speed == 0 {
            if pal { DEFAULT_PAL_SPEED_US } else { DEFAULT_NTSC_SPEED_US }
        } else {
            declared_speed
        };
        let mut initial_banks = [0; 8];
        initial_banks.copy_from_slice(&bytes[0x70..0x78]);
        let banked = initial_banks.iter().any(|bank| *bank != 0);
        let payload = bytes[NSF_HEADER_SIZE..].to_vec();
        if payload.is_empty() {
            return Err("NSF HAS NO PROGRAM DATA".to_owned());
        }
        if !banked && usize::from(load_address) + payload.len() > 0x1_0000 {
            return Err("NSF PROGRAM DOES NOT FIT IN 64 KIB".to_owned());
        }
        Ok(Self {
            songs,
            start_song,
            load_address,
            init_address,
            play_address,
            title: header_text(&bytes[0x0e..0x2e]),
            artist: header_text(&bytes[0x2e..0x4e]),
            cpu_rate,
            speed_us,
            region_x: u8::from(pal),
            initial_banks,
            banked,
            payload,
        })
    }
}

struct NsfPlayer {
    image: NsfImage,
    track: u8,
    cpu: Cpu,
    bus: NsfBus,
    routine_active: bool,
    init_complete: bool,
    source_cycles: u64,
    next_play_cycle: u64,
    play_period: u64,
    frame_remainder: u32,
    samples: Vec<u16>,
    seen_play_states: HashMap<u64, (u64, u64)>,
    loop_fingerprint: Option<u64>,
    loop_period: u64,
    next_loop_call: u64,
    loops_completed: u8,
    loop_limit: u8,
    play_calls: u64,
    finished: bool,
    loop_detected: bool,
    loop_start_cycle: u64,
}

impl NsfPlayer {
    fn new(bytes: &[u8], requested_track: u8) -> Result<Self, String> {
        let image = NsfImage::parse(bytes)?;
        let track = if requested_track == 0 { image.start_song } else { requested_track };
        if track == 0 || track > image.songs {
            return Err(format!("NSF TRACK MUST BE 1-{}", image.songs));
        }
        let bus = NsfBus::new(&image);
        let play_period =
            (u64::from(image.speed_us) * u64::from(image.cpu_rate) + 500_000) / 1_000_000;
        let mut player = Self {
            image,
            track,
            cpu: Cpu::default(),
            bus,
            routine_active: false,
            init_complete: false,
            source_cycles: 0,
            next_play_cycle: u64::MAX,
            play_period: play_period.max(1),
            frame_remainder: 0,
            samples: Vec::new(),
            seen_play_states: HashMap::new(),
            loop_fingerprint: None,
            loop_period: 0,
            next_loop_call: 0,
            loops_completed: 0,
            loop_limit: 2,
            play_calls: 0,
            finished: false,
            loop_detected: false,
            loop_start_cycle: 0,
        };
        player.begin_routine(player.image.init_address, track - 1, player.image.region_x, 0);
        Ok(player)
    }

    fn render_frame(&mut self) {
        self.bus.apu.retrigger = [false; 4];
        self.frame_remainder = self.frame_remainder.wrapping_add(self.image.cpu_rate);
        let cycles = self.frame_remainder / 60;
        self.frame_remainder %= 60;
        self.samples.clear();
        self.samples.reserve(cycles as usize);
        for _ in 0..cycles {
            if self.routine_active {
                // The Ricoh 2A03 exposes the D flag but ignores decimal arithmetic.
                // Clearing it before every CPU cycle gives NSF code the same arithmetic.
                self.cpu.status.0 &= !Status::DECIMAL;
                self.cpu.clock(&mut self.bus);
                if self.cpu.jammed() {
                    self.routine_active = false;
                } else if self.cpu.instruction_boundary() && self.cpu.pc == RETURN_PC {
                    self.routine_active = false;
                    if !self.init_complete {
                        self.init_complete = true;
                        self.next_play_cycle = self.source_cycles + self.play_period;
                    }
                }
            } else if self.init_complete && self.source_cycles >= self.next_play_cycle {
                if self.detect_completed_loop() {
                    self.finished = true;
                } else {
                    self.begin_routine(self.image.play_address, self.cpu.a, self.cpu.x, self.cpu.y);
                    self.next_play_cycle = self.next_play_cycle.saturating_add(self.play_period);
                }
            }
            self.bus.apu.tick();
            self.samples.push(self.bus.apu.sample);
            self.source_cycles = self.source_cycles.wrapping_add(1);
        }
    }

    fn render_capture_frame(&mut self) {
        self.bus.apu.retrigger = [false; 4];
        self.frame_remainder = self.frame_remainder.wrapping_add(self.image.cpu_rate);
        let cycles = self.frame_remainder / 60;
        self.frame_remainder %= 60;
        let target_cycle = self.source_cycles.saturating_add(u64::from(cycles));
        while self.source_cycles < target_cycle && !self.finished {
            if self.routine_active {
                self.cpu.status.0 &= !Status::DECIMAL;
                self.cpu.clock(&mut self.bus);
                if self.cpu.jammed() {
                    self.routine_active = false;
                } else if self.cpu.instruction_boundary() && self.cpu.pc == RETURN_PC {
                    self.routine_active = false;
                    if !self.init_complete {
                        self.init_complete = true;
                        self.next_play_cycle = self.source_cycles + self.play_period;
                    }
                }
                self.bus.apu.advance_capture_cycles(1);
                self.source_cycles = self.source_cycles.wrapping_add(1);
            } else if self.init_complete && self.source_cycles >= self.next_play_cycle {
                if self.detect_completed_loop() {
                    self.finished = true;
                } else {
                    self.begin_routine(self.image.play_address, self.cpu.a, self.cpu.x, self.cpu.y);
                    self.next_play_cycle = self.next_play_cycle.saturating_add(self.play_period);
                }
            } else {
                let next_event = if self.init_complete {
                    self.next_play_cycle.min(target_cycle)
                } else {
                    target_cycle
                };
                let skipped = next_event.saturating_sub(self.source_cycles);
                self.bus.apu.advance_capture_cycles(skipped);
                self.source_cycles = next_event;
            }
        }
    }

    fn begin_routine(&mut self, address: u16, a: u8, x: u8, y: u8) {
        let return_address = RETURN_PC.wrapping_sub(1).to_le_bytes();
        self.bus.write_ram(0x01fe, return_address[0]);
        self.bus.write_ram(0x01ff, return_address[1]);
        self.cpu.pc = address;
        self.cpu.sp = 0xfd;
        self.cpu.a = a;
        self.cpu.x = x;
        self.cpu.y = y;
        self.cpu.status = Status(Status::UNUSED | Status::INTERRUPT_DISABLE);
        self.routine_active = true;
    }

    fn set_loop_limit(&mut self, loops: u8) {
        self.loop_limit = loops.max(1);
    }

    fn detect_completed_loop(&mut self) -> bool {
        self.play_calls = self.play_calls.saturating_add(1);
        let fingerprint = self.bus.fingerprint(self.cpu.a, self.cpu.x, self.cpu.y);
        if let Some(loop_fingerprint) = self.loop_fingerprint {
            if self.play_calls < self.next_loop_call {
                return false;
            }
            if fingerprint == loop_fingerprint {
                self.loops_completed = self.loops_completed.saturating_add(1);
                self.next_loop_call = self.next_loop_call.saturating_add(self.loop_period);
                let complete = self.loops_completed >= self.loop_limit;
                self.loop_detected |= complete;
                return complete;
            }
            self.loop_fingerprint = None;
            self.loop_period = 0;
            self.next_loop_call = 0;
            self.loops_completed = 0;
        }
        if let Some((previous_call, previous_cycle)) =
            self.seen_play_states.get(&fingerprint).copied()
            && self.play_calls.saturating_sub(previous_call) >= 30
        {
            self.loop_period = self.play_calls - previous_call;
            self.loop_fingerprint = Some(fingerprint);
            self.next_loop_call = self.play_calls.saturating_add(self.loop_period);
            self.loops_completed = 1;
            self.loop_start_cycle = previous_cycle;
            let complete = self.loop_limit == 1;
            self.loop_detected |= complete;
            return complete;
        }
        self.seen_play_states.entry(fingerprint).or_insert((self.play_calls, self.source_cycles));
        // Classic NSF has no duration metadata. This ten-minute guard prevents
        // a monotonic driver state from defeating exact repeat detection forever.
        self.play_calls >= 36_000
    }
}

struct NsfBus {
    ram: [u8; 0x800],
    memory: Box<[u8; 0x1_0000]>,
    bank_rom: Vec<u8>,
    banks: [u8; 8],
    banked: bool,
    apu: NsfApu,
}

impl NsfBus {
    fn new(image: &NsfImage) -> Self {
        let mut memory = Box::new([0; 0x1_0000]);
        let mut bank_rom = Vec::new();
        if image.banked {
            bank_rom.resize(usize::from(image.load_address & 0x0fff), 0);
            bank_rom.extend_from_slice(&image.payload);
        } else {
            let start = usize::from(image.load_address);
            memory[start..start + image.payload.len()].copy_from_slice(&image.payload);
        }
        let mut apu = NsfApu::new(image.cpu_rate);
        // NSF drivers may assume the player has enabled the four base APU
        // voices before INIT. Real NSF players conventionally bootstrap
        // $4015/$4017 this way; leaving $4015 at reset zero causes subsequent
        // timer-high writes to discard every length-counter reload, producing
        // silence in otherwise valid drivers.
        apu.write(0x4015, 0x0f);
        apu.write(0x4017, 0x40);
        Self {
            ram: [0; 0x800],
            memory,
            bank_rom,
            banks: image.initial_banks,
            banked: image.banked,
            apu,
        }
    }

    fn write_ram(&mut self, address: u16, value: u8) {
        self.ram[usize::from(address) & 0x07ff] = value;
    }

    fn read_program(&self, address: u16) -> u8 {
        if self.banked && address >= 0x8000 {
            let slot = usize::from((address - 0x8000) >> 12);
            let offset = usize::from(self.banks[slot]) * 0x1000 + usize::from(address & 0x0fff);
            self.bank_rom.get(offset).copied().unwrap_or(0)
        } else {
            self.memory[usize::from(address)]
        }
    }

    fn fingerprint(&self, a: u8, x: u8, y: u8) -> u64 {
        let mut hash = 0xcbf2_9ce4_8422_2325_u64;
        for byte in self
            .ram
            .iter()
            .chain(self.memory[0x6000..0x8000].iter())
            .chain(self.banks.iter())
            .chain(self.apu.registers.iter())
            .copied()
            .chain([a, x, y])
        {
            hash ^= u64::from(byte);
            hash = hash.wrapping_mul(0x0000_0100_0000_01b3);
        }
        hash
    }
}

impl Bus for NsfBus {
    fn read(&mut self, address: u16) -> u8 {
        match address {
            0x0000..=0x1fff => self.ram[usize::from(address) & 0x07ff],
            0x4015 => self.apu.status_value(),
            0x6000..=0xffff => self.read_program(address),
            _ => 0,
        }
    }

    fn write(&mut self, address: u16, value: u8) {
        match address {
            0x0000..=0x1fff => self.write_ram(address, value),
            0x4000..=0x4017 => self.apu.write(address, value),
            0x5ff8..=0x5fff if self.banked => {
                self.banks[usize::from(address - 0x5ff8)] = value;
            }
            0x6000..=0x7fff => self.memory[usize::from(address)] = value,
            0x8000..=0xffff if !self.banked => self.memory[usize::from(address)] = value,
            _ => {}
        }
    }
}

#[derive(Clone, Copy, Default)]
struct NsfEnvelope {
    start: bool,
    divider: u8,
    decay: u8,
}

impl NsfEnvelope {
    fn clock(&mut self, control: u8) {
        let period = control & 15;
        if self.start {
            self.start = false;
            self.decay = 15;
            self.divider = period;
        } else if self.divider == 0 {
            self.divider = period;
            if self.decay != 0 {
                self.decay -= 1;
            } else if control & 0x20 != 0 {
                self.decay = 15;
            }
        } else {
            self.divider -= 1;
        }
    }

    fn volume(self, control: u8) -> u8 {
        if control & 0x10 != 0 { control & 15 } else { self.decay }
    }
}

#[derive(Clone, Copy, Default)]
struct NsfPulse {
    control: u8,
    timer: u16,
    divider: u16,
    phase: u8,
    length: u8,
    envelope: NsfEnvelope,
}

impl NsfPulse {
    fn tick(&mut self, even: bool) {
        if !even {
            return;
        }
        if self.divider == 0 {
            self.phase = (self.phase + 1) & 7;
            self.divider = self.timer;
        } else {
            self.divider -= 1;
        }
    }

    fn level(self) -> u8 {
        if self.length == 0 || self.timer < 8 {
            return 0;
        }
        PULSE_DUTY_TABLE[((self.control >> 6) & 3) as usize][self.phase as usize]
            * self.envelope.volume(self.control)
    }
}

#[derive(Clone, Copy, Default)]
struct NsfTriangle {
    timer: u16,
    divider: u16,
    phase: u8,
    length: u8,
    linear_counter: u8,
    linear_reload: bool,
}

impl NsfTriangle {
    fn tick(&mut self) {
        if self.length == 0 || self.linear_counter == 0 || self.timer < 2 {
            return;
        }
        if self.divider == 0 {
            self.phase = (self.phase + 1) & 31;
            self.divider = self.timer;
        } else {
            self.divider -= 1;
        }
    }

    fn level(self) -> u8 {
        if self.length != 0 && self.linear_counter != 0 && self.timer >= 2 {
            TRIANGLE_SEQUENCE[self.phase as usize]
        } else {
            0
        }
    }
}

#[derive(Clone, Copy)]
struct NsfNoise {
    control: u8,
    period: u8,
    mode: bool,
    divider: u16,
    lfsr: u16,
    length: u8,
    envelope: NsfEnvelope,
}

impl Default for NsfNoise {
    fn default() -> Self {
        Self {
            control: 0,
            period: 0,
            mode: false,
            divider: 0,
            lfsr: 1,
            length: 0,
            envelope: NsfEnvelope::default(),
        }
    }
}

impl NsfNoise {
    fn tick(&mut self) {
        if self.divider == 0 {
            self.lfsr = step_noise_lfsr(self.lfsr, self.mode);
            self.divider = NES_NOISE_PERIODS[self.period as usize] - 1;
        } else {
            self.divider -= 1;
        }
    }

    fn level(self) -> u8 {
        if self.length == 0 || self.lfsr & 1 != 0 { 0 } else { self.envelope.volume(self.control) }
    }
}

struct NsfApu {
    pulse: [NsfPulse; 2],
    triangle: NsfTriangle,
    noise: NsfNoise,
    registers: [u8; 0x18],
    status: u8,
    cycle: u64,
    sample: u16,
    cpu_rate: u32,
    frame_rate: u32,
    frame_phase: u32,
    frame_step: u8,
    gate_state: [bool; 4],
    declick_correction: i32,
    declick_remaining: u32,
    declick_cycles: u32,
    retrigger: [bool; 4],
    dpcm_used: bool,
}

impl NsfApu {
    fn new(cpu_rate: u32) -> Self {
        Self {
            pulse: [NsfPulse::default(); 2],
            triangle: NsfTriangle::default(),
            noise: NsfNoise::default(),
            registers: [0; 0x18],
            status: 0,
            cycle: 0,
            sample: 0,
            cpu_rate,
            frame_rate: if cpu_rate == PAL_CPU_HZ { 200 } else { 240 },
            frame_phase: 0,
            frame_step: 0,
            gate_state: [false; 4],
            declick_correction: 0,
            declick_remaining: 0,
            declick_cycles: (cpu_rate / 1_000).max(1),
            retrigger: [false; 4],
            dpcm_used: false,
        }
    }

    fn tick(&mut self) {
        let even = self.cycle.is_multiple_of(2);
        self.pulse[0].tick(even);
        self.pulse[1].tick(even);
        self.triangle.tick();
        self.noise.tick();
        self.frame_phase += self.frame_rate;
        if self.frame_phase >= self.cpu_rate {
            self.frame_phase -= self.cpu_rate;
            self.clock_quarter_frame();
            if self.frame_step & 1 != 0 {
                self.clock_half_frame();
            }
            self.frame_step = (self.frame_step + 1) & 3;
        }
        self.cycle = self.cycle.wrapping_add(1);
        let levels = [
            self.pulse[0].level(),
            self.pulse[1].level(),
            self.triangle.level(),
            self.noise.level(),
        ];
        let gates = [
            self.pulse[0].length != 0
                && self.pulse[0].timer >= 8
                && self.pulse[0].envelope.volume(self.pulse[0].control) != 0,
            self.pulse[1].length != 0
                && self.pulse[1].timer >= 8
                && self.pulse[1].envelope.volume(self.pulse[1].control) != 0,
            self.triangle.length != 0
                && self.triangle.linear_counter != 0
                && self.triangle.timer >= 2,
            self.noise.length != 0 && self.noise.envelope.volume(self.noise.control) != 0,
        ];
        let raw_sample = mix_sample(levels[0], levels[1], levels[2], levels[3], 15);
        if gates != self.gate_state {
            self.gate_state = gates;
            self.declick_correction = i32::from(self.sample) - i32::from(raw_sample);
            self.declick_remaining = self.declick_cycles;
        }
        self.sample =
            (i32::from(raw_sample) + self.declick_correction).clamp(0, i32::from(u16::MAX)) as u16;
        if self.declick_remaining != 0 {
            self.declick_correction = self.declick_correction * (self.declick_remaining - 1) as i32
                / self.declick_remaining as i32;
            self.declick_remaining -= 1;
        }
    }

    fn advance_capture_cycles(&mut self, cycles: u64) {
        let accumulated =
            u64::from(self.frame_phase) + cycles.saturating_mul(u64::from(self.frame_rate));
        let clocks = accumulated / u64::from(self.cpu_rate);
        self.frame_phase = (accumulated % u64::from(self.cpu_rate)) as u32;
        for _ in 0..clocks {
            self.clock_quarter_frame();
            if self.frame_step & 1 != 0 {
                self.clock_half_frame();
            }
            self.frame_step = (self.frame_step + 1) & 3;
        }
        self.cycle = self.cycle.wrapping_add(cycles);
    }

    fn clock_quarter_frame(&mut self) {
        self.pulse[0].envelope.clock(self.pulse[0].control);
        self.pulse[1].envelope.clock(self.pulse[1].control);
        self.noise.envelope.clock(self.noise.control);

        let control = self.registers[0x08];
        if self.triangle.linear_reload {
            self.triangle.linear_counter = control & 0x7f;
        } else if self.triangle.linear_counter != 0 {
            self.triangle.linear_counter -= 1;
        }
        if control & 0x80 == 0 {
            self.triangle.linear_reload = false;
        }
    }

    fn clock_half_frame(&mut self) {
        for pulse in &mut self.pulse {
            if pulse.length != 0 && pulse.control & 0x20 == 0 {
                pulse.length -= 1;
            }
        }
        if self.noise.length != 0 && self.noise.control & 0x20 == 0 {
            self.noise.length -= 1;
        }
        if self.triangle.length != 0 && self.registers[0x08] & 0x80 == 0 {
            self.triangle.length -= 1;
        }
    }

    fn status_value(&self) -> u8 {
        u8::from(self.pulse[0].length != 0)
            | (u8::from(self.pulse[1].length != 0) << 1)
            | (u8::from(self.triangle.length != 0) << 2)
            | (u8::from(self.noise.length != 0) << 3)
    }

    fn capture_frame(&self) -> [u8; 16] {
        let mut frame = [0_u8; 16];
        for voice in 0..2 {
            let pulse = self.pulse[voice];
            let active = pulse.length != 0 && pulse.timer >= 8;
            let base = voice * 4;
            if active {
                frame[base] =
                    0x80 | ((pulse.control >> 6) & 3) << 5 | pulse.envelope.volume(pulse.control);
                frame[base + 1] = pulse.timer as u8;
                frame[base + 2] = (pulse.timer >> 8) as u8;
                frame[base + 3] = u8::from(self.retrigger[voice]);
            }
        }
        if self.triangle.length != 0
            && self.triangle.linear_counter != 0
            && self.triangle.timer >= 2
        {
            frame[8] = 1;
            frame[9] = self.triangle.timer as u8;
            frame[10] = (self.triangle.timer >> 8) as u8;
            frame[11] = u8::from(self.retrigger[2]);
        }
        if self.noise.length != 0 {
            frame[12] = 0x80
                | (u8::from(self.noise.mode) << 6)
                | self.noise.envelope.volume(self.noise.control);
            frame[13] = self.noise.period;
            frame[15] = u8::from(self.retrigger[3]);
        }
        frame
    }

    fn write(&mut self, address: u16, value: u8) {
        let offset = usize::from(address - 0x4000);
        if let Some(register) = self.registers.get_mut(offset) {
            *register = value;
        }
        match address {
            0x4000 | 0x4004 => {
                let channel = usize::from((address - 0x4000) / 4);
                self.pulse[channel].control = value;
            }
            0x4002 | 0x4006 => {
                let channel = usize::from((address - 0x4002) / 4);
                self.refresh_pulse_timer(channel);
            }
            0x4003 | 0x4007 => {
                let channel = usize::from((address - 0x4003) / 4);
                self.retrigger[channel] = true;
                self.refresh_pulse_timer(channel);
                self.pulse[channel].phase = 0;
                self.pulse[channel].divider = self.pulse[channel].timer;
                if self.status & (1 << channel) != 0 {
                    self.pulse[channel].length = NES_LENGTH_TABLE[usize::from(value >> 3)];
                }
                self.pulse[channel].envelope.start = true;
            }
            0x4008 => {}
            0x400a => self.refresh_triangle_timer(),
            0x400b => {
                self.retrigger[2] = true;
                self.refresh_triangle_timer();
                if self.status & 4 != 0 {
                    self.triangle.length = NES_LENGTH_TABLE[usize::from(value >> 3)];
                }
                self.triangle.linear_reload = true;
            }
            0x400c => self.noise.control = value,
            0x400e => {
                self.noise.period = value & 15;
                self.noise.mode = value & 0x80 != 0;
            }
            0x400f => {
                self.retrigger[3] = true;
                if self.status & 8 != 0 {
                    self.noise.length = NES_LENGTH_TABLE[usize::from(value >> 3)];
                }
                self.noise.envelope.start = true;
            }
            0x4015 => {
                self.dpcm_used |= value & 0x10 != 0;
                self.status = value & 0x1f;
                if value & 1 == 0 {
                    self.pulse[0].length = 0;
                }
                if value & 2 == 0 {
                    self.pulse[1].length = 0;
                }
                if value & 4 == 0 {
                    self.triangle.length = 0;
                }
                if value & 8 == 0 {
                    self.noise.length = 0;
                }
            }
            0x4017 => {
                self.frame_phase = 0;
                self.frame_step = 0;
                if value & 0x80 != 0 {
                    self.clock_quarter_frame();
                    self.clock_half_frame();
                }
            }
            0x4010..=0x4013 => self.dpcm_used = true,
            _ => {}
        }
    }

    fn refresh_pulse_timer(&mut self, channel: usize) {
        let base = channel * 4;
        self.pulse[channel].timer =
            u16::from(self.registers[base + 2]) | (u16::from(self.registers[base + 3] & 7) << 8);
    }

    fn refresh_triangle_timer(&mut self) {
        self.triangle.timer =
            u16::from(self.registers[0x0a]) | (u16::from(self.registers[0x0b] & 7) << 8);
    }
}

fn header_text(bytes: &[u8]) -> String {
    let length = bytes.iter().position(|byte| *byte == 0).unwrap_or(bytes.len());
    bytes[..length]
        .iter()
        .map(|byte| if byte.is_ascii_graphic() || *byte == b' ' { char::from(*byte) } else { '?' })
        .collect::<String>()
        .trim()
        .to_owned()
}

fn marquee_text(text: &str, offset: usize, width: usize) -> String {
    if width == 0 {
        return String::new();
    }
    let text = if text.is_empty() { "UNTITLED" } else { text };
    let cycle = format!("{text}   ");
    let bytes = cycle.as_bytes();
    (0..width).map(|index| char::from(bytes[(offset + index) % bytes.len()])).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nsf(program: &[u8], songs: u8, start: u8) -> Vec<u8> {
        let mut bytes = vec![0; NSF_HEADER_SIZE];
        bytes[..5].copy_from_slice(b"NESM\x1A");
        bytes[5] = 1;
        bytes[6] = songs;
        bytes[7] = start;
        bytes[8..14].copy_from_slice(&[0x00, 0x80, 0x00, 0x80, 0x20, 0x80]);
        bytes[0x0e..0x12].copy_from_slice(b"TEST");
        bytes[0x2e..0x34].copy_from_slice(b"AUTHOR");
        bytes[0x6e..0x70].copy_from_slice(&DEFAULT_NTSC_SPEED_US.to_le_bytes());
        bytes.extend_from_slice(program);
        bytes
    }

    #[test]
    fn parses_metadata_and_rejects_expansion_audio() {
        let mut bytes = nsf(&[0x60], 3, 2);
        let image = NsfImage::parse(&bytes).unwrap();
        assert_eq!((image.songs, image.start_song), (3, 2));
        assert_eq!((image.title.as_str(), image.artist.as_str()), ("TEST", "AUTHOR"));
        bytes[0x7b] = 1;
        assert!(NsfImage::parse(&bytes).unwrap_err().contains("EXPANSION"));
    }

    #[test]
    fn compact_status_marquee_rotates_the_title() {
        let status = MusicStatus {
            filename: "song.nsf".to_owned(),
            title: "ABCDEFG".to_owned(),
            artist: String::new(),
            track: 2,
            tracks: 9,
            paused: false,
            looping: true,
            position: None,
            channel_levels: [0; 4],
        };
        assert!(status.display_marquee(0).ends_with("[ABCDEF]"));
        assert!(status.display_marquee(2).ends_with("[CDEFG ]"));
    }

    #[test]
    fn init_and_play_routines_drive_the_shared_four_voice_mixer() {
        let mut program = vec![0xea; 0x40];
        // INIT: enable pulse 1, set constant volume/duty/timer, return.
        program[..19].copy_from_slice(&[
            0xa9, 0x01, 0x8d, 0x15, 0x40, 0xa9, 0x9f, 0x8d, 0x00, 0x40, 0xa9, 0x20, 0x8d, 0x02,
            0x40, 0x8d, 0x03, 0x40, 0x60,
        ]);
        program[0x20] = 0x60; // PLAY: RTS
        let mut player = NsfPlayer::new(&nsf(&program, 1, 1), 1).unwrap();
        player.render_frame();
        assert!(player.samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn nsf_host_enables_base_apu_channels_before_init() {
        let mut program = vec![0xea; 0x40];
        // This valid driver intentionally relies on the NSF host's conventional
        // $4015 bootstrap instead of enabling pulse 1 itself.
        program[..14].copy_from_slice(&[
            0xa9, 0x9f, 0x8d, 0x00, 0x40, 0xa9, 0x20, 0x8d, 0x02, 0x40, 0x8d, 0x03, 0x40, 0x60,
        ]);
        program[0x20] = 0x60;
        let mut player = NsfPlayer::new(&nsf(&program, 1, 1), 1).unwrap();
        player.render_frame();
        assert_eq!(player.bus.apu.status & 0x0f, 0x0f);
        assert!(player.samples.iter().any(|sample| *sample != 0));
    }

    #[test]
    fn nsf_import_captures_an_editable_v2_tracker_song() {
        let mut program = vec![0xea; 0x40];
        program[..19].copy_from_slice(&[
            0xa9, 0x01, 0x8d, 0x15, 0x40, 0xa9, 0x9f, 0x8d, 0x00, 0x40, 0xa9, 0x20, 0x8d, 0x02,
            0x40, 0x8d, 0x03, 0x40, 0x60,
        ]);
        program[0x20] = 0x60;
        let imported = import_nsf_to_mus(&nsf(&program, 1, 1), 1, "IMPORT.MUS").unwrap();
        assert!((1..=60).contains(&imported.captured_frames));
        assert!(!imported.dpcm_omitted);
        assert!(imported.source.contains(";@FANTICON-MUSIC 2"));
        let tracker = MusicEditor::compile(&imported.source).unwrap();
        assert_eq!(tracker.ticks_per_row, 1);
        assert_eq!(tracker.tracker_rows, imported.captured_frames.div_ceil(16) * 16);
        assert!(tracker.loop_frame < tracker.frames.len());
        assert!(tracker.frames.iter().any(|frame| frame[0] != 0));
    }

    #[test]
    fn noise_percussion_envelope_decays_and_length_counter_stops_it() {
        let mut apu = NsfApu::new(NTSC_CPU_HZ);
        apu.write(0x4015, 0x08);
        apu.write(0x400c, 0x00); // envelope volume, fastest decay, no length halt
        apu.write(0x400e, 0x84); // short-mode noise, period 4
        apu.write(0x400f, 0x00); // start envelope, length-table entry 0

        assert_eq!(apu.status_value() & 0x08, 0x08);
        assert!(apu.noise.mode);
        apu.clock_quarter_frame();
        assert_eq!(apu.noise.envelope.volume(apu.noise.control), 15);
        for _ in 0..15 {
            apu.clock_quarter_frame();
        }
        assert_eq!(apu.noise.envelope.volume(apu.noise.control), 0);

        for _ in 0..10 {
            apu.clock_half_frame();
        }
        assert_eq!(apu.noise.length, 0);
        assert_eq!(apu.status_value() & 0x08, 0);
    }

    #[test]
    fn triangle_linear_counter_gates_notes_and_status_write_clears_lengths() {
        let mut apu = NsfApu::new(NTSC_CPU_HZ);
        apu.write(0x4015, 0x05);
        apu.write(0x4008, 0x02);
        apu.write(0x400a, 0x20);
        apu.write(0x400b, 0x00);
        apu.clock_quarter_frame();
        assert_eq!(apu.triangle.linear_counter, 2);
        apu.clock_quarter_frame();
        apu.clock_quarter_frame();
        assert_eq!(apu.triangle.linear_counter, 0);

        apu.write(0x4000, 0x1f);
        apu.write(0x4002, 0x20);
        apu.write(0x4003, 0x00);
        assert_ne!(apu.status_value() & 1, 0);
        apu.write(0x4015, 0);
        assert_eq!(apu.status_value(), 0);
    }

    #[test]
    fn gate_transition_correction_keeps_note_end_continuous_then_expires() {
        let mut apu = NsfApu::new(NTSC_CPU_HZ);
        apu.write(0x4015, 0x01);
        apu.write(0x4000, 0x9f);
        apu.write(0x4002, 0x20);
        apu.write(0x4003, 0x00);
        for _ in 0..apu.declick_cycles + 1 {
            apu.tick();
        }
        while apu.sample == 0 {
            apu.tick();
        }
        let before = apu.sample;

        apu.write(0x4015, 0);
        apu.tick();
        assert_eq!(apu.sample, before);
        assert_ne!(apu.declick_remaining, 0);
        for _ in 0..apu.declick_cycles {
            apu.tick();
        }
        assert_eq!(apu.sample, 0);
        assert_eq!(apu.declick_correction, 0);
    }

    #[test]
    fn radio_navigates_wraps_pauses_and_stops() {
        let bytes = nsf(&[0x60; 0x40], 3, 2);
        let mut radio = MusicRadio::new();
        radio
            .apply(MusicCommand::Load { filename: "music.nsf".to_owned(), bytes, track: 0 })
            .unwrap();
        assert_eq!(radio.status().unwrap().track, 2);
        radio.apply(MusicCommand::Next).unwrap();
        assert_eq!(radio.status().unwrap().track, 3);
        radio.apply(MusicCommand::Next).unwrap();
        assert_eq!(radio.status().unwrap().track, 1);
        radio.apply(MusicCommand::Pause).unwrap();
        assert!(radio.render_frame().is_none());
        radio.apply(MusicCommand::Stop).unwrap();
        assert!(radio.status().is_none());
    }

    #[test]
    fn playlist_nsf_stops_after_one_detected_loop_instead_of_changing_tracks() {
        let bytes = nsf(&[0x60; 0x40], 3, 1);
        assert_eq!(nsf_track_count(&bytes).unwrap(), 3);
        let mut radio = MusicRadio::new();
        radio
            .apply(MusicCommand::LoadPlaylistNsf {
                filename: "/music/album.nsf".to_owned(),
                bytes,
                track: 2,
            })
            .unwrap();
        let player = radio.player.as_mut().unwrap();
        player.init_complete = true;
        let fingerprint = player.bus.fingerprint(player.cpu.a, player.cpu.x, player.cpu.y);
        player.seen_play_states.insert(fingerprint, (0, 0));
        player.play_calls = 30;
        assert!(player.detect_completed_loop());
        player.finished = true;
        radio.advance_pending = true;
        assert!(radio.render_frame().is_none());
        assert!(radio.status().is_none());
    }

    #[test]
    fn short_nsf_filter_only_rejects_tracks_with_a_proven_early_loop() {
        let bytes = nsf(&[0x60; 0x40], 1, 1);
        let mut probe = NsfTrackProbe::new(&bytes, 1, 10).unwrap();
        assert_eq!(probe.step(1), None);
        let result = (0..20).find_map(|_| probe.step(30));
        assert_eq!(result, Some(true));

        let mut disabled = NsfTrackProbe::new(&bytes, 1, 0).unwrap();
        assert_eq!(disabled.step(1), Some(false));
    }

    #[test]
    fn tracker_radio_renders_the_four_voice_asset_at_vm_rate() {
        let source = crate::host::music_editor::MusicEditor::default().serialize("theme.mus");
        let mut radio = MusicRadio::new();
        radio
            .apply(MusicCommand::LoadTracker { filename: "theme.mus".to_owned(), source })
            .unwrap();
        let status = radio.status().unwrap();
        assert_eq!(status.title, "theme.mus");
        assert_eq!(status.position, Some((0, 64)));
        assert!(status.display_marquee(0).starts_with("MUS >"));
        let frame = radio.render_frame().unwrap();
        assert_eq!(frame.source_rate, CPU_CLOCK_HZ);
        assert_eq!(frame.samples.len(), CPU_CYCLES_PER_FRAME as usize);
        assert!(frame.samples.iter().any(|sample| *sample != 0));
        assert_eq!(radio.status().unwrap().channel_levels, [15; 4]);
        radio.apply(MusicCommand::Pause).unwrap();
        assert_eq!(radio.status().unwrap().channel_levels, [0; 4]);
    }

    #[test]
    fn repeated_driver_state_finishes_after_the_second_detected_loop() {
        let mut player = NsfPlayer::new(&nsf(&[0x60; 0x40], 1, 1), 1).unwrap();
        player.init_complete = true;
        player.routine_active = false;
        player.set_loop_limit(2);
        for _ in 0..60 {
            assert!(!player.detect_completed_loop());
        }
        assert!(player.detect_completed_loop());
    }
}
