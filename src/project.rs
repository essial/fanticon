//! `FANTICON.CFG` cartridge projects and project-image compilation.

use std::collections::BTreeMap;

use crate::{
    assembler::{
        BankUsage, CartridgeSourceMapEntry, CartridgeSymbol, Diagnostic,
        assemble_cartridge_with_loader,
    },
    cartridge::Cartridge,
};

pub const MANIFEST_NAME: &str = "fanticon.cfg";

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectManifest {
    pub title: String,
    pub id: u64,
    pub main: String,
    pub output: String,
    pub save_banks: u8,
    pub machine_major: u8,
    pub machine_minor: u8,
    pub author: Option<String>,
    pub description: Option<String>,
    pub icon: Option<String>,
    pub web_background: String,
    pub web_foreground: String,
}

impl ProjectManifest {
    pub fn parse(source: &str) -> Result<Self, String> {
        let mut values = BTreeMap::new();
        for (index, raw) in source.replace('\r', "").lines().enumerate() {
            let line = raw.trim();
            if line.is_empty() || line.starts_with(';') {
                continue;
            }
            let Some((key, value)) = line.split_once('=') else {
                return Err(format!("FANTICON.CFG:{} expected KEY=VALUE", index + 1));
            };
            let key = key.trim().to_ascii_uppercase();
            let value = value.trim().to_owned();
            if !matches!(
                key.as_str(),
                "TITLE"
                    | "ID"
                    | "MAIN"
                    | "OUTPUT"
                    | "SAVE_BANKS"
                    | "MACHINE"
                    | "AUTHOR"
                    | "DESCRIPTION"
                    | "ICON"
                    | "WEB_BACKGROUND"
                    | "WEB_FOREGROUND"
            ) {
                return Err(format!("FANTICON.CFG:{} unknown key {key}", index + 1));
            }
            if values.insert(key.clone(), value).is_some() {
                return Err(format!("FANTICON.CFG:{} duplicate key {key}", index + 1));
            }
        }
        let required = |key: &str| {
            values.get(key).cloned().ok_or_else(|| format!("FANTICON.CFG is missing {key}"))
        };
        let title = required("TITLE")?;
        if title.is_empty()
            || title.len() > 22
            || !title.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
        {
            return Err("TITLE must be 1-22 printable ASCII characters".to_owned());
        }
        let id_text = required("ID")?;
        if id_text.len() != 16 || !id_text.bytes().all(|byte| byte.is_ascii_hexdigit()) {
            return Err("ID must contain exactly 16 hexadecimal digits".to_owned());
        }
        let id =
            u64::from_str_radix(&id_text, 16).map_err(|_| "invalid cartridge ID".to_owned())?;
        if id == 0 {
            return Err("ID must be nonzero".to_owned());
        }
        let main = required("MAIN")?.to_ascii_lowercase();
        if !valid_83_path(&main) || !main.ends_with(".asm") {
            return Err("MAIN must be an 8.3 ASM path".to_owned());
        }
        let output = required("OUTPUT")?.to_ascii_lowercase();
        if !valid_83_path(&output) || !output.ends_with(".fcn") {
            return Err("OUTPUT must be an 8.3 FCN filename".to_owned());
        }
        let save_banks = required("SAVE_BANKS")?
            .parse::<u8>()
            .map_err(|_| "SAVE_BANKS must be 0-4".to_owned())?;
        if save_banks > 4 {
            return Err("SAVE_BANKS must be 0-4".to_owned());
        }
        let machine = required("MACHINE")?;
        let Some((major, minor)) = machine.split_once('.') else {
            return Err("MACHINE must use MAJOR.MINOR".to_owned());
        };
        let machine_major =
            major.parse::<u8>().map_err(|_| "invalid MACHINE version".to_owned())?;
        let machine_minor =
            minor.parse::<u8>().map_err(|_| "invalid MACHINE version".to_owned())?;
        if (machine_major, machine_minor) != (1, 0) {
            return Err("v0.1 projects require MACHINE=1.0".to_owned());
        }
        let optional_text = |key: &str, maximum: usize| -> Result<Option<String>, String> {
            let Some(value) = values.get(key) else { return Ok(None) };
            if value.is_empty()
                || value.len() > maximum
                || !value.bytes().all(|byte| (0x20..=0x7e).contains(&byte))
            {
                return Err(format!("{key} must be 1-{maximum} printable ASCII characters"));
            }
            Ok(Some(value.clone()))
        };
        let author = optional_text("AUTHOR", 64)?;
        let description = optional_text("DESCRIPTION", 160)?;
        let icon = values.get("ICON").map(|value| value.to_ascii_lowercase());
        if icon.as_deref().is_some_and(|icon| !valid_83_path(icon) || !icon.ends_with(".png")) {
            return Err("ICON must be an 8.3 PNG path".to_owned());
        }
        let web_color = |key: &str, default: &str| -> Result<String, String> {
            let value = values.get(key).map(String::as_str).unwrap_or(default);
            if value.len() != 7
                || !value.starts_with('#')
                || !value[1..].bytes().all(|byte| byte.is_ascii_hexdigit())
            {
                return Err(format!("{key} must use #RRGGBB"));
            }
            Ok(value.to_ascii_uppercase())
        };
        let web_background = web_color("WEB_BACKGROUND", "#08080C")?;
        let web_foreground = web_color("WEB_FOREGROUND", "#EEEEEE")?;
        Ok(Self {
            title,
            id,
            main,
            output,
            save_banks,
            machine_major,
            machine_minor,
            author,
            description,
            icon,
            web_background,
            web_foreground,
        })
    }

    #[cfg(not(target_arch = "wasm32"))]
    pub fn export_metadata(&self, directory: &std::path::Path) -> crate::export::ExportMetadata {
        crate::export::ExportMetadata {
            title: self.title.clone(),
            cartridge_id: Some(self.id),
            author: self.author.clone(),
            description: self.description.clone(),
            icon: self.icon.as_ref().map(|path| directory.join(path)),
            web_background: self.web_background.clone(),
            web_foreground: self.web_foreground.clone(),
        }
    }

    pub fn template(
        title: &str,
        main: &str,
        output: &str,
        save_banks: u8,
        id: u64,
    ) -> Result<String, String> {
        let candidate = format!(
            "TITLE={title}\nID={id:016X}\nMAIN={main}\nOUTPUT={output}\nSAVE_BANKS={save_banks}\nMACHINE=1.0\n; OPTIONAL EXPORT METADATA:\n; AUTHOR=YOUR NAME\n; DESCRIPTION=A SHORT GAME DESCRIPTION\n; ICON=ICON.PNG\n; WEB_BACKGROUND=#08080C\n; WEB_FOREGROUND=#EEEEEE\n"
        );
        Self::parse(&candidate)?;
        Ok(candidate)
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectBuild {
    pub manifest: ProjectManifest,
    pub cartridge: Cartridge,
    pub bytes: Vec<u8>,
    pub symbols: BTreeMap<String, CartridgeSymbol>,
    pub source_map: Vec<CartridgeSourceMapEntry>,
    pub bank_usage: Vec<BankUsage>,
}

pub fn build_project_with_loader<F>(
    manifest_source: &str,
    mut loader: F,
) -> Result<ProjectBuild, Vec<Diagnostic>>
where
    F: FnMut(&str) -> Result<String, String>,
{
    let manifest = ProjectManifest::parse(manifest_source).map_err(|message| {
        vec![Diagnostic { source: MANIFEST_NAME.to_owned(), line: 1, column: 1, message }]
    })?;
    let source = loader(&manifest.main).map_err(|message| {
        vec![Diagnostic { source: manifest.main.clone(), line: 1, column: 1, message }]
    })?;
    let assembled = assemble_cartridge_with_loader(&manifest.main, &source, |path| loader(path))?;
    let symbols = assembled.symbols;
    let source_map = assembled.source_map;
    let bank_usage = assembled.bank_usage;
    let mut cartridge = Cartridge::new(
        manifest.title.clone(),
        manifest.id,
        manifest.save_banks,
        assembled.fixed_rom,
        assembled.rom_banks,
    )
    .map_err(|error| {
        vec![Diagnostic { source: manifest.main.clone(), line: 1, column: 1, message: error.0 }]
    })?;
    cartridge.machine_major = manifest.machine_major;
    cartridge.machine_minor = manifest.machine_minor;
    let bytes = cartridge.to_bytes().map_err(|error| {
        vec![Diagnostic { source: manifest.main.clone(), line: 1, column: 1, message: error.0 }]
    })?;
    Ok(ProjectBuild { manifest, cartridge, bytes, symbols, source_map, bank_usage })
}

pub fn generate_cartridge_id() -> Result<u64, String> {
    let mut bytes = [0; 8];
    getrandom::fill(&mut bytes)
        .map_err(|error| format!("could not generate cartridge ID: {error}"))?;
    let id = u64::from_le_bytes(bytes);
    if id == 0 { generate_cartridge_id() } else { Ok(id) }
}

fn valid_83_path(path: &str) -> bool {
    path.split(['/', '\\']).all(|component| {
        let mut parts = component.split('.');
        let stem = parts.next().unwrap_or_default();
        let extension = parts.next();
        parts.next().is_none()
            && !stem.is_empty()
            && stem.len() <= 8
            && extension.is_none_or(|value| !value.is_empty() && value.len() <= 3)
            && stem.bytes().all(valid_name_byte)
            && extension.is_none_or(|value| value.bytes().all(valid_name_byte))
    })
}

const fn valid_name_byte(byte: u8) -> bool {
    byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn manifest_is_strict_and_project_build_produces_valid_fcn() {
        let manifest = "TITLE=TEST GAME\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=GAME.FCN\nSAVE_BANKS=1\nMACHINE=1.0\n";
        let source =
            " FIXED\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ";
        let build = build_project_with_loader(manifest, |path| {
            (path.eq_ignore_ascii_case("main.asm"))
                .then(|| source.to_owned())
                .ok_or_else(|| "not found".to_owned())
        })
        .unwrap();
        assert_eq!(build.manifest.output, "game.fcn");
        assert_eq!(Cartridge::from_bytes(&build.bytes).unwrap(), build.cartridge);
    }

    #[test]
    fn project_builds_can_always_include_fanticon_definitions() {
        let manifest = "TITLE=INCLUDE TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=GAME.FCN\nSAVE_BANKS=0\nMACHINE=1.0\n";
        let source = r#"
         INCLUDE FANTICON.INC
         FIXED
         ORG   FIXED_ROM
RESET    LDA   #RGB332_RED
         STA   BACKDROP_COLOR
LOOP     JMP   LOOP
NMI      RTI
IRQ      RTI
         ORG   VECTOR_NMI
         DA    NMI,RESET,IRQ
"#;
        let build = build_project_with_loader(manifest, |path| {
            path.eq_ignore_ascii_case("main.asm")
                .then(|| source.to_owned())
                .ok_or_else(|| "not found".to_owned())
        })
        .unwrap();
        assert_eq!(&build.cartridge.fixed_rom[0x100..0x105], [0xa9, 0xe0, 0x8d, 0x12, 0xc0]);
    }

    #[test]
    fn manifest_rejects_unknown_keys_and_zero_identity() {
        let base =
            "TITLE=X\nID=0000000000000000\nMAIN=M.ASM\nOUTPUT=X.FCN\nSAVE_BANKS=0\nMACHINE=1.0\n";
        assert!(ProjectManifest::parse(base).unwrap_err().contains("nonzero"));
        assert!(
            ProjectManifest::parse(
                &(base.replace("ID=0000000000000000", "ID=0000000000000001") + "BOGUS=1\n")
            )
            .unwrap_err()
            .contains("unknown")
        );
    }

    #[test]
    fn manifest_accepts_and_validates_optional_export_metadata() {
        let source = "TITLE=EXPORT TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=GAME.FCN\nSAVE_BANKS=0\nMACHINE=1.0\nAUTHOR=FANTICON MAKER\nDESCRIPTION=A SMALL GAME\nICON=ICON.PNG\nWEB_BACKGROUND=#112233\nWEB_FOREGROUND=#AABBCC\n";
        let manifest = ProjectManifest::parse(source).unwrap();
        assert_eq!(manifest.author.as_deref(), Some("FANTICON MAKER"));
        assert_eq!(manifest.icon.as_deref(), Some("icon.png"));
        assert_eq!(manifest.web_background, "#112233");
        assert!(ProjectManifest::parse(&source.replace("#112233", "navy")).is_err());
        assert!(ProjectManifest::parse(&source.replace("ICON.PNG", "TOO-LONG-NAME.PNG")).is_err());
    }
}
