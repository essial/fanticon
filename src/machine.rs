//! Stable timing and address-map constants for the Fanticon VM.
//!
//! Device implementations will be added behind these contracts. Native editor
//! tools do not use this address space.

#[cfg(test)]
use crate::video::DOTS_PER_FRAME;
use crate::video::{DOTS_PER_SCANLINE, SCANLINES_PER_FRAME};

pub const FRAME_RATE_HZ: u32 = 60;
pub const VIDEO_DOTS_PER_CPU_CYCLE: u32 = 2;
pub const CPU_CYCLES_PER_SCANLINE: u32 = DOTS_PER_SCANLINE as u32 / VIDEO_DOTS_PER_CPU_CYCLE;
pub const CPU_CYCLES_PER_FRAME: u32 = CPU_CYCLES_PER_SCANLINE * SCANLINES_PER_FRAME as u32;
pub const CPU_CLOCK_HZ: u32 = CPU_CYCLES_PER_FRAME * FRAME_RATE_HZ;

pub const MAIN_RAM_START: u16 = 0x0000;
pub const MAIN_RAM_END: u16 = 0x7fff;
pub const BANK_WINDOW_START: u16 = 0x8000;
pub const BANK_WINDOW_END: u16 = 0xbfff;
pub const IO_START: u16 = 0xc000;
pub const IO_END: u16 = 0xc0ff;
pub const FIXED_ROM_START: u16 = 0xc100;
pub const FIXED_ROM_END: u16 = 0xffff;

pub const BANK_SIZE: usize = 16 * 1024;
pub const MAIN_RAM_SIZE: usize = 32 * 1024;
pub const VIDEO_RAM_BANKS: usize = 3;
pub const VIDEO_RAM_SIZE: usize = VIDEO_RAM_BANKS * BANK_SIZE;
pub const TILEMAP_WIDTH: usize = 64;
pub const TILEMAP_HEIGHT: usize = 32;
pub const TILEMAP_CELLS: usize = TILEMAP_WIDTH * TILEMAP_HEIGHT;
pub const TILEMAP_PIXEL_WIDTH: usize = TILEMAP_WIDTH * 8;
pub const TILEMAP_PIXEL_HEIGHT: usize = TILEMAP_HEIGHT * 8;
pub const WORK_RAM_BANKS: usize = 4;
pub const MAX_CARTRIDGE_BANKS: usize = 256;
pub const MAX_SAVE_RAM_BANKS: usize = 4;
pub const MAX_SAVE_RAM_SIZE: usize = MAX_SAVE_RAM_BANKS * BANK_SIZE;
pub const CARTRIDGE_HEADER_SIZE: usize = 64;
pub const CARTRIDGE_TITLE_SIZE: usize = 22;
pub const FIXED_ROM_IMAGE_SIZE: usize = BANK_SIZE;
pub const MAX_CARTRIDGE_FILE_SIZE: usize =
    CARTRIDGE_HEADER_SIZE + FIXED_ROM_IMAGE_SIZE + MAX_CARTRIDGE_BANKS * BANK_SIZE;

pub mod register {
    pub const BANK_KIND: u16 = 0xc000;
    pub const BANK_NUMBER: u16 = 0xc001;
    pub const IRQ_PENDING: u16 = 0xc002;
    pub const IRQ_ENABLE: u16 = 0xc003;
    pub const FRAME_LOW: u16 = 0xc004;
    pub const FRAME_HIGH: u16 = 0xc005;
    pub const MACHINE_MAJOR: u16 = 0xc006;
    pub const MACHINE_MINOR: u16 = 0xc007;

    pub const VIDEO_MODE: u16 = 0xc010;
    pub const VIDEO_CONTROL: u16 = 0xc011;
    pub const BACKDROP_COLOR: u16 = 0xc012;
    pub const SCROLL_X_LOW: u16 = 0xc013;
    pub const SCROLL_X_HIGH: u16 = 0xc014;
    pub const SCROLL_Y_LOW: u16 = 0xc015;
    pub const SCROLL_Y_HIGH: u16 = 0xc016;
    pub const RASTER_X_LOW: u16 = 0xc017;
    pub const RASTER_X_HIGH: u16 = 0xc018;
    pub const RASTER_Y_LOW: u16 = 0xc019;
    pub const RASTER_Y_HIGH: u16 = 0xc01a;
    pub const PALETTE_INDEX: u16 = 0xc01b;
    pub const PALETTE_DATA: u16 = 0xc01c;
    pub const BITMAP_PALETTE: u16 = 0xc01d;
    pub const VIDEO_STATUS: u16 = 0xc01e;

    pub const PULSE1_BASE: u16 = 0xc030;
    pub const PULSE2_BASE: u16 = 0xc034;
    pub const TRIANGLE_BASE: u16 = 0xc038;
    pub const NOISE_BASE: u16 = 0xc03c;
    pub const AUDIO_MASTER: u16 = 0xc040;

    pub const PAD0_STATE: u16 = 0xc050;
    pub const PAD0_PRESSED: u16 = 0xc051;
    pub const PAD1_STATE: u16 = 0xc052;
    pub const PAD1_PRESSED: u16 = 0xc053;

    pub const TIMER0_BASE: u16 = 0xc060;
    pub const TIMER1_BASE: u16 = 0xc068;
}

pub mod video_mode {
    pub const BLANK: u8 = 0;
    pub const TILE: u8 = 1;
    pub const BITMAP: u8 = 2;
}

pub mod video_control {
    pub const BACKGROUND_ENABLE: u8 = 1 << 0;
    pub const SPRITES_ENABLE: u8 = 1 << 1;
}

pub mod video_status {
    pub const VBLANK: u8 = 1 << 0;
    pub const HBLANK: u8 = 1 << 1;
    pub const SPRITE_OVERFLOW: u8 = 1 << 2;
}

pub const MACHINE_VERSION_MAJOR: u8 = 1;
pub const MACHINE_VERSION_MINOR: u8 = 0;

pub mod bank_kind {
    pub const CARTRIDGE_ROM: u8 = 0;
    pub const WORK_RAM: u8 = 1;
    pub const VIDEO_RAM: u8 = 2;
    pub const SAVE_RAM: u8 = 3;
}

const _: () = assert!((DOTS_PER_SCANLINE as u32).is_multiple_of(VIDEO_DOTS_PER_CPU_CYCLE));

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn master_timing_is_an_integer_clock_tree() {
        assert_eq!(CPU_CYCLES_PER_SCANLINE, 200);
        assert_eq!(CPU_CYCLES_PER_FRAME, 52_400);
        assert_eq!(CPU_CLOCK_HZ, 3_144_000);
        assert_eq!(CPU_CYCLES_PER_FRAME * VIDEO_DOTS_PER_CPU_CYCLE, DOTS_PER_FRAME);
    }

    #[test]
    fn primary_memory_regions_are_contiguous_and_non_overlapping() {
        assert_eq!(MAIN_RAM_END.wrapping_add(1), BANK_WINDOW_START);
        assert_eq!(BANK_WINDOW_END.wrapping_add(1), IO_START);
        assert_eq!(IO_END.wrapping_add(1), FIXED_ROM_START);
        assert_eq!(usize::from(BANK_WINDOW_END - BANK_WINDOW_START) + 1, BANK_SIZE);
        assert_eq!(usize::from(MAIN_RAM_END - MAIN_RAM_START) + 1, MAIN_RAM_SIZE);
        assert_eq!(FIXED_ROM_END, u16::MAX);
    }

    #[test]
    fn video_layers_have_independent_enable_bits() {
        assert_ne!(video_control::BACKGROUND_ENABLE, video_control::SPRITES_ENABLE);
        assert_eq!(video_control::BACKGROUND_ENABLE & video_control::SPRITES_ENABLE, 0);
        assert_eq!(video_mode::TILE, 1);
        assert_eq!(VIDEO_RAM_BANKS, 3);
        assert_eq!(VIDEO_RAM_SIZE, 48 * 1024);
        assert_eq!((TILEMAP_PIXEL_WIDTH, TILEMAP_PIXEL_HEIGHT), (512, 256));
        assert_eq!(
            video_status::VBLANK | video_status::HBLANK | video_status::SPRITE_OVERFLOW,
            0x07
        );
    }

    #[test]
    fn cartridge_and_save_limits_match_the_eight_bit_bank_selector() {
        assert_eq!(MAX_CARTRIDGE_BANKS, usize::from(u8::MAX) + 1);
        assert_eq!(MAX_CARTRIDGE_BANKS * BANK_SIZE, 4 * 1024 * 1024);
        assert_eq!(MAX_SAVE_RAM_SIZE, 64 * 1024);
        assert_eq!(MAX_CARTRIDGE_FILE_SIZE, 0x40_4040);
        assert_eq!(CARTRIDGE_TITLE_SIZE, 22);
        assert_eq!(bank_kind::SAVE_RAM, 3);
    }
}
