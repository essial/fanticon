use std::{cell::Cell, rc::Rc};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct UiColors {
    pub background: u8,
    pub foreground: u8,
}

impl Default for UiColors {
    fn default() -> Self {
        Self { background: 0, foreground: 255 }
    }
}

pub type SharedUiColors = Rc<Cell<UiColors>>;

pub fn shared_ui_colors() -> SharedUiColors {
    Rc::new(Cell::new(UiColors::default()))
}

pub fn parse_palette_index(text: &str) -> Result<u8, String> {
    let value = if let Some(hex) = text.strip_prefix('$') {
        u8::from_str_radix(hex, 16)
    } else if let Some(hex) = text.strip_prefix("0x").or_else(|| text.strip_prefix("0X")) {
        u8::from_str_radix(hex, 16)
    } else {
        text.parse()
    };
    value.map_err(|_| "COLOR INDEX MUST BE 0-255".to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn palette_indexes_accept_decimal_and_hex() {
        assert_eq!(parse_palette_index("255"), Ok(255));
        assert_eq!(parse_palette_index("$ff"), Ok(255));
        assert_eq!(parse_palette_index("0x1f"), Ok(31));
        assert!(parse_palette_index("256").is_err());
    }
}
