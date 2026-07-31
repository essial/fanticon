//! Pixel-timed video state for the Fanticon display.

use core::fmt;

pub const DISPLAY_WIDTH: usize = 320;
pub const DISPLAY_HEIGHT: usize = 200;
pub const FRAMEBUFFER_LEN: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT;
pub const RGBA_FRAME_LEN: usize = FRAMEBUFFER_LEN * 4;

/// Initial timing envelope. Active pixels occupy dots 0..320 on lines 0..200.
/// Blanking time is deliberately represented so future video hardware can run
/// work outside the visible region without changing the timestamp format.
pub const DOTS_PER_SCANLINE: u16 = 400;
pub const SCANLINES_PER_FRAME: u16 = 262;
pub const DOTS_PER_FRAME: u32 = DOTS_PER_SCANLINE as u32 * SCANLINES_PER_FRAME as u32;

#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord)]
pub struct RasterTick(u32);

impl RasterTick {
    pub const FRAME_START: Self = Self(0);

    pub const fn new(scanline: u16, dot: u16) -> Option<Self> {
        if scanline < SCANLINES_PER_FRAME && dot < DOTS_PER_SCANLINE {
            Some(Self(scanline as u32 * DOTS_PER_SCANLINE as u32 + dot as u32))
        } else {
            None
        }
    }

    pub const fn from_raw(tick: u32) -> Option<Self> {
        if tick < DOTS_PER_FRAME { Some(Self(tick)) } else { None }
    }

    pub const fn raw(self) -> u32 {
        self.0
    }

    pub const fn scanline(self) -> u16 {
        (self.0 / DOTS_PER_SCANLINE as u32) as u16
    }

    pub const fn dot(self) -> u16 {
        (self.0 % DOTS_PER_SCANLINE as u32) as u16
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VideoTimingError {
    EventOutOfOrder { previous: RasterTick, next: RasterTick },
    PixelOutOfRange { x: u16, y: u16 },
    OutputSize { expected: usize, actual: usize },
}

impl fmt::Display for VideoTimingError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match *self {
            Self::EventOutOfOrder { previous, next } => write!(
                f,
                "video event at {}:{} precedes prior event at {}:{}",
                next.scanline(),
                next.dot(),
                previous.scanline(),
                previous.dot()
            ),
            Self::PixelOutOfRange { x, y } => write!(f, "pixel ({x}, {y}) is outside 320x200"),
            Self::OutputSize { expected, actual } => {
                write!(f, "RGBA output is {actual} bytes; expected {expected}")
            }
        }
    }
}

impl std::error::Error for VideoTimingError {}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum RasterEventKind {
    Palette { index: u8, rgba: [u8; 4] },
    Pixel { offset: u16, color: u8 },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct RasterEvent {
    tick: RasterTick,
    kind: RasterEventKind,
}

/// Persistent indexed video memory plus an ordered, per-frame raster log.
///
/// Call `begin_frame` before executing a VM frame. Untimed writes through
/// `pixels_mut` and `set_palette` are intended for initialization or vertical
/// blank. During active emulation, use the timestamped write methods. Finally,
/// call `resolve_rgba` once to materialize exactly what the beam observed.
pub struct Video {
    pixels: Box<[u8; FRAMEBUFFER_LEN]>,
    palette: [[u8; 4]; 256],
    frame_pixels: Box<[u8; FRAMEBUFFER_LEN]>,
    frame_palette: [[u8; 4]; 256],
    resolve_pixels: Box<[u8; FRAMEBUFFER_LEN]>,
    events: Vec<RasterEvent>,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

impl Video {
    pub fn new() -> Self {
        let palette = rgb332_palette();
        Self {
            pixels: Box::new([0; FRAMEBUFFER_LEN]),
            palette,
            frame_pixels: Box::new([0; FRAMEBUFFER_LEN]),
            frame_palette: palette,
            resolve_pixels: Box::new([0; FRAMEBUFFER_LEN]),
            events: Vec::with_capacity(256),
        }
    }

    pub fn pixels(&self) -> &[u8; FRAMEBUFFER_LEN] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8; FRAMEBUFFER_LEN] {
        &mut self.pixels
    }

    pub fn palette(&self) -> &[[u8; 4]; 256] {
        &self.palette
    }

    pub fn set_palette(&mut self, index: u8, rgba: [u8; 4]) {
        self.palette[index as usize] = rgba;
    }

    /// Snapshot persistent video state at the leading edge of a new frame.
    pub fn begin_frame(&mut self) {
        self.frame_pixels.copy_from_slice(self.pixels.as_slice());
        self.frame_palette = self.palette;
        self.events.clear();
    }

    pub fn write_palette_at(
        &mut self,
        tick: RasterTick,
        index: u8,
        rgba: [u8; 4],
    ) -> Result<(), VideoTimingError> {
        self.record(tick, RasterEventKind::Palette { index, rgba })?;
        self.palette[index as usize] = rgba;
        Ok(())
    }

    pub fn write_pixel_at(
        &mut self,
        tick: RasterTick,
        x: u16,
        y: u16,
        color: u8,
    ) -> Result<(), VideoTimingError> {
        if x as usize >= DISPLAY_WIDTH || y as usize >= DISPLAY_HEIGHT {
            return Err(VideoTimingError::PixelOutOfRange { x, y });
        }
        let offset = y as usize * DISPLAY_WIDTH + x as usize;
        self.record(tick, RasterEventKind::Pixel { offset: offset as u16, color })?;
        self.pixels[offset] = color;
        Ok(())
    }

    /// Resolve the indexed frame in a single linear pass with no allocation.
    pub fn resolve_rgba(&mut self, output: &mut [u8]) -> Result<(), VideoTimingError> {
        if output.len() != RGBA_FRAME_LEN {
            return Err(VideoTimingError::OutputSize {
                expected: RGBA_FRAME_LEN,
                actual: output.len(),
            });
        }

        self.resolve_pixels.copy_from_slice(self.frame_pixels.as_slice());
        let mut palette = self.frame_palette;
        let mut event_index = 0;

        for y in 0..DISPLAY_HEIGHT {
            for x in 0..DISPLAY_WIDTH {
                let beam = y as u32 * DOTS_PER_SCANLINE as u32 + x as u32;
                while let Some(event) =
                    self.events.get(event_index).filter(|e| e.tick.raw() <= beam)
                {
                    match event.kind {
                        RasterEventKind::Palette { index, rgba } => palette[index as usize] = rgba,
                        RasterEventKind::Pixel { offset, color } => {
                            self.resolve_pixels[offset as usize] = color;
                        }
                    }
                    event_index += 1;
                }

                let pixel = y * DISPLAY_WIDTH + x;
                let rgba = palette[self.resolve_pixels[pixel] as usize];
                output[pixel * 4..pixel * 4 + 4].copy_from_slice(&rgba);
            }
        }
        Ok(())
    }

    fn record(&mut self, tick: RasterTick, kind: RasterEventKind) -> Result<(), VideoTimingError> {
        if let Some(previous) = self.events.last().map(|event| event.tick)
            && tick < previous
        {
            return Err(VideoTimingError::EventOutOfOrder { previous, next: tick });
        }
        self.events.push(RasterEvent { tick, kind });
        Ok(())
    }
}

fn rgb332_palette() -> [[u8; 4]; 256] {
    let mut palette = [[0; 4]; 256];
    let mut index = 0;
    while index < 256 {
        let r = ((index >> 5) & 7) as u8;
        let g = ((index >> 2) & 7) as u8;
        let b = (index & 3) as u8;
        palette[index] = [
            (u16::from(r) * 255 / 7) as u8,
            (u16::from(g) * 255 / 7) as u8,
            (u16::from(b) * 255 / 3) as u8,
            255,
        ];
        index += 1;
    }
    palette
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn raster_tick_round_trips_position() {
        let tick = RasterTick::new(199, 319).unwrap();
        assert_eq!(tick.scanline(), 199);
        assert_eq!(tick.dot(), 319);
        assert!(RasterTick::new(SCANLINES_PER_FRAME, 0).is_none());
        assert!(RasterTick::new(0, DOTS_PER_SCANLINE).is_none());
    }

    #[test]
    fn palette_change_takes_effect_at_exact_dot() {
        let mut video = Video::new();
        video.pixels_mut()[0..4].fill(1);
        video.set_palette(1, [255, 0, 0, 255]);
        video.begin_frame();
        video.write_palette_at(RasterTick::new(0, 2).unwrap(), 1, [0, 255, 0, 255]).unwrap();

        let mut rgba = vec![0; RGBA_FRAME_LEN];
        video.resolve_rgba(&mut rgba).unwrap();
        assert_eq!(&rgba[0..4], &[255, 0, 0, 255]);
        assert_eq!(&rgba[4..8], &[255, 0, 0, 255]);
        assert_eq!(&rgba[8..12], &[0, 255, 0, 255]);
    }

    #[test]
    fn pixel_write_only_affects_pixels_fetched_after_it() {
        let mut video = Video::new();
        video.set_palette(0, [0, 0, 0, 255]);
        video.set_palette(7, [255, 255, 255, 255]);
        video.begin_frame();
        video.write_pixel_at(RasterTick::new(0, 2).unwrap(), 0, 0, 7).unwrap();
        video.write_pixel_at(RasterTick::new(0, 2).unwrap(), 3, 0, 7).unwrap();

        let mut rgba = vec![0; RGBA_FRAME_LEN];
        video.resolve_rgba(&mut rgba).unwrap();
        assert_eq!(&rgba[0..4], &[0, 0, 0, 255]);
        assert_eq!(&rgba[12..16], &[255, 255, 255, 255]);
    }

    #[test]
    fn events_must_be_recorded_in_beam_order() {
        let mut video = Video::new();
        video.begin_frame();
        video.write_palette_at(RasterTick::new(10, 0).unwrap(), 0, [0; 4]).unwrap();
        assert!(matches!(
            video.write_palette_at(RasterTick::new(9, 399).unwrap(), 0, [0; 4]),
            Err(VideoTimingError::EventOutOfOrder { .. })
        ));
    }
}
