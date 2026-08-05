use fanticon::video::{Video, rgb332_to_rgba};

pub const GLYPH_WIDTH: usize = 8;
pub const GLYPH_HEIGHT: usize = 8;
pub const BOX_HORIZONTAL: u8 = 1;
pub const BOX_VERTICAL: u8 = 2;
pub const BOX_TOP_LEFT: u8 = 3;
pub const BOX_TOP_RIGHT: u8 = 4;
pub const BOX_BOTTOM_LEFT: u8 = 5;
pub const BOX_BOTTOM_RIGHT: u8 = 6;
pub const SYMBOL_ARROW_RIGHT: u8 = 7;
pub const SYMBOL_CHECK: u8 = 8;
pub const SYMBOL_CROSS: u8 = 9;
pub const SYMBOL_BUSY: u8 = 10;
pub const BOX_TOP_HORIZONTAL: u8 = 11;
pub const BOX_BOTTOM_HORIZONTAL: u8 = 12;
pub const BOX_RIGHT_VERTICAL: u8 = 13;
pub const BOX_CAPTION_LEFT: u8 = 14;
pub const BOX_CAPTION_RIGHT: u8 = 15;
/// Double-ruled frame set. The focused pane and every dialog use these so the
/// active window reads at a glance, the way DOS-era IDEs marked focus.
pub const DBL_HORIZONTAL: u8 = 16;
pub const DBL_VERTICAL: u8 = 17;
pub const DBL_TOP_LEFT: u8 = 18;
pub const DBL_TOP_RIGHT: u8 = 19;
pub const DBL_BOTTOM_LEFT: u8 = 20;
pub const DBL_BOTTOM_RIGHT: u8 = 21;
pub const DBL_TOP_HORIZONTAL: u8 = 22;
pub const DBL_BOTTOM_HORIZONTAL: u8 = 23;
pub const DBL_RIGHT_VERTICAL: u8 = 24;
pub const DBL_CAPTION_LEFT: u8 = 25;
pub const DBL_CAPTION_RIGHT: u8 = 26;
/// Dither blocks and arrows for scrollbars and window shadows.
pub const SHADE_LIGHT: u8 = 27;
pub const SHADE_MEDIUM: u8 = 28;
pub const SYMBOL_ARROW_UP: u8 = 29;
pub const SYMBOL_ARROW_DOWN: u8 = 30;
pub const CHARACTER_ROM: [[u8; GLYPH_HEIGHT]; 128] = build_character_rom();

pub type TextGradient = [[u8; GLYPH_HEIGHT]; 256];

/// Create one palette level per glyph scanline for every color used by text.
/// Keeping shading in palette-indexed pixels leaves ordinary cell backgrounds
/// untouched, including colored build dialogs.
pub fn configure_text_gradient(
    video: &mut Video,
    colors: impl IntoIterator<Item = u8>,
) -> TextGradient {
    let mut gradient = core::array::from_fn(|index| [index as u8; GLYPH_HEIGHT]);
    let mut sources = [false; 256];
    for color in colors {
        sources[color as usize] = true;
    }
    let mut unavailable = sources;
    let mut next_candidate = 0usize;

    for source in 0..256 {
        if !sources[source] {
            continue;
        }
        let rgba = video.palette()[source];
        let denominator = (GLYPH_HEIGHT * 2 - 2) as u16;
        for (step, gradient_step) in gradient[source].iter_mut().enumerate().skip(1) {
            let numerator = denominator - step as u16;
            while next_candidate < 256 && unavailable[next_candidate] {
                next_candidate += 1;
            }
            assert!(next_candidate < 256, "text gradient exhausted the palette");
            let shade = next_candidate as u8;
            unavailable[next_candidate] = true;
            next_candidate += 1;
            video.set_palette(
                shade,
                [
                    (u16::from(rgba[0]) * numerator / denominator) as u8,
                    (u16::from(rgba[1]) * numerator / denominator) as u8,
                    (u16::from(rgba[2]) * numerator / denominator) as u8,
                    rgba[3],
                ],
            );
            *gradient_step = shade;
        }
    }
    gradient
}

/// Approximate the same top-to-bottom shading as [`configure_text_gradient`]
/// without claiming any palette entries. Each scanline below the first is
/// darkened by the same fraction the palette-backed gradient uses, then
/// matched to its nearest already-existing RGB332 byte. Tools that hold the
/// identity palette (the graphics editor's canvas view) need every one of the
/// 256 indexes to keep meaning its own color, so this reuses bytes that are
/// already the right shade instead of reassigning any of them.
pub fn identity_text_gradient(colors: impl IntoIterator<Item = u8>) -> TextGradient {
    let mut gradient = core::array::from_fn(|index| [index as u8; GLYPH_HEIGHT]);
    let mut seen = [false; 256];
    for color in colors {
        if seen[color as usize] {
            continue;
        }
        seen[color as usize] = true;
        let base = rgb332_to_rgba(color);
        let denominator = (GLYPH_HEIGHT * 2 - 2) as u16;
        for (step, gradient_step) in gradient[color as usize].iter_mut().enumerate().skip(1) {
            let numerator = denominator - step as u16;
            let target = [
                (u16::from(base[0]) * numerator / denominator) as u8,
                (u16::from(base[1]) * numerator / denominator) as u8,
                (u16::from(base[2]) * numerator / denominator) as u8,
            ];
            *gradient_step = nearest_rgb332_byte(target);
        }
    }
    gradient
}

/// Nearest RGB332 byte to a target color, by squared channel distance.
fn nearest_rgb332_byte(target: [u8; 3]) -> u8 {
    let mut best = 0u8;
    let mut best_distance = u32::MAX;
    for candidate in 0..=255u8 {
        let rgba = rgb332_to_rgba(candidate);
        let dr = i32::from(rgba[0]) - i32::from(target[0]);
        let dg = i32::from(rgba[1]) - i32::from(target[1]);
        let db = i32::from(rgba[2]) - i32::from(target[2]);
        let distance = (dr * dr + dg * dg + db * db) as u32;
        if distance < best_distance {
            best_distance = distance;
            best = candidate;
        }
    }
    best
}

pub fn gradient_color(gradient: &TextGradient, color: u8, glyph_y: usize) -> u8 {
    gradient[color as usize][glyph_y.min(GLYPH_HEIGHT - 1)]
}

const fn build_character_rom() -> [[u8; GLYPH_HEIGHT]; 128] {
    let mut rom = [[0; GLYPH_HEIGHT]; 128];
    let mut character = 0;
    while character < 128 {
        let source = glyph_5x7(character as u8);
        let mut row = 0;
        while row < 7 {
            // Center the five source columns in the 8-pixel cell. Bold glyphs
            // expand one pixel to the right, leaving equal outer margins
            // instead of touching the cell's right edge.
            let stroke = source[row] << 2;
            rom[character][row] = if matches!(
                character as u8,
                b'A'..=b'Z' | b'0'..=b'9' | b'/' | b'\\' | b'>' | b'<' | b'|' | b'^'
            ) {
                stroke | (stroke >> 1)
            } else {
                stroke
            };
            row += 1;
        }
        character += 1;
    }
    rom[BOX_HORIZONTAL as usize] = [0x00, 0x00, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00];
    rom[BOX_VERTICAL as usize] = [0x80; 8];
    rom[BOX_TOP_LEFT as usize] = [0xff, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80];
    rom[BOX_TOP_RIGHT as usize] = [0xff, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01];
    rom[BOX_BOTTOM_LEFT as usize] = [0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0x80, 0xff];
    rom[BOX_BOTTOM_RIGHT as usize] = [0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0x01, 0xff];
    rom[BOX_TOP_HORIZONTAL as usize] = [0xff, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00];
    rom[BOX_BOTTOM_HORIZONTAL as usize] = [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0xff];
    rom[BOX_RIGHT_VERTICAL as usize] = [0x01; 8];
    rom[BOX_CAPTION_LEFT as usize] = [0x00, 0x00, 0x00, 0xff, 0x80, 0x80, 0x80, 0x80];
    rom[BOX_CAPTION_RIGHT as usize] = [0x00, 0x00, 0x00, 0xff, 0x01, 0x01, 0x01, 0x01];
    // Double rules sit on the same cell edges as the single set, with the second
    // stroke two pixels inside, so both frame styles align on the same grid.
    rom[DBL_HORIZONTAL as usize] = [0x00, 0x00, 0xff, 0x00, 0xff, 0x00, 0x00, 0x00];
    rom[DBL_VERTICAL as usize] = [0xa0; 8];
    rom[DBL_RIGHT_VERTICAL as usize] = [0x05; 8];
    rom[DBL_TOP_HORIZONTAL as usize] = [0xff, 0x00, 0xff, 0x00, 0x00, 0x00, 0x00, 0x00];
    rom[DBL_BOTTOM_HORIZONTAL as usize] = [0x00, 0x00, 0x00, 0x00, 0x00, 0xff, 0x00, 0xff];
    rom[DBL_TOP_LEFT as usize] = [0xff, 0x80, 0xbf, 0xa0, 0xa0, 0xa0, 0xa0, 0xa0];
    rom[DBL_TOP_RIGHT as usize] = [0xff, 0x01, 0xfd, 0x05, 0x05, 0x05, 0x05, 0x05];
    rom[DBL_BOTTOM_LEFT as usize] = [0xa0, 0xa0, 0xa0, 0xa0, 0xa0, 0xbf, 0x80, 0xff];
    rom[DBL_BOTTOM_RIGHT as usize] = [0x05, 0x05, 0x05, 0x05, 0x05, 0xfd, 0x01, 0xff];
    rom[DBL_CAPTION_LEFT as usize] = [0x00, 0x00, 0xff, 0x80, 0xbf, 0xa0, 0xa0, 0xa0];
    rom[DBL_CAPTION_RIGHT as usize] = [0x00, 0x00, 0xff, 0x01, 0xfd, 0x05, 0x05, 0x05];
    rom[SHADE_LIGHT as usize] = [0x88, 0x00, 0x22, 0x00, 0x88, 0x00, 0x22, 0x00];
    rom[SHADE_MEDIUM as usize] = [0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55, 0xaa, 0x55];
    rom[SYMBOL_ARROW_UP as usize] = [0x00, 0x10, 0x38, 0x7c, 0xfe, 0x00, 0x00, 0x00];
    rom[SYMBOL_ARROW_DOWN as usize] = [0x00, 0x00, 0x00, 0xfe, 0x7c, 0x38, 0x10, 0x00];
    rom[SYMBOL_ARROW_RIGHT as usize] = [0x00, 0x20, 0x10, 0xf8, 0x10, 0x20, 0x00, 0x00];
    rom[SYMBOL_CHECK as usize] = [0x00, 0x00, 0x04, 0x08, 0x90, 0x60, 0x00, 0x00];
    rom[SYMBOL_CROSS as usize] = [0x00, 0x42, 0x24, 0x18, 0x18, 0x24, 0x42, 0x00];
    rom[SYMBOL_BUSY as usize] = [0x7e, 0x24, 0x18, 0x18, 0x18, 0x24, 0x7e, 0x00];
    rom
}

#[rustfmt::skip]
const fn glyph_5x7(character: u8) -> [u8; 7] {
    match character {
        b'A' => [0x0e, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'B' => [0x1e, 0x11, 0x11, 0x1e, 0x11, 0x11, 0x1e],
        b'C' => [0x0f, 0x10, 0x10, 0x10, 0x10, 0x10, 0x0f],
        b'D' => [0x1e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x1e],
        b'E' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x1f],
        b'F' => [0x1f, 0x10, 0x10, 0x1e, 0x10, 0x10, 0x10],
        b'G' => [0x0f, 0x10, 0x10, 0x17, 0x11, 0x11, 0x0f],
        b'H' => [0x11, 0x11, 0x11, 0x1f, 0x11, 0x11, 0x11],
        b'I' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x1f],
        b'J' => [0x07, 0x02, 0x02, 0x02, 0x12, 0x12, 0x0c],
        b'K' => [0x11, 0x12, 0x14, 0x18, 0x14, 0x12, 0x11],
        b'L' => [0x10, 0x10, 0x10, 0x10, 0x10, 0x10, 0x1f],
        b'M' => [0x11, 0x1b, 0x15, 0x15, 0x11, 0x11, 0x11],
        b'N' => [0x11, 0x19, 0x15, 0x13, 0x11, 0x11, 0x11],
        b'O' => [0x0e, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'P' => [0x1e, 0x11, 0x11, 0x1e, 0x10, 0x10, 0x10],
        b'Q' => [0x0e, 0x11, 0x11, 0x11, 0x15, 0x12, 0x0d],
        b'R' => [0x1e, 0x11, 0x11, 0x1e, 0x14, 0x12, 0x11],
        b'S' => [0x0f, 0x10, 0x10, 0x0e, 0x01, 0x01, 0x1e],
        b'T' => [0x1f, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'U' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x11, 0x0e],
        b'V' => [0x11, 0x11, 0x11, 0x11, 0x11, 0x0a, 0x04],
        b'W' => [0x11, 0x11, 0x11, 0x15, 0x15, 0x15, 0x0a],
        b'X' => [0x11, 0x11, 0x0a, 0x04, 0x0a, 0x11, 0x11],
        b'Y' => [0x11, 0x11, 0x0a, 0x04, 0x04, 0x04, 0x04],
        b'Z' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x10, 0x1f],
        b'0' => [0x0e, 0x11, 0x13, 0x15, 0x19, 0x11, 0x0e],
        b'1' => [0x04, 0x0c, 0x14, 0x04, 0x04, 0x04, 0x1f],
        b'2' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x08, 0x1f],
        b'3' => [0x1e, 0x01, 0x01, 0x0e, 0x01, 0x01, 0x1e],
        b'4' => [0x02, 0x06, 0x0a, 0x12, 0x1f, 0x02, 0x02],
        b'5' => [0x1f, 0x10, 0x10, 0x1e, 0x01, 0x01, 0x1e],
        b'6' => [0x0e, 0x10, 0x10, 0x1e, 0x11, 0x11, 0x0e],
        b'7' => [0x1f, 0x01, 0x02, 0x04, 0x08, 0x08, 0x08],
        b'8' => [0x0e, 0x11, 0x11, 0x0e, 0x11, 0x11, 0x0e],
        b'9' => [0x0e, 0x11, 0x11, 0x0f, 0x01, 0x01, 0x0e],
        b'!' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x00, 0x04],
        b'"' => [0x0a, 0x0a, 0x0a, 0x00, 0x00, 0x00, 0x00],
        b'#' => [0x0a, 0x1f, 0x0a, 0x0a, 0x1f, 0x0a, 0x00],
        b'$' => [0x04, 0x0f, 0x14, 0x0e, 0x05, 0x1e, 0x04],
        b'%' => [0x19, 0x1a, 0x02, 0x04, 0x08, 0x0b, 0x13],
        b'&' => [0x0c, 0x12, 0x14, 0x08, 0x15, 0x12, 0x0d],
        b'\'' => [0x04, 0x04, 0x08, 0x00, 0x00, 0x00, 0x00],
        b'(' => [0x02, 0x04, 0x08, 0x08, 0x08, 0x04, 0x02],
        b')' => [0x08, 0x04, 0x02, 0x02, 0x02, 0x04, 0x08],
        b'*' => [0x00, 0x0a, 0x04, 0x1f, 0x04, 0x0a, 0x00],
        b'+' => [0x00, 0x04, 0x04, 0x1f, 0x04, 0x04, 0x00],
        b',' => [0x00, 0x00, 0x00, 0x00, 0x04, 0x04, 0x08],
        b'-' => [0x00, 0x00, 0x00, 0x1f, 0x00, 0x00, 0x00],
        b'.' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x0c, 0x0c],
        b'/' => [0x01, 0x02, 0x02, 0x04, 0x08, 0x08, 0x10],
        b':' => [0x00, 0x0c, 0x0c, 0x00, 0x0c, 0x0c, 0x00],
        b';' => [0x00, 0x0c, 0x0c, 0x00, 0x04, 0x04, 0x08],
        b'<' => [0x02, 0x04, 0x08, 0x10, 0x08, 0x04, 0x02],
        b'=' => [0x00, 0x00, 0x1f, 0x00, 0x1f, 0x00, 0x00],
        b'>' => [0x08, 0x04, 0x02, 0x01, 0x02, 0x04, 0x08],
        b'?' => [0x0e, 0x11, 0x01, 0x02, 0x04, 0x00, 0x04],
        b'@' => [0x0e, 0x11, 0x17, 0x15, 0x17, 0x10, 0x0f],
        b'[' => [0x0e, 0x08, 0x08, 0x08, 0x08, 0x08, 0x0e],
        b'\\' => [0x10, 0x08, 0x08, 0x04, 0x02, 0x02, 0x01],
        b']' => [0x0e, 0x02, 0x02, 0x02, 0x02, 0x02, 0x0e],
        b'^' => [0x04, 0x0a, 0x11, 0x00, 0x00, 0x00, 0x00],
        b'_' => [0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x1f],
        b'{' => [0x03, 0x04, 0x04, 0x18, 0x04, 0x04, 0x03],
        b'|' => [0x04, 0x04, 0x04, 0x04, 0x04, 0x04, 0x04],
        b'}' => [0x18, 0x04, 0x04, 0x03, 0x04, 0x04, 0x18],
        b'~' => [0x00, 0x00, 0x09, 0x16, 0x00, 0x00, 0x00],
        _ => [0; 7],
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rom_contains_expected_ascii_glyphs() {
        assert_ne!(CHARACTER_ROM[b'A' as usize], [0; 8]);
        assert_ne!(CHARACTER_ROM[b'0' as usize], [0; 8]);
        assert_ne!(CHARACTER_ROM[b'?' as usize], [0; 8]);
        assert_eq!(CHARACTER_ROM[b' ' as usize], [0; 8]);
        assert_ne!(CHARACTER_ROM[BOX_TOP_LEFT as usize], [0; 8]);
        assert_ne!(CHARACTER_ROM[SYMBOL_CHECK as usize], [0; 8]);
    }

    #[test]
    fn ascii_glyphs_use_bold_horizontal_strokes() {
        // The top of A is 00111000 before emboldening; its right-hand neighbor
        // is added without losing the centered cell margins.
        assert_eq!(CHARACTER_ROM[b'A' as usize][0], 0x3c);
        assert!(CHARACTER_ROM[b'I' as usize].iter().any(|row| row.count_ones() >= 6));
    }

    #[test]
    fn double_frames_add_an_inner_rule_on_the_single_sets_cell_edges() {
        // Outer strokes sit exactly where the single-line set puts them, so the
        // two styles can be swapped per window without shifting the layout.
        assert_eq!(CHARACTER_ROM[DBL_TOP_HORIZONTAL as usize][0], 0xff);
        assert_eq!(CHARACTER_ROM[BOX_TOP_HORIZONTAL as usize][0], 0xff);
        assert_eq!(CHARACTER_ROM[DBL_BOTTOM_HORIZONTAL as usize][7], 0xff);
        assert_eq!(CHARACTER_ROM[BOX_BOTTOM_HORIZONTAL as usize][7], 0xff);
        for row in CHARACTER_ROM[DBL_VERTICAL as usize] {
            assert_eq!(row & 0x80, 0x80, "outer stroke must match BOX_VERTICAL");
            assert_eq!(row & 0x20, 0x20, "inner stroke is what makes it double");
        }
        for row in CHARACTER_ROM[DBL_RIGHT_VERTICAL as usize] {
            assert_eq!(row & 0x01, 0x01);
            assert_eq!(row & 0x04, 0x04);
        }
        assert_eq!(CHARACTER_ROM[DBL_TOP_HORIZONTAL as usize][2], 0xff);

        // Shadow, scrollbar, and cap glyphs are all present.
        for glyph in [SHADE_LIGHT, SHADE_MEDIUM, SYMBOL_ARROW_UP, SYMBOL_ARROW_DOWN] {
            assert_ne!(CHARACTER_ROM[glyph as usize], [0; 8]);
        }
    }

    #[test]
    fn printable_ascii_keeps_both_cursor_cell_edges_clear() {
        for character in b'!'..=b'~' {
            for row in CHARACTER_ROM[character as usize] {
                assert_eq!(row & 0x81, 0, "{} touches an outer cell edge", character as char);
            }
        }
    }

    #[test]
    fn punctuation_preserves_open_thin_strokes() {
        assert_eq!(CHARACTER_ROM[b'$' as usize][1], 0x3c);
        assert_eq!(CHARACTER_ROM[b'%' as usize][0], 0x64);
        assert_eq!(CHARACTER_ROM[b'@' as usize][0], 0x38);
    }

    #[test]
    fn structural_symbols_are_bold_and_braces_are_available() {
        assert_eq!(CHARACTER_ROM[b'/' as usize][0], 0x06);
        assert_eq!(CHARACTER_ROM[b'\\' as usize][0], 0x60);
        assert_eq!(CHARACTER_ROM[b'>' as usize][0], 0x30);
        assert_eq!(CHARACTER_ROM[b'<' as usize][0], 0x0c);
        assert_eq!(CHARACTER_ROM[b'|' as usize][0], 0x18);
        assert_eq!(CHARACTER_ROM[b'^' as usize][0], 0x18);
        assert_ne!(CHARACTER_ROM[b'{' as usize], [0; 8]);
        assert_ne!(CHARACTER_ROM[b'}' as usize], [0; 8]);
    }

    #[test]
    fn text_gradient_has_one_smooth_step_per_scanline_from_full_to_half_brightness() {
        let mut video = Video::new_with_size(8, 8);
        video.set_palette(42, [120, 90, 60, 255]);
        video.set_palette(99, [11, 22, 33, 255]);

        let gradient = configure_text_gradient(&mut video, [42]);
        let shades = core::array::from_fn::<_, GLYPH_HEIGHT, _>(|row| {
            video.palette()[gradient_color(&gradient, 42, row) as usize]
        });

        assert_eq!(
            shades,
            [
                [120, 90, 60, 255],
                [111, 83, 55, 255],
                [102, 77, 51, 255],
                [94, 70, 47, 255],
                [85, 64, 42, 255],
                [77, 57, 38, 255],
                [68, 51, 34, 255],
                [60, 45, 30, 255],
            ]
        );
        assert_eq!(video.palette()[99], [11, 22, 33, 255]);
    }
}
