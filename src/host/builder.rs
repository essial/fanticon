use std::collections::BTreeMap;

use fanticon::{
    assembler::{
        AssembledProgram, CartridgeSourceMapEntry, CartridgeSymbol, Diagnostic,
        assemble_with_loader,
    },
    cartridge::{Cartridge, SaveImage},
    project::{MANIFEST_NAME, build_project_with_loader},
};

use super::filesystem::SharedFilesystem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSuccess {
    pub output: String,
    pub origin: u16,
    pub size: usize,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ProjectBuildSuccess {
    pub output: String,
    pub title: String,
    pub banks: usize,
    pub size: usize,
    pub symbols: BTreeMap<String, CartridgeSymbol>,
    pub source_map: Vec<CartridgeSourceMapEntry>,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GameLaunch {
    pub cartridge: Cartridge,
    pub cartridge_path: String,
    pub save_path: Option<String>,
    pub save_ram: Vec<u8>,
    pub symbols: BTreeMap<String, CartridgeSymbol>,
    pub source_map: Vec<CartridgeSourceMapEntry>,
    pub breakpoints: Vec<(fanticon::assembler::SymbolSection, u16)>,
}

pub fn build_project(
    filesystem: &SharedFilesystem,
) -> Result<ProjectBuildSuccess, Vec<Diagnostic>> {
    let manifest_source = filesystem.borrow().read_text(MANIFEST_NAME).map_err(|message| {
        vec![Diagnostic { source: MANIFEST_NAME.to_owned(), line: 1, column: 1, message }]
    })?;
    let build =
        build_project_with_loader(&manifest_source, |path| filesystem.borrow().read_text(path))?;
    filesystem.borrow_mut().write_binary(&build.manifest.output, &build.bytes).map_err(
        |message| {
            vec![Diagnostic { source: build.manifest.output.clone(), line: 1, column: 1, message }]
        },
    )?;
    Ok(ProjectBuildSuccess {
        output: build.manifest.output,
        title: build.manifest.title,
        banks: build.cartridge.rom_banks.len(),
        size: build.bytes.len(),
        symbols: build.symbols,
        source_map: build.source_map,
    })
}

pub fn build_and_load_project(
    filesystem: &SharedFilesystem,
) -> Result<GameLaunch, Vec<Diagnostic>> {
    let success = build_project(filesystem)?;
    let mut launch = load_cartridge(filesystem, &success.output)?;
    launch.symbols = success.symbols;
    launch.source_map = success.source_map;
    Ok(launch)
}

pub fn load_cartridge(
    filesystem: &SharedFilesystem,
    path: &str,
) -> Result<GameLaunch, Vec<Diagnostic>> {
    let diagnostic =
        |message| vec![Diagnostic { source: path.to_owned(), line: 1, column: 1, message }];
    let bytes = filesystem.borrow().read_binary(path).map_err(diagnostic)?;
    let cartridge = Cartridge::from_bytes(&bytes).map_err(|error| diagnostic(error.0))?;
    let save_path = (cartridge.save_banks != 0).then(|| replace_extension(path, "sav"));
    let expected = usize::from(cartridge.save_banks) * fanticon::machine::BANK_SIZE;
    let save_ram = if let Some(save_path) = &save_path {
        #[cfg(not(target_arch = "wasm32"))]
        let save_read = filesystem.borrow().read_binary(save_path);
        #[cfg(target_arch = "wasm32")]
        let save_read = read_browser_save(cartridge.id);
        match save_read {
            Ok(bytes) => {
                let save = SaveImage::from_bytes(&bytes).map_err(|error| diagnostic(error.0))?;
                if save.cartridge_id != cartridge.id {
                    return Err(diagnostic("save belongs to a different cartridge ID".to_owned()));
                }
                if save.ram.len() == expected {
                    save.ram
                } else {
                    let ram = vec![0; expected];
                    write_save(filesystem, save_path, cartridge.id, &ram).map_err(diagnostic)?;
                    ram
                }
            }
            Err(message) if message == "FILE NOT FOUND" => vec![0; expected],
            Err(message) => return Err(diagnostic(message)),
        }
    } else {
        Vec::new()
    };
    Ok(GameLaunch {
        cartridge,
        cartridge_path: path.to_owned(),
        save_path,
        save_ram,
        symbols: BTreeMap::new(),
        source_map: Vec::new(),
        breakpoints: Vec::new(),
    })
}

pub fn write_save(
    filesystem: &SharedFilesystem,
    path: &str,
    cartridge_id: u64,
    ram: &[u8],
) -> Result<(), String> {
    let image = SaveImage { cartridge_id, ram: ram.to_vec() };
    let bytes = image.to_bytes().map_err(|error| error.0)?;
    #[cfg(not(target_arch = "wasm32"))]
    return filesystem.borrow_mut().write_binary_atomic(path, &bytes);
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (filesystem, path);
        let storage = web_sys::window()
            .ok_or_else(|| "browser window is unavailable".to_owned())?
            .local_storage()
            .map_err(|_| "browser storage is unavailable".to_owned())?
            .ok_or_else(|| "browser storage is disabled".to_owned())?;
        storage
            .set_item(&browser_save_key(cartridge_id), &hex_encode(&bytes))
            .map_err(|_| "could not persist browser save".to_owned())
    }
}

#[cfg(target_arch = "wasm32")]
fn read_browser_save(cartridge_id: u64) -> Result<Vec<u8>, String> {
    let storage = web_sys::window()
        .ok_or_else(|| "browser window is unavailable".to_owned())?
        .local_storage()
        .map_err(|_| "browser storage is unavailable".to_owned())?
        .ok_or_else(|| "browser storage is disabled".to_owned())?;
    let encoded = storage
        .get_item(&browser_save_key(cartridge_id))
        .map_err(|_| "could not read browser save".to_owned())?
        .ok_or_else(|| "FILE NOT FOUND".to_owned())?;
    hex_decode(&encoded)
}

#[cfg(target_arch = "wasm32")]
fn browser_save_key(cartridge_id: u64) -> String {
    format!("fanticon-save-{cartridge_id:016x}")
}

#[cfg(target_arch = "wasm32")]
fn hex_encode(bytes: &[u8]) -> String {
    const DIGITS: &[u8; 16] = b"0123456789abcdef";
    let mut encoded = String::with_capacity(bytes.len() * 2);
    for &byte in bytes {
        encoded.push(DIGITS[(byte >> 4) as usize] as char);
        encoded.push(DIGITS[(byte & 15) as usize] as char);
    }
    encoded
}

#[cfg(target_arch = "wasm32")]
fn hex_decode(encoded: &str) -> Result<Vec<u8>, String> {
    if !encoded.len().is_multiple_of(2) {
        return Err("browser save encoding is invalid".to_owned());
    }
    encoded
        .as_bytes()
        .chunks_exact(2)
        .map(|pair| {
            let high = (pair[0] as char)
                .to_digit(16)
                .ok_or_else(|| "browser save encoding is invalid".to_owned())?;
            let low = (pair[1] as char)
                .to_digit(16)
                .ok_or_else(|| "browser save encoding is invalid".to_owned())?;
            Ok(((high << 4) | low) as u8)
        })
        .collect()
}

fn replace_extension(path: &str, extension: &str) -> String {
    let (stem, _) = path.rsplit_once('.').unwrap_or((path, ""));
    format!("{stem}.{extension}")
}

pub fn build_file(
    filesystem: &SharedFilesystem,
    source_name: &str,
    output_name: Option<&str>,
) -> Result<BuildSuccess, Vec<Diagnostic>> {
    let source = filesystem.borrow().read_text(source_name).map_err(|message| {
        vec![Diagnostic { source: source_name.to_owned(), line: 1, column: 1, message }]
    })?;
    build_source(filesystem, source_name, &source, output_name)
}

pub fn build_source(
    filesystem: &SharedFilesystem,
    source_name: &str,
    source: &str,
    output_name: Option<&str>,
) -> Result<BuildSuccess, Vec<Diagnostic>> {
    let output = output_name.map(str::to_owned).unwrap_or_else(|| default_output(source_name));
    let program =
        assemble_with_loader(source_name, source, |path| filesystem.borrow().read_text(path))?;
    write_program(filesystem, &output, &program).map_err(|message| {
        vec![Diagnostic { source: source_name.to_owned(), line: 1, column: 1, message }]
    })?;
    Ok(BuildSuccess { output, origin: program.origin, size: program.bytes.len() })
}

fn write_program(
    filesystem: &SharedFilesystem,
    output: &str,
    program: &AssembledProgram,
) -> Result<(), String> {
    filesystem.borrow_mut().write_binary(output, &program.bytes)
}

fn default_output(source: &str) -> String {
    let (directory, filename) =
        source.rfind(['/', '\\']).map_or(("", source), |index| source.split_at(index + 1));
    let stem = filename.rsplit_once('.').map_or(filename, |(stem, _)| stem);
    format!("{directory}{stem}.bin")
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::host::filesystem::shared_filesystem;

    #[test]
    fn build_writes_binary_and_derives_output_name() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text("hello.asm", " ORG $8000\n LDA #$42\n RTS").unwrap();
        let success = build_file(&filesystem, "hello.asm", None).unwrap();
        assert_eq!(success.output, "hello.bin");
        assert_eq!(success.origin, 0x8000);
        assert_eq!(filesystem.borrow().read_binary("hello.bin").unwrap(), [0xa9, 0x42, 0x60]);
    }

    #[test]
    fn project_build_and_run_use_manifest_cartridge_and_save_formats() {
        let filesystem = shared_filesystem();
        filesystem.borrow_mut().write_text(
            "fanticon.cfg",
            "TITLE=HOST TEST\nID=0123456789ABCDEF\nMAIN=MAIN.ASM\nOUTPUT=TEST.FCN\nSAVE_BANKS=2\nMACHINE=1.0\n",
        ).unwrap();
        filesystem.borrow_mut().write_text(
            "main.asm",
            " FIXED\n ORG $C100\nRESET JMP RESET\nNMI RTI\nIRQ RTI\n ORG $FFFA\n DA NMI,RESET,IRQ",
        ).unwrap();
        let launch = build_and_load_project(&filesystem).unwrap();
        assert_eq!(launch.cartridge.title, "HOST TEST");
        assert_eq!(launch.save_ram.len(), 2 * fanticon::machine::BANK_SIZE);
        assert_eq!(
            Cartridge::from_bytes(&filesystem.borrow().read_binary("test.fcn").unwrap()).unwrap(),
            launch.cartridge
        );
    }

    #[test]
    fn valid_save_with_changed_size_is_recreated_but_invalid_save_is_preserved() {
        let filesystem = shared_filesystem();
        let mut fixed = [0xff; fanticon::machine::BANK_SIZE];
        fixed[0x3ffa..].copy_from_slice(&[0x00, 0xc1, 0x00, 0xc1, 0x00, 0xc1]);
        let cartridge = Cartridge::new("SAVE TEST", 9, 2, fixed, Vec::new()).unwrap();
        filesystem.borrow_mut().write_binary("save.fcn", &cartridge.to_bytes().unwrap()).unwrap();
        let old = SaveImage::new(9, 1).unwrap().to_bytes().unwrap();
        filesystem.borrow_mut().write_binary("save.sav", &old).unwrap();
        let launch = load_cartridge(&filesystem, "save.fcn").unwrap();
        assert_eq!(launch.save_ram.len(), 2 * fanticon::machine::BANK_SIZE);
        let recreated =
            SaveImage::from_bytes(&filesystem.borrow().read_binary("save.sav").unwrap()).unwrap();
        assert_eq!(recreated.ram.len(), 2 * fanticon::machine::BANK_SIZE);

        filesystem.borrow_mut().write_binary("save.sav", b"bad save").unwrap();
        assert!(load_cartridge(&filesystem, "save.fcn").is_err());
        assert_eq!(filesystem.borrow().read_binary("save.sav").unwrap(), b"bad save");
    }
}
