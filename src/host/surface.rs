//! True-color drawing surface for the host's own interface.
//!
//! The console's video chip is palette indexed because the hardware is, and the
//! VM's 256 entries belong to the running cartridge. The editor, terminal, and
//! asset tools are host software rather than console output, so they draw here
//! in plain RGBA instead of competing for those entries. That keeps the
//! graphics editor free to show every RGB332 byte as its true color, and lets
//! chrome shade itself with arithmetic instead of reserved palette slots.

use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};

pub type Rgba = [u8; 4];

/// Scale a color's channels, leaving alpha alone.
pub const fn scale(color: Rgba, numerator: u16, denominator: u16) -> Rgba {
    [
        ((color[0] as u16 * numerator) / denominator) as u8,
        ((color[1] as u16 * numerator) / denominator) as u8,
        ((color[2] as u16 * numerator) / denominator) as u8,
        color[3],
    ]
}

/// The chrome's top-to-bottom shading: full brightness on a cell's first
/// scanline, falling to half on its last. Previously this needed a reserved
/// palette entry per level; in true color it is one multiply.
pub const fn scanline_shade(color: Rgba, glyph_y: usize) -> Rgba {
    let denominator = (GLYPH_HEIGHT * 2 - 2) as u16;
    let step = if glyph_y >= GLYPH_HEIGHT { GLYPH_HEIGHT - 1 } else { glyph_y };
    scale(color, denominator - step as u16, denominator)
}

pub struct Surface {
    pixels: Vec<u8>,
    width: usize,
    height: usize,
}

impl Surface {
    pub fn new(width: usize, height: usize) -> Self {
        Self { pixels: vec![0; width * height * 4], width, height }
    }

    pub fn dimensions(&self) -> (usize, usize) {
        (self.width, self.height)
    }

    pub fn pixels(&self) -> &[u8] {
        &self.pixels
    }

    pub fn resize(&mut self, width: usize, height: usize) {
        if (width, height) != (self.width, self.height) {
            self.width = width;
            self.height = height;
            self.pixels.clear();
            self.pixels.resize(width * height * 4, 0);
        }
    }

    pub fn clear(&mut self, color: Rgba) {
        for pixel in self.pixels.chunks_exact_mut(4) {
            pixel.copy_from_slice(&color);
        }
    }

    /// The color at a coordinate, or transparent black when off-surface.
    #[cfg(test)]
    pub fn pixel(&self, x: usize, y: usize) -> Rgba {
        if x >= self.width || y >= self.height {
            return [0, 0, 0, 0];
        }
        let offset = (y * self.width + x) * 4;
        [
            self.pixels[offset],
            self.pixels[offset + 1],
            self.pixels[offset + 2],
            self.pixels[offset + 3],
        ]
    }

    pub fn put_pixel(&mut self, x: usize, y: usize, color: Rgba) {
        if x >= self.width || y >= self.height {
            return;
        }
        let offset = (y * self.width + x) * 4;
        self.pixels[offset..offset + 4].copy_from_slice(&color);
    }

    pub fn fill_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Rgba) {
        for row in y..(y + height).min(self.height) {
            for column in x..(x + width).min(self.width) {
                let offset = (row * self.width + column) * 4;
                self.pixels[offset..offset + 4].copy_from_slice(&color);
            }
        }
    }

    pub fn stroke_rect(&mut self, x: usize, y: usize, width: usize, height: usize, color: Rgba) {
        if width == 0 || height == 0 {
            return;
        }
        self.fill_rect(x, y, width, 1, color);
        self.fill_rect(x, y + height - 1, width, 1, color);
        self.fill_rect(x, y, 1, height, color);
        self.fill_rect(x + width - 1, y, 1, height, color);
    }

    /// Draw one character cell. `background` of `None` leaves what is already
    /// there, which is how glyphs land on top of artwork.
    pub fn blit_glyph(
        &mut self,
        x: usize,
        y: usize,
        character: u8,
        foreground: Rgba,
        background: Option<Rgba>,
        shaded: bool,
    ) {
        let glyph = CHARACTER_ROM[usize::from(character).min(CHARACTER_ROM.len() - 1)];
        for (glyph_y, bits) in glyph.into_iter().enumerate() {
            if let Some(background) = background {
                let color = if shaded { scanline_shade(background, glyph_y) } else { background };
                self.fill_rect(x, y + glyph_y, GLYPH_WIDTH, 1, color);
            }
            if bits == 0 {
                continue;
            }
            let color = if shaded { scanline_shade(foreground, glyph_y) } else { foreground };
            for glyph_x in 0..GLYPH_WIDTH {
                if bits & (0x80 >> glyph_x) != 0 {
                    self.put_pixel(x + glyph_x, y + glyph_y, color);
                }
            }
        }
    }

    /// Draw a string of cells left to right, one glyph per character.
    pub fn draw_text(
        &mut self,
        x: usize,
        y: usize,
        text: &str,
        foreground: Rgba,
        background: Option<Rgba>,
    ) {
        for (index, byte) in text.bytes().enumerate() {
            self.blit_glyph(x + index * GLYPH_WIDTH, y, byte, foreground, background, false);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scanline_shading_runs_from_full_brightness_to_half() {
        let white = [255, 255, 255, 255];
        assert_eq!(scanline_shade(white, 0), white);
        let last = scanline_shade(white, GLYPH_HEIGHT - 1);
        assert_eq!(last[0], 127, "the last scanline sits at half brightness");
        assert_eq!(last[3], 255, "alpha survives shading");
        for glyph_y in 1..GLYPH_HEIGHT {
            assert!(
                scanline_shade(white, glyph_y)[0] < scanline_shade(white, glyph_y - 1)[0],
                "each scanline must be darker than the one above it"
            );
        }
    }

    #[test]
    fn drawing_is_clipped_to_the_surface_instead_of_panicking() {
        let mut surface = Surface::new(4, 4);
        surface.clear([1, 2, 3, 255]);
        surface.put_pixel(9, 9, [9, 9, 9, 255]);
        surface.fill_rect(2, 2, 100, 100, [7, 7, 7, 255]);
        assert_eq!(&surface.pixels()[..4], &[1, 2, 3, 255]);
        assert_eq!(surface.pixels().len(), 4 * 4 * 4);
        let corner = (3 * 4 + 3) * 4;
        assert_eq!(&surface.pixels()[corner..corner + 4], &[7, 7, 7, 255]);
    }

    #[test]
    fn glyphs_can_leave_the_background_untouched() {
        let mut surface = Surface::new(GLYPH_WIDTH, GLYPH_HEIGHT);
        surface.clear([10, 20, 30, 255]);
        surface.blit_glyph(0, 0, b' ', [255, 255, 255, 255], None, false);
        assert_eq!(&surface.pixels()[..4], &[10, 20, 30, 255], "space kept the artwork");
        surface.blit_glyph(0, 0, b' ', [255, 255, 255, 255], Some([0, 0, 0, 255]), false);
        assert_eq!(&surface.pixels()[..4], &[0, 0, 0, 255], "an opaque cell clears it");
    }
}
