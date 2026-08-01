//! Versioned Fanticon cartridge and battery-backed save images.

use core::fmt;

use crate::machine::{
    BANK_SIZE, CARTRIDGE_HEADER_SIZE, CARTRIDGE_TITLE_SIZE, FIXED_ROM_IMAGE_SIZE,
    MACHINE_VERSION_MAJOR, MACHINE_VERSION_MINOR, MAX_CARTRIDGE_BANKS, MAX_SAVE_RAM_BANKS,
};

pub const MAGIC: &[u8; 8] = b"FANTICON";
pub const SAVE_MAGIC: &[u8; 8] = b"FCNSAVE\x1a";
pub const FORMAT_MAJOR: u8 = 1;
pub const FORMAT_MINOR: u8 = 0;
pub const SAVE_HEADER_SIZE: usize = 32;
pub const FLAG_BATTERY_RAM: u32 = 1;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Cartridge {
    pub title: String,
    pub id: u64,
    pub machine_major: u8,
    pub machine_minor: u8,
    pub save_banks: u8,
    pub fixed_rom: Box<[u8; FIXED_ROM_IMAGE_SIZE]>,
    pub rom_banks: Vec<Box<[u8; BANK_SIZE]>>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CartridgeError(pub String);

impl fmt::Display for CartridgeError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for CartridgeError {}

impl Cartridge {
    pub fn new(
        title: impl Into<String>,
        id: u64,
        save_banks: u8,
        fixed_rom: [u8; FIXED_ROM_IMAGE_SIZE],
        rom_banks: Vec<[u8; BANK_SIZE]>,
    ) -> Result<Self, CartridgeError> {
        let cartridge = Self {
            title: title.into(),
            id,
            machine_major: MACHINE_VERSION_MAJOR,
            machine_minor: MACHINE_VERSION_MINOR,
            save_banks,
            fixed_rom: Box::new(fixed_rom),
            rom_banks: rom_banks.into_iter().map(Box::new).collect(),
        };
        cartridge.validate_metadata()?;
        cartridge.validate_vectors()?;
        Ok(cartridge)
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CartridgeError> {
        if bytes.len() < CARTRIDGE_HEADER_SIZE + FIXED_ROM_IMAGE_SIZE {
            return err("cartridge is shorter than its header and fixed ROM");
        }
        if &bytes[..8] != MAGIC {
            return err("invalid FANTICON cartridge magic");
        }
        if bytes[8] != FORMAT_MAJOR || bytes[9] > FORMAT_MINOR {
            return err("unsupported cartridge format version");
        }
        if read_u16(bytes, 0x0a) as usize != CARTRIDGE_HEADER_SIZE {
            return err("invalid cartridge header size");
        }
        let flags = read_u32(bytes, 0x0c);
        if flags & !FLAG_BATTERY_RAM != 0 {
            return err("cartridge uses unknown feature flags");
        }
        let bank_count = read_u16(bytes, 0x10) as usize;
        if bank_count > MAX_CARTRIDGE_BANKS {
            return err("cartridge declares more than 256 ROM banks");
        }
        let save_banks = bytes[0x12];
        if usize::from(save_banks) > MAX_SAVE_RAM_BANKS {
            return err("cartridge declares more than four save banks");
        }
        if (flags & FLAG_BATTERY_RAM != 0) != (save_banks != 0) {
            return err("battery-RAM flag does not match save-bank count");
        }
        if bytes[0x13] != 0 {
            return err("unsupported cartridge mapper");
        }
        let id = read_u64(bytes, 0x1c);
        let title_bytes = &bytes[0x24..0x24 + CARTRIDGE_TITLE_SIZE];
        let title_end = title_bytes.iter().position(|&byte| byte == 0).unwrap_or(title_bytes.len());
        if title_bytes[title_end..].iter().any(|&byte| byte != 0)
            || title_bytes[..title_end].iter().any(|byte| !(0x20..=0x7e).contains(byte))
        {
            return err("cartridge title is not printable, NUL-padded ASCII");
        }
        let machine_major = bytes[0x3a];
        let machine_minor = bytes[0x3b];
        let expected_len = CARTRIDGE_HEADER_SIZE + FIXED_ROM_IMAGE_SIZE + bank_count * BANK_SIZE;
        if bytes.len() != expected_len {
            return err("cartridge length does not match its ROM-bank count");
        }
        if crc32(&bytes[..0x3c]) != read_u32(bytes, 0x3c) {
            return err("cartridge header CRC does not match");
        }
        let fixed_start = CARTRIDGE_HEADER_SIZE;
        let fixed_end = fixed_start + FIXED_ROM_IMAGE_SIZE;
        if crc32(&bytes[fixed_start..fixed_end]) != read_u32(bytes, 0x14) {
            return err("fixed-ROM CRC does not match");
        }
        let banked = &bytes[fixed_end..];
        let expected_banked_crc = if banked.is_empty() { 0 } else { crc32(banked) };
        if expected_banked_crc != read_u32(bytes, 0x18) {
            return err("banked-ROM CRC does not match");
        }

        let fixed_rom = Box::new(bytes[fixed_start..fixed_end].try_into().expect("fixed length"));
        let rom_banks = banked
            .chunks_exact(BANK_SIZE)
            .map(|bank| Box::new(bank.try_into().expect("bank length")))
            .collect();
        let cartridge = Self {
            title: String::from_utf8(title_bytes[..title_end].to_vec()).expect("ASCII title"),
            id,
            machine_major,
            machine_minor,
            save_banks,
            fixed_rom,
            rom_banks,
        };
        cartridge.validate_metadata()?;
        cartridge.validate_vectors()?;
        Ok(cartridge)
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CartridgeError> {
        self.validate_metadata()?;
        self.validate_vectors()?;
        let mut bytes = vec![0; CARTRIDGE_HEADER_SIZE];
        bytes[..8].copy_from_slice(MAGIC);
        bytes[8] = FORMAT_MAJOR;
        bytes[9] = FORMAT_MINOR;
        write_u16(&mut bytes, 0x0a, CARTRIDGE_HEADER_SIZE as u16);
        write_u32(&mut bytes, 0x0c, u32::from(self.save_banks != 0));
        write_u16(&mut bytes, 0x10, self.rom_banks.len() as u16);
        bytes[0x12] = self.save_banks;
        bytes[0x13] = 0;
        write_u64(&mut bytes, 0x1c, self.id);
        bytes[0x24..0x24 + self.title.len()].copy_from_slice(self.title.as_bytes());
        bytes[0x3a] = self.machine_major;
        bytes[0x3b] = self.machine_minor;
        bytes.extend_from_slice(self.fixed_rom.as_slice());
        for bank in &self.rom_banks {
            bytes.extend_from_slice(bank.as_slice());
        }
        write_u32(&mut bytes, 0x14, crc32(self.fixed_rom.as_slice()));
        let banked_crc = if self.rom_banks.is_empty() {
            0
        } else {
            crc32(&bytes[CARTRIDGE_HEADER_SIZE + FIXED_ROM_IMAGE_SIZE..])
        };
        write_u32(&mut bytes, 0x18, banked_crc);
        let header_crc = crc32(&bytes[..0x3c]);
        write_u32(&mut bytes, 0x3c, header_crc);
        Ok(bytes)
    }

    fn validate_metadata(&self) -> Result<(), CartridgeError> {
        if self.id == 0 {
            return err("cartridge ID must be nonzero");
        }
        if self.title.is_empty()
            || self.title.len() > CARTRIDGE_TITLE_SIZE
            || !self.title.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return err("cartridge title must be 1-22 printable ASCII characters");
        }
        if self.rom_banks.len() > MAX_CARTRIDGE_BANKS {
            return err("cartridge contains more than 256 ROM banks");
        }
        if usize::from(self.save_banks) > MAX_SAVE_RAM_BANKS {
            return err("cartridge contains more than four save banks");
        }
        if self.machine_major != MACHINE_VERSION_MAJOR || self.machine_minor > MACHINE_VERSION_MINOR
        {
            return err("cartridge requires an incompatible Fanticon machine version");
        }
        Ok(())
    }

    fn validate_vectors(&self) -> Result<(), CartridgeError> {
        let reset = u16::from_le_bytes([self.fixed_rom[0x3ffc], self.fixed_rom[0x3ffd]]);
        if !(reset <= 0x7fff || reset >= 0xc100) {
            return err("RESET vector must point to main RAM or fixed ROM");
        }
        Ok(())
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SaveImage {
    pub cartridge_id: u64,
    pub ram: Vec<u8>,
}

impl SaveImage {
    pub fn new(cartridge_id: u64, banks: u8) -> Result<Self, CartridgeError> {
        if cartridge_id == 0 || banks == 0 || usize::from(banks) > MAX_SAVE_RAM_BANKS {
            return err("save requires a nonzero cartridge ID and 1-4 banks");
        }
        Ok(Self { cartridge_id, ram: vec![0; usize::from(banks) * BANK_SIZE] })
    }

    pub fn from_bytes(bytes: &[u8]) -> Result<Self, CartridgeError> {
        if bytes.len() < SAVE_HEADER_SIZE || &bytes[..8] != SAVE_MAGIC {
            return err("invalid Fanticon save header");
        }
        if bytes[8] != FORMAT_MAJOR
            || bytes[9] > FORMAT_MINOR
            || read_u16(bytes, 0x0a) as usize != SAVE_HEADER_SIZE
        {
            return err("unsupported Fanticon save format");
        }
        let cartridge_id = read_u64(bytes, 0x0c);
        let size = read_u32(bytes, 0x14) as usize;
        if cartridge_id == 0
            || size == 0
            || size > MAX_SAVE_RAM_BANKS * BANK_SIZE
            || !size.is_multiple_of(BANK_SIZE)
            || bytes.len() != SAVE_HEADER_SIZE + size
        {
            return err("save identity or payload size is invalid");
        }
        if crc32(&bytes[..0x1c]) != read_u32(bytes, 0x1c)
            || crc32(&bytes[SAVE_HEADER_SIZE..]) != read_u32(bytes, 0x18)
        {
            return err("save CRC does not match");
        }
        Ok(Self { cartridge_id, ram: bytes[SAVE_HEADER_SIZE..].to_vec() })
    }

    pub fn to_bytes(&self) -> Result<Vec<u8>, CartridgeError> {
        let banks = self.ram.len() / BANK_SIZE;
        if self.cartridge_id == 0
            || !matches!(banks, 1..=MAX_SAVE_RAM_BANKS)
            || self.ram.len() != banks * BANK_SIZE
        {
            return err("save identity or payload size is invalid");
        }
        let mut bytes = vec![0; SAVE_HEADER_SIZE];
        bytes[..8].copy_from_slice(SAVE_MAGIC);
        bytes[8] = FORMAT_MAJOR;
        bytes[9] = FORMAT_MINOR;
        write_u16(&mut bytes, 0x0a, SAVE_HEADER_SIZE as u16);
        write_u64(&mut bytes, 0x0c, self.cartridge_id);
        write_u32(&mut bytes, 0x14, self.ram.len() as u32);
        write_u32(&mut bytes, 0x18, crc32(&self.ram));
        let header_crc = crc32(&bytes[..0x1c]);
        write_u32(&mut bytes, 0x1c, header_crc);
        bytes.extend_from_slice(&self.ram);
        Ok(bytes)
    }
}

pub fn crc32(bytes: &[u8]) -> u32 {
    let mut crc = u32::MAX;
    for &byte in bytes {
        crc ^= u32::from(byte);
        for _ in 0..8 {
            crc = (crc >> 1) ^ (0xedb8_8320 & 0u32.wrapping_sub(crc & 1));
        }
    }
    !crc
}

fn err<T>(message: impl Into<String>) -> Result<T, CartridgeError> {
    Err(CartridgeError(message.into()))
}

fn read_u16(bytes: &[u8], offset: usize) -> u16 {
    u16::from_le_bytes(bytes[offset..offset + 2].try_into().expect("validated header"))
}
fn read_u32(bytes: &[u8], offset: usize) -> u32 {
    u32::from_le_bytes(bytes[offset..offset + 4].try_into().expect("validated header"))
}
fn read_u64(bytes: &[u8], offset: usize) -> u64 {
    u64::from_le_bytes(bytes[offset..offset + 8].try_into().expect("validated header"))
}
fn write_u16(bytes: &mut [u8], offset: usize, value: u16) {
    bytes[offset..offset + 2].copy_from_slice(&value.to_le_bytes());
}
fn write_u32(bytes: &mut [u8], offset: usize, value: u32) {
    bytes[offset..offset + 4].copy_from_slice(&value.to_le_bytes());
}
fn write_u64(bytes: &mut [u8], offset: usize, value: u64) {
    bytes[offset..offset + 8].copy_from_slice(&value.to_le_bytes());
}

#[cfg(test)]
mod tests {
    use super::*;

    fn cartridge() -> Cartridge {
        let mut fixed = [0xff; BANK_SIZE];
        fixed[0x3ffa..0x4000].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        Cartridge::new("TEST CART", 0x0123_4567_89ab_cdef, 2, fixed, vec![[0x42; BANK_SIZE]])
            .unwrap()
    }

    #[test]
    fn cartridge_round_trip_and_crc_validation() {
        let cart = cartridge();
        let bytes = cart.to_bytes().unwrap();
        assert_eq!(bytes.len(), 0x8040);
        assert_eq!(Cartridge::from_bytes(&bytes).unwrap(), cart);
        let mut damaged = bytes;
        damaged[0x4040] ^= 1;
        assert!(Cartridge::from_bytes(&damaged).unwrap_err().0.contains("banked-ROM CRC"));
    }

    #[test]
    fn save_round_trip_and_wrong_size_rejected() {
        let mut save = SaveImage::new(7, 1).unwrap();
        save.ram[123] = 0xa5;
        assert_eq!(SaveImage::from_bytes(&save.to_bytes().unwrap()).unwrap(), save);
        save.ram.pop();
        assert!(save.to_bytes().is_err());
    }

    #[test]
    fn crc32_matches_standard_check_value() {
        assert_eq!(crc32(b"123456789"), 0xcbf4_3926);
    }
}
