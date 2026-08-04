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
pub const DEFAULT_RASTER_TARGET: (u16, u16) = (511, 511);
pub const BITMAP_VRAM_START: usize = 0x4000;
pub const BITMAP_BYTES: usize = DISPLAY_WIDTH * DISPLAY_HEIGHT / 2;
pub const BITMAP_VRAM_END: usize = BITMAP_VRAM_START + BITMAP_BYTES - 1;
pub const SPRITE_COUNT: usize = 32;
pub const SPRITES_PER_SCANLINE: usize = 8;

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
    Pixel { offset: u32, color: u8 },
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
    width: usize,
    height: usize,
    pixels: Vec<u8>,
    palette: [[u8; 4]; 256],
    frame_pixels: Vec<u8>,
    frame_palette: [[u8; 4]; 256],
    resolve_pixels: Vec<u8>,
    events: Vec<RasterEvent>,
}

impl Default for Video {
    fn default() -> Self {
        Self::new()
    }
}

impl Video {
    pub fn new() -> Self {
        Self::new_with_size(DISPLAY_WIDTH, DISPLAY_HEIGHT)
    }

    pub fn new_with_size(width: usize, height: usize) -> Self {
        assert!(width > 0 && height > 0);
        let pixel_count = width.checked_mul(height).expect("video dimensions are too large");
        let palette = rgb332_palette();
        Self {
            width,
            height,
            pixels: vec![0; pixel_count],
            palette,
            frame_pixels: vec![0; pixel_count],
            frame_palette: palette,
            resolve_pixels: vec![0; pixel_count],
            events: Vec::with_capacity(256),
        }
    }

    pub const fn width(&self) -> usize {
        self.width
    }

    pub const fn height(&self) -> usize {
        self.height
    }

    pub const fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub const fn rgba_len(&self) -> usize {
        self.width * self.height * 4
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn pixels_mut(&mut self) -> &mut [u8] {
        &mut self.pixels
    }

    pub fn palette(&self) -> &[[u8; 4]; 256] {
        &self.palette
    }

    pub fn set_palette(&mut self, index: u8, rgba: [u8; 4]) {
        self.palette[index as usize] = rgba;
    }

    /// Restore the identity RGB332 palette, so every index expands to the color
    /// its own bits describe. Host tools that draw raw RGB332 data need this
    /// after any chrome that remapped entries for its own use.
    pub fn reset_palette(&mut self) {
        self.palette = rgb332_palette();
    }

    /// Snapshot persistent video state at the leading edge of a new frame.
    pub fn begin_frame(&mut self) {
        self.frame_pixels.copy_from_slice(&self.pixels);
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
        if x as usize >= self.width || y as usize >= self.height {
            return Err(VideoTimingError::PixelOutOfRange { x, y });
        }
        let offset = y as usize * self.width + x as usize;
        self.record(tick, RasterEventKind::Pixel { offset: offset as u32, color })?;
        self.pixels[offset] = color;
        Ok(())
    }

    /// Resolve the indexed frame in a single linear pass with no allocation.
    pub fn resolve_rgba(&mut self, output: &mut [u8]) -> Result<(), VideoTimingError> {
        let expected = self.rgba_len();
        if output.len() != expected {
            return Err(VideoTimingError::OutputSize { expected, actual: output.len() });
        }

        self.resolve_pixels.copy_from_slice(&self.frame_pixels);
        let mut palette = self.frame_palette;
        let mut event_index = 0;

        for y in 0..self.height {
            for x in 0..self.width {
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

                let pixel = y * self.width + x;
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

#[inline]
pub const fn rgb332_to_rgba(value: u8) -> [u8; 4] {
    let r = (value >> 5) & 7;
    let g = (value >> 2) & 7;
    let b = value & 3;
    [
        ((r as u16 * 255 + 3) / 7) as u8,
        ((g as u16 * 255 + 3) / 7) as u8,
        ((b as u16 * 255 + 1) / 3) as u8,
        255,
    ]
}

#[inline]
pub const fn decode_sprite_x(raw: u16) -> i16 {
    let raw = raw & 0x01ff;
    if raw >= 0x01f0 { raw as i16 - 512 } else { raw as i16 }
}

#[inline]
pub const fn decode_sprite_y(raw: u8) -> i16 {
    if raw >= 0xf0 { raw as i16 - 256 } else { raw as i16 }
}

#[inline]
pub const fn sprite_has_priority(
    background_color: u8,
    background_foreground: bool,
    sprite_behind_background: bool,
) -> bool {
    background_color & 0x0f == 0 || (!background_foreground && !sprite_behind_background)
}

#[inline]
pub const fn is_hblank(dot: u16) -> bool {
    dot >= DISPLAY_WIDTH as u16 && dot < DOTS_PER_SCANLINE
}

#[inline]
pub const fn is_vblank(scanline: u16) -> bool {
    scanline >= DISPLAY_HEIGHT as u16 && scanline < SCANLINES_PER_FRAME
}

fn rgb332_palette() -> [[u8; 4]; 256] {
    let mut palette = [[0; 4]; 256];
    let mut index = 0;
    while index < 256 {
        palette[index] = rgb332_to_rgba(index as u8);
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

    #[test]
    fn reset_palette_is_identity_rgb332_with_rounded_expansion() {
        let video = Video::new();
        assert_eq!(video.palette()[0], [0, 0, 0, 255]);
        assert_eq!(rgb332_to_rgba(0x40), [73, 0, 0, 255]);
        assert_eq!(rgb332_to_rgba(0x08), [0, 73, 0, 255]);
        assert_eq!(rgb332_to_rgba(0x02), [0, 0, 170, 255]);
        assert_eq!(video.palette()[255], [255, 255, 255, 255]);
    }

    #[test]
    fn sprite_coordinates_support_clipping_past_all_four_edges() {
        assert_eq!(decode_sprite_x(0x01f0), -16);
        assert_eq!(decode_sprite_x(0x01ff), -1);
        assert_eq!(decode_sprite_x(0x0140), 320);
        assert_eq!(decode_sprite_y(0xf0), -16);
        assert_eq!(decode_sprite_y(0xff), -1);
        assert_eq!(decode_sprite_y(0xc8), 200);
    }

    #[test]
    fn sprite_background_priority_matches_the_frozen_truth_table() {
        assert!(sprite_has_priority(0, true, true));
        assert!(sprite_has_priority(1, false, false));
        assert!(!sprite_has_priority(1, false, true));
        assert!(!sprite_has_priority(1, true, false));
        assert!(!sprite_has_priority(1, true, true));
    }

    #[test]
    fn blanking_and_bitmap_boundaries_match_the_memory_map() {
        assert!(!is_hblank(319));
        assert!(is_hblank(320));
        assert!(is_hblank(399));
        assert!(!is_vblank(199));
        assert!(is_vblank(200));
        assert!(is_vblank(261));
        assert_eq!(BITMAP_BYTES, 32_000);
        assert_eq!(BITMAP_VRAM_END, 0xbcff);
        assert_eq!(DEFAULT_RASTER_TARGET, (511, 511));
    }

    #[test]
    fn host_tools_can_use_a_larger_framebuffer_without_changing_vm_dimensions() {
        let mut video = Video::new_with_size(640, 400);
        assert_eq!(video.dimensions(), (640, 400));
        assert_eq!(video.pixels().len(), 256_000);
        assert_eq!(video.rgba_len(), 1_024_000);
        assert_eq!((DISPLAY_WIDTH, DISPLAY_HEIGHT), (320, 200));

        video.pixels_mut()[639] = 1;
        video.begin_frame();
        let mut rgba = vec![0; video.rgba_len()];
        video.resolve_rgba(&mut rgba).unwrap();
        assert_eq!(&rgba[639 * 4..639 * 4 + 4], &rgb332_to_rgba(1));
    }
}
