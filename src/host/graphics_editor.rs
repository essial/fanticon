use fanticon::{
    machine::{TILEMAP_CELLS, TILEMAP_HEIGHT, TILEMAP_WIDTH},
    video::rgb332_to_rgba,
};
use winit::keyboard::{Key, ModifiersState, NamedKey};

use super::EDITOR_DISPLAY_WIDTH;
use super::character_rom::{GLYPH_HEIGHT, GLYPH_WIDTH};
use super::surface::Surface;

pub const TILE_BYTES: usize = 256 * 32;
pub const MAP_CELLS: usize = TILEMAP_CELLS;
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
const MAP_VIEW_WIDTH: usize = 40;
const MAP_VIEW_HEIGHT: usize = 25;
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
    map_view_x: usize,
    map_view_y: usize,
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
            map_view_x: 0,
            map_view_y: 0,
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
        let graphics_version = source.lines().find_map(|line| {
            line.trim()
                .strip_prefix(";@FANTICON-GFX ")
                .and_then(|version| version.parse::<u8>().ok())
        });
        if !palette_document && !matches!(graphics_version, Some(1..=3)) {
            return Err("File is missing ;@FANTICON-GFX 1/2/3 or ;@FANTICON-PAL 1".to_owned());
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
                "Bitmap start bank must be 0-253 (bitmap + sprite patterns use 3 banks)".to_owned()
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
            let legacy_map = graphics_version.is_some_and(|version| version < 3);
            let map_bytes = if legacy_map { 40 * 25 } else { MAP_CELLS };
            (
                parse_section(source, ";@TILES", TILE_BYTES)?,
                expand_legacy_map(parse_section(source, ";@MAP", map_bytes)?, legacy_map),
                expand_legacy_map(parse_section(source, ";@ATTRIBUTES", map_bytes)?, legacy_map),
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
        let mut output =
            String::from(";@FANTICON-GFX 3\n; ASCII SOURCE - EDIT VISUALLY OR BY HAND\n");
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
            return Err("Palette must contain exactly 256 RGB332 bytes".to_owned());
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

    /// Move to another of the 16 palette banks, wrapping at both ends. Every view
    /// that draws through a bank re-reads it immediately, so artwork recolors live.
    fn step_palette_bank(&mut self, step: i16) {
        let banks = (PALETTE_BYTES / 16) as i16;
        let bank = (i16::from(self.palette_bank) + step).rem_euclid(banks);
        self.palette_bank = bank as u8;
        self.palette_preset = palette_preset_for_bank(&self.asset.palette, self.palette_bank);
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
                "[" => self.step_palette_bank(-1),
                "]" => self.step_palette_bank(1),
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
                if self.view == GraphicsView::Map {
                    self.map_view_x = (self.map_view_x + TILEMAP_WIDTH - 1) % TILEMAP_WIDTH;
                } else {
                    self.selected_tile = self.selected_tile.wrapping_sub(1);
                }
            }
            Key::Named(NamedKey::ArrowRight) => {
                if self.view == GraphicsView::Map {
                    self.map_view_x = (self.map_view_x + 1) % TILEMAP_WIDTH;
                } else {
                    self.selected_tile = self.selected_tile.wrapping_add(1);
                }
            }
            Key::Named(NamedKey::ArrowUp) => {
                if self.view == GraphicsView::Map {
                    self.map_view_y = (self.map_view_y + TILEMAP_HEIGHT - 1) % TILEMAP_HEIGHT;
                } else {
                    self.selected_tile = self.selected_tile.wrapping_sub(16);
                }
            }
            Key::Named(NamedKey::ArrowDown) => {
                if self.view == GraphicsView::Map {
                    self.map_view_y = (self.map_view_y + 1) % TILEMAP_HEIGHT;
                } else {
                    self.selected_tile = self.selected_tile.wrapping_add(16);
                }
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
        if let Some((origin_x, origin_y)) = bank_button_origin(self.view)
            && let Some(step) = bank_button_at(x, y, origin_x, origin_y)
        {
            self.step_palette_bank(step);
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

    pub fn render(&self, surface: &mut Surface) {
        fill_rect(surface, PANE_LEFT, OUTER_TOP, EDITOR_DISPLAY_WIDTH - PANE_LEFT, 368, UI_BLACK);
        let workspace_caption = if self.palette_document {
            "Shared Palette Resource - 16 Banks x 16 Colors"
        } else {
            match self.view {
                GraphicsView::Tiles => "Graphics - Shared 8x8 Pattern (Map + Sprites)",
                GraphicsView::Map => "Graphics - Background Map (Tilemap Mode)",
                GraphicsView::Sprite => "Graphics - 16x16 Sprite Composite",
                GraphicsView::Palette => "Graphics - Color Palette",
                GraphicsView::Bitmap => "Graphics - Full-Screen Bitmap (Bitmap Mode)",
            }
        };
        draw_group_box(
            surface,
            PANE_LEFT,
            OUTER_TOP,
            EDITOR_DISPLAY_WIDTH - PANE_LEFT,
            368,
            workspace_caption,
        );
        if self.palette_document {
            draw_text(
                surface,
                PANE_LEFT + 4,
                PANE_TOP + 4,
                "Shared by every GFX file that references this palette",
                UI_WHITE,
            );
            draw_text(
                surface,
                PANE_LEFT + 4,
                PANE_TOP + 18,
                "N Preset   Shift+N Previous   R/G/B Edit",
                UI_GRAY,
            );
        } else {
            draw_toolbar(surface, self.view, self.tool, self.bitmap_asset);
        }
        match self.view {
            GraphicsView::Tiles => self.render_tiles(surface, false),
            GraphicsView::Sprite => self.render_tiles(surface, true),
            GraphicsView::Map => self.render_map(surface),
            GraphicsView::Palette => self.render_palette(surface),
            GraphicsView::Bitmap => self.render_bitmap(surface),
        }
    }

    pub fn status(&self) -> String {
        if self.palette_document {
            return format!(
                " Shared palette resource - Bank {} Color {:X}  Changes affect all references",
                self.palette_bank, self.selected_color
            );
        }
        match self.view {
            GraphicsView::Tiles => format!(
                " 8x8 pattern ${:02X} - Map + Sprites  BG={} Pal {} Color {:X}",
                self.selected_tile,
                if self.bitmap_asset { "Bitmap" } else { "Map" },
                self.palette_bank,
                self.selected_color
            ),
            GraphicsView::Map => format!(
                " 64x32 map view {},{} - Place pattern ${:02X}  Pal {} Color {:X}",
                self.map_view_x,
                self.map_view_y,
                self.selected_tile,
                self.palette_bank,
                self.selected_color
            ),
            GraphicsView::Sprite => {
                let first = self.selected_tile & !3;
                format!(
                    " 16x16 sprite - Patterns ${first:02X}-${:02X}  BG={} Pal {} Color {:X}",
                    first + 3,
                    if self.bitmap_asset { "Bitmap" } else { "Map" },
                    self.palette_bank,
                    self.selected_color
                )
            }
            GraphicsView::Palette => match &self.palette_reference {
                Some(reference) => format!(
                    " Shared {reference} - Bank {} Color {:X}  Saves to palette resource",
                    self.palette_bank, self.selected_color
                ),
                None => format!(
                    " Embedded palette - Bank {} Color {:X}  Used by this gfx set",
                    self.palette_bank, self.selected_color
                ),
            },
            GraphicsView::Bitmap => format!(
                " 320x200 bitmap - Pal {} Color {:X}  ROM banks {}-{} (+sprite chr)",
                self.palette_bank,
                self.selected_color,
                self.bitmap_bank,
                self.bitmap_bank + 2
            ),
        }
    }

    fn render_tiles(&self, surface: &mut Surface, sprite: bool) {
        let size = if sprite { 16 } else { 8 };
        let scale = if sprite { 14 } else { 28 };
        let origin = (PANE_LEFT + 12, PANE_TOP + 38);
        let first = self.selected_tile & !3;
        let canvas_caption = if sprite {
            format!("16x16 Sprite ${first:02X}-${:02X}", first + 3)
        } else {
            format!("8x8 Pattern ${:02X}", self.selected_tile)
        };
        draw_group_box(surface, PANE_LEFT + 6, PANE_TOP + 32, 236, 238, &canvas_caption);
        draw_group_box(surface, PANE_LEFT + 246, PANE_TOP + 32, 220, 206, "Shared 8x8 Patterns");
        draw_group_box(
            surface,
            PANE_LEFT + 6,
            PANE_TOP + 308,
            460,
            38,
            &format!("Palette Bank {}", self.palette_bank),
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
                    surface,
                    origin.0 + px * scale,
                    origin.1 + py * scale,
                    scale.saturating_sub(1),
                    scale.saturating_sub(1),
                    self.asset.palette[palette_index],
                );
            }
        }
        self.render_tile_sheet(surface, (PANE_LEFT + 252, PANE_TOP + 38), 12);
        let relationship = if sprite {
            "8x8 sprites use one pattern (Mode 1)"
        } else {
            "One pattern can be a map tile or 8x8 sprite"
        };
        draw_text(surface, PANE_LEFT + 12, PANE_TOP + 282, relationship, UI_GRAY);
        self.render_palette_strip(surface, PANE_LEFT + 12, PANE_TOP + 318);
    }

    fn render_tile_sheet(&self, surface: &mut Surface, origin: (usize, usize), pitch: usize) {
        for tile in 0..256 {
            let tx = tile % 16;
            let ty = tile / 16;
            for py in 0..8 {
                for px in 0..8 {
                    let color = self.tile_pixel(tile as u8, px, py);
                    let index = usize::from(self.palette_bank) * 16 + usize::from(color);
                    let display = self.asset.palette[index];
                    put_pixel(
                        surface,
                        origin.0 + tx * pitch + px,
                        origin.1 + ty * pitch + py,
                        display,
                    );
                }
            }
            if tile == usize::from(self.selected_tile) {
                stroke_rect(
                    surface,
                    origin.0 + tx * pitch,
                    origin.1 + ty * pitch,
                    pitch.min(9),
                    pitch.min(9),
                    UI_WHITE,
                );
            }
        }
    }

    fn render_palette_strip(&self, surface: &mut Surface, x: usize, y: usize) {
        for color in 0..16 {
            let index = usize::from(self.palette_bank) * 16 + color;
            fill_rect(surface, x + color * 24, y, 22, 20, self.asset.palette[index]);
            if color == usize::from(self.selected_color) {
                stroke_rect(surface, x + color * 24, y, 22, 20, UI_WHITE);
            }
        }
        self.render_bank_buttons(surface);
    }

    /// `<` and `>` step the palette bank from any view that draws through one.
    fn render_bank_buttons(&self, surface: &mut Surface) {
        let Some((x, y)) = bank_button_origin(self.view) else { return };
        for (offset, label) in [(0, "<"), (26, ">")] {
            fill_rect(surface, x + offset, y, 22, 20, UI_BLACK);
            stroke_rect(surface, x + offset, y, 22, 20, UI_WHITE);
            draw_text(surface, x + offset + 7, y + 6, label, UI_WHITE);
        }
    }

    fn render_map(&self, surface: &mut Surface) {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        draw_group_box(
            surface,
            PANE_LEFT + 2,
            PANE_TOP + 32,
            326,
            210,
            &format!("64x32 Map - View {},{}", self.map_view_x, self.map_view_y),
        );
        draw_group_box(surface, PANE_LEFT + 332, PANE_TOP + 32, 136, 142, "8x8 Patterns");
        draw_group_box(surface, PANE_LEFT + 332, PANE_TOP + 180, 136, 62, "Cell Options");
        draw_group_box(
            surface,
            PANE_LEFT + 2,
            PANE_TOP + 270,
            466,
            38,
            &format!("Palette Bank {}", self.palette_bank),
        );
        for cell_y in 0..MAP_VIEW_HEIGHT {
            for cell_x in 0..MAP_VIEW_WIDTH {
                let map_x = (self.map_view_x + cell_x) % TILEMAP_WIDTH;
                let map_y = (self.map_view_y + cell_y) % TILEMAP_HEIGHT;
                let cell = map_y * TILEMAP_WIDTH + map_x;
                let tile = self.asset.map[cell];
                let attribute = self.asset.attributes[cell];
                for py in 0..8 {
                    for px in 0..8 {
                        let source_x = if attribute & 0x10 != 0 { 7 - px } else { px };
                        let source_y = if attribute & 0x20 != 0 { 7 - py } else { py };
                        let color = self.tile_pixel(tile, source_x, source_y);
                        let index = usize::from(attribute & 15) * 16 + usize::from(color);
                        put_pixel(
                            surface,
                            origin.0 + cell_x * 8 + px,
                            origin.1 + cell_y * 8 + py,
                            self.asset.palette[index],
                        );
                    }
                }
            }
        }
        self.render_tile_sheet(surface, (PANE_LEFT + 336, PANE_TOP + 38), 8);
        draw_text(surface, PANE_LEFT + 338, PANE_TOP + 194, "H/V Flip", UI_GRAY);
        draw_text(surface, PANE_LEFT + 338, PANE_TOP + 210, "Q Priority", UI_GRAY);
        draw_text(
            surface,
            PANE_LEFT + 8,
            PANE_TOP + 252,
            "Arrows pan 64x32 map - view wraps at edges",
            UI_GRAY,
        );
        self.render_palette_strip(surface, PANE_LEFT + 4, PANE_TOP + 278);
    }

    fn render_palette(&self, surface: &mut Surface) {
        let origin = (PANE_LEFT + 28, PANE_TOP + 38);
        draw_group_box(surface, PANE_LEFT + 20, PANE_TOP + 32, 328, 300, "256-Color Palette");
        draw_group_box(surface, PANE_LEFT + 352, PANE_TOP + 32, 114, 204, "Presets");
        for index in 0..256 {
            let x = index % 16;
            let y = index / 16;
            fill_rect(
                surface,
                origin.0 + x * 20,
                origin.1 + y * 18,
                18,
                16,
                self.asset.palette[index],
            );
            if index == usize::from(self.palette_bank) * 16 + usize::from(self.selected_color) {
                stroke_rect(
                    surface,
                    origin.0 + x * 20 - 1,
                    origin.1 + y * 18 - 1,
                    20,
                    18,
                    UI_WHITE,
                );
            }
        }
        let index = usize::from(self.palette_bank) * 16 + usize::from(self.selected_color);
        let value = self.asset.palette[index];
        draw_text(
            surface,
            PANE_LEFT + 358,
            PANE_TOP + 50,
            &format!("Index ${index:02X}"),
            UI_WHITE,
        );
        draw_text(
            surface,
            PANE_LEFT + 358,
            PANE_TOP + 66,
            &format!("RGB332 ${value:02X}"),
            UI_WHITE,
        );
        for (preset, (name, _)) in PALETTE_PRESETS.iter().enumerate() {
            let y = PANE_TOP + 92 + preset * 24;
            if Some(preset) == self.palette_preset {
                fill_rect(surface, PANE_LEFT + 358, y - 2, 98, 18, UI_BLUE);
            }
            draw_text(surface, PANE_LEFT + 364, y, name, UI_WHITE);
        }
        draw_text(surface, PANE_LEFT + 358, PANE_TOP + 190, "N Next", UI_GRAY);
        draw_text(surface, PANE_LEFT + 358, PANE_TOP + 206, "R/G/B Edit", UI_GRAY);
    }

    fn render_bitmap(&self, surface: &mut Surface) {
        let origin = (PANE_LEFT + 4, PANE_TOP + 38);
        draw_group_box(surface, PANE_LEFT + 2, PANE_TOP + 32, 326, 210, "320 x 200 Bitmap");
        draw_group_box(surface, PANE_LEFT + 332, PANE_TOP + 32, 136, 190, "Bitmap Settings");
        for y in 0..200 {
            for x in 0..320 {
                let color = self.bitmap_pixel(x, y);
                let index = usize::from(self.palette_bank) * 16 + usize::from(color);
                put_pixel(surface, origin.0 + x, origin.1 + y, self.asset.palette[index]);
            }
        }
        draw_text(
            surface,
            PANE_LEFT + 332,
            PANE_TOP + 42,
            &format!("ROM {}-{}", self.bitmap_bank, self.bitmap_bank.wrapping_add(2)),
            UI_WHITE,
        );
        draw_text(surface, PANE_LEFT + 332, PANE_TOP + 58, "BM+BM+CHR", UI_GRAY);
        draw_text(surface, PANE_LEFT + 332, PANE_TOP + 72, ",/. ROM Bank", UI_GRAY);
        for color in 0..16 {
            let index = usize::from(self.palette_bank) * 16 + color;
            fill_rect(
                surface,
                PANE_LEFT + 336 + color % 4 * 28,
                PANE_TOP + 88 + color / 4 * 24,
                24,
                20,
                self.asset.palette[index],
            );
        }
        draw_text(
            surface,
            PANE_LEFT + 332,
            PANE_TOP + 188,
            &format!("Pal Bank {}", self.palette_bank),
            UI_WHITE,
        );
        self.render_bank_buttons(surface);
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
        let Some(view_x) =
            x.checked_sub(origin.0).map(|value| value / 8).filter(|value| *value < MAP_VIEW_WIDTH)
        else {
            return false;
        };
        let Some(view_y) =
            y.checked_sub(origin.1).map(|value| value / 8).filter(|value| *value < MAP_VIEW_HEIGHT)
        else {
            return false;
        };
        let cell_x = (self.map_view_x + view_x) % TILEMAP_WIDTH;
        let cell_y = (self.map_view_y + view_y) % TILEMAP_HEIGHT;
        let cell = cell_y * TILEMAP_WIDTH + cell_x;
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
                let index = y * TILEMAP_WIDTH + x;
                if self.asset.map[index] != old_tile
                    || self.asset.attributes[index] != old_attribute
                {
                    continue;
                }
                self.asset.map[index] = self.selected_tile;
                self.asset.attributes[index] = attribute;
                stack.push(((x + TILEMAP_WIDTH - 1) % TILEMAP_WIDTH, y));
                stack.push(((x + 1) % TILEMAP_WIDTH, y));
                stack.push((x, (y + TILEMAP_HEIGHT - 1) % TILEMAP_HEIGHT));
                stack.push((x, (y + 1) % TILEMAP_HEIGHT));
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

fn expand_legacy_map(bytes: Vec<u8>, legacy: bool) -> Vec<u8> {
    if !legacy {
        return bytes;
    }
    let mut expanded = vec![0; MAP_CELLS];
    for y in 0..25 {
        expanded[y * TILEMAP_WIDTH..y * TILEMAP_WIDTH + 40]
            .copy_from_slice(&bytes[y * 40..y * 40 + 40]);
    }
    expanded
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
            let code = trimmed.split(';').next().unwrap_or("").trim();
            let fields = code.split_whitespace().collect::<Vec<_>>();
            let operation = fields.iter().take(2).position(|field| {
                field.eq_ignore_ascii_case("HEX") || field.eq_ignore_ascii_case("DS")
            });
            let Some(operation) = operation else {
                continue;
            };
            if fields[operation].eq_ignore_ascii_case("DS") {
                let Some(size) = fields.get(operation + 1) else {
                    return Err(format!("{marker} DS is missing a size"));
                };
                if fields.len() != operation + 2 {
                    return Err(format!("{marker} DS requires one constant size"));
                }
                let count = size
                    .strip_prefix('$')
                    .map_or_else(|| size.parse::<usize>(), |hex| usize::from_str_radix(hex, 16))
                    .map_err(|_| format!("Invalid DS size in {marker}"))?;
                let new_length = bytes
                    .len()
                    .checked_add(count)
                    .filter(|&length| length <= expected)
                    .ok_or_else(|| format!("{marker} contains more than {expected} bytes"))?;
                bytes.resize(new_length, 0);
            } else {
                let hex = fields[operation + 1..].join("");
                if !hex.chars().all(|character| character.is_ascii_hexdigit() || character == ',') {
                    return Err(format!("Invalid hex in {marker}"));
                }
                let compact = hex.replace(',', "");
                if !compact.len().is_multiple_of(2) {
                    return Err(format!("{marker} has an odd hex digit"));
                }
                for pair in compact.as_bytes().chunks_exact(2) {
                    let text = core::str::from_utf8(pair).expect("ASCII hex");
                    bytes.push(
                        u8::from_str_radix(text, 16)
                            .map_err(|_| format!("Invalid hex in {marker}"))?,
                    );
                }
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

fn draw_toolbar(surface: &mut Surface, view: GraphicsView, tool: GraphicsTool, bitmap_asset: bool) {
    draw_text(
        surface,
        PANE_LEFT + 4,
        PANE_TOP + 4,
        "1 Pattern  2 Map  3 16x16 Sprite  4 Palette  5 Bitmap",
        UI_WHITE,
    );
    draw_text(surface, PANE_LEFT + 4, PANE_TOP + 18, "P Pencil  F Fill  I Pick", UI_GRAY);
    // Map and Bitmap are the same slot, not two places to be. Name the mode the
    // asset is actually in, and bar the tab that owns it, so opening the other
    // one reads as changing the asset rather than changing the view.
    draw_text(
        surface,
        PANE_LEFT + 208,
        PANE_TOP + 18,
        if bitmap_asset { "Background: Bitmap" } else { "Background: Tilemap" },
        UI_WHITE,
    );
    let (mode_x, mode_width) = if bitmap_asset { (364, 64) } else { (92, 40) };
    fill_rect(surface, PANE_LEFT + mode_x, PANE_TOP + 14, mode_width, 2, UI_WHITE);
    let view_x = match view {
        GraphicsView::Tiles => 4,
        GraphicsView::Map => 92,
        GraphicsView::Sprite => 148,
        GraphicsView::Palette => 276,
        GraphicsView::Bitmap => 364,
    };
    stroke_rect(
        surface,
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
        surface,
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

/// Step for the `<` and `>` buttons that sit to the right of a palette strip.
fn bank_button_at(x: usize, y: usize, origin_x: usize, origin_y: usize) -> Option<i16> {
    if !(origin_y..origin_y + 20).contains(&y) {
        return None;
    }
    match x.checked_sub(origin_x)? {
        0..=21 => Some(-1),
        26..=47 => Some(1),
        _ => None,
    }
}

/// Where the bank stepper lives in each view that shows a palette bank.
fn bank_button_origin(view: GraphicsView) -> Option<(usize, usize)> {
    match view {
        GraphicsView::Tiles | GraphicsView::Sprite => Some((PANE_LEFT + 402, PANE_TOP + 318)),
        GraphicsView::Map => Some((PANE_LEFT + 394, PANE_TOP + 278)),
        GraphicsView::Bitmap => Some((PANE_LEFT + 336, PANE_TOP + 200)),
        GraphicsView::Palette => None,
    }
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
    surface: &mut Surface,
    x: usize,
    y: usize,
    width: usize,
    height: usize,
    caption: &str,
) {
    stroke_rect(surface, x, y, width, height, UI_GRAY);
    let caption_width = (caption.len() + 2) * GLYPH_WIDTH;
    fill_rect(surface, x + 6, y.saturating_sub(3), caption_width, GLYPH_HEIGHT, UI_BLACK);
    draw_text(surface, x + 10, y.saturating_sub(3), caption, UI_WHITE);
}

// Colors here are RGB332 bytes throughout: the UI constants are chosen from the
// console's own range, and asset pixels are literally cartridge palette bytes.
// Expanding at the edge means what the artist sees is exactly what the hardware
// would output, with no palette entries reserved for the interface.
fn fill_rect(surface: &mut Surface, x: usize, y: usize, width: usize, height: usize, color: u8) {
    surface.fill_rect(x, y, width, height, rgb332_to_rgba(color));
}

fn stroke_rect(surface: &mut Surface, x: usize, y: usize, width: usize, height: usize, color: u8) {
    surface.stroke_rect(x, y, width, height, rgb332_to_rgba(color));
}

fn put_pixel(surface: &mut Surface, x: usize, y: usize, color: u8) {
    surface.put_pixel(x, y, rgb332_to_rgba(color));
}

fn draw_text(surface: &mut Surface, x: usize, y: usize, text: &str, color: u8) {
    surface.draw_text(x, y, text, rgb332_to_rgba(color), None);
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ascii_round_trip_preserves_hardware_native_data() {
        let mut editor =
            GraphicsEditor { selected_tile: 3, selected_color: 12, ..GraphicsEditor::default() };
        editor.set_tile_pixel(3, 7, 7, 12);
        editor.asset.map[MAP_CELLS - 1] = 3;
        editor.asset.attributes[MAP_CELLS - 1] = 0x5f;
        let source = editor.serialize("hero.gfx");
        assert!(source.is_ascii());
        assert!(source.contains("HERO_CHR"));
        let restored = GraphicsEditor::parse(&source).unwrap();
        assert_eq!(restored.tile_pixel(3, 7, 7), 12);
        assert_eq!(restored.asset.map[MAP_CELLS - 1], 3);
        assert_eq!(restored.asset.attributes[MAP_CELLS - 1], 0x5f);
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
        assert!(source.starts_with(";@FANTICON-GFX 3"));
        assert!(source.contains(";@PALETTE-FILE GAME.PAL"));
        assert!(!source.contains(";@PALETTE\n"));
        assert_eq!(GraphicsEditor::parse(&source).unwrap().palette_reference(), Some("GAME.PAL"));
        assert_eq!(
            fanticon::assembler::assemble(&source).unwrap().bytes.len(),
            TILE_BYTES + MAP_CELLS * 2
        );
    }

    #[test]
    fn compact_zero_filled_sections_are_valid_editor_and_assembler_input() {
        let source = ";@FANTICON-PAL 1\n;@PALETTE\nGAME_PAL\n HEX 000102030405060708090A0B0C0D0E0F\n DS 240\n";
        let restored = GraphicsEditor::parse(source).unwrap();
        assert_eq!(&restored.asset.palette[..16], &(0_u8..16).collect::<Vec<_>>());
        assert!(restored.asset.palette[16..].iter().all(|&byte| byte == 0));
        assert_eq!(fanticon::assembler::assemble(source).unwrap().bytes.len(), PALETTE_BYTES);
    }

    #[test]
    fn graphics_demo_asset_opens_in_the_visual_editor() {
        let source = include_str!("../../code-assets/demos/graphics/scene.gfx");
        let restored = GraphicsEditor::parse(source).unwrap();
        assert_eq!(restored.palette_reference(), Some("GAME.PAL"));
        assert_eq!(restored.tile_pixel(4, 6, 0), 8);
        assert_eq!(restored.asset.map[13 * TILEMAP_WIDTH + 5], 10);
        assert_eq!(restored.asset.map[19 * TILEMAP_WIDTH], 2);
        assert!(restored.asset.attributes.iter().all(|&byte| byte == 0));

        let palette =
            GraphicsEditor::parse(include_str!("../../code-assets/demos/graphics/game.pal"))
                .unwrap();
        assert_eq!(&palette.asset.palette[..16], &DB16);
    }

    #[test]
    fn legacy_40x25_maps_expand_into_the_new_64x32_layout() {
        let legacy = GraphicsEditor::with_shared_palette("GAME.PAL");
        let mut source = legacy.serialize("legacy.gfx");
        source = source.replacen(";@FANTICON-GFX 3", ";@FANTICON-GFX 2", 1);
        let old_map = vec![7; 40 * 25];
        let old_attributes = vec![3; 40 * 25];
        let map_start = source.find(";@MAP").unwrap();
        source.truncate(map_start);
        write_section(&mut source, ";@MAP", "LEGACY_MAP", &old_map, 20);
        write_section(&mut source, ";@ATTRIBUTES", "LEGACY_ATR", &old_attributes, 20);
        let restored = GraphicsEditor::parse(&source).unwrap();
        assert_eq!(restored.asset.map[24 * TILEMAP_WIDTH + 39], 7);
        assert_eq!(restored.asset.attributes[24 * TILEMAP_WIDTH + 39], 3);
        assert_eq!(restored.asset.map[24 * TILEMAP_WIDTH + 40], 0);
        assert_eq!(restored.asset.map[25 * TILEMAP_WIDTH], 0);
    }

    #[test]
    fn map_view_pans_and_edits_across_the_circular_64x32_map() {
        let mut editor = GraphicsEditor { view: GraphicsView::Map, ..GraphicsEditor::default() };
        editor.map_view_x = TILEMAP_WIDTH - 1;
        editor.map_view_y = TILEMAP_HEIGHT - 1;
        editor.selected_tile = 9;
        assert!(editor.apply_map(PANE_LEFT + 4 + 8, PANE_TOP + 38 + 8));
        assert_eq!(editor.asset.map[0], 9);
        editor.handle_key(&Key::Named(NamedKey::ArrowRight), ModifiersState::empty());
        editor.handle_key(&Key::Named(NamedKey::ArrowDown), ModifiersState::empty());
        assert_eq!((editor.map_view_x, editor.map_view_y), (0, 0));
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
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, 400);
        editor.render(&mut surface);
        assert_eq!(surface.pixel(PANE_LEFT, OUTER_TOP), rgb332_to_rgba(UI_GRAY));
    }

    #[test]
    fn mode_labels_explain_the_shared_pattern_model() {
        let mut editor = GraphicsEditor::default();
        assert!(editor.status().contains("Map + Sprites"));

        editor.handle_mouse_press(PANE_LEFT + 100, PANE_TOP + 6);
        assert_eq!(editor.view, GraphicsView::Map);
        assert!(editor.status().contains("Place pattern"));

        editor.selected_tile = 7;
        editor.handle_mouse_press(PANE_LEFT + 160, PANE_TOP + 6);
        assert_eq!(editor.view, GraphicsView::Sprite);
        assert!(editor.status().contains("Patterns $04-$07"));
    }

    #[test]
    fn the_toolbar_names_the_background_mode_the_asset_is_actually_in() {
        let mut surface = Surface::new(EDITOR_DISPLAY_WIDTH, 400);
        let bar_row = PANE_TOP + 14;
        let bar_at = |surface: &Surface, x: usize| surface.pixel(x, bar_row);
        let white = rgb332_to_rgba(UI_WHITE);

        // A tilemap asset bars the MAP tab, wherever you happen to be looking.
        let tilemap = GraphicsEditor { view: GraphicsView::Tiles, ..GraphicsEditor::default() };
        tilemap.render(&mut surface);
        assert_eq!(bar_at(&surface, PANE_LEFT + 100), white, "MAP should carry the mode bar");
        assert_ne!(bar_at(&surface, PANE_LEFT + 380), white, "BITMAP should not");

        // Choosing bitmap moves the bar, so the pair reads as one exclusive slot.
        let bitmap = GraphicsEditor {
            view: GraphicsView::Sprite,
            bitmap_asset: true,
            ..GraphicsEditor::default()
        };
        bitmap.render(&mut surface);
        assert_eq!(bar_at(&surface, PANE_LEFT + 380), white, "BITMAP should carry the mode bar");
        assert_ne!(bar_at(&surface, PANE_LEFT + 100), white, "MAP should not");
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
        assert!(editor.status().contains("Shared palette resource"));
    }

    #[test]
    fn palette_bank_steppers_switch_banks_from_every_view_that_shows_one() {
        for view in
            [GraphicsView::Tiles, GraphicsView::Sprite, GraphicsView::Map, GraphicsView::Bitmap]
        {
            let mut editor = GraphicsEditor { view, ..GraphicsEditor::default() };
            let (x, y) = bank_button_origin(view).expect("view shows a palette bank");
            assert_eq!(editor.palette_bank, 0);

            editor.handle_mouse_press(x + 30, y + 8);
            assert_eq!(editor.palette_bank, 1, "{view:?} should step forward");
            editor.handle_mouse_press(x + 4, y + 8);
            assert_eq!(editor.palette_bank, 0, "{view:?} should step back");

            // Both ends wrap across the 16 banks rather than sticking.
            editor.handle_mouse_press(x + 4, y + 8);
            assert_eq!(editor.palette_bank, 15, "{view:?} should wrap below zero");
            editor.handle_mouse_press(x + 30, y + 8);
            assert_eq!(editor.palette_bank, 0, "{view:?} should wrap past the last bank");

            // Clicking a stepper only changes the bank; it never paints.
            assert!(!editor.handle_mouse_press(x + 30, y + 8));
            assert_eq!(editor.palette_bank, 1);
        }

        // The all-banks palette view has nothing to step through.
        assert!(bank_button_origin(GraphicsView::Palette).is_none());

        // Keyboard reaches the same control.
        let mut editor = GraphicsEditor::default();
        editor.handle_key(&Key::Character("]".into()), ModifiersState::empty());
        assert_eq!(editor.palette_bank, 1);
        editor.handle_key(&Key::Character("[".into()), ModifiersState::empty());
        assert_eq!(editor.palette_bank, 0);
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
