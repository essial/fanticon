use fanticon::video::Video;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::EDITOR_DISPLAY_WIDTH;
use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};

const VOICES: usize = 4;
const DEFAULT_PATTERN_ROWS: usize = 16;
const VISIBLE_ROWS: usize = 32;
const PANE_LEFT: usize = 21 * GLYPH_WIDTH;
const PANE_TOP: usize = 3 * GLYPH_HEIGHT;
const UI_BLACK: u8 = 0;
const GRAD_P1: u8 = 160;
const GRAD_P2: u8 = 168;
const GRAD_TRI: u8 = 176;
const GRAD_NOISE: u8 = 184;
const GRAD_CHROME: u8 = 192;
const GRAD_WHITE: u8 = 200;
const PLAYING_BG: u8 = 208;
const SELECT_BG: u8 = 209;
const METER_BACKGROUNDS: [u8; VOICES] = [210, 211, 212, 213];
/// Alternating tint and boundary rule that make each pattern region in the
/// concatenated row list readable as its own block.
const REGION_BG: u8 = 214;
const REGION_RULE: u8 = 215;
const CHANNEL_GRADIENTS: [u8; VOICES] = [GRAD_P1, GRAD_P2, GRAD_TRI, GRAD_NOISE];
const CPU_CLOCK_HZ: f64 = 1_789_773.0;
const NOTE_LEGATO: u8 = 0x80;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Step {
    note: u8,
    instrument: u8,
    volume: u8,
}

impl Default for Step {
    fn default() -> Self {
        Self { note: 0xff, instrument: 0xff, volume: 0xff }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Pattern {
    steps: Vec<Step>,
}

impl Pattern {
    fn empty(rows: usize) -> Self {
        Self { steps: vec![Step::default(); rows] }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Envelope {
    values: Vec<i8>,
    loop_at: Option<usize>,
}

impl Envelope {
    fn value(&self, tick: usize) -> i8 {
        if self.values.is_empty() {
            return 0;
        }
        let index = if tick < self.values.len() {
            tick
        } else if let Some(loop_at) = self.loop_at.filter(|loop_at| *loop_at < self.values.len()) {
            loop_at + (tick - loop_at) % (self.values.len() - loop_at)
        } else {
            self.values.len() - 1
        };
        self.values[index]
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Instrument {
    volume: Envelope,
    arpeggio: Envelope,
    pitch: Envelope,
    tone: Envelope,
}

impl Default for Instrument {
    fn default() -> Self {
        Self {
            volume: Envelope { values: vec![15], loop_at: Some(0) },
            arpeggio: Envelope { values: vec![0], loop_at: Some(0) },
            pitch: Envelope { values: vec![0], loop_at: Some(0) },
            tone: Envelope { values: vec![2], loop_at: Some(0) },
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
struct Song {
    tempo: u8,
    pattern_rows: usize,
    loop_row: usize,
    frames: Vec<[u8; VOICES]>,
    patterns: [Vec<Pattern>; VOICES],
    instruments: Vec<Instrument>,
}

impl Song {
    fn from_flat(tempo: u8, rows: &[([u8; VOICES], [u8; VOICES])]) -> Self {
        let pattern_rows = DEFAULT_PATTERN_ROWS;
        let frame_count = rows.len().div_ceil(pattern_rows).max(1);
        let frames = (0..frame_count).map(|frame| [frame as u8; VOICES]).collect::<Vec<_>>();
        let mut patterns: [Vec<Pattern>; VOICES] = core::array::from_fn(|_| {
            (0..frame_count).map(|_| Pattern::empty(pattern_rows)).collect()
        });
        for (row, (notes, volumes)) in rows.iter().enumerate() {
            let frame = row / pattern_rows;
            let local = row % pattern_rows;
            for voice in 0..VOICES {
                patterns[voice][frame].steps[local] = Step {
                    note: notes[voice],
                    instrument: if notes[voice] != 0xff { 0 } else { 0xff },
                    volume: if notes[voice] != 0xff { volumes[voice] & 15 } else { 0xff },
                };
            }
        }
        Self {
            tempo,
            pattern_rows,
            loop_row: 0,
            frames,
            patterns,
            instruments: vec![Instrument::default()],
        }
    }

    fn total_rows(&self) -> usize {
        self.frames.len() * self.pattern_rows
    }

    fn from_steps(
        tempo: u8,
        pattern_rows: usize,
        rows: &[[Step; VOICES]],
        instruments: Vec<Instrument>,
    ) -> Result<Self, String> {
        let frame_count = rows.len().div_ceil(pattern_rows).max(1);
        let mut frames = vec![[0; VOICES]; frame_count];
        let mut patterns: [Vec<Pattern>; VOICES] = core::array::from_fn(|_| Vec::new());
        for (frame, order) in frames.iter_mut().enumerate() {
            for voice in 0..VOICES {
                let mut pattern = Pattern::empty(pattern_rows);
                for local in 0..pattern_rows {
                    if let Some(row) = rows.get(frame * pattern_rows + local) {
                        pattern.steps[local] = row[voice];
                    }
                }
                let number = patterns[voice]
                    .iter()
                    .position(|existing| *existing == pattern)
                    .unwrap_or_else(|| {
                        patterns[voice].push(pattern);
                        patterns[voice].len() - 1
                    });
                if number > u8::MAX as usize {
                    return Err(
                        "IMPORT NEEDS MORE THAN 256 UNIQUE PATTERNS ON ONE CHANNEL".to_owned()
                    );
                }
                order[voice] = number as u8;
            }
        }
        Ok(Self { tempo, pattern_rows, loop_row: 0, frames, patterns, instruments })
    }

    fn step(&self, row: usize, voice: usize) -> Step {
        let frame = (row / self.pattern_rows).min(self.frames.len() - 1);
        let pattern = usize::from(self.frames[frame][voice]);
        self.patterns[voice]
            .get(pattern)
            .and_then(|pattern| pattern.steps.get(row % self.pattern_rows))
            .copied()
            .unwrap_or_default()
    }

    fn ensure_pattern(&mut self, voice: usize, pattern: usize) {
        while self.patterns[voice].len() <= pattern {
            self.patterns[voice].push(Pattern::empty(self.pattern_rows));
        }
    }

    fn step_mut(&mut self, row: usize, voice: usize) -> &mut Step {
        let frame = (row / self.pattern_rows).min(self.frames.len() - 1);
        let pattern = usize::from(self.frames[frame][voice]);
        self.ensure_pattern(voice, pattern);
        &mut self.patterns[voice][pattern].steps[row % self.pattern_rows]
    }
}

impl Default for Song {
    fn default() -> Self {
        let mut flat = vec![([0xff; VOICES], [15; VOICES]); 64];
        for (row, notes) in [
            (0, [48, 55, 36, 13]),
            (8, [52, 59, 40, 0]),
            (16, [55, 60, 43, 9]),
            (24, [52, 59, 40, 0]),
            (32, [48, 55, 36, 13]),
            (40, [43, 52, 36, 0]),
            (48, [45, 53, 41, 9]),
            (56, [47, 55, 43, 0]),
        ] {
            flat[row].0 = notes;
        }
        Self::from_flat(6, &flat)
    }
}

#[derive(Clone, Debug)]
pub(crate) struct CompiledSong {
    pub frames: Vec<[u8; 16]>,
    pub tracker_rows: usize,
    pub ticks_per_row: u8,
    pub loop_frame: usize,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum View {
    Pattern,
    Frames,
    Instrument,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum EnvKind {
    Volume,
    Arpeggio,
    Pitch,
    Tone,
}

pub struct MusicEditor {
    song: Song,
    undo: Vec<Song>,
    row: usize,
    voice: usize,
    field: usize,
    octave: u8,
    scroll: usize,
    playback_row: Option<usize>,
    playback_levels: [u8; VOICES],
    view: View,
    frame_cursor: usize,
    instrument: usize,
    env_kind: EnvKind,
    env_cursor: usize,
    mouse_dragging_envelope: bool,
}

impl Default for MusicEditor {
    fn default() -> Self {
        Self {
            song: Song::default(),
            undo: Vec::new(),
            row: 0,
            voice: 0,
            field: 0,
            octave: 4,
            scroll: 0,
            playback_row: None,
            playback_levels: [0; VOICES],
            view: View::Pattern,
            frame_cursor: 0,
            instrument: 0,
            env_kind: EnvKind::Volume,
            env_cursor: 0,
            mouse_dragging_envelope: false,
        }
    }
}

impl MusicEditor {
    pub(crate) fn captured_frames_to_source(
        filename: &str,
        frames: &[[u8; 16]],
        source_rate: u32,
        loop_row: usize,
    ) -> Result<String, String> {
        if frames.is_empty() {
            return Err("NSF CAPTURE PRODUCED NO AUDIO FRAMES".to_owned());
        }
        let instruments = (0..4)
            .map(|tone| Instrument {
                tone: Envelope { values: vec![tone], loop_at: Some(0) },
                ..Instrument::default()
            })
            .collect::<Vec<_>>();
        let mut previous = [0_u8; 16];
        let mut rows = Vec::with_capacity(frames.len());
        for frame in frames {
            let mut steps = [Step::default(); VOICES];
            for (voice, step) in steps.iter_mut().enumerate() {
                let base = voice * 4;
                let active = match voice {
                    0 | 1 => frame[base] & 0x80 != 0,
                    2 => frame[base] != 0,
                    _ => frame[base] & 0x80 != 0,
                };
                let was_active = match voice {
                    0 | 1 => previous[base] & 0x80 != 0,
                    2 => previous[base] != 0,
                    _ => previous[base] & 0x80 != 0,
                };
                if !active && was_active {
                    step.note = 0;
                } else if active {
                    let timer_changed = frame[base + 1..base + 3] != previous[base + 1..base + 3];
                    if !was_active || timer_changed || frame[base + 3] != 0 {
                        let note = if voice == 3 {
                            frame[base + 1].min(15) + 1
                        } else {
                            timer_to_note(
                                u16::from_le_bytes([frame[base + 1], frame[base + 2]]),
                                voice == 2,
                                source_rate,
                            )
                        };
                        // Timer writes can bend pitch without resetting oscillator
                        // phase. Only the NSF high-register/retrigger write is a
                        // fresh tracker attack; all other pitch changes are legato.
                        step.note = if frame[base + 3] != 0 { note } else { note | NOTE_LEGATO };
                    }
                    let instrument = if voice == 3 {
                        u8::from(frame[base] & 0x40 != 0)
                    } else if voice < 2 {
                        (frame[base] >> 5) & 3
                    } else {
                        0
                    };
                    let previous_instrument = if voice == 3 {
                        u8::from(previous[base] & 0x40 != 0)
                    } else if voice < 2 {
                        (previous[base] >> 5) & 3
                    } else {
                        0
                    };
                    if !was_active || instrument != previous_instrument {
                        step.instrument = instrument;
                    }
                    let volume = if voice == 2 { 15 } else { frame[base] & 15 };
                    let previous_volume = if voice == 2 { 15 } else { previous[base] & 15 };
                    if !was_active || volume != previous_volume {
                        step.volume = volume;
                    }
                }
            }
            rows.push(steps);
            previous = *frame;
        }
        let mut song = Song::from_steps(1, DEFAULT_PATTERN_ROWS, &rows, instruments)?;
        song.loop_row = loop_row.min(song.total_rows() - 1);
        Ok(Self { song, ..Self::default() }.serialize(filename))
    }
    pub(crate) fn compile(source: &str) -> Result<CompiledSong, String> {
        Ok(Self::parse(source)?.compile_song())
    }

    pub fn instrument_audition_source(&self, key: &Key) -> Option<String> {
        if self.view != View::Instrument {
            return None;
        }
        let Key::Character(key) = key else { return None };
        let semitone = piano_key(&key.to_ascii_lowercase())?;
        let note = if self.voice == 3 {
            1 + semitone.min(15) as u8
        } else {
            self.octave * 12 + semitone as u8
        };
        let mut rows = vec![[Step::default(); VOICES]; 64];
        rows[0][self.voice] = Step { note, instrument: 0, volume: 15 };
        let instrument = self.song.instruments[self.instrument].clone();
        let song = Song::from_steps(60, DEFAULT_PATTERN_ROWS, &rows, vec![instrument]).ok()?;
        Some(Self { song, ..Self::default() }.serialize("AUDITION.MUS"))
    }

    fn compile_song(&self) -> CompiledSong {
        #[derive(Clone, Copy)]
        struct State {
            active: bool,
            note: i16,
            instrument: usize,
            volume: u8,
            tick: usize,
        }
        let mut states =
            [State { active: false, note: 48, instrument: 0, volume: 15, tick: 0 }; VOICES];
        let mut frames = Vec::with_capacity(self.song.total_rows() * usize::from(self.song.tempo));
        for row in 0..self.song.total_rows() {
            let mut retrigger = [false; VOICES];
            for voice in 0..VOICES {
                let step = self.song.step(row, voice);
                if step.instrument != 0xff {
                    states[voice].instrument =
                        usize::from(step.instrument).min(self.song.instruments.len() - 1);
                    states[voice].tick = 0;
                }
                if step.volume != 0xff {
                    states[voice].volume = step.volume & 15;
                }
                if step.note == 0 {
                    states[voice].active = false;
                } else if step.note != 0xff {
                    states[voice].active = true;
                    states[voice].note = i16::from(step.note & !NOTE_LEGATO);
                    if step.note & NOTE_LEGATO == 0 {
                        states[voice].tick = 0;
                        retrigger[voice] = true;
                    }
                }
            }
            for tick_in_row in 0..self.song.tempo {
                let mut frame = [0_u8; 16];
                for voice in 0..VOICES {
                    let state = &mut states[voice];
                    if !state.active {
                        continue;
                    }
                    let instrument = &self.song.instruments[state.instrument];
                    let envelope_volume = instrument.volume.value(state.tick).clamp(0, 15) as u8;
                    let volume =
                        ((u16::from(state.volume) * u16::from(envelope_volume) + 7) / 15) as u8;
                    let note = (state.note + i16::from(instrument.arpeggio.value(state.tick)))
                        .clamp(1, 127) as u8;
                    let pitch = i32::from(instrument.pitch.value(state.tick));
                    let reset = u8::from(tick_in_row == 0 && retrigger[voice]);
                    let base = voice * 4;
                    match voice {
                        0 | 1 => {
                            let timer =
                                (i32::from(note_timer(note, false)) + pitch).clamp(0, 0x7ff) as u16;
                            let duty = instrument.tone.value(state.tick).clamp(0, 3) as u8;
                            frame[base] = 0x80 | duty << 5 | volume;
                            frame[base + 1] = timer as u8;
                            frame[base + 2] = (timer >> 8) as u8;
                            frame[base + 3] = reset;
                        }
                        2 => {
                            let timer =
                                (i32::from(note_timer(note, true)) + pitch).clamp(0, 0x7ff) as u16;
                            frame[base] = u8::from(volume != 0);
                            frame[base + 1] = timer as u8;
                            frame[base + 2] = (timer >> 8) as u8;
                            frame[base + 3] = reset;
                        }
                        _ => {
                            let period = i16::from(note.saturating_sub(1).min(15))
                                + i16::from(instrument.pitch.value(state.tick));
                            let short = u8::from(instrument.tone.value(state.tick) != 0);
                            frame[base] = 0x80 | short << 6 | volume;
                            frame[base + 1] = period.clamp(0, 15) as u8;
                            frame[base + 3] = reset;
                        }
                    }
                    state.tick += 1;
                }
                frames.push(frame);
            }
        }
        CompiledSong {
            frames,
            tracker_rows: self.song.total_rows(),
            ticks_per_row: self.song.tempo,
            loop_frame: self.song.loop_row.min(self.song.total_rows() - 1)
                * usize::from(self.song.tempo),
        }
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        if source.lines().any(|line| line.trim() == ";@FANTICON-MUSIC 2") {
            return Self::parse_v2(source);
        }
        Self::parse_v1(source)
    }

    fn parse_v1(source: &str) -> Result<Self, String> {
        if !source.lines().any(|line| line.trim() == ";@FANTICON-MUSIC 1") {
            return Err("FILE IS MISSING ;@FANTICON-MUSIC 1 OR 2".to_owned());
        }
        let tempo = parse_tempo(source)?;
        let row_count = metadata(source, ";@ROWS ")?
            .parse::<usize>()
            .map_err(|_| "MUSIC ROW COUNT MUST BE A NUMBER FROM 1 TO 255".to_owned())?;
        if !(1..=255).contains(&row_count) {
            return Err("MUSIC ROW COUNT MUST BE FROM 1 TO 255".to_owned());
        }
        let marker = source
            .lines()
            .position(|line| line.trim() == ";@DATA")
            .ok_or_else(|| "MUSIC FILE IS MISSING ;@DATA".to_owned())?;
        let mut bytes = Vec::with_capacity(row_count * 8);
        for line in source.lines().skip(marker + 1) {
            let code = line.split(';').next().unwrap_or("").trim();
            let Some(values) = code.strip_prefix("DFB ").or_else(|| code.strip_prefix("dfb "))
            else {
                continue;
            };
            for value in values.split(',') {
                bytes.push(parse_byte(value.trim())?);
            }
            if bytes.len() >= row_count * 8 {
                break;
            }
        }
        if bytes.len() != row_count * 8 {
            return Err(format!(
                "MUSIC DATA HAS {} BYTES; EXPECTED {}",
                bytes.len(),
                row_count * 8
            ));
        }
        let flat = bytes
            .chunks_exact(8)
            .map(|bytes| {
                let notes = core::array::from_fn(|voice| bytes[voice * 2]);
                let volumes = core::array::from_fn(|voice| bytes[voice * 2 + 1] & 15);
                (notes, volumes)
            })
            .collect::<Vec<_>>();
        Ok(Self { song: Song::from_flat(tempo, &flat), ..Self::default() })
    }

    fn parse_v2(source: &str) -> Result<Self, String> {
        let tempo = parse_tempo(source)?;
        let pattern_rows = metadata(source, ";@PATTERN-ROWS ")?
            .parse::<usize>()
            .map_err(|_| "PATTERN ROWS MUST BE A NUMBER".to_owned())?;
        if !(4..=64).contains(&pattern_rows) {
            return Err("PATTERN ROWS MUST BE FROM 4 TO 64".to_owned());
        }
        let mut frames: Vec<[u8; VOICES]> = Vec::new();
        let mut instruments = Vec::<Instrument>::new();
        let mut patterns: [Vec<Pattern>; VOICES] = core::array::from_fn(|_| Vec::new());
        let mut current_instrument = None;
        let mut current_pattern = None;
        for raw in source.lines() {
            let line = raw.trim();
            if let Some(values) = line.strip_prefix(";@FRAME ") {
                let mut parts = values.split_whitespace();
                let _number = parts.next();
                let values = parts
                    .next()
                    .ok_or_else(|| "FRAME IS MISSING FOUR PATTERN NUMBERS".to_owned())?;
                let parsed = values.split(',').map(parse_byte).collect::<Result<Vec<_>, _>>()?;
                if parsed.len() != VOICES {
                    return Err("EACH FRAME MUST SELECT FOUR PATTERNS".to_owned());
                }
                frames.push(parsed.try_into().unwrap());
                current_pattern = None;
            } else if let Some(value) = line.strip_prefix(";@INSTRUMENT ") {
                let index = parse_byte(value)? as usize;
                while instruments.len() <= index {
                    instruments.push(Instrument::default());
                }
                current_instrument = Some(index);
                current_pattern = None;
            } else if let Some(values) = line.strip_prefix(";@ENV ") {
                let index = current_instrument
                    .ok_or_else(|| "ENVELOPE APPEARS BEFORE AN INSTRUMENT".to_owned())?;
                let (kind, data) =
                    values.split_once(' ').ok_or_else(|| "INVALID ENVELOPE".to_owned())?;
                let (data, loop_text) = data.split_once(" LOOP ").unwrap_or((data, "--"));
                let sequence = data
                    .split(',')
                    .filter(|value| !value.is_empty())
                    .map(|value| {
                        value.parse::<i8>().map_err(|_| format!("INVALID ENVELOPE VALUE {value}"))
                    })
                    .collect::<Result<Vec<_>, _>>()?;
                if sequence.is_empty() || sequence.len() > 32 {
                    return Err("ENVELOPES MUST HAVE 1 TO 32 VALUES".to_owned());
                }
                let loop_at = if loop_text == "--" {
                    None
                } else {
                    Some(
                        loop_text
                            .parse::<usize>()
                            .map_err(|_| "INVALID ENVELOPE LOOP".to_owned())?,
                    )
                };
                let envelope = Envelope { values: sequence, loop_at };
                match kind {
                    "VOLUME" => instruments[index].volume = envelope,
                    "ARPEGGIO" => instruments[index].arpeggio = envelope,
                    "PITCH" => instruments[index].pitch = envelope,
                    "TONE" => instruments[index].tone = envelope,
                    _ => return Err(format!("UNKNOWN ENVELOPE {kind}")),
                }
            } else if let Some(values) = line.strip_prefix(";@PATTERN ") {
                let mut parts = values.split_whitespace();
                let voice = match parts.next() {
                    Some("P1") => 0,
                    Some("P2") => 1,
                    Some("TRI") => 2,
                    Some("NOI") => 3,
                    _ => return Err("INVALID PATTERN CHANNEL".to_owned()),
                };
                let pattern = parse_byte(
                    parts.next().ok_or_else(|| "PATTERN IS MISSING A NUMBER".to_owned())?,
                )? as usize;
                while patterns[voice].len() <= pattern {
                    patterns[voice].push(Pattern::empty(pattern_rows));
                }
                current_pattern = Some((voice, pattern));
                current_instrument = None;
            } else if let Some(values) = line.strip_prefix(";@STEP ") {
                let (voice, pattern) =
                    current_pattern.ok_or_else(|| "STEP APPEARS BEFORE A PATTERN".to_owned())?;
                let mut parts = values.split_whitespace();
                let row =
                    parse_byte(parts.next().ok_or_else(|| "STEP IS MISSING A ROW".to_owned())?)?
                        as usize;
                let values = parts
                    .next()
                    .ok_or_else(|| "STEP IS MISSING NOTE, INSTRUMENT, VOLUME".to_owned())?;
                let parsed = values.split(',').map(parse_byte).collect::<Result<Vec<_>, _>>()?;
                if row >= pattern_rows || parsed.len() != 3 {
                    return Err("INVALID PATTERN STEP".to_owned());
                }
                patterns[voice][pattern].steps[row] =
                    Step { note: parsed[0], instrument: parsed[1], volume: parsed[2] };
            }
        }
        if frames.is_empty() {
            return Err("MUSIC FILE HAS NO FRAMES".to_owned());
        }
        if instruments.is_empty() {
            instruments.push(Instrument::default());
        }
        for frame in &frames {
            for voice in 0..VOICES {
                while patterns[voice].len() <= usize::from(frame[voice]) {
                    patterns[voice].push(Pattern::empty(pattern_rows));
                }
            }
        }
        let loop_row = source
            .lines()
            .find_map(|line| line.trim().strip_prefix(";@LOOP-ROW "))
            .map(parse_index)
            .transpose()?
            .map_or(0, usize::from)
            .min(frames.len() * pattern_rows - 1);
        Ok(Self {
            song: Song { tempo, pattern_rows, loop_row, frames, patterns, instruments },
            ..Self::default()
        })
    }

    pub fn serialize(&self, filename: &str) -> String {
        let stem = filename
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("song.mus")
            .split('.')
            .next()
            .unwrap_or("song")
            .to_ascii_uppercase();
        let compiled = self.compile_song();
        let mut packets = Vec::new();
        let mut previous = [0_u8; 16];
        for (frame_index, frame) in compiled.frames.iter().enumerate() {
            let mut mask = if frame_index == compiled.loop_frame { 0x0f } else { 0 };
            for voice in 0..VOICES {
                if frame[voice * 4..voice * 4 + 4] != previous[voice * 4..voice * 4 + 4] {
                    mask |= 1 << voice;
                }
            }
            let mut packet = vec![mask];
            for voice in 0..VOICES {
                if mask & (1 << voice) != 0 {
                    packet.extend_from_slice(&frame[voice * 4..voice * 4 + 4]);
                }
            }
            packets.push(packet);
            previous = *frame;
        }
        let mut output = format!(
            ";@FANTICON-MUSIC 2\n; FANTICON TRACKER SOURCE\n;@TEMPO {}\n;@PATTERN-ROWS {}\n;@LOOP-ROW {:X}\n{stem}_MUSIC\n         DFB   $F2\n         DA    {}\n         DA    {}\n         DA    {stem}_STREAM\n         DA    {stem}_LOOP\n{stem}_STREAM\n",
            self.song.tempo,
            self.song.pattern_rows,
            compiled.loop_frame / usize::from(self.song.tempo),
            compiled.frames.len(),
            compiled.loop_frame,
        );
        for (frame_index, packet) in packets.iter().enumerate() {
            if frame_index == compiled.loop_frame {
                output.push_str(&format!("{stem}_LOOP\n"));
            }
            for bytes in packet.chunks(8) {
                output.push_str("         DFB   ");
                for (index, byte) in bytes.iter().enumerate() {
                    if index != 0 {
                        output.push(',');
                    }
                    output.push_str(&format!("${byte:02X}"));
                }
                output.push('\n');
            }
        }
        output.push_str("\n; AUTHORING DATA BELOW IS COMMENT METADATA.\n");
        for (index, frame) in self.song.frames.iter().enumerate() {
            output.push_str(&format!(
                ";@FRAME {index:02X} ${:02X},${:02X},${:02X},${:02X}\n",
                frame[0], frame[1], frame[2], frame[3]
            ));
        }
        for (index, instrument) in self.song.instruments.iter().enumerate() {
            output.push_str(&format!(";@INSTRUMENT {index:02X}\n"));
            for (name, envelope) in [
                ("VOLUME", &instrument.volume),
                ("ARPEGGIO", &instrument.arpeggio),
                ("PITCH", &instrument.pitch),
                ("TONE", &instrument.tone),
            ] {
                let values =
                    envelope.values.iter().map(i8::to_string).collect::<Vec<_>>().join(",");
                let loop_at =
                    envelope.loop_at.map_or_else(|| "--".to_owned(), |value| value.to_string());
                output.push_str(&format!(";@ENV {name} {values} LOOP {loop_at}\n"));
            }
        }
        for voice in 0..VOICES {
            for (number, pattern) in self.song.patterns[voice].iter().enumerate() {
                output.push_str(&format!(
                    ";@PATTERN {} {number:02X}\n",
                    ["P1", "P2", "TRI", "NOI"][voice]
                ));
                for (row, step) in pattern.steps.iter().enumerate() {
                    output.push_str(&format!(
                        ";@STEP {row:02X} ${:02X},${:02X},${:02X}\n",
                        step.note, step.instrument, step.volume
                    ));
                }
            }
        }
        output
    }

    pub fn handle_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        if modifiers.control_key() || modifiers.super_key() {
            return false;
        }
        if matches!(key, Key::Character(value) if value.eq_ignore_ascii_case("v")) {
            self.view = match self.view {
                View::Pattern => View::Frames,
                View::Frames => View::Instrument,
                View::Instrument => View::Pattern,
            };
            return false;
        }
        match self.view {
            View::Pattern => self.handle_pattern_key(key),
            View::Frames => self.handle_frame_key(key),
            View::Instrument => self.handle_instrument_key(key),
        }
    }

    fn handle_pattern_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::ArrowUp) => self.move_row(-1),
            Key::Named(NamedKey::ArrowDown) => self.move_row(1),
            Key::Named(NamedKey::PageUp) => self.move_row(-(self.song.pattern_rows as isize)),
            Key::Named(NamedKey::PageDown) => self.move_row(self.song.pattern_rows as isize),
            Key::Named(NamedKey::Home) => self.set_row(0),
            Key::Named(NamedKey::End) => self.set_row(self.song.total_rows() - 1),
            Key::Named(NamedKey::ArrowLeft) => {
                if self.field > 0 {
                    self.field -= 1
                } else if self.voice > 0 {
                    self.voice -= 1;
                    self.field = 2;
                }
            }
            Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::Tab) => {
                if self.field < 2 {
                    self.field += 1
                } else {
                    self.field = 0;
                    self.voice = (self.voice + 1) % VOICES;
                    if self.voice == 0 {
                        self.move_row(1);
                    }
                }
            }
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace) => {
                self.record_undo();
                let field = self.field;
                let step = self.song.step_mut(self.row, self.voice);
                match field {
                    0 => step.note = 0,
                    1 => step.instrument = 0xff,
                    _ => step.volume = 0xff,
                };
                return true;
            }
            Key::Character(text) => {
                let text = text.to_ascii_lowercase();
                if text == "z" {
                    self.octave = self.octave.saturating_sub(1).max(2);
                } else if text == "x" {
                    self.octave = (self.octave + 1).min(7);
                } else if text == "[" {
                    self.move_row(
                        -((self.row % self.song.pattern_rows + self.song.pattern_rows) as isize),
                    );
                } else if text == "]" {
                    self.move_row(
                        (self.song.pattern_rows - self.row % self.song.pattern_rows) as isize,
                    );
                } else if text == "+" || text == "=" {
                    self.record_undo();
                    self.song.tempo = self.song.tempo.saturating_add(1).min(60);
                    return true;
                } else if text == "_" {
                    self.record_undo();
                    self.song.tempo = self.song.tempo.saturating_sub(1).max(1);
                    return true;
                } else if text == "-" || text == "." {
                    self.record_undo();
                    let field = self.field;
                    let step = self.song.step_mut(self.row, self.voice);
                    match field {
                        0 => step.note = 0xff,
                        1 => step.instrument = 0xff,
                        _ => step.volume = 0xff,
                    };
                    return true;
                } else if self.field > 0 {
                    if let Some(value) = text.chars().next().and_then(|value| value.to_digit(16)) {
                        self.record_undo();
                        let field = self.field;
                        let step = self.song.step_mut(self.row, self.voice);
                        if field == 1 {
                            step.instrument = value as u8;
                        } else {
                            step.volume = value as u8;
                        }
                        self.move_row(1);
                        return true;
                    }
                } else if let Some(semitone) = piano_key(&text) {
                    self.record_undo();
                    let octave = self.octave;
                    let voice = self.voice;
                    self.song.step_mut(self.row, voice).note = if voice == 3 {
                        1 + semitone.min(15) as u8
                    } else {
                        octave * 12 + semitone as u8
                    };
                    self.move_row(1);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn handle_frame_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::ArrowUp) => {
                self.frame_cursor = self.frame_cursor.saturating_sub(1)
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.frame_cursor = (self.frame_cursor + 1).min(self.song.frames.len() - 1)
            }
            Key::Named(NamedKey::ArrowLeft) => self.voice = self.voice.saturating_sub(1),
            Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::Tab) => {
                self.voice = (self.voice + 1) % VOICES
            }
            Key::Named(NamedKey::Insert) => {
                self.record_undo();
                let frame = self.song.frames[self.frame_cursor];
                self.song.frames.insert(self.frame_cursor + 1, frame);
                self.frame_cursor += 1;
                return true;
            }
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)
                if self.song.frames.len() > 1 =>
            {
                self.record_undo();
                self.song.frames.remove(self.frame_cursor);
                self.frame_cursor = self.frame_cursor.min(self.song.frames.len() - 1);
                return true;
            }
            Key::Character(value) => {
                let value = value.to_ascii_lowercase();
                if value == "+" || value == "=" {
                    self.record_undo();
                    self.change_pattern(1);
                    return true;
                }
                if value == "-" {
                    self.record_undo();
                    self.change_pattern(-1);
                    return true;
                }
                if value == "l" {
                    self.record_undo();
                    self.song.loop_row = self.frame_cursor * self.song.pattern_rows;
                    return true;
                }
                if let Some(hex) = value.chars().next().and_then(|value| value.to_digit(16)) {
                    self.record_undo();
                    self.song.frames[self.frame_cursor][self.voice] = hex as u8;
                    self.song.ensure_pattern(self.voice, hex as usize);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    fn handle_instrument_key(&mut self, key: &Key) -> bool {
        match key {
            Key::Named(NamedKey::ArrowUp) => {
                self.env_kind = match self.env_kind {
                    EnvKind::Volume => EnvKind::Tone,
                    EnvKind::Arpeggio => EnvKind::Volume,
                    EnvKind::Pitch => EnvKind::Arpeggio,
                    EnvKind::Tone => EnvKind::Pitch,
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.env_kind = match self.env_kind {
                    EnvKind::Volume => EnvKind::Arpeggio,
                    EnvKind::Arpeggio => EnvKind::Pitch,
                    EnvKind::Pitch => EnvKind::Tone,
                    EnvKind::Tone => EnvKind::Volume,
                }
            }
            Key::Named(NamedKey::ArrowLeft) => self.env_cursor = self.env_cursor.saturating_sub(1),
            Key::Named(NamedKey::ArrowRight) | Key::Named(NamedKey::Tab) => {
                self.env_cursor = (self.env_cursor + 1).min(self.envelope().values.len() - 1)
            }
            Key::Named(NamedKey::Insert) => {
                self.record_undo();
                let cursor = self.env_cursor;
                let value = self.envelope().values[cursor];
                self.envelope_mut().values.insert(cursor + 1, value);
                self.env_cursor += 1;
                return true;
            }
            Key::Named(NamedKey::Delete) | Key::Named(NamedKey::Backspace)
                if self.envelope().values.len() > 1 =>
            {
                self.record_undo();
                let cursor = self.env_cursor;
                self.envelope_mut().values.remove(cursor);
                self.env_cursor = self.env_cursor.min(self.envelope().values.len() - 1);
                return true;
            }
            Key::Character(value) => {
                let value = value.to_ascii_lowercase();
                if value == "[" {
                    self.instrument = self.instrument.saturating_sub(1);
                    self.env_cursor = 0;
                } else if value == "]" {
                    self.record_undo();
                    self.instrument += 1;
                    while self.song.instruments.len() <= self.instrument {
                        self.song.instruments.push(Instrument::default());
                    }
                    self.env_cursor = 0;
                    return true;
                } else if value == "l" {
                    self.record_undo();
                    let cursor = self.env_cursor;
                    let envelope = self.envelope_mut();
                    envelope.loop_at =
                        if envelope.loop_at == Some(cursor) { None } else { Some(cursor) };
                    return true;
                } else if value == "+" || value == "=" {
                    self.record_undo();
                    self.adjust_envelope(1);
                    return true;
                } else if value == "-" {
                    self.record_undo();
                    self.adjust_envelope(-1);
                    return true;
                } else if matches!(self.env_kind, EnvKind::Volume | EnvKind::Tone)
                    && let Some(hex) = value.chars().next().and_then(|value| value.to_digit(16))
                {
                    self.record_undo();
                    let max = if self.env_kind == EnvKind::Tone { 3 } else { 15 };
                    let cursor = self.env_cursor;
                    self.envelope_mut().values[cursor] = (hex as i8).min(max);
                    return true;
                }
            }
            _ => {}
        }
        false
    }

    pub fn undo(&mut self) -> bool {
        let Some(song) = self.undo.pop() else { return false };
        self.song = song;
        self.set_row(self.row);
        true
    }

    pub fn handle_mouse_press(&mut self, x: usize, y: usize) -> bool {
        let column = x.saturating_sub(PANE_LEFT) / GLYPH_WIDTH;
        let line = y.saturating_sub(PANE_TOP) / GLYPH_HEIGHT;
        if line == 0 {
            if (2..11).contains(&column) {
                self.view = View::Pattern;
            } else if (12..20).contains(&column) {
                self.view = View::Frames;
            } else if (21..33).contains(&column) {
                self.view = View::Instrument;
            }
            return false;
        }
        match self.view {
            View::Pattern => {
                if line == 4 {
                    if (2..6).contains(&column) {
                        self.move_row(-(self.song.pattern_rows as isize));
                    } else if (6..10).contains(&column) {
                        self.move_row(self.song.pattern_rows as isize);
                    } else if (14..18).contains(&column) {
                        self.record_undo();
                        self.song.tempo = self.song.tempo.saturating_sub(1).max(1);
                        return true;
                    } else if (18..22).contains(&column) {
                        self.record_undo();
                        self.song.tempo = self.song.tempo.saturating_add(1).min(60);
                        return true;
                    } else if (27..31).contains(&column) {
                        self.octave = self.octave.saturating_sub(1).max(2);
                    } else if (31..35).contains(&column) {
                        self.octave = (self.octave + 1).min(7);
                    }
                    return false;
                }
                if line == 43 && (2..50).contains(&column) {
                    let semitone = (column - 2) / 4;
                    self.record_undo();
                    let voice = self.voice;
                    let octave = self.octave;
                    self.song.step_mut(self.row, voice).note = if voice == 3 {
                        semitone.min(15) as u8 + 1
                    } else {
                        octave * 12 + semitone as u8
                    };
                    self.move_row(1);
                    return true;
                }
                if line == 44 {
                    if (2..9).contains(&column) {
                        self.record_undo();
                        self.song.step_mut(self.row, self.voice).note = 0;
                        return true;
                    }
                    if (10..18).contains(&column) {
                        self.record_undo();
                        self.song.step_mut(self.row, self.voice).note = 0xff;
                        return true;
                    }
                    if (25..29).contains(&column) || (29..33).contains(&column) {
                        let current = self.song.step(self.row, self.voice).instrument;
                        self.record_undo();
                        self.song.step_mut(self.row, self.voice).instrument = if column < 29 {
                            current.saturating_sub(1)
                        } else {
                            current.saturating_add(1).min(15)
                        };
                        return true;
                    }
                    if (39..43).contains(&column) || (43..47).contains(&column) {
                        let current = self.song.step(self.row, self.voice).volume;
                        self.record_undo();
                        self.song.step_mut(self.row, self.voice).volume = if column < 43 {
                            current.min(15).saturating_sub(1)
                        } else if current == 0xff {
                            15
                        } else {
                            current.saturating_add(1).min(15)
                        };
                        return true;
                    }
                }
                let top = PANE_TOP + 6 * GLYPH_HEIGHT;
                if y < top || y >= top + VISIBLE_ROWS * GLYPH_HEIGHT {
                    return false;
                }
                let clicked = self.display_start() + ((y - top) / GLYPH_HEIGHT) as isize;
                if clicked < 0 || clicked >= self.song.total_rows() as isize {
                    return false;
                }
                self.set_row(clicked as usize);
                let voice = x.saturating_sub(PANE_LEFT + 6 * GLYPH_WIDTH) / (11 * GLYPH_WIDTH);
                self.voice = voice.min(VOICES - 1);
                let local =
                    x.saturating_sub(PANE_LEFT + (6 + self.voice * 11) * GLYPH_WIDTH) / GLYPH_WIDTH;
                self.field = if local < 4 {
                    0
                } else if local < 7 {
                    1
                } else {
                    2
                };
                false
            }
            View::Frames => {
                if line == 3 {
                    if (2..8).contains(&column) {
                        self.record_undo();
                        self.song.frames.push([0; VOICES]);
                        self.frame_cursor = self.song.frames.len() - 1;
                        return true;
                    }
                    if (8..15).contains(&column) {
                        self.record_undo();
                        let frame = self.song.frames[self.frame_cursor];
                        self.song.frames.insert(self.frame_cursor + 1, frame);
                        self.frame_cursor += 1;
                        return true;
                    }
                    if (15..23).contains(&column) && self.song.frames.len() > 1 {
                        self.record_undo();
                        self.song.frames.remove(self.frame_cursor);
                        self.frame_cursor = self.frame_cursor.min(self.song.frames.len() - 1);
                        return true;
                    }
                    if (28..32).contains(&column) {
                        self.record_undo();
                        self.change_pattern(-1);
                        return true;
                    }
                    if (32..36).contains(&column) {
                        self.record_undo();
                        self.change_pattern(1);
                        return true;
                    }
                    if (38..49).contains(&column) {
                        self.record_undo();
                        self.song.loop_row = self.frame_cursor * self.song.pattern_rows;
                        return true;
                    }
                }
                let start = self.frame_cursor.saturating_sub(14);
                if (5..35).contains(&line) {
                    let frame = start + line - 5;
                    if frame < self.song.frames.len() {
                        self.frame_cursor = frame;
                        for voice in 0..VOICES {
                            if (10 + voice * 10..14 + voice * 10).contains(&column) {
                                self.voice = voice;
                            }
                        }
                    }
                }
                false
            }
            View::Instrument => {
                if line == 4 {
                    if (2..6).contains(&column) {
                        self.instrument = self.instrument.saturating_sub(1);
                        self.env_cursor = 0;
                    } else if (6..10).contains(&column) {
                        self.record_undo();
                        self.instrument += 1;
                        while self.song.instruments.len() <= self.instrument {
                            self.song.instruments.push(Instrument::default());
                        }
                        self.env_cursor = 0;
                        return true;
                    }
                }
                if line == 35 {
                    if (2..12).contains(&column) {
                        self.record_undo();
                        let cursor = self.env_cursor;
                        let value = self.envelope().values[cursor];
                        self.envelope_mut().values.insert(cursor + 1, value);
                        self.env_cursor += 1;
                        return true;
                    }
                    if (13..23).contains(&column) && self.envelope().values.len() > 1 {
                        self.record_undo();
                        let cursor = self.env_cursor;
                        self.envelope_mut().values.remove(cursor);
                        self.env_cursor = self.env_cursor.min(self.envelope().values.len() - 1);
                        return true;
                    }
                    if (24..32).contains(&column) {
                        self.record_undo();
                        let cursor = self.env_cursor;
                        let envelope = self.envelope_mut();
                        envelope.loop_at =
                            if envelope.loop_at == Some(cursor) { None } else { Some(cursor) };
                        return true;
                    }
                    if (33..37).contains(&column) || (37..41).contains(&column) {
                        self.record_undo();
                        self.adjust_envelope(if column < 37 { -1 } else { 1 });
                        return true;
                    }
                }
                for (index, kind) in
                    [EnvKind::Volume, EnvKind::Arpeggio, EnvKind::Pitch, EnvKind::Tone]
                        .into_iter()
                        .enumerate()
                {
                    let base_line = 6 + index * 7;
                    if (base_line..base_line + 5).contains(&line) {
                        self.env_kind = kind;
                        if (12..48).contains(&column) {
                            self.record_undo();
                            self.mouse_dragging_envelope = true;
                            return self.set_envelope_from_mouse(x, y);
                        }
                    }
                }
                false
            }
        }
    }

    pub fn play_button_hit(&self, x: usize, y: usize) -> bool {
        let column = x.saturating_sub(PANE_LEFT) / GLYPH_WIDTH;
        let line = y.saturating_sub(PANE_TOP) / GLYPH_HEIGHT;
        line == 0 && (43..54).contains(&column)
    }

    pub fn handle_mouse_move(&mut self, x: usize, y: usize) -> bool {
        self.mouse_dragging_envelope && self.set_envelope_from_mouse(x, y)
    }

    pub fn handle_mouse_release(&mut self) {
        self.mouse_dragging_envelope = false;
    }

    pub fn handle_mouse_wheel(&mut self, vertical: isize) {
        match self.view {
            View::Pattern => self.move_row(-vertical),
            View::Frames => {
                self.frame_cursor = self
                    .frame_cursor
                    .saturating_add_signed(-vertical)
                    .min(self.song.frames.len() - 1);
            }
            View::Instrument => {
                self.env_cursor = self
                    .env_cursor
                    .saturating_add_signed(-vertical)
                    .min(self.envelope().values.len() - 1);
            }
        }
    }

    pub fn status(&self) -> String {
        match self.view {
            View::Pattern => format!(
                " MUSIC ROW {:03X}/{}  {}  {}  OCT {}  TEMPO {}",
                self.row,
                self.song.total_rows(),
                ["PULSE 1", "PULSE 2", "TRIANGLE", "NOISE"][self.voice],
                ["NOTE", "INSTR", "VOLUME"][self.field],
                self.octave,
                self.song.tempo
            ),
            View::Frames => format!(
                " MUSIC FRAME {:02X}/{}  {} PATTERN {:02X}",
                self.frame_cursor,
                self.song.frames.len(),
                ["PULSE 1", "PULSE 2", "TRIANGLE", "NOISE"][self.voice],
                self.song.frames[self.frame_cursor][self.voice]
            ),
            View::Instrument => format!(
                " MUSIC INSTRUMENT {:X}  {:?} STEP {}",
                self.instrument, self.env_kind, self.env_cursor
            ),
        }
    }

    pub fn follow_playback(&mut self, row: Option<usize>, levels: [u8; VOICES]) {
        self.playback_row = row.filter(|row| *row < self.song.total_rows());
        self.playback_levels = if self.playback_row.is_some() { levels } else { [0; VOICES] };
        if let Some(row) = self.playback_row {
            self.set_row(row);
        }
    }

    #[cfg(test)]
    pub fn playback_view(&self) -> (usize, Option<usize>, core::ops::Range<usize>) {
        (self.row, self.playback_row, self.scroll..self.scroll + VISIBLE_ROWS)
    }

    pub fn render(&self, video: &mut Video) {
        configure_palette(video);
        button(video, 43, 0, " PLAY/STOP ", GRAD_WHITE);
        button(
            video,
            2,
            0,
            " PATTERN ",
            if self.view == View::Pattern { GRAD_WHITE } else { GRAD_CHROME },
        );
        button(
            video,
            12,
            0,
            " FRAMES ",
            if self.view == View::Frames { GRAD_WHITE } else { GRAD_CHROME },
        );
        button(
            video,
            21,
            0,
            " INSTRUMENT ",
            if self.view == View::Instrument { GRAD_WHITE } else { GRAD_CHROME },
        );
        match self.view {
            View::Pattern => self.render_patterns(video),
            View::Frames => self.render_frames(video),
            View::Instrument => self.render_instrument(video),
        }
    }

    fn render_patterns(&self, video: &mut Video) {
        let frame = self.row / self.song.pattern_rows;
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 3 * GLYPH_HEIGHT,
            &format!(
                "FRAME {frame:02X}/{:02X}  PAT {:02X} {:02X} {:02X} {:02X}  SPEED {}",
                self.song.frames.len() - 1,
                self.song.frames[frame][0],
                self.song.frames[frame][1],
                self.song.frames[frame][2],
                self.song.frames[frame][3],
                self.song.tempo
            ),
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 2, 4, " < ", GRAD_WHITE);
        button(video, 6, 4, " > ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 11 * GLYPH_WIDTH,
            PANE_TOP + 4 * GLYPH_HEIGHT,
            "SPEED",
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 14, 4, " - ", GRAD_WHITE);
        button(video, 18, 4, " + ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 23 * GLYPH_WIDTH,
            PANE_TOP + 4 * GLYPH_HEIGHT,
            "OCT",
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 27, 4, " - ", GRAD_WHITE);
        button(video, 31, 4, " + ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + GLYPH_WIDTH,
            PANE_TOP + 5 * GLYPH_HEIGHT,
            "ROW",
            GRAD_CHROME,
            UI_BLACK,
        );
        for (voice, label) in ["PULSE 1", "PULSE 2", "TRIANGLE", "NOISE"].into_iter().enumerate() {
            channel_meter_header(
                video,
                6 + voice * 11,
                5,
                10,
                label,
                voice,
                self.playback_levels[voice],
            );
        }
        for (semitone, label) in ["C", "C#", "D", "D#", "E", "F", "F#", "G", "G#", "A", "A#", "B"]
            .into_iter()
            .enumerate()
        {
            button(
                video,
                2 + semitone * 4,
                43,
                &format!("{label:^3}"),
                CHANNEL_GRADIENTS[self.voice],
            );
        }
        button(video, 2, 44, " OFF  ", GRAD_WHITE);
        button(video, 10, 44, " HOLD  ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 20 * GLYPH_WIDTH,
            PANE_TOP + 44 * GLYPH_HEIGHT,
            "INST",
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 25, 44, " - ", GRAD_WHITE);
        button(video, 29, 44, " + ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 35 * GLYPH_WIDTH,
            PANE_TOP + 44 * GLYPH_HEIGHT,
            "VOL",
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 39, 44, " - ", GRAD_WHITE);
        button(video, 43, 44, " + ", GRAD_WHITE);
        for screen_row in 0..VISIBLE_ROWS {
            let row = self.display_start() + screen_row as isize;
            if row < 0 {
                continue;
            }
            let row = row as usize;
            if row >= self.song.total_rows() {
                break;
            }
            let y = PANE_TOP + (6 + screen_row) * GLYPH_HEIGHT;
            let playing = self.playback_row == Some(row);
            let row_width = EDITOR_DISPLAY_WIDTH - PANE_LEFT - 10;
            // Rows are one flat list, so shade alternate pattern regions and rule
            // off each boundary to show where one frame's patterns end.
            let region = row / self.song.pattern_rows;
            let region_start = row.is_multiple_of(self.song.pattern_rows);
            let background = match (playing, region.is_multiple_of(2)) {
                (true, _) => PLAYING_BG,
                (false, true) => UI_BLACK,
                (false, false) => REGION_BG,
            };
            fill(video, PANE_LEFT + 1, y, row_width, GLYPH_HEIGHT, background);
            if region_start {
                text(
                    video,
                    PANE_LEFT + 51 * GLYPH_WIDTH,
                    y,
                    &format!("F{region:02X}"),
                    GRAD_WHITE,
                    background,
                );
            }
            text(
                video,
                PANE_LEFT + GLYPH_WIDTH,
                y,
                &format!("{row:03X}"),
                if playing { GRAD_WHITE } else { GRAD_CHROME },
                background,
            );
            for (voice, channel_gradient) in CHANNEL_GRADIENTS.iter().copied().enumerate() {
                let step = self.song.step(row, voice);
                let x = PANE_LEFT + (6 + voice * 11) * GLYPH_WIDTH;
                let selected = !playing && row == self.row && voice == self.voice;
                let bg = if selected { SELECT_BG } else { background };
                let fg = if playing { GRAD_WHITE } else { channel_gradient };
                text(
                    video,
                    x,
                    y,
                    &format!(
                        "{} {} {}",
                        display_note(step.note, voice),
                        display_hex(step.instrument),
                        display_nibble(step.volume)
                    ),
                    fg,
                    bg,
                );
            }
            // Drawn last: the glyph cells above would otherwise paint over it.
            // The playing row keeps its own highlight unbroken.
            if region_start && !playing {
                fill(video, PANE_LEFT + 1, y, row_width, 1, REGION_RULE);
            }
        }
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 40 * GLYPH_HEIGHT,
            "NOTE  INST VOL   V=VIEW  [ ]=FRAME  +/-=SPEED",
            GRAD_CHROME,
            UI_BLACK,
        );
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 41 * GLYPH_HEIGHT,
            "SPACE PLAY/STOP  Z/X OCTAVE  DEL OFF  - HOLD",
            GRAD_CHROME,
            UI_BLACK,
        );
    }

    fn render_frames(&self, video: &mut Video) {
        button(video, 2, 3, " ADD ", GRAD_WHITE);
        button(video, 8, 3, " DUP  ", GRAD_WHITE);
        button(video, 15, 3, " DELETE ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 24 * GLYPH_WIDTH,
            PANE_TOP + 3 * GLYPH_HEIGHT,
            "PAT",
            GRAD_CHROME,
            UI_BLACK,
        );
        button(video, 28, 3, " - ", GRAD_WHITE);
        button(video, 32, 3, " + ", GRAD_WHITE);
        button(video, 38, 3, " SET LOOP ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 4 * GLYPH_HEIGHT,
            "ORDER",
            GRAD_CHROME,
            UI_BLACK,
        );
        for (voice, label) in ["PULSE 1", "PULSE 2", "TRIANGLE", "NOISE"].into_iter().enumerate() {
            channel_meter_header(
                video,
                9 + voice * 10,
                4,
                9,
                label,
                voice,
                self.playback_levels[voice],
            );
        }
        let start = self.frame_cursor.saturating_sub(14);
        for (screen, frame) in self.song.frames.iter().enumerate().skip(start).take(30) {
            let y = PANE_TOP + (5 + screen - start) * GLYPH_HEIGHT;
            text(
                video,
                PANE_LEFT + 2 * GLYPH_WIDTH,
                y,
                &format!("{screen:02X}"),
                GRAD_CHROME,
                UI_BLACK,
            );
            if self.song.loop_row / self.song.pattern_rows == screen {
                text(video, PANE_LEFT + 5 * GLYPH_WIDTH, y, "L", GRAD_WHITE, UI_BLACK);
            }
            for voice in 0..VOICES {
                let selected = screen == self.frame_cursor && voice == self.voice;
                text(
                    video,
                    PANE_LEFT + (10 + voice * 10) * GLYPH_WIDTH,
                    y,
                    &format!("{:02X}", frame[voice]),
                    CHANNEL_GRADIENTS[voice],
                    if selected { SELECT_BG } else { UI_BLACK },
                );
            }
        }
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 38 * GLYPH_HEIGHT,
            "EACH FRAME SELECTS ONE PATTERN PER CHANNEL.",
            GRAD_CHROME,
            UI_BLACK,
        );
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 39 * GLYPH_HEIGHT,
            "HEX/+/- PATTERN  INS DUPLICATE  DEL REMOVE",
            GRAD_CHROME,
            UI_BLACK,
        );
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 40 * GLYPH_HEIGHT,
            "V=NEXT VIEW",
            GRAD_CHROME,
            UI_BLACK,
        );
    }

    fn render_instrument(&self, video: &mut Video) {
        button(video, 2, 4, " < ", GRAD_WHITE);
        button(video, 6, 4, " > ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 11 * GLYPH_WIDTH,
            PANE_TOP + 4 * GLYPH_HEIGHT,
            &format!("INSTRUMENT {:X}   CLICK/DRAG GRAPHS", self.instrument),
            GRAD_WHITE,
            UI_BLACK,
        );
        for (line, (kind, name)) in [
            (EnvKind::Volume, "VOLUME"),
            (EnvKind::Arpeggio, "ARPEGGIO"),
            (EnvKind::Pitch, "PITCH"),
            (EnvKind::Tone, "TONE/DUTY"),
        ]
        .into_iter()
        .enumerate()
        {
            let envelope = self.envelope_for(kind);
            let base_line = 6 + line * 7;
            let y = PANE_TOP + base_line * GLYPH_HEIGHT;
            text(
                video,
                PANE_LEFT + 2 * GLYPH_WIDTH,
                y,
                name,
                if kind == self.env_kind { GRAD_WHITE } else { GRAD_CHROME },
                UI_BLACK,
            );
            let start = if kind == self.env_kind {
                self.env_cursor.saturating_sub(4).min(envelope.values.len().saturating_sub(9))
            } else {
                0
            };
            let graph_x = PANE_LEFT + 12 * GLYPH_WIDTH;
            let graph_y = y + GLYPH_HEIGHT;
            let graph_height = 4 * GLYPH_HEIGHT;
            fill(video, graph_x, graph_y, 36 * GLYPH_WIDTH, graph_height, SELECT_BG);
            let zero_y = graph_y + 15 * (graph_height - 1) / 31;
            if matches!(kind, EnvKind::Arpeggio | EnvKind::Pitch) {
                fill(video, graph_x, zero_y, 36 * GLYPH_WIDTH, 1, GRAD_CHROME);
            }
            for (slot, (index, value)) in
                envelope.values.iter().enumerate().skip(start).take(9).enumerate()
            {
                let value_y = match kind {
                    EnvKind::Volume => {
                        graph_y + (15 - (*value).clamp(0, 15) as usize) * (graph_height - 1) / 15
                    }
                    EnvKind::Tone => {
                        graph_y + (3 - (*value).clamp(0, 3) as usize) * (graph_height - 1) / 3
                    }
                    EnvKind::Arpeggio | EnvKind::Pitch => {
                        graph_y
                            + (15 - i16::from(*value).clamp(-16, 15)) as usize * (graph_height - 1)
                                / 31
                    }
                };
                let bottom = if matches!(kind, EnvKind::Arpeggio | EnvKind::Pitch) {
                    zero_y
                } else {
                    graph_y + graph_height - 1
                };
                let top = value_y.min(bottom);
                let height = value_y.abs_diff(bottom).max(1);
                let bar_x = graph_x + slot * 4 * GLYPH_WIDTH + 4;
                fill(video, bar_x, top, 3 * GLYPH_WIDTH, height, CHANNEL_GRADIENTS[line]);
                if kind == self.env_kind && index == self.env_cursor {
                    frame(
                        video,
                        graph_x + slot * 4 * GLYPH_WIDTH,
                        graph_y,
                        4 * GLYPH_WIDTH,
                        graph_height,
                    );
                }
                text(
                    video,
                    graph_x + slot * 4 * GLYPH_WIDTH,
                    y + 5 * GLYPH_HEIGHT,
                    &format!(
                        "{:>3}{}",
                        value,
                        if envelope.loop_at == Some(index) { "L" } else { " " }
                    ),
                    CHANNEL_GRADIENTS[line],
                    UI_BLACK,
                );
            }
        }
        button(video, 2, 35, " ADD STEP ", GRAD_WHITE);
        button(video, 13, 35, " DEL STEP ", GRAD_WHITE);
        button(video, 24, 35, " LOOP   ", GRAD_WHITE);
        button(video, 33, 35, " - ", GRAD_WHITE);
        button(video, 37, 35, " + ", GRAD_WHITE);
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 36 * GLYPH_HEIGHT,
            "CLICK/DRAG GRAPH OR USE BUTTONS  V=NEXT VIEW",
            GRAD_CHROME,
            UI_BLACK,
        );
        text(
            video,
            PANE_LEFT + 2 * GLYPH_WIDTH,
            PANE_TOP + 38 * GLYPH_HEIGHT,
            "TONE: PULSE DUTY 0-3 / NOISE MODE 0-1",
            GRAD_CHROME,
            UI_BLACK,
        );
    }

    fn record_undo(&mut self) {
        if self.undo.len() == 64 {
            self.undo.remove(0);
        }
        self.undo.push(self.song.clone());
    }
    fn move_row(&mut self, amount: isize) {
        self.set_row(self.row.saturating_add_signed(amount).min(self.song.total_rows() - 1));
    }
    fn set_row(&mut self, row: usize) {
        self.row = row.min(self.song.total_rows() - 1);
        let max = self.song.total_rows().saturating_sub(VISIBLE_ROWS);
        self.scroll = self.row.saturating_sub(VISIBLE_ROWS / 2).min(max);
        self.frame_cursor = self.row / self.song.pattern_rows;
    }
    fn display_start(&self) -> isize {
        self.playback_row
            .map_or(self.scroll as isize, |row| row as isize - VISIBLE_ROWS as isize / 2)
    }
    fn change_pattern(&mut self, amount: i8) {
        let value = self.song.frames[self.frame_cursor][self.voice].saturating_add_signed(amount);
        self.song.frames[self.frame_cursor][self.voice] = value;
        self.song.ensure_pattern(self.voice, usize::from(value));
    }
    fn set_envelope_from_mouse(&mut self, x: usize, y: usize) -> bool {
        let kind_index = match self.env_kind {
            EnvKind::Volume => 0,
            EnvKind::Arpeggio => 1,
            EnvKind::Pitch => 2,
            EnvKind::Tone => 3,
        };
        let graph_x = PANE_LEFT + 12 * GLYPH_WIDTH;
        let graph_y = PANE_TOP + (7 + kind_index * 7) * GLYPH_HEIGHT;
        let start =
            self.env_cursor.saturating_sub(4).min(self.envelope().values.len().saturating_sub(9));
        let slot = x.saturating_sub(graph_x) / (4 * GLYPH_WIDTH);
        let cursor = (start + slot.min(8)).min(self.envelope().values.len() - 1);
        let vertical = y.saturating_sub(graph_y).min(4 * GLYPH_HEIGHT - 1);
        let value = match self.env_kind {
            EnvKind::Volume => 15 - (vertical * 15 / (4 * GLYPH_HEIGHT - 1)) as i8,
            EnvKind::Tone => 3 - (vertical * 3 / (4 * GLYPH_HEIGHT - 1)) as i8,
            EnvKind::Arpeggio | EnvKind::Pitch => {
                15 - (vertical * 31 / (4 * GLYPH_HEIGHT - 1)) as i8
            }
        };
        self.env_cursor = cursor;
        let changed = self.envelope().values[cursor] != value;
        self.envelope_mut().values[cursor] = value;
        changed
    }
    fn envelope_for(&self, kind: EnvKind) -> &Envelope {
        let instrument = &self.song.instruments[self.instrument];
        match kind {
            EnvKind::Volume => &instrument.volume,
            EnvKind::Arpeggio => &instrument.arpeggio,
            EnvKind::Pitch => &instrument.pitch,
            EnvKind::Tone => &instrument.tone,
        }
    }
    fn envelope(&self) -> &Envelope {
        self.envelope_for(self.env_kind)
    }
    fn envelope_mut(&mut self) -> &mut Envelope {
        let instrument = &mut self.song.instruments[self.instrument];
        match self.env_kind {
            EnvKind::Volume => &mut instrument.volume,
            EnvKind::Arpeggio => &mut instrument.arpeggio,
            EnvKind::Pitch => &mut instrument.pitch,
            EnvKind::Tone => &mut instrument.tone,
        }
    }
    fn adjust_envelope(&mut self, amount: i8) {
        let kind = self.env_kind;
        let cursor = self.env_cursor;
        let value = &mut self.envelope_mut().values[cursor];
        *value = match kind {
            EnvKind::Volume => value.saturating_add(amount).clamp(0, 15),
            EnvKind::Tone => value.saturating_add(amount).clamp(0, 3),
            _ => value.saturating_add(amount),
        };
    }
}

fn parse_tempo(source: &str) -> Result<u8, String> {
    let tempo = metadata(source, ";@TEMPO ")?
        .parse::<u8>()
        .map_err(|_| "MUSIC TEMPO MUST BE A NUMBER FROM 1 TO 60".to_owned())?;
    if !(1..=60).contains(&tempo) {
        return Err("MUSIC TEMPO MUST BE FROM 1 TO 60 FRAMES PER ROW".to_owned());
    }
    Ok(tempo)
}
fn metadata<'a>(source: &'a str, prefix: &str) -> Result<&'a str, String> {
    source
        .lines()
        .find_map(|line| line.trim().strip_prefix(prefix))
        .ok_or_else(|| format!("MUSIC FILE IS MISSING {prefix}"))
}
fn parse_byte(value: &str) -> Result<u8, String> {
    let value = value.trim();
    let parsed = if let Some(hex) = value.strip_prefix('$') {
        u8::from_str_radix(hex, 16)
    } else {
        u8::from_str_radix(value, 16).or_else(|_| value.parse())
    };
    parsed.map_err(|_| format!("INVALID MUSIC BYTE {value}"))
}
fn parse_index(value: &str) -> Result<usize, String> {
    let value = value.trim().trim_start_matches('$');
    usize::from_str_radix(value, 16).map_err(|_| format!("INVALID MUSIC INDEX {value}"))
}
fn piano_key(key: &str) -> Option<usize> {
    Some(match key {
        "a" => 0,
        "w" => 1,
        "s" => 2,
        "e" => 3,
        "d" => 4,
        "f" => 5,
        "t" => 6,
        "g" => 7,
        "y" => 8,
        "h" => 9,
        "u" => 10,
        "j" => 11,
        _ => return None,
    })
}
fn display_note(note: u8, voice: usize) -> String {
    if note == 0 {
        return "OFF".to_owned();
    }
    if note == 0xff {
        return "---".to_owned();
    }
    let note = note & !NOTE_LEGATO;
    if voice == 3 {
        return format!("N{:02X}", note.saturating_sub(1));
    }
    let names = ["C-", "C#", "D-", "D#", "E-", "F-", "F#", "G-", "G#", "A-", "A#", "B-"];
    format!("{}{}", names[usize::from(note % 12)], note / 12)
}
fn display_hex(value: u8) -> String {
    if value == 0xff { "--".to_owned() } else { format!("{value:02X}") }
}
fn display_nibble(value: u8) -> String {
    if value == 0xff { "-".to_owned() } else { format!("{:X}", value & 15) }
}
fn note_timer(note: u8, triangle: bool) -> u16 {
    let frequency = 16.351_597_831_287_414 * 2_f64.powf(f64::from(note) / 12.0);
    let divisor = if triangle { 32.0 } else { 16.0 };
    ((CPU_CLOCK_HZ / (divisor * frequency) - 1.0).round() as i64).clamp(0, 0x7ff) as u16
}

fn timer_to_note(timer: u16, triangle: bool, source_rate: u32) -> u8 {
    let divisor = if triangle { 32.0 } else { 16.0 };
    let frequency = f64::from(source_rate) / (divisor * (f64::from(timer) + 1.0));
    (12.0 * (frequency / 16.351_597_831_287_414).log2()).round().clamp(1.0, 127.0) as u8
}

fn configure_palette(video: &mut Video) {
    let colors = [
        [245, 194, 231],
        [203, 166, 247],
        [166, 227, 161],
        [249, 226, 175],
        [166, 173, 200],
        [205, 214, 244],
    ];
    for (group, rgb) in colors.into_iter().enumerate() {
        for row in 0..8 {
            let scale = 1.0 - 0.5 * row as f32 / 7.0;
            video.set_palette(
                160 + (group * 8 + row) as u8,
                [
                    (rgb[0] as f32 * scale) as u8,
                    (rgb[1] as f32 * scale) as u8,
                    (rgb[2] as f32 * scale) as u8,
                    255,
                ],
            );
        }
    }
    video.set_palette(PLAYING_BG, [30, 55, 92, 255]);
    video.set_palette(SELECT_BG, [49, 50, 68, 255]);
    // Every other pattern region gets a faint lift off black, and each region
    // starts on a bright rule, so block boundaries read at a glance.
    video.set_palette(REGION_BG, [22, 23, 32, 255]);
    video.set_palette(REGION_RULE, [116, 125, 161, 255]);
    for (index, color) in
        [[61, 48, 58], [51, 42, 62], [42, 57, 40], [62, 56, 44]].into_iter().enumerate()
    {
        video.set_palette(METER_BACKGROUNDS[index], [color[0], color[1], color[2], 255]);
    }
}

fn channel_meter_header(
    video: &mut Video,
    column: usize,
    row: usize,
    width: usize,
    label: &str,
    voice: usize,
    level: u8,
) {
    let x = PANE_LEFT + column * GLYPH_WIDTH;
    let y = PANE_TOP + row * GLYPH_HEIGHT;
    let width_pixels = width * GLYPH_WIDTH;
    fill(video, x, y, width_pixels, GLYPH_HEIGHT, UI_BLACK);
    let meter_width = width_pixels * usize::from(level.min(15)) / 15;
    if meter_width != 0 {
        fill(
            video,
            x + (width_pixels - meter_width) / 2,
            y,
            meter_width,
            GLYPH_HEIGHT,
            METER_BACKGROUNDS[voice],
        );
    }
    let label_x = x + width_pixels.saturating_sub(label.len() * GLYPH_WIDTH) / 2;
    text_transparent(video, label_x, y, label, CHANNEL_GRADIENTS[voice]);
}
fn button(video: &mut Video, column: usize, row: usize, label: &str, foreground: u8) {
    text(
        video,
        PANE_LEFT + column * GLYPH_WIDTH,
        PANE_TOP + row * GLYPH_HEIGHT,
        label,
        foreground,
        SELECT_BG,
    );
}
fn frame(video: &mut Video, x: usize, y: usize, width: usize, height: usize) {
    for px in x..x + width {
        put(video, px, y, GRAD_WHITE);
        put(video, px, y + height - 1, GRAD_WHITE);
    }
    for py in y..y + height {
        put(video, x, py, GRAD_WHITE);
        put(video, x + width - 1, py, GRAD_WHITE);
    }
}
fn fill(video: &mut Video, x: usize, y: usize, width: usize, height: usize, color: u8) {
    for py in y..y + height {
        for px in x..x + width {
            put(video, px, py, color);
        }
    }
}
fn text(video: &mut Video, x: usize, y: usize, value: &str, foreground: u8, background: u8) {
    for (column, byte) in value.bytes().enumerate() {
        let glyph = CHARACTER_ROM[usize::from(byte.to_ascii_uppercase())];
        for (row, bits) in glyph.into_iter().enumerate() {
            for bit in 0..GLYPH_WIDTH {
                let color =
                    if bits & (0x80 >> bit) != 0 { foreground + row as u8 } else { background };
                put(video, x + column * GLYPH_WIDTH + bit, y + row, color);
            }
        }
    }
}
fn text_transparent(video: &mut Video, x: usize, y: usize, value: &str, foreground: u8) {
    for (column, byte) in value.bytes().enumerate() {
        let glyph = CHARACTER_ROM[usize::from(byte.to_ascii_uppercase())];
        for (row, bits) in glyph.into_iter().enumerate() {
            for bit in 0..GLYPH_WIDTH {
                if bits & (0x80 >> bit) != 0 {
                    put(video, x + column * GLYPH_WIDTH + bit, y + row, foreground + row as u8);
                }
            }
        }
    }
}
fn put(video: &mut Video, x: usize, y: usize, color: u8) {
    if x < EDITOR_DISPLAY_WIDTH && y < video.dimensions().1 {
        let width = video.dimensions().0;
        video.pixels_mut()[y * width + x] = color;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tracker_v2_round_trips_frames_patterns_instruments_and_assembles() {
        let mut editor = MusicEditor::default();
        editor.song.frames.push([0, 1, 2, 3]);
        editor.song.loop_row = 16;
        editor.song.instruments[0].volume =
            Envelope { values: vec![15, 12, 8, 4], loop_at: Some(1) };
        let source = editor.serialize("theme.mus");
        let restored = MusicEditor::parse(&source).unwrap();
        assert_eq!(restored.song, editor.song);
        assert_eq!(MusicEditor::compile(&source).unwrap().loop_frame, 96);
        let wrapper = " ORG $2000\n PUT theme.mus\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ";
        fanticon::assembler::assemble_with_loader("main.asm", wrapper, |path| {
            (path == "theme.mus").then(|| source.clone()).ok_or_else(|| "missing".to_owned())
        })
        .unwrap();

        let player = include_str!("../../code-assets/demos/music/player.inc");
        let cartridge = " ORG $C100\nRESET JSR MUSIC_TICK\n JMP RESET\nNMI RTI\nIRQ RTI\n PUT player.inc\n PUT theme.mus\n ORG $FFFA\n DA NMI,RESET,IRQ";
        fanticon::assembler::assemble_with_loader("main.asm", cartridge, |path| {
            if path.eq_ignore_ascii_case("player.inc") {
                Ok(player.to_owned())
            } else if path.eq_ignore_ascii_case("theme.mus") {
                Ok(source.clone())
            } else {
                Err("missing".to_owned())
            }
        })
        .unwrap();
    }

    #[test]
    fn note_entry_instrument_volume_and_undo_work() {
        let mut editor = MusicEditor::default();
        editor.handle_key(&Key::Character("a".into()), ModifiersState::empty());
        assert_eq!(editor.song.step(0, 0).note, 48);
        editor.row = 0;
        editor.field = 1;
        editor.handle_key(&Key::Character("c".into()), ModifiersState::empty());
        assert_eq!(editor.song.step(0, 0).instrument, 12);
        assert!(editor.undo());
        assert_eq!(editor.song.step(0, 0).instrument, 0);
    }

    #[test]
    fn instrument_view_piano_keys_compile_a_non_destructive_audition_note() {
        let mut editor = MusicEditor { view: View::Instrument, ..MusicEditor::default() };
        editor.song.instruments[0].volume = Envelope { values: vec![7], loop_at: Some(0) };
        editor.song.instruments[0].tone = Envelope { values: vec![2], loop_at: Some(0) };
        let original = editor.serialize("song.mus");

        let source = editor.instrument_audition_source(&Key::Character("a".into())).unwrap();
        let audition = MusicEditor::compile(&source).unwrap();

        assert_eq!(audition.frames[0][0], 0x80 | 2 << 5 | 7);
        assert_eq!(audition.frames[0][3], 1);
        assert_eq!(editor.serialize("song.mus"), original);
        assert!(
            MusicEditor::default()
                .instrument_audition_source(&Key::Character("a".into()))
                .is_none()
        );
    }

    #[test]
    fn nsf_capture_preserves_phase_across_pitch_and_zero_volume_changes() {
        let pulse_frame = |note: u8, volume: u8, retrigger: bool| {
            let timer = note_timer(note, false);
            let mut frame = [0_u8; 16];
            frame[0] = 0x80 | 2 << 5 | volume;
            frame[1] = timer as u8;
            frame[2] = (timer >> 8) as u8;
            frame[3] = u8::from(retrigger);
            frame
        };
        let captured = [
            pulse_frame(48, 15, true),
            pulse_frame(50, 0, false),
            pulse_frame(50, 15, false),
            pulse_frame(52, 15, true),
        ];

        let source =
            MusicEditor::captured_frames_to_source("capture.mus", &captured, 1_789_773, 0).unwrap();
        let compiled = MusicEditor::compile(&source).unwrap();

        assert_eq!(compiled.frames[0][3], 1);
        assert_eq!(compiled.frames[1][3], 0, "a pitch write must remain legato");
        assert_eq!(compiled.frames[1][0] & 0x80, 0x80, "zero volume must retain the gate");
        assert_eq!(compiled.frames[1][0] & 15, 0);
        assert_eq!(compiled.frames[2][3], 0, "restoring volume must not retrigger");
        assert_eq!(compiled.frames[3][3], 1, "an NSF retrigger must still reset phase");
    }

    #[test]
    fn playback_row_is_centered_and_compiler_applies_envelopes() {
        let mut editor = MusicEditor::default();
        editor.song.instruments[0].volume = Envelope { values: vec![15, 8, 0], loop_at: None };
        editor.follow_playback(Some(40), [15, 8, 4, 2]);
        assert_eq!(editor.playback_view(), (40, Some(40), 24..56));
        let compiled = editor.compile_song();
        assert_eq!(compiled.frames.len(), 64 * 6);
        assert_eq!(compiled.frames[0][0] & 15, 15);
        assert_eq!(compiled.frames[1][0] & 15, 8);
        assert_eq!(compiled.frames[2][0] & 15, 0);
    }

    #[test]
    fn playback_highlights_full_center_row_and_tracker_uses_colored_gradients() {
        let mut editor = MusicEditor::default();
        editor.follow_playback(Some(0), [15, 8, 4, 2]);
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, 400);
        editor.render(&mut video);
        let centered_y = PANE_TOP + (6 + VISIBLE_ROWS / 2) * GLYPH_HEIGHT;
        assert_eq!(
            video.pixels()[centered_y * EDITOR_DISPLAY_WIDTH + EDITOR_DISPLAY_WIDTH - 12],
            PLAYING_BG
        );
        assert!(video.pixels().contains(&GRAD_P1));
        assert!(video.pixels().contains(&GRAD_P2));
        assert_eq!(video.pixels()[PANE_TOP * EDITOR_DISPLAY_WIDTH + PANE_LEFT], UI_BLACK);
        assert_eq!(
            video.pixels()[(PANE_TOP + 45 * GLYPH_HEIGHT) * EDITOR_DISPLAY_WIDTH + PANE_LEFT],
            UI_BLACK,
            "the redundant tracker title frame must remain absent"
        );
        for color in METER_BACKGROUNDS {
            assert!(video.pixels().contains(&color));
        }
        let pulse_2_left = PANE_LEFT + 17 * GLYPH_WIDTH;
        let pulse_2_center = pulse_2_left + 5 * GLYPH_WIDTH;
        let header_y = PANE_TOP + 5 * GLYPH_HEIGHT;
        let column_contains = |x: usize, color: u8| {
            (header_y..header_y + GLYPH_HEIGHT)
                .any(|y| video.pixels()[y * EDITOR_DISPLAY_WIDTH + x] == color)
        };
        assert!(!column_contains(pulse_2_left, METER_BACKGROUNDS[1]));
        assert!(column_contains(pulse_2_center, METER_BACKGROUNDS[1]));
        assert_ne!(
            video.palette()[GRAD_WHITE as usize],
            video.palette()[(GRAD_WHITE + 7) as usize]
        );
    }

    #[test]
    fn graphical_controls_edit_notes_frames_and_envelope_bars() {
        let mut editor = MusicEditor::default();
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 10 * GLYPH_WIDTH, PANE_TOP + 43 * GLYPH_HEIGHT,)
        );
        assert_eq!(editor.song.step(0, 0).note, 50);

        editor.handle_mouse_press(PANE_LEFT + 16 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Frames);
        let frames = editor.song.frames.len();
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 3 * GLYPH_WIDTH, PANE_TOP + 3 * GLYPH_HEIGHT,)
        );
        assert_eq!(editor.song.frames.len(), frames + 1);

        editor.handle_mouse_press(PANE_LEFT + 30 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Instrument);
        let graph_x = PANE_LEFT + 12 * GLYPH_WIDTH + 4;
        let graph_bottom = PANE_TOP + 11 * GLYPH_HEIGHT - 1;
        assert!(editor.handle_mouse_press(graph_x, graph_bottom));
        assert_eq!(editor.song.instruments[0].volume.values[0], 0);
        assert!(editor.handle_mouse_move(graph_x, PANE_TOP + 7 * GLYPH_HEIGHT));
        assert_eq!(editor.song.instruments[0].volume.values[0], 15);
        editor.handle_mouse_release();
        assert!(!editor.mouse_dragging_envelope);
    }

    #[test]
    fn workspace_tabs_and_remaining_edit_controls_have_matching_mouse_targets() {
        let mut editor = MusicEditor::default();

        editor.handle_mouse_press(PANE_LEFT + 12 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Frames);
        editor.handle_mouse_press(PANE_LEFT + 20 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Frames, "the visual gap is not a hidden tab target");
        editor.handle_mouse_press(PANE_LEFT + 21 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Instrument);

        let old_steps = editor.envelope().values.len();
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 2 * GLYPH_WIDTH, PANE_TOP + 35 * GLYPH_HEIGHT)
        );
        assert_eq!(editor.envelope().values.len(), old_steps + 1);
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 24 * GLYPH_WIDTH, PANE_TOP + 35 * GLYPH_HEIGHT)
        );
        assert_eq!(editor.envelope().loop_at, Some(editor.env_cursor));
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 13 * GLYPH_WIDTH, PANE_TOP + 35 * GLYPH_HEIGHT)
        );
        assert_eq!(editor.envelope().values.len(), old_steps);

        editor.handle_mouse_press(PANE_LEFT + 2 * GLYPH_WIDTH, PANE_TOP);
        assert_eq!(editor.view, View::Pattern);
        let instrument = editor.song.step(0, 0).instrument;
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 29 * GLYPH_WIDTH, PANE_TOP + 44 * GLYPH_HEIGHT)
        );
        assert_eq!(editor.song.step(0, 0).instrument, instrument.saturating_add(1).min(15));
        let volume = editor.song.step(0, 0).volume;
        assert!(
            editor.handle_mouse_press(PANE_LEFT + 43 * GLYPH_WIDTH, PANE_TOP + 44 * GLYPH_HEIGHT)
        );
        assert_eq!(
            editor.song.step(0, 0).volume,
            if volume == 0xff { 15 } else { volume.saturating_add(1).min(15) }
        );
    }

    #[test]
    fn pattern_regions_are_marked_by_alternating_shade_and_boundary_rules() {
        let source = include_str!("../../code-assets/demos/music/song.mus");
        let editor = MusicEditor::parse(source).unwrap();
        assert_eq!(editor.song.pattern_rows, 16);
        assert!(editor.song.frames.len() > 1, "needs several regions to tell them apart");

        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, 400);
        editor.render(&mut video);
        let row_y = |row: usize| PANE_TOP + (6 + row) * GLYPH_HEIGHT;
        // Sample clear of the glyph columns so only the row background is read.
        let x = EDITOR_DISPLAY_WIDTH - 12;
        let at = |x: usize, y: usize| video.pixels()[y * EDITOR_DISPLAY_WIDTH + x];

        // Regions alternate: the first sits on black, the second is lifted off it.
        assert_eq!(at(x, row_y(0) + GLYPH_HEIGHT / 2), UI_BLACK);
        assert_eq!(at(x, row_y(16) + GLYPH_HEIGHT / 2), REGION_BG);
        assert_eq!(at(x, row_y(31) + GLYPH_HEIGHT / 2), REGION_BG);

        // Each region opens on a rule, and rows inside one never draw it.
        assert_eq!(at(x, row_y(0)), REGION_RULE);
        assert_eq!(at(x, row_y(16)), REGION_RULE);
        assert_ne!(at(x, row_y(8)), REGION_RULE);
        assert_ne!(at(x, row_y(17)), REGION_RULE);

        // The boundary row is labelled with the frame that starts there.
        assert!(video.pixels().contains(&GRAD_WHITE));
        assert_ne!(video.palette()[REGION_BG as usize], video.palette()[UI_BLACK as usize]);
        assert_ne!(video.palette()[REGION_RULE as usize], video.palette()[REGION_BG as usize]);
    }

    #[test]
    fn v1_ode_demo_migrates_to_frames_without_losing_rows() {
        let source = include_str!("../../code-assets/demos/music/song.mus");
        let editor = MusicEditor::parse(source).unwrap();
        assert_eq!(editor.song.total_rows(), 128);
        assert_eq!(editor.song.frames.len(), 8);
        assert_eq!(editor.song.step(0, 0).note, 0x3b);
        assert_eq!(editor.song.step(4, 0).note, 0x3c);
        assert_eq!(editor.song.step(6, 0).note, 0x3e);
        assert_eq!(editor.song.step(96, 0).note, 0x3b);
    }
}
