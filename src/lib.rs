#![forbid(unsafe_code)]

pub mod assembler;
pub mod audio;
mod cpu;
pub mod machine;
pub mod video;

pub use cpu::{Bus, ClockResult, Cpu, Pins, Status};

/// A flat, allocation-free 64 KiB bus suitable for a fantasy-console prototype.
/// Replace this with a mapped bus when video, audio, and input devices are added.
pub struct Ram(pub [u8; 65536]);

impl Ram {
    pub fn new() -> Self {
        Self([0; 65536])
    }
}

impl Default for Ram {
    fn default() -> Self {
        Self::new()
    }
}

impl Bus for Ram {
    #[inline(always)]
    fn read(&mut self, address: u16) -> u8 {
        self.0[address as usize]
    }

    #[inline(always)]
    fn write(&mut self, address: u16, value: u8) {
        self.0[address as usize] = value;
    }
}
