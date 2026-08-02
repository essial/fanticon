use fanticon::video::Video;
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::EDITOR_DISPLAY_WIDTH;
use super::character_rom::{CHARACTER_ROM, GLYPH_HEIGHT, GLYPH_WIDTH};

pub const TILE_BYTES: usize = 256 * 32;
pub const MAP_CELLS: usize = 40 * 25;
pub const PALETTE_BYTES: usize = 256;
pub const BITMAP_BYTES: usize = 320 * 200 / 2;
pub const DEFAULT_PALETTE_FILE: &str = "GAME.PAL";

const UI_WHITE: u8 = 0xff;
const UI_BLACK: u8 = 0x00;
const UI_BLUE: u8 = 0x1f;
const UI_GRAY: u8 = 0x92;
const PANE_LEFT: usize = 21 * GLYPH_WIDTH;
const PANE_TOP: usize = 3 * GLYPH_HEIGHT;
const OUTER_TOP: usize = 2 * GLYPH_HEIGHT + 4;
const DB16: [u8; 16] = [
    0x00, 0x45, 0x25, 0x49, 0x89, 0x2c, 0xc9, 0x6d, 0x4e, 0xcd, 0x92, 0x75, 0xd6, 0x76, 0xd9, 0xdf,
];
const PICO8: [u8; 16] = [
    0x00, 0x25, 0x65, 0x11, 0xa9, 0x69, 0xb6, 0xff, 0xe1, 0xf0, 0xf8, 0x19, 0x37, 0x8e, 0xee, 0xfa,
];
const C64: [u8; 16] = [
    0x00, 0xff, 0x85, 0x7a, 0x8a, 0x55, 0x26, 0xfd, 0x88, 0x48, 0xad, 0x49, 0x6d, 0xbe, 0x6f, 0xb6,
];
const EGA: [u8; 16] = [
    0x00, 0x02, 0x14, 0x16, 0xa0, 0xa2, 0xa8, 0xb6, 0x49, 0x4b, 0x5d, 0x5f, 0xe9, 0xeb, 0xfd, 0xff,
];
const PALETTE_PRESETS: [(&str, [u8; 16]); 4] =
    [("DB16", DB16), ("PICO-8", PICO8), ("C64", C64), ("EGA", EGA)];

fn palette_preset_for_bank(palette: &[u8], bank: u8) -> Option<usize> {
    let start = usize::from(bank) * 16;
    let colors = palette.get(start..start + 16)?;
    PALETTE_PRESETS.iter().position(|(_, preset)| colors == preset)
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsView {
    Tiles,
    Map,
    Sprite,
    Palette,
    Bitmap,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum GraphicsTool {
    Pencil,
    Fill,
    Eyedropper,
}

#[derive(Clone)]
struct GraphicsAsset {
    palette: Vec<u8>,
    tiles: Vec<u8>,
    map: Vec<u8>,
    attributes: Vec<u8>,
    bitmap: Vec<u8>,
}

impl Default for GraphicsAsset {
    fn default() -> Self {
        let mut palette = Vec::with_capacity(PALETTE_BYTES);
        for _ in 0..16 {
            palette.extend_from_slice(&DB16);
        }
        Self {
            palette,
            tiles: vec![0; TILE_BYTES],
            map: vec![0; MAP_CELLS],
            attributes: vec![0; MAP_CELLS],
            bitmap: vec![0; BITMAP_BYTES],
        }
    }
}

pub struct GraphicsEditor {
    asset: GraphicsAsset,
    undo: Vec<GraphicsAsset>,
    clipboard: Option<[u8; 32]>,
    pub view: GraphicsView,
    pub tool: GraphicsTool,
    pub selected_tile: u8,
    pub palette_bank: u8,
    pub selected_color: u8,
    map_priority: bool,
    map_h_flip: bool,
    map_v_flip: bool,
    drawing: bool,
    stroke_changed: bool,
    bitmap_asset: bool,
    bitmap_bank: u8,
    palette_preset: Option<usize>,
    palette_reference: Option<String>,
    palette_document: bool,
}

impl Default for GraphicsEditor {
    fn default() -> Self {
        Self {
            asset: GraphicsAsset::default(),
            undo: Vec::new(),
            clipboard: None,
            view: GraphicsView::Tiles,
            tool: GraphicsTool::Pencil,
            selected_tile: 0,
            palette_bank: 0,
            selected_color: 1,
            map_priority: false,
            map_h_flip: false,
            map_v_flip: false,
            drawing: false,
            stroke_changed: false,
            bitmap_asset: false,
            bitmap_bank: 0,
            palette_preset: Some(0),
            palette_reference: None,
            palette_document: false,
        }
    }
}

impl GraphicsEditor {
    pub fn with_shared_palette(reference: &str) -> Self {
        Self { palette_reference: Some(reference.to_ascii_uppercase()), ..Self::default() }
    }

    pub fn palette_document() -> Self {
        Self { view: GraphicsView::Palette, palette_document: true, ..Self::default() }
    }

    pub fn parse(source: &str) -> Result<Self, String> {
        let palette_document = source.lines().any(|line| line.trim() == ";@FANTICON-PAL 1");
        let graphics_document = source
            .lines()
            .any(|line| matches!(line.trim(), ";@FANTICON-GFX 1" | ";@FANTICON-GFX 2"));
        if !palette_document && !graphics_document {
            return Err("FILE IS MISSING ;@FANTICON-GFX 1/2 OR ;@FANTICON-PAL 1".to_owned());
        }
        let palette_reference = source
            .lines()
            .find_map(|line| line.trim().strip_prefix(";@PALETTE-FILE "))
            .map(str::trim)
            .filter(|reference| !reference.is_empty())
            .map(str::to_ascii_uppercase);
        let palette = if palette_document || palette_reference.is_none() {
            parse_section(source, ";@PALETTE", PALETTE_BYTES)?
        } else {
            GraphicsAsset::default().palette
        };
        if palette_document {
            let palette_preset = palette_preset_for_bank(&palette, 0);
            return Ok(Self {
                asset: GraphicsAsset { palette, ..GraphicsAsset::default() },
                view: GraphicsView::Palette,
                palette_preset,
                palette_document: true,
                ..Self::default()
            });
        }
        let bitmap_asset = source.lines().any(|line| line.trim() == ";@MODE BITMAP");
        let bitmap_bank = source
            .lines()
            .find_map(|line| line.trim().strip_prefix(";@BANK "))
            .and_then(|value| value.parse().ok())
            .unwrap_or(0);
        if bitmap_asset && bitmap_bank > 253 {
            return Err(
                "BITMAP START BANK MUST BE 0-253 (BITMAP + SPRITE PATTERNS USE 3 BANKS)".to_owned()
            );
        }
        let (tiles, map, attributes, bitmap) = if bitmap_asset {
            (
                if source.lines().any(|line| line.trim() == ";@TILES") {
                    parse_section(source, ";@TILES", TILE_BYTES)?
                } else {
                    vec![0; TILE_BYTES]
                },
                vec![0; MAP_CELLS],
                vec![0; MAP_CELLS],
                parse_section(source, ";@BITMAP", BITMAP_BYTES)?,
            )
        } else {
            (
                parse_section(source, ";@TILES", TILE_BYTES)?,
                parse_section(source, ";@MAP", MAP_CELLS)?,
                parse_section(source, ";@ATTRIBUTES", MAP_CELLS)?,
                vec![0; BITMAP_BYTES],
            )
        };
        let palette_preset = palette_preset_for_bank(&palette, 0);
        Ok(Self {
            asset: GraphicsAsset { palette, tiles, map, attributes, bitmap },
            view: if bitmap_asset { GraphicsView::Bitmap } else { GraphicsView::Tiles },
            bitmap_asset,
            bitmap_bank,
            palette_preset,
            palette_reference,
            ..Self::default()
        })
    }

    pub fn serialize(&self, filename: &str) -> String {
        let stem = filename
            .rsplit(['/', '\\'])
            .next()
            .unwrap_or("graphics.gfx")
            .split('.')
            .next()
            .unwrap_or("graphics")
            .to_ascii_uppercase();
        if self.palette_document {
            let mut output = String::from(
                ";@FANTICON-PAL 1\n; SHARED 256-COLOR RGB332 PALETTE - 16 BANKS OF 16\n",
            );
            write_section(
                &mut output,
                ";@PALETTE",
                &format!("{stem}_PAL"),
                &self.asset.palette,
                16,
            );
            return output;
        }
        let mut output = String::from(if self.palette_reference.is_some() {
            ";@FANTICON-GFX 2\n; ASCII SOURCE - EDIT VISUALLY OR BY HAND\n"
        } else {
            ";@FANTICON-GFX 1\n; ASCII SOURCE - EDIT VISUALLY OR BY HAND\n"
        });
        if let Some(reference) = &self.palette_reference {
            output.push_str(&format!(";@PALETTE-FILE {reference}\n"));
        }
        output.push_str(if self.bitmap_asset { ";@MODE BITMAP\n" } else { ";@MODE TILEMAP\n" });
        if self.bitmap_asset {
            output.push_str(&format!(";@BANK {}\n", self.bitmap_bank));
            output
                .push_str(&format!("         BANK  {}\n         ORG   $8000\n", self.bitmap_bank));
        }
        if self.palette_reference.is_none() {
            write_section(
                &mut output,
                ";@PALETTE",
                &format!("{stem}_PAL"),
                &self.asset.palette,
                16,
            );
        }
        if self.bitmap_asset {
            output.push_str(";@BITMAP\n");
            output.push_str(&format!("{stem}_BM0\n"));
            let first = 0x4000 - if self.palette_reference.is_none() { PALETTE_BYTES } else { 0 };
            for chunk in self.asset.bitmap[..first].chunks(20) {
                write_hex_line(&mut output, chunk);
            }
            output.push_str(&format!(
                "         BANK  {}\n         ORG   $8000\n{stem}_BM1\n",
                self.bitmap_bank.wrapping_add(1)
            ));
            for chunk in self.asset.bitmap[first..].chunks(20) {
                write_hex_line(&mut output, chunk);
            }
            output.push_str(&format!(
                "         BANK  {}\n         ORG   $8000\n",
                self.bitmap_bank.wrapping_add(2)
            ));
            self.write_tiles(&mut output, &stem);
            return output;
        }
        self.write_tiles(&mut output, &stem);
        write_section(&mut output, ";@MAP", &format!("{stem}_MAP"), &self.asset.map, 20);
        write_section(
            &mut output,
            ";@ATTRIBUTES",
            &format!("{stem}_ATR"),
            &self.asset.attributes,
            20,
        );
        output
    }

    fn write_tiles(&self, output: &mut String, stem: &str) {
        output.push_str(";@TILES\n");
        output.push_str(&format!("{stem}_CHR\n"));
        for tile in 0..256 {
            output.push_str(&format!("; TILE ${tile:02X}\n"));
            for row in 0..8 {
                write_hex_line(output, &self.asset.tiles[tile * 32 + row * 4..][..4]);
            }
        }
    }

    pub fn is_palette_document(&self) -> bool {
        self.palette_document
    }

    pub fn palette_reference(&self) -> Option<&str> {
        self.palette_reference.as_deref()
    }

    pub fn palette(&self) -> &[u8] {
        &self.asset.palette
    }

    pub fn replace_palette(&mut self, palette: &[u8]) -> Result<(), String> {
        if palette.len() != PALETTE_BYTES {
            return Err("PALETTE MUST CONTAIN EXACTLY 256 RGB332 BYTES".to_owned());
        }
        self.asset.palette.copy_from_slice(palette);
        self.palette_preset = palette_preset_for_bank(&self.asset.palette, self.palette_bank);
        Ok(())
    }

    pub fn undo(&mut self) -> bool {
        let Some(asset) = self.undo.pop() else { return false };
        self.asset = asset;
        self.palette_preset = palette_preset_for_bank(&self.asset.palette, self.palette_bank);
        true
    }

    pub fn copy(&mut self) {
        let start = usize::from(self.selected_tile) * 32;
        self.clipboard = self.asset.tiles[start..start + 32].try_into().ok();
    }

    pub fn paste(&mut self) -> bool {
        let Some(tile) = self.clipboard else { return false };
        self.record_undo();
        let start = usize::from(self.selected_tile) * 32;
        self.asset.tiles[start..start + 32].copy_from_slice(&tile);
        true
    }

    pub fn handle_key(&mut self, key: &Key, modifiers: ModifiersState) -> bool {
        match key {
            Key::Character(text) => match text.to_ascii_lowercase().as_str() {
                "1" | "2" | "3" | "5" if self.palette_document => return false,
                "1" => {
                    self.view = GraphicsView::Tiles;
                }
                "2" => {
                    self.view = GraphicsView::Map;
                    self.bitmap_asset = false;
                }
                "3" => {
                    self.view = GraphicsView::Sprite;
                }
                "4" => self.view = GraphicsView::Palette,
                "5" => {
                    self.view = GraphicsView::Bitmap;
                    self.bitmap_asset = true;
                }
                "," if self.bitmap_asset => self.bitmap_bank = self.bitmap_bank.saturating_sub(1),
                "." if self.bitmap_asset => {
                    self.bitmap_bank = self.bitmap_bank.saturating_add(1).min(253)
                }
                "p" => self.tool = GraphicsTool::Pencil,
                "f" => self.tool = GraphicsTool::Fill,
                "i" => self.tool = GraphicsTool::Eyedropper,
                "h" if self.view == GraphicsView::Map => self.map_h_flip = !self.map_h_flip,
                "v" if self.view == GraphicsView::Map => self.map_v_flip = !self.map_v_flip,
                "q" if self.view == GraphicsView::Map => self.map_priority = !self.map_priority,
                "h" => return self.transform_tile(TileTransform::FlipHorizontal),
                "v" => return self.transform_tile(TileTransform::FlipVertical),
                "r" if self.view == GraphicsView::Palette => {
                    return self.adjust_palette(5, modifiers.shift_key());
                }
                "g" if self.view == GraphicsView::Palette => {
                    return self.adjust_palette(2, modifiers.shift_key());
                }
                "b" if self.view == GraphicsView::Palette => {
                    return self.adjust_palette(0, modifiers.shift_key());
                }
                "n" if self.view == GraphicsView::Palette => {
                    let current = self.palette_preset.unwrap_or(0);
                    let next = if modifiers.shift_key() {
                        (current + PALETTE_PRESETS.len() - 1) % PALETTE_PRESETS.len()
                    } else {
                        (current + 1) % PALETTE_PRESETS.len()
                    };
                    return self.apply_palette_preset(next);
                }
                "r" => return self.transform_tile(TileTransform::RotateClockwise),
                _ => return false,
            },
            Key::Named(NamedKey::Delete | NamedKey::Backspace) => {
                self.record_undo();
                if self.view == GraphicsView::Bitmap {
                    self.asset.bitmap.fill(0);
                } else {
                    let start = usize::from(self.selected_tile) * 32;
                    self.asset.tiles[start..start + 32].fill(0);
                }
                return true;
            }
            Key::Named(NamedKey::ArrowLeft) => {
                self.selected_tile = self.selected_tile.wrapping_sub(1)
            }
            Key::Named(NamedKey::ArrowRight) => {
                self.selected_tile = self.selected_tile.wrapping_add(1)
            }
            Key::Named(NamedKey::ArrowUp) => {
                self.selected_tile = self.selected_tile.wrapping_sub(16)
            }
            Key::Named(NamedKey::ArrowDown) => {
                self.selected_tile = self.selected_tile.wrapping_add(16)
            }
            _ => return false,
        }
        false
    }

    pub fn handle_mouse_press(&mut self, x: usize, y: usize) -> bool {
        if let Some(view) = view_button_at(x, y) {
            if self.palette_document && view != GraphicsView::Palette {
                return false;
            }
            self.view = view;
            if view == GraphicsView::Bitmap {
                self.bitmap_asset = true;
            } else if view == GraphicsView::Map {
                self.bitmap_asset = false;
            }
            return false;
        }
        if let Some(tool) = tool_button_at(x, y) {
            self.tool = tool;
            return false;
        }
        if self.view == GraphicsView::Palette
            && let Some(preset) = preset_button_at(x, y)
        {
            return self.apply_palette_preset(preset);
        }
        if self.view == GraphicsView::Palette
            && let Some(color) = palette_swatch_at(x, y)
        {
            self.palette_bank = (color / 16) as u8;
            self.selected_color = (color % 16) as u8;
            self.palette_preset = palette_preset_for_bank(&self.asset.palette, self.palette_bank);
            return false;
        }
        let strip = match self.view {
            GraphicsView::Tiles | GraphicsView::Sprite => {
                palette_strip_at(x, y, PANE_LEFT + 12, PANE_TOP + 318)
            }
            GraphicsView::Map => palette_strip_at(x, y, PANE_LEFT + 4, PANE_TOP + 278),
            GraphicsView::Palette => None,
            GraphicsView::Bitmap => bitmap_palette_at(x, y),
        };
        if let Some(color) = strip {
            self.selected_color = color as u8;
            return false;
        }
        let tile = if self.view == GraphicsView::Map {
            tile_sheet_at(x, y, (PANE_LEFT + 336, PANE_TOP + 38), 8)
        } else {
            tile_sheet_at(x, y, (PANE_LEFT + 252, PANE_TOP + 38), 12)
        };
        if let Some(tile) = tile {
            self.selected_tile = tile as u8;
            return false;
        }
        self.record_undo();
        self.drawing = true;
        self.stroke_changed = self.apply_at(x, y);
        if !self.stroke_changed {
            self.undo.pop();
        }
        self.stroke_changed
    }

    pub fn handle_mouse_move(&mut self, x: usize, y: usize) -> bool {
        if !self.drawing || self.tool != GraphicsTool::Pencil {
            return false;
        }
        let changed = self.apply_at(x, y);
        self.stroke_changed |= changed;
        changed
    }

    pub fn handle_mouse_release(&mut self) {
        self.drawing = false;
        self.stroke_changed = false;
    }

    pub fn render(&self, video: &mut Video) {
        fill_rect(video, PANE_LEFT, OUTER_TOP, EDITOR_DISPLAY_WIDTH - PANE_LEFT, 368, UI_BLACK);
        let workspace_caption = if self.palette_document {
            "SHARED PALETTE RESOURCE - 16 BANKS X 16 COLORS"
        } else {
            match self.view {
                GraphicsView::Tiles => "GRAPHICS - SHARED 8X8 PATTERN",
                GraphicsView::Map => "GRAPHICS - BACKGROUND MAP",
                GraphicsView::Sprite => "GRAPHICS - 16X16 SPRITE COMPOSITE",
                GraphicsView::Palette => "GRAPHICS - COLOR PALETTE",
                GraphicsView::Bitmap => "GRAPHICS - FULL-SCREEN BITMAP",
            }
        };
        draw_group_box(
            video,
            PANE_LEFT,
            OUTER_TOP,
            EDITOR_DISPLAY_WIDTH - PANE_LEFT,
            368,
            workspace_caption,
        );
        if self.palette_document {
            draw_text(
                video,
                PANE_LEFT + 4,
                PANE_TOP + 4,
                "SHARED BY EVERY GFX FILE THAT REFERENCES THIS PALETTE",
                UI_WHITE,
            );
            draw_text(
                video,
                PANE_LEFT + 4,
                PANE_TOP + 18,
                "N PRESET   SHIFT+N PREVIOUS   R/G/B EDIT",
                UI_GRAY,
            );
        } else {
            draw_toolbar(video, self.view, self.tool);
        }
        match self.view {
            GraphicsView::Tiles => self.render_tiles(video, false),
            GraphicsView::Sprite => self.render_tiles(video, true),
            GraphicsView::Map => self.render_map(video),
            GraphicsView::Palette => self.render_palette(video),
            GraphicsView::Bitmap => self.render_bitmap(video),
        }
    }

    pub fn status(&self) -> String {
        if self.palette_document {
            return format!(
                " SHARED PALETTE RESOURCE - BANK {} COLOR {:X}  CHANGES AFFECT ALL REFERENCES",
                self.palette_bank, self.selected_color
            );
        }
        match self.view {
            GraphicsView::Tiles => format!(
                " 8X8 PATTERN ${:02X} - MAP + SPRITES  BG={} PAL {} COLOR {:X}",
                self.selected_tile,
                if self.bitmap_asset { "BITMAP" } else { "MAP" },
                self.palette_bank,
                self.selected_color
            ),
            GraphicsView::Map => format!(
                " BACKGROUND MAP - PLACE PATTERN ${:02X}  PAL {} COLOR {:X}",
                self.selected_tile, self.palette_bank, self.selected_color
            ),
            GraphicsView::Sprite => {
                let first = self.selected_tile & !3;
                format!(
                    " 16X16 SPRITE - PATTERNS ${first:02X}-${:02X}  BG={} PAL {} COLOR {:X}",
                    first + 3,
                    if self.bitmap_asset { "BITMAP" } else { "MAP" },
                    self.palette_bank,
                    self.selected_color
                )
            }
            GraphicsView::Palette => match &self.palette_reference {
                Some(reference) => format!(
                    " SHARED {reference} - BANK {} COLOR {:X}  SAVES TO PALETTE RESOURCE",
                    self.palette_bank, self.selected_color
                ),
                None => format!(
                    " EMBEDDED PALETTE - BANK {} COLOR {:X}  USED BY THIS GFX SET",
                    self.palette_bank, self.selected_color
                ),
            },
            GraphicsView::Bitmap => format!(
                " 320X200 BITMAP - PAL {} COLOR {:X}  ROM BANKS {}-{} (+SPRITE CHR)",
                self.palette_bank,
                self.selected_color,
                self.bitmap_bank,
                self.bitmap_bank + 2
            ),
        }
    }

    fn render_tiles(&self, video: &mut Video, sprite: bool) {
        let size = if sprite { 16 } else { 8 };
        let scale = if sprite { 14 } else { 28 };
        let origin = (PANE_LEFT + 12, PANE_TOP + 38);
        let first = self.selected_tile & !3;
        let canvas_caption = if sprite {
            format!("16X16 SPRITE ${first:02X}-${:02X}", first + 3)
        } else {
            format!("8X8 PATTERN ${:02X}", self.selected_tile)
        };
        draw_group_box(video, PANE_LEFT + 6, PANE_TOP + 32, 236, 238, &canvas_caption);
        draw_group_box(video, PANE_LEFT + 246, PANE_TOP + 32, 220, 206, "SHARED 8X8 PATTERNS");
        draw_group_box(
            video,
            PANE_LEFT + 6,
            PANE_TOP + 308,
            460,
            38,
            &format!("PALETTE BANK {}", self.palette_bank),
        );
        for py in 0..size {
            for px in 0..size {
                let color = if sprite {
                    let tile = first.wrapping_add((py / 8 * 2 + px / 8) as u8);
                    self.tile_pixel(tile, px % 8, py % 8)
                } else {
                    self.tile_pixel(self.selected_tile, px, py)
                };
                let palette_index = usize::from(self.palette_bank) * 16 + usize::from(color);
                fill_rect(
                    video,
                    origin.0 + px * scale,
                    origin.1 + py * scale,
                    scale.saturating_sub(1),
                    scale.saturating_sub(1),
                    self.asset.palette[palette_index],
                );
            }
        }
        self.render_tile_sheet(video, (PANE_LEFT + 252, PANE_TOP + 38), 12);
        let relationship = if sprite {
            "8X8 SPRITES USE ONE PATTERN (MODE 1)"
        } else {
            "ONE PATTERN CAN BE A MAP TILE OR 8X8 SPRITE"
        };
        draw_text(video, PANE_LEFT + 12, PANE_TOP + 282, relationship, UI_GRAY);
        self.render_palette_strip(video, PANE_LEFT + 12, PANE_TOP + 318);
    }

    fn render_tile_sheet(&self, video: &mut Video, origin: (usize, usize), pitch: usize) {
        for tile in 0..256 {
            let tx = tile % 16;
            let ty = tile / 16;
            for py in 0..8 {
                for px in 0..8 {
                    let color = self.tile_pixel(tile as u8, px, py);
                    let index = usize::from(self.palette_bank) * 16 + usize::from(color);
                    let display = self.asset.palette[index];
                    put_pixel(
                        video,
                        origin.0 + tx * pitch + px,
                        origin.1 + ty * pitch + py,
                        display,
                    );
                }
            }
            if tile == usize::from(self.selected_tile) {
                stroke_rect(
                    video,
                    origin.0 + tx * pitch,
                    origin.1 + ty * pitch,
                    pitch.min(9),
                    pitch.min(9),
                    UI_WHITE,
                );
            }
        }
    }

    fn render_palette_strip(&self, video: &mut Video, x: usize, y: usize) {
        for color in 0..16 {
            let index = usize::from(self.palette_bank) * 16 + color;
            fill_rect(video, x + color * 24, y, 22, 20, self.asset.palette[index]);
            if color == usize::from(self.selected_color) {
                stroke_rect(video, x + color * 24, y, 22, 20, UI_WHITE);
            }
        }
    }

    fn render_map(&self, video: &mut Video) {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        draw_group_box(video, PANE_LEFT + 2, PANE_TOP + 32, 326, 210, "40 X 25 BACKGROUND MAP");
        draw_group_box(video, PANE_LEFT + 332, PANE_TOP + 32, 136, 142, "8X8 PATTERNS");
        draw_group_box(video, PANE_LEFT + 332, PANE_TOP + 180, 136, 62, "CELL OPTIONS");
        draw_group_box(
            video,
            PANE_LEFT + 2,
            PANE_TOP + 270,
            466,
            38,
            &format!("PALETTE BANK {}", self.palette_bank),
        );
        for cell_y in 0..25 {
            for cell_x in 0..40 {
                let cell = cell_y * 40 + cell_x;
                let tile = self.asset.map[cell];
                let attribute = self.asset.attributes[cell];
                for py in 0..8 {
                    for px in 0..8 {
                        let source_x = if attribute & 0x10 != 0 { 7 - px } else { px };
                        let source_y = if attribute & 0x20 != 0 { 7 - py } else { py };
                        let color = self.tile_pixel(tile, source_x, source_y);
                        let index = usize::from(attribute & 15) * 16 + usize::from(color);
                        put_pixel(
                            video,
                            origin.0 + cell_x * 8 + px,
                            origin.1 + cell_y * 8 + py,
                            self.asset.palette[index],
                        );
                    }
                }
            }
        }
        self.render_tile_sheet(video, (PANE_LEFT + 336, PANE_TOP + 38), 8);
        draw_text(video, PANE_LEFT + 338, PANE_TOP + 194, "H/V FLIP", UI_GRAY);
        draw_text(video, PANE_LEFT + 338, PANE_TOP + 210, "Q PRIORITY", UI_GRAY);
        draw_text(
            video,
            PANE_LEFT + 8,
            PANE_TOP + 252,
            "MAP CELLS REFERENCE THE SHARED 8X8 PATTERNS",
            UI_GRAY,
        );
        self.render_palette_strip(video, PANE_LEFT + 4, PANE_TOP + 278);
    }

    fn render_palette(&self, video: &mut Video) {
        let origin = (PANE_LEFT + 28, PANE_TOP + 38);
        draw_group_box(video, PANE_LEFT + 20, PANE_TOP + 32, 328, 300, "256-COLOR PALETTE");
        draw_group_box(video, PANE_LEFT + 352, PANE_TOP + 32, 114, 204, "PRESETS");
        for index in 0..256 {
            let x = index % 16;
            let y = index / 16;
            fill_rect(
                video,
                origin.0 + x * 20,
                origin.1 + y * 18,
                18,
                16,
                self.asset.palette[index],
            );
            if index == usize::from(self.palette_bank) * 16 + usize::from(self.selected_color) {
                stroke_rect(video, origin.0 + x * 20 - 1, origin.1 + y * 18 - 1, 20, 18, UI_WHITE);
            }
        }
        let index = usize::from(self.palette_bank) * 16 + usize::from(self.selected_color);
        let value = self.asset.palette[index];
        draw_text(video, PANE_LEFT + 358, PANE_TOP + 50, &format!("INDEX ${index:02X}"), UI_WHITE);
        draw_text(video, PANE_LEFT + 358, PANE_TOP + 66, &format!("RGB332 ${value:02X}"), UI_WHITE);
        for (preset, (name, _)) in PALETTE_PRESETS.iter().enumerate() {
            let y = PANE_TOP + 92 + preset * 24;
            if Some(preset) == self.palette_preset {
                fill_rect(video, PANE_LEFT + 358, y - 2, 98, 18, UI_BLUE);
            }
            draw_text(video, PANE_LEFT + 364, y, name, UI_WHITE);
        }
        draw_text(video, PANE_LEFT + 358, PANE_TOP + 190, "N NEXT", UI_GRAY);
        draw_text(video, PANE_LEFT + 358, PANE_TOP + 206, "R/G/B EDIT", UI_GRAY);
    }

    fn render_bitmap(&self, video: &mut Video) {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        draw_group_box(video, PANE_LEFT + 2, PANE_TOP + 32, 326, 210, "320 X 200 BITMAP");
        draw_group_box(video, PANE_LEFT + 332, PANE_TOP + 32, 136, 166, "BITMAP SETTINGS");
        for y in 0..200 {
            for x in 0..320 {
                let color = self.bitmap_pixel(x, y);
                let index = usize::from(self.palette_bank) * 16 + usize::from(color);
                put_pixel(video, origin.0 + x, origin.1 + y, self.asset.palette[index]);
            }
        }
        draw_text(
            video,
            PANE_LEFT + 332,
            PANE_TOP + 42,
            &format!("ROM {}-{}", self.bitmap_bank, self.bitmap_bank.wrapping_add(2)),
            UI_WHITE,
        );
        draw_text(video, PANE_LEFT + 332, PANE_TOP + 58, "BM+BM+CHR", UI_GRAY);
        draw_text(video, PANE_LEFT + 332, PANE_TOP + 72, ",/. BANK", UI_GRAY);
        for color in 0..16 {
            let index = usize::from(self.palette_bank) * 16 + color;
            fill_rect(
                video,
                PANE_LEFT + 336 + color % 4 * 28,
                PANE_TOP + 88 + color / 4 * 24,
                24,
                20,
                self.asset.palette[index],
            );
        }
    }

    fn apply_at(&mut self, x: usize, y: usize) -> bool {
        match self.view {
            GraphicsView::Tiles | GraphicsView::Sprite => self.apply_canvas(x, y),
            GraphicsView::Map => self.apply_map(x, y),
            GraphicsView::Palette => false,
            GraphicsView::Bitmap => self.apply_bitmap(x, y),
        }
    }

    fn apply_canvas(&mut self, x: usize, y: usize) -> bool {
        let sprite = self.view == GraphicsView::Sprite;
        let size = if sprite { 16 } else { 8 };
        let scale = if sprite { 14 } else { 28 };
        let origin = (PANE_LEFT + 12, PANE_TOP + 38);
        let Some(px) =
            x.checked_sub(origin.0).map(|value| value / scale).filter(|value| *value < size)
        else {
            return false;
        };
        let Some(py) =
            y.checked_sub(origin.1).map(|value| value / scale).filter(|value| *value < size)
        else {
            return false;
        };
        let (tile, tx, ty) = if sprite {
            (self.selected_tile & !3 | (py / 8 * 2 + px / 8) as u8, px % 8, py % 8)
        } else {
            (self.selected_tile, px, py)
        };
        let old = self.tile_pixel(tile, tx, ty);
        if self.tool == GraphicsTool::Eyedropper {
            self.selected_color = old;
            return false;
        }
        if self.tool == GraphicsTool::Fill {
            return self.flood_tile(tile, tx, ty, old, self.selected_color);
        }
        if old == self.selected_color {
            return false;
        }
        self.set_tile_pixel(tile, tx, ty, self.selected_color);
        true
    }

    fn apply_map(&mut self, x: usize, y: usize) -> bool {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        let Some(cell_x) =
            x.checked_sub(origin.0).map(|value| value / 8).filter(|value| *value < 40)
        else {
            return false;
        };
        let Some(cell_y) =
            y.checked_sub(origin.1).map(|value| value / 8).filter(|value| *value < 25)
        else {
            return false;
        };
        let cell = cell_y * 40 + cell_x;
        if self.tool == GraphicsTool::Eyedropper {
            self.selected_tile = self.asset.map[cell];
            let attribute = self.asset.attributes[cell];
            self.palette_bank = attribute & 15;
            self.palette_preset = palette_preset_for_bank(&self.asset.palette, self.palette_bank);
            self.map_h_flip = attribute & 0x10 != 0;
            self.map_v_flip = attribute & 0x20 != 0;
            self.map_priority = attribute & 0x40 != 0;
            return false;
        }
        let attribute = self.palette_bank
            | u8::from(self.map_h_flip) << 4
            | u8::from(self.map_v_flip) << 5
            | u8::from(self.map_priority) << 6;
        if self.tool == GraphicsTool::Fill {
            let old_tile = self.asset.map[cell];
            let old_attribute = self.asset.attributes[cell];
            if old_tile == self.selected_tile && old_attribute == attribute {
                return false;
            }
            let mut stack = vec![(cell_x, cell_y)];
            while let Some((x, y)) = stack.pop() {
                let index = y * 40 + x;
                if self.asset.map[index] != old_tile
                    || self.asset.attributes[index] != old_attribute
                {
                    continue;
                }
                self.asset.map[index] = self.selected_tile;
                self.asset.attributes[index] = attribute;
                if x > 0 {
                    stack.push((x - 1, y));
                }
                if x < 39 {
                    stack.push((x + 1, y));
                }
                if y > 0 {
                    stack.push((x, y - 1));
                }
                if y < 24 {
                    stack.push((x, y + 1));
                }
            }
            return true;
        }
        if self.asset.map[cell] == self.selected_tile && self.asset.attributes[cell] == attribute {
            return false;
        }
        self.asset.map[cell] = self.selected_tile;
        self.asset.attributes[cell] = attribute;
        true
    }

    fn apply_bitmap(&mut self, x: usize, y: usize) -> bool {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        let Some(px) = x.checked_sub(origin.0).filter(|value| *value < 320) else { return false };
        let Some(py) = y.checked_sub(origin.1).filter(|value| *value < 200) else { return false };
        let old = self.bitmap_pixel(px, py);
        if self.tool == GraphicsTool::Eyedropper {
            self.selected_color = old;
            return false;
        }
        if self.tool == GraphicsTool::Fill {
            return self.flood_bitmap(px, py, old, self.selected_color);
        }
        if old == self.selected_color {
            return false;
        }
        self.set_bitmap_pixel(px, py, self.selected_color);
        true
    }

    fn tile_pixel(&self, tile: u8, x: usize, y: usize) -> u8 {
        let byte = self.asset.tiles[usize::from(tile) * 32 + y * 4 + x / 2];
        if x.is_multiple_of(2) { byte >> 4 } else { byte & 15 }
    }

    fn set_tile_pixel(&mut self, tile: u8, x: usize, y: usize, color: u8) {
        let index = usize::from(tile) * 32 + y * 4 + x / 2;
        if x.is_multiple_of(2) {
            self.asset.tiles[index] = self.asset.tiles[index] & 15 | color << 4;
        } else {
            self.asset.tiles[index] = self.asset.tiles[index] & 0xf0 | color;
        }
    }

    fn bitmap_pixel(&self, x: usize, y: usize) -> u8 {
        let byte = self.asset.bitmap[y * 160 + x / 2];
        if x.is_multiple_of(2) { byte >> 4 } else { byte & 15 }
    }

    fn set_bitmap_pixel(&mut self, x: usize, y: usize, color: u8) {
        let index = y * 160 + x / 2;
        if x.is_multiple_of(2) {
            self.asset.bitmap[index] = self.asset.bitmap[index] & 15 | color << 4;
        } else {
            self.asset.bitmap[index] = self.asset.bitmap[index] & 0xf0 | color;
        }
    }

    fn flood_tile(&mut self, tile: u8, x: usize, y: usize, old: u8, new: u8) -> bool {
        if old == new {
            return false;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if self.tile_pixel(tile, x, y) != old {
                continue;
            }
            self.set_tile_pixel(tile, x, y, new);
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x < 7 {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y < 7 {
                stack.push((x, y + 1));
            }
        }
        true
    }

    fn flood_bitmap(&mut self, x: usize, y: usize, old: u8, new: u8) -> bool {
        if old == new {
            return false;
        }
        let mut stack = vec![(x, y)];
        while let Some((x, y)) = stack.pop() {
            if self.bitmap_pixel(x, y) != old {
                continue;
            }
            self.set_bitmap_pixel(x, y, new);
            if x > 0 {
                stack.push((x - 1, y));
            }
            if x < 319 {
                stack.push((x + 1, y));
            }
            if y > 0 {
                stack.push((x, y - 1));
            }
            if y < 199 {
                stack.push((x, y + 1));
            }
        }
        true
    }

    fn transform_tile(&mut self, transform: TileTransform) -> bool {
        self.record_undo();
        let tile = self.selected_tile;
        let mut pixels = [[0; 8]; 8];
        for (y, row) in pixels.iter_mut().enumerate() {
            for (x, pixel) in row.iter_mut().enumerate() {
                *pixel = self.tile_pixel(tile, x, y);
            }
        }
        for y in 0..8 {
            for x in 0..8 {
                let value = match transform {
                    TileTransform::FlipHorizontal => pixels[y][7 - x],
                    TileTransform::FlipVertical => pixels[7 - y][x],
                    TileTransform::RotateClockwise => pixels[7 - x][y],
                };
                self.set_tile_pixel(tile, x, y, value);
            }
        }
        true
    }

    fn adjust_palette(&mut self, shift: u8, decrement: bool) -> bool {
        self.record_undo();
        let index = usize::from(self.palette_bank) * 16 + usize::from(self.selected_color);
        let width = if shift == 0 { 2 } else { 3 };
        let mask = ((1 << width) - 1) << shift;
        let component = (self.asset.palette[index] & mask) >> shift;
        let next = if decrement {
            component.saturating_sub(1)
        } else {
            (component + 1).min((1 << width) - 1)
        };
        if component == next {
            self.undo.pop();
            return false;
        }
        self.asset.palette[index] = self.asset.palette[index] & !mask | next << shift;
        self.palette_preset = None;
        true
    }

    fn apply_palette_preset(&mut self, preset: usize) -> bool {
        let preset = preset.min(PALETTE_PRESETS.len() - 1);
        let start = usize::from(self.palette_bank) * 16;
        if self.asset.palette[start..start + 16] == PALETTE_PRESETS[preset].1 {
            self.palette_preset = Some(preset);
            return false;
        }
        self.record_undo();
        self.asset.palette[start..start + 16].copy_from_slice(&PALETTE_PRESETS[preset].1);
        self.palette_preset = Some(preset);
        true
    }

    fn record_undo(&mut self) {
        if self.undo.len() == 32 {
            self.undo.remove(0);
        }
        self.undo.push(self.asset.clone());
    }
}

#[derive(Clone, Copy)]
enum TileTransform {
    FlipHorizontal,
    FlipVertical,
    RotateClockwise,
}

fn parse_section(source: &str, marker: &str, expected: usize) -> Result<Vec<u8>, String> {
    let mut active = false;
    let mut bytes = Vec::new();
    for line in source.lines() {
        let trimmed = line.trim();
        if trimmed == marker {
            active = true;
            continue;
        }
        if active && trimmed.starts_with(";@") {
            break;
        }
        if active {
            let mut fields = trimmed.split_whitespace();
            let first = fields.next().unwrap_or("");
            let is_hex = first.eq_ignore_ascii_case("HEX")
                || fields.next().is_some_and(|field| field.eq_ignore_ascii_case("HEX"));
            if !is_hex {
                continue;
            }
            let hex = fields.collect::<String>();
            let compact =
                hex.chars().filter(|character| character.is_ascii_hexdigit()).collect::<String>();
            if !compact.len().is_multiple_of(2) {
                return Err(format!("{marker} HAS AN ODD HEX DIGIT"));
            }
            for pair in compact.as_bytes().chunks_exact(2) {
                let text = core::str::from_utf8(pair).expect("ASCII hex");
                bytes.push(
                    u8::from_str_radix(text, 16).map_err(|_| format!("INVALID HEX IN {marker}"))?,
                );
            }
        }
    }
    if bytes.len() != expected {
        return Err(format!("{marker} EXPECTED {expected} BYTES, FOUND {}", bytes.len()));
    }
    Ok(bytes)
}

fn write_section(output: &mut String, marker: &str, label: &str, bytes: &[u8], width: usize) {
    output.push_str(marker);
    output.push('\n');
    output.push_str(label);
    output.push('\n');
    for chunk in bytes.chunks(width) {
        write_hex_line(output, chunk);
    }
}

fn write_hex_line(output: &mut String, bytes: &[u8]) {
    output.push_str("         HEX   ");
    for byte in bytes {
        output.push_str(&format!("{byte:02X}"));
    }
    output.push('\n');
}

fn draw_toolbar(video: &mut Video, view: GraphicsView, tool: GraphicsTool) {
    draw_text(
        video,
        PANE_LEFT + 4,
        PANE_TOP + 4,
        "1 PATTERN  2 MAP  3 16X16 SPRITE  4 PALETTE  5 BITMAP",
        UI_WHITE,
    );
    draw_text(video, PANE_LEFT + 4, PANE_TOP + 18, "P PENCIL  F FILL  I PICK", UI_GRAY);
    let guide = match view {
        GraphicsView::Tiles => "SHARED BY MAP+SPRITES",
        GraphicsView::Map => "PLACE 8X8 PATTERNS",
        GraphicsView::Sprite => "4-PATTERN COMPOSITE",
        GraphicsView::Palette => "EDIT ONE 16-COLOR BANK",
        GraphicsView::Bitmap => "FULL-SCREEN PIXELS",
    };
    draw_text(video, PANE_LEFT + 220, PANE_TOP + 18, guide, UI_GRAY);
    let view_x = match view {
        GraphicsView::Tiles => 4,
        GraphicsView::Map => 92,
        GraphicsView::Sprite => 148,
        GraphicsView::Palette => 276,
        GraphicsView::Bitmap => 364,
    };
    stroke_rect(
        video,
        PANE_LEFT + view_x,
        PANE_TOP + 2,
        match view {
            GraphicsView::Tiles => 72,
            GraphicsView::Map => 40,
            GraphicsView::Sprite => 112,
            GraphicsView::Palette => 72,
            GraphicsView::Bitmap => 64,
        },
        12,
        UI_BLUE,
    );
    let tool_x = match tool {
        GraphicsTool::Pencil => 4,
        GraphicsTool::Fill => 84,
        GraphicsTool::Eyedropper => 140,
    };
    stroke_rect(
        video,
        PANE_LEFT + tool_x,
        PANE_TOP + 16,
        match tool {
            GraphicsTool::Pencil => 64,
            GraphicsTool::Fill => 40,
            GraphicsTool::Eyedropper => 56,
        },
        12,
        UI_BLUE,
    );
}

fn view_button_at(x: usize, y: usize) -> Option<GraphicsView> {
    if !(PANE_TOP..PANE_TOP + 16).contains(&y) {
        return None;
    }
    match x.checked_sub(PANE_LEFT)? {
        0..=80 => Some(GraphicsView::Tiles),
        81..=136 => Some(GraphicsView::Map),
        137..=264 => Some(GraphicsView::Sprite),
        265..=352 => Some(GraphicsView::Palette),
        353..=432 => Some(GraphicsView::Bitmap),
        _ => None,
    }
}

fn tool_button_at(x: usize, y: usize) -> Option<GraphicsTool> {
    if !(PANE_TOP + 16..PANE_TOP + 32).contains(&y) {
        return None;
    }
    match x.checked_sub(PANE_LEFT)? {
        0..=76 => Some(GraphicsTool::Pencil),
        77..=136 => Some(GraphicsTool::Fill),
        137..=208 => Some(GraphicsTool::Eyedropper),
        _ => None,
    }
}

fn tile_sheet_at(x: usize, y: usize, origin: (usize, usize), pitch: usize) -> Option<usize> {
    let tx = x.checked_sub(origin.0)? / pitch;
    let ty = y.checked_sub(origin.1)? / pitch;
    (tx < 16 && ty < 16).then_some(ty * 16 + tx)
}

fn palette_swatch_at(x: usize, y: usize) -> Option<usize> {
    let origin = (PANE_LEFT + 28, PANE_TOP + 38);
    let px = x.checked_sub(origin.0)? / 20;
    let py = y.checked_sub(origin.1)? / 18;
    (px < 16 && py < 16).then_some(py * 16 + px)
}

fn palette_strip_at(x: usize, y: usize, origin_x: usize, origin_y: usize) -> Option<usize> {
    let color = x.checked_sub(origin_x)? / 24;
    (color < 16 && (origin_y..origin_y + 20).contains(&y)).then_some(color)
}

fn bitmap_palette_at(x: usize, y: usize) -> Option<usize> {
    let x = x.checked_sub(PANE_LEFT + 336)? / 28;
    let y = y.checked_sub(PANE_TOP + 88)? / 24;
    (x < 4 && y < 4).then_some(y * 4 + x)
}

fn preset_button_at(x: usize, y: usize) -> Option<usize> {
    let row = y.checked_sub(PANE_TOP + 88)? / 24;
    ((PANE_LEFT + 354..PANE_LEFT + 462).contains(&x) && row < PALETTE_PRESETS.len()).then_some(row)
}

fn draw_group_box(
    video: &mut Video,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    caption: &str,
) {
    stroke_rect(video, x, y, width, height, UI_GRAY);
    let caption_width = (caption.len() + 2) * GLYPH_WIDTH;
    fill_rect(video, x + 6, y.saturating_sub(3), caption_width, GLYPH_HEIGHT, UI_BLACK);
    draw_text(video, x + 10, y.saturating_sub(3), caption, UI_WHITE);
}

fn fill_rect(video: &mut Video, x: usize, y: usize, width: usize, height: usize, color: u8) {
    let dimensions = video.dimensions();
    let end_y = (y + height).min(dimensions.1);
    let end_x = (x + width).min(dimensions.0);
    let pixels = video.pixels_mut();
    for py in y.min(end_y)..end_y {
        pixels[py * dimensions.0 + x.min(end_x)..py * dimensions.0 + end_x].fill(color);
    }
}

fn stroke_rect(video: &mut Video, x: usize, y: usize, width: usize, height: usize, color: u8) {
    fill_rect(video, x, y, width, 1, color);
    fill_rect(video, x, y + height.saturating_sub(1), width, 1, color);
    fill_rect(video, x, y, 1, height, color);
    fill_rect(video, x + width.saturating_sub(1), y, 1, height, color);
}

fn put_pixel(video: &mut Video, x: usize, y: usize, color: u8) {
    let dimensions = video.dimensions();
    if x < dimensions.0 && y < dimensions.1 {
        video.pixels_mut()[y * dimensions.0 + x] = color;
    }
}

fn draw_text(video: &mut Video, x: usize, y: usize, text: &str, color: u8) {
    for (index, byte) in text.bytes().enumerate() {
        let glyph = CHARACTER_ROM[usize::from(byte.to_ascii_uppercase())];
        for (glyph_y, bits) in glyph.into_iter().enumerate() {
            for glyph_x in 0..GLYPH_WIDTH {
                if bits & (0x80 >> glyph_x) != 0 {
                    put_pixel(video, x + index * GLYPH_WIDTH + glyph_x, y + glyph_y, color);
                }
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trip_preserves_hardware_native_data() {
        let mut editor =
            GraphicsEditor { selected_tile: 3, selected_color: 12, ..GraphicsEditor::default() };
        editor.set_tile_pixel(3, 7, 7, 12);
        editor.asset.map[999] = 3;
        editor.asset.attributes[999] = 0x5f;
        let source = editor.serialize("hero.gfx");
        assert!(source.is_ascii());
        assert!(source.contains("HERO_CHR"));
        let restored = GraphicsEditor::parse(&source).unwrap();
        assert_eq!(restored.tile_pixel(3, 7, 7), 12);
        assert_eq!(restored.asset.map[999], 3);
        assert_eq!(restored.asset.attributes[999], 0x5f);
    }

    #[test]
    fn output_is_directly_accepted_by_the_assembler() {
        let source = GraphicsEditor::default().serialize("world.gfx");
        let program = fanticon::assembler::assemble(&source).unwrap();
        assert_eq!(program.bytes.len(), PALETTE_BYTES + TILE_BYTES + MAP_CELLS * 2);
        assert_eq!(program.symbols["WORLD_PAL"], 0);
        assert_eq!(program.symbols["WORLD_CHR"], PALETTE_BYTES as u16);
        assert_eq!(program.symbols["WORLD_MAP"], (PALETTE_BYTES + TILE_BYTES) as u16);
    }

    #[test]
    fn shared_palette_resources_round_trip_separately_from_graphics_data() {
        let mut palette = GraphicsEditor::palette_document();
        palette.asset.palette[17] = 0xa5;
        let palette_source = palette.serialize("game.pal");
        let restored_palette = GraphicsEditor::parse(&palette_source).unwrap();
        assert!(restored_palette.is_palette_document());
        assert_eq!(restored_palette.asset.palette[17], 0xa5);
        assert_eq!(fanticon::assembler::assemble(&palette_source).unwrap().bytes.len(), 256);

        let graphics = GraphicsEditor::with_shared_palette("GAME.PAL");
        let source = graphics.serialize("world.gfx");
        assert!(source.starts_with(";@FANTICON-GFX 2"));
        assert!(source.contains(";@PALETTE-FILE GAME.PAL"));
        assert!(!source.contains(";@PALETTE\n"));
        assert_eq!(GraphicsEditor::parse(&source).unwrap().palette_reference(), Some("GAME.PAL"));
        assert_eq!(
            fanticon::assembler::assemble(&source).unwrap().bytes.len(),
            TILE_BYTES + MAP_CELLS * 2
        );
    }

    #[test]
    fn packed_tile_pixels_match_vram_nibble_order() {
        let mut editor = GraphicsEditor::default();
        editor.set_tile_pixel(0, 0, 0, 0xa);
        editor.set_tile_pixel(0, 1, 0, 0x5);
        assert_eq!(editor.asset.tiles[0], 0xa5);
    }

    #[test]
    fn new_assets_use_db16_in_every_palette_bank() {
        let editor = GraphicsEditor::default();
        assert_eq!(DB16[0], 0x00, "DB16's dark purple is represented as RGB332 black");
        for bank in editor.asset.palette.chunks_exact(16) {
            assert_eq!(bank, DB16);
        }
        assert_eq!(editor.palette_preset, Some(0));
    }

    #[test]
    fn palette_presets_apply_to_one_bank_and_remain_undoable() {
        let mut editor = GraphicsEditor {
            palette_bank: 3,
            view: GraphicsView::Palette,
            ..GraphicsEditor::default()
        };
        assert!(editor.apply_palette_preset(1));
        assert_eq!(&editor.asset.palette[48..64], &PICO8);
        assert_eq!(&editor.asset.palette[0..16], &DB16);
        assert_eq!(editor.palette_preset, Some(1));
        assert!(editor.undo());
        assert_eq!(&editor.asset.palette[48..64], &DB16);
        assert_eq!(editor.palette_preset, Some(0));

        assert!(editor.adjust_palette(5, false));
        assert_eq!(editor.palette_preset, None);
    }

    #[test]
    fn graphics_workspace_has_a_visible_outer_border() {
        let editor = GraphicsEditor::default();
        let mut video = Video::new_with_size(EDITOR_DISPLAY_WIDTH, 400);
        editor.render(&mut video);
        assert_eq!(video.pixels()[OUTER_TOP * EDITOR_DISPLAY_WIDTH + PANE_LEFT], UI_GRAY);
    }

    #[test]
    fn mode_labels_explain_the_shared_pattern_model() {
        let mut editor = GraphicsEditor::default();
        assert!(editor.status().contains("MAP + SPRITES"));

        editor.handle_mouse_press(PANE_LEFT + 100, PANE_TOP + 6);
        assert_eq!(editor.view, GraphicsView::Map);
        assert!(editor.status().contains("PLACE PATTERN"));

        editor.selected_tile = 7;
        editor.handle_mouse_press(PANE_LEFT + 160, PANE_TOP + 6);
        assert_eq!(editor.view, GraphicsView::Sprite);
        assert!(editor.status().contains("PATTERNS $04-$07"));
    }

    #[test]
    fn editor_view_does_not_accidentally_change_bitmap_background_mode() {
        let mut editor = GraphicsEditor {
            bitmap_asset: true,
            view: GraphicsView::Bitmap,
            ..GraphicsEditor::default()
        };
        editor.handle_key(&Key::Character("3".into()), ModifiersState::empty());
        assert_eq!(editor.view, GraphicsView::Sprite);
        assert!(editor.bitmap_asset);

        editor.selected_color = 6;
        assert!(editor.handle_mouse_press(PANE_LEFT + 13, PANE_TOP + 39));
        editor.handle_mouse_release();
        let restored = GraphicsEditor::parse(&editor.serialize("scene.gfx")).unwrap();
        assert_eq!(restored.tile_pixel(0, 0, 0), 6);
        assert!(restored.bitmap_asset);

        editor.handle_key(&Key::Character("2".into()), ModifiersState::empty());
        assert_eq!(editor.view, GraphicsView::Map);
        assert!(!editor.bitmap_asset);
    }

    #[test]
    fn palette_resources_expose_only_the_palette_workspace() {
        let mut editor = GraphicsEditor::palette_document();
        editor.handle_key(&Key::Character("1".into()), ModifiersState::empty());
        assert_eq!(editor.view, GraphicsView::Palette);
        editor.handle_mouse_press(PANE_LEFT + 160, PANE_TOP + 6);
        assert_eq!(editor.view, GraphicsView::Palette);
        assert!(editor.status().contains("SHARED PALETTE RESOURCE"));
    }

    #[test]
    fn visual_pencil_fill_undo_and_palette_selection_edit_the_ascii_model() {
        let mut editor = GraphicsEditor { selected_color: 7, ..GraphicsEditor::default() };
        assert!(editor.handle_mouse_press(PANE_LEFT + 13, PANE_TOP + 39));
        editor.handle_mouse_release();
        assert_eq!(editor.tile_pixel(0, 0, 0), 7);
        assert!(editor.undo());
        assert_eq!(editor.tile_pixel(0, 0, 0), 0);

        editor.handle_mouse_press(PANE_LEFT + 12 + 5 * 24, PANE_TOP + 319);
        assert_eq!(editor.selected_color, 5);
        editor.tool = GraphicsTool::Fill;
        assert!(editor.handle_mouse_press(PANE_LEFT + 13, PANE_TOP + 39));
        assert!((0..8).all(|y| (0..8).all(|x| editor.tile_pixel(0, x, y) == 5)));
    }

    #[test]
    fn bitmap_ascii_uses_two_image_banks_and_one_resident_sprite_pattern_bank() {
        let mut editor = GraphicsEditor {
            bitmap_asset: true,
            view: GraphicsView::Bitmap,
            bitmap_bank: 7,
            ..GraphicsEditor::default()
        };
        editor.set_bitmap_pixel(319, 199, 0xe);
        let source = editor.serialize("title.gfx");
        let restored = GraphicsEditor::parse(&source).unwrap();
        assert_eq!(restored.bitmap_pixel(319, 199), 0xe);
        assert_eq!(restored.bitmap_bank, 7);
        assert!(source.contains("TITLE_CHR"));

        let wrapper = " PUT title.gfx\n FIXED\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ";
        let cartridge =
            fanticon::assembler::assemble_cartridge_with_loader("main.asm", wrapper, |path| {
                (path == "title.gfx").then(|| source.clone()).ok_or_else(|| "missing".to_owned())
            })
            .unwrap();
        assert_eq!(cartridge.rom_banks.len(), 10);
        assert_eq!(cartridge.rom_banks[7].len(), 0x4000);
        assert_eq!(cartridge.rom_banks[8][15_871] & 0x0f, 0xe);
        assert_eq!(cartridge.rom_banks[9].len(), 0x4000);
    }
}
