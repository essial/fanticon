//! Toolchain-free HTML and standalone-player packaging.

use std::{
    env, fs,
    fs::File,
    io::{Cursor, Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

use editpe::{
    Image as PeImage, VersionInfo, VersionStringTable,
    constants::{
        VS_COMMENTS, VS_COMPANY_NAME, VS_FILE_DESCRIPTION, VS_FILE_VERSION, VS_INTERNAL_NAME,
        VS_ORIGINAL_FILENAME, VS_PRODUCT_NAME, VS_PRODUCT_VERSION,
    },
    types::VersionU16,
};
use flate2::{Compression, write::GzEncoder};
use image::{ImageFormat, imageops::FilterType};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use zip::{CompressionMethod, ZipWriter, write::SimpleFileOptions};

const FOOTER_MAGIC: [u8; 16] = *b"FANTICON-EXPORT\x01";
const FOOTER_SIZE: u64 = 24;
const MANIFEST_NAME: &str = "manifest.txt";
const DEFAULT_ICON_PNG: &[u8] = include_bytes!("../assets/branding/fanticon-icon-master.png");

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportPlatform {
    WindowsX86_64,
    WindowsArm64,
    LinuxX86_64,
    LinuxArm64,
    MacosUniversal,
}

impl ExportPlatform {
    pub const ALL: [Self; 5] = [
        Self::WindowsX86_64,
        Self::WindowsArm64,
        Self::LinuxX86_64,
        Self::LinuxArm64,
        Self::MacosUniversal,
    ];

    pub const fn name(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "windows-x86_64",
            Self::WindowsArm64 => "windows-arm64",
            Self::LinuxX86_64 => "linux-x86_64",
            Self::LinuxArm64 => "linux-arm64",
            Self::MacosUniversal => "macos-universal",
        }
    }

    const fn template(self) -> &'static str {
        match self {
            Self::WindowsX86_64 => "windows-x86_64/fanticon-player.exe",
            Self::WindowsArm64 => "windows-arm64/fanticon-player.exe",
            Self::LinuxX86_64 => "linux-x86_64/fanticon-player",
            Self::LinuxArm64 => "linux-arm64/fanticon-player",
            Self::MacosUniversal => "macos-universal/fanticon-player",
        }
    }
}

impl std::str::FromStr for ExportPlatform {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        Self::ALL
            .into_iter()
            .find(|platform| platform.name().eq_ignore_ascii_case(value))
            .ok_or_else(|| format!("unsupported export platform {value}"))
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportMetadata {
    pub title: String,
    pub cartridge_id: Option<u64>,
    pub author: Option<String>,
    pub description: Option<String>,
    pub icon: Option<PathBuf>,
    pub web_background: String,
    pub web_foreground: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseFile {
    pub path: String,
    pub bytes: u64,
    pub sha256: String,
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ReleaseManifest {
    pub format: u32,
    pub fanticon_version: String,
    pub title: String,
    pub cartridge_id: Option<String>,
    pub files: Vec<ReleaseFile>,
}

impl ExportMetadata {
    pub fn from_title(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
            cartridge_id: None,
            author: None,
            description: None,
            icon: None,
            web_background: "#08080C".to_owned(),
            web_foreground: "#EEEEEE".to_owned(),
        }
    }
}

#[derive(Clone, Debug)]
pub struct RuntimeKit {
    root: PathBuf,
}

impl RuntimeKit {
    pub fn locate(explicit: Option<&Path>) -> Result<Self, String> {
        let candidates = explicit
            .map(Path::to_path_buf)
            .into_iter()
            .chain(env::var_os("FANTICON_RUNTIME_KIT").map(PathBuf::from))
            .chain(
                env::current_exe()
                    .ok()
                    .and_then(|path| path.parent().map(|parent| parent.join("runtimes"))),
            )
            .chain(env::current_exe().ok().and_then(|path| {
                path.parent()
                    .and_then(Path::parent)
                    .map(|prefix| prefix.join("lib/fanticon/runtimes"))
            }))
            .chain([PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("runtime-kit")]);
        let root = candidates.into_iter().find(|path| path.is_dir()).ok_or_else(|| {
            "runtime kit not found; reinstall Fanticon or pass --runtime-kit".to_owned()
        })?;
        let kit = Self { root };
        kit.validate()?;
        Ok(kit)
    }

    pub fn validate(&self) -> Result<(), String> {
        let source = fs::read_to_string(self.root.join(MANIFEST_NAME))
            .map_err(|_| "runtime kit manifest is missing".to_owned())?;
        let expected = format!("VERSION={}", env!("CARGO_PKG_VERSION"));
        if !source.lines().any(|line| line.trim() == "FANTICON_RUNTIME_KIT=1") {
            return Err("runtime kit format is not supported".to_owned());
        }
        if !source.lines().any(|line| line.trim() == expected) {
            return Err(format!(
                "runtime kit version does not match Fanticon {}",
                env!("CARGO_PKG_VERSION")
            ));
        }
        for platform in ExportPlatform::ALL {
            require_file(&self.root.join(platform.template()))?;
        }
        for path in [
            "web/fanticon.js",
            "web/fanticon_bg.wasm",
            "licenses/LICENSE-MIT",
            "licenses/LICENSE-APACHE",
        ] {
            require_file(&self.root.join(path))?;
        }
        Ok(())
    }
}

pub fn export_html(
    kit: &RuntimeKit,
    cartridge: &[u8],
    metadata: &ExportMetadata,
    output: &Path,
) -> Result<(), String> {
    let runtime = kit.root.join("web");
    require_file(&runtime.join("fanticon.js"))?;
    require_file(&runtime.join("fanticon_bg.wasm"))?;
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    fs::copy(runtime.join("fanticon.js"), output.join("fanticon.js"))
        .map_err(|error| error.to_string())?;
    fs::copy(runtime.join("fanticon_bg.wasm"), output.join("fanticon_bg.wasm"))
        .map_err(|error| error.to_string())?;
    fs::write(output.join("game.fcn"), cartridge).map_err(|error| error.to_string())?;
    write_web_icons(metadata, output)?;
    copy_licenses(kit, output)?;
    fs::write(output.join("manifest.webmanifest"), web_manifest(metadata))
        .map_err(|error| error.to_string())?;
    fs::write(output.join("service-worker.js"), service_worker(metadata, cartridge))
        .map_err(|error| error.to_string())?;
    fs::write(output.join("index.html"), html_shell(metadata)).map_err(|error| error.to_string())
}

pub fn export_binary(
    kit: &RuntimeKit,
    platform: ExportPlatform,
    cartridge: &[u8],
    metadata: &ExportMetadata,
    output: &Path,
) -> Result<(), String> {
    let template = kit.root.join(platform.template());
    require_file(&template)?;
    let data = standalone_player_data(&template, platform, cartridge, metadata)?;
    write_export_file(output, &data)?;
    let directory = output.parent().unwrap_or_else(|| Path::new("."));
    copy_licenses(kit, directory)?;
    let mut manifest = format!("TITLE={}\n", metadata.title);
    if let Some(author) = &metadata.author {
        manifest.push_str(&format!("AUTHOR={author}\n"));
    }
    if let Some(description) = &metadata.description {
        manifest.push_str(&format!("DESCRIPTION={description}\n"));
    }
    fs::write(output.with_extension("txt"), manifest)
        .map_err(|error| format!("could not write export metadata: {error}"))?;
    if let Some(icon) = &metadata.icon {
        let extension = icon.extension().and_then(|value| value.to_str()).unwrap_or("png");
        fs::copy(icon, output.with_extension(extension))
            .map_err(|error| format!("could not copy export icon: {error}"))?;
    }
    Ok(())
}

/// Create a publishable platform-native game archive without invoking a compiler.
pub fn export_package(
    kit: &RuntimeKit,
    platform: ExportPlatform,
    cartridge: &[u8],
    metadata: &ExportMetadata,
    output: &Path,
) -> Result<PathBuf, String> {
    let template = kit.root.join(platform.template());
    require_file(&template)?;
    let player = standalone_player_data(&template, platform, cartridge, metadata)?;
    let slug = file_slug(&metadata.title);
    let notes = package_notes(metadata, platform);
    let licenses = license_entries(kit)?;

    match platform {
        ExportPlatform::WindowsX86_64 | ExportPlatform::WindowsArm64 => {
            let root = format!("{slug}-{}", platform.name());
            let mut entries = vec![
                ArchiveEntry::file(format!("{root}/{slug}.exe"), player, 0o755),
                ArchiveEntry::text(format!("{root}/README.txt"), notes, 0o644),
            ];
            append_license_entries(&mut entries, &format!("{root}/licenses"), licenses);
            write_zip(output, &entries)?;
        }
        ExportPlatform::LinuxX86_64 | ExportPlatform::LinuxArm64 => {
            let root = format!("{slug}-{}.AppDir", platform.name());
            let desktop = linux_desktop(metadata, &slug);
            let app_run = linux_app_run(&slug);
            let mut entries = vec![
                ArchiveEntry::text(format!("{root}/AppRun"), app_run, 0o755),
                ArchiveEntry::file(format!("{root}/usr/bin/{slug}"), player, 0o755),
                ArchiveEntry::text(
                    format!("{root}/usr/share/applications/{slug}.desktop"),
                    desktop,
                    0o644,
                ),
                ArchiveEntry::text(format!("{root}/usr/share/doc/{slug}/README.txt"), notes, 0o644),
            ];
            entries.push(ArchiveEntry::file(
                format!("{root}/usr/share/icons/hicolor/256x256/apps/{slug}.png"),
                resized_icon(metadata, 256)?,
                0o644,
            ));
            append_license_entries(
                &mut entries,
                &format!("{root}/usr/share/licenses/{slug}"),
                licenses,
            );
            write_tar_gz(output, &entries)?;
        }
        ExportPlatform::MacosUniversal => {
            let bundle = format!("{}.app", bundle_name(&metadata.title));
            let executable = format!("{bundle}/Contents/MacOS/{slug}");
            let mut entries = vec![
                ArchiveEntry::file(executable, player, 0o755),
                ArchiveEntry::text(
                    format!("{bundle}/Contents/Info.plist"),
                    macos_info_plist(metadata, &slug),
                    0o644,
                ),
                ArchiveEntry::text(format!("{bundle}/Contents/Resources/README.txt"), notes, 0o644),
            ];
            entries.push(ArchiveEntry::file(
                format!("{bundle}/Contents/Resources/GameIcon.icns"),
                make_icns(metadata)?,
                0o644,
            ));
            append_license_entries(
                &mut entries,
                &format!("{bundle}/Contents/Resources/licenses"),
                licenses,
            );
            write_zip(output, &entries)?;
        }
    }
    Ok(output.to_path_buf())
}

/// Export the web player and every native package into one release directory.
pub fn export_all(
    kit: &RuntimeKit,
    cartridge: &[u8],
    metadata: &ExportMetadata,
    output: &Path,
) -> Result<Vec<PathBuf>, String> {
    fs::create_dir_all(output).map_err(|error| error.to_string())?;
    let slug = file_slug(&metadata.title);
    let web = output.join(format!("{slug}-web"));
    export_html(kit, cartridge, metadata, &web)?;
    let mut artifacts = vec![web];
    for platform in ExportPlatform::ALL {
        let extension = match platform {
            ExportPlatform::LinuxX86_64 | ExportPlatform::LinuxArm64 => "tar.gz",
            _ => "zip",
        };
        let artifact = output.join(format!("{slug}-{}.{extension}", platform.name()));
        export_package(kit, platform, cartridge, metadata, &artifact)?;
        artifacts.push(artifact);
    }
    let manifest = artifacts
        .iter()
        .map(|path| path.file_name().unwrap_or_default().to_string_lossy())
        .collect::<Vec<_>>()
        .join("\n");
    fs::write(
        output.join("RELEASE.txt"),
        format!("{}\n\nArtifacts:\n{manifest}\n", release_notes(metadata)),
    )
    .map_err(|error| error.to_string())?;
    write_release_manifest(output, metadata)?;
    Ok(artifacts)
}

/// Verify every file recorded by an `export_all` release manifest.
pub fn verify_release(output: &Path) -> Result<ReleaseManifest, String> {
    let manifest_path = output.join("release.json");
    let manifest: ReleaseManifest = serde_json::from_slice(
        &fs::read(&manifest_path)
            .map_err(|error| format!("could not read {}: {error}", manifest_path.display()))?,
    )
    .map_err(|error| format!("release manifest is invalid: {error}"))?;
    if manifest.format != 1 {
        return Err(format!("release manifest format {} is not supported", manifest.format));
    }
    let actual = release_files(output)?;
    if manifest.files != actual {
        let expected_paths = manifest.files.iter().map(|file| &file.path).collect::<Vec<_>>();
        let actual_paths = actual.iter().map(|file| &file.path).collect::<Vec<_>>();
        if expected_paths != actual_paths {
            return Err("release file inventory does not match release.json".to_owned());
        }
        return Err("release file size or SHA-256 does not match release.json".to_owned());
    }
    Ok(manifest)
}

pub fn write_standalone_player(
    template: &Path,
    cartridge: &[u8],
    output: &Path,
) -> Result<(), String> {
    if template == output {
        return Err("runtime template and export output must be different files".to_owned());
    }
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let mut data = fs::read(template).map_err(|error| {
        format!("could not read runtime template {}: {error}", template.display())
    })?;
    append_cartridge(&mut data, cartridge);
    write_export_file(output, &data)
}

#[derive(Clone, Debug)]
struct ArchiveEntry {
    path: String,
    data: Vec<u8>,
    mode: u32,
}

impl ArchiveEntry {
    fn file(path: String, data: Vec<u8>, mode: u32) -> Self {
        Self { path, data, mode }
    }

    fn text(path: String, data: String, mode: u32) -> Self {
        Self::file(path, data.into_bytes(), mode)
    }
}

fn standalone_player_data(
    template: &Path,
    platform: ExportPlatform,
    cartridge: &[u8],
    metadata: &ExportMetadata,
) -> Result<Vec<u8>, String> {
    let mut data = fs::read(template).map_err(|error| {
        format!("could not read runtime template {}: {error}", template.display())
    })?;
    if matches!(platform, ExportPlatform::WindowsX86_64 | ExportPlatform::WindowsArm64)
        && data.starts_with(b"MZ")
    {
        data = customize_windows_player(data, metadata)?;
    }
    append_cartridge(&mut data, cartridge);
    Ok(data)
}

fn append_cartridge(data: &mut Vec<u8>, cartridge: &[u8]) {
    data.extend_from_slice(cartridge);
    data.extend_from_slice(&(cartridge.len() as u64).to_le_bytes());
    data.extend_from_slice(&FOOTER_MAGIC);
}

fn write_export_file(output: &Path, data: &[u8]) -> Result<(), String> {
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    fs::write(output, data)
        .map_err(|error| format!("could not write {}: {error}", output.display()))
}

fn customize_windows_player(data: Vec<u8>, metadata: &ExportMetadata) -> Result<Vec<u8>, String> {
    let mut image = PeImage::parse(data)
        .map_err(|error| format!("could not parse Windows runtime template: {error}"))?;
    let mut resources = image.resource_directory().cloned().unwrap_or_default();
    if let Some(icon) = metadata.icon.as_deref() {
        let icon = image::ImageReader::open(icon)
            .map_err(|error| format!("could not open export icon: {error}"))?
            .decode()
            .map_err(|error| format!("could not decode export icon: {error}"))?;
        resources
            .set_main_icon(icon)
            .map_err(|error| format!("could not embed Windows icon: {error}"))?;
    }

    let mut version = resources
        .get_version_info()
        .map_err(|error| format!("could not read Windows version metadata: {error}"))?
        .unwrap_or_else(VersionInfo::default);
    let mut strings = version.strings.first().cloned().unwrap_or_else(|| VersionStringTable {
        key: "040904B0".to_owned(),
        ..VersionStringTable::default()
    });
    let slug = file_slug(&metadata.title);
    let original_filename = format!("{slug}.exe");
    let description = metadata.description.as_deref().unwrap_or(&metadata.title);
    let author = metadata.author.as_deref().unwrap_or("Fanticon");
    for (key, value) in [
        (VS_PRODUCT_NAME, metadata.title.as_str()),
        (VS_FILE_DESCRIPTION, description),
        (VS_COMPANY_NAME, author),
        (VS_INTERNAL_NAME, slug.as_str()),
        (VS_ORIGINAL_FILENAME, original_filename.as_str()),
        (VS_FILE_VERSION, env!("CARGO_PKG_VERSION")),
        (VS_PRODUCT_VERSION, env!("CARGO_PKG_VERSION")),
        (VS_COMMENTS, "Packaged by Fanticon"),
    ] {
        strings.strings.insert(key.to_owned(), value.to_owned());
    }
    version.strings = vec![strings];
    if version.vars.is_empty() {
        version.vars.push(VersionU16 { major: 0x0409, minor: 1200 });
    }
    resources
        .set_version_info(&version)
        .map_err(|error| format!("could not embed Windows version metadata: {error}"))?;
    image
        .set_resource_directory(resources)
        .map_err(|error| format!("could not rebuild Windows resources: {error}"))?;
    let mut output = Vec::new();
    image
        .write_writer(&mut output)
        .map_err(|error| format!("could not write customized Windows player: {error}"))?;
    Ok(output)
}

fn license_entries(kit: &RuntimeKit) -> Result<Vec<(String, Vec<u8>)>, String> {
    let root = kit.root.join("licenses");
    ["LICENSE-MIT", "LICENSE-APACHE"]
        .into_iter()
        .map(|name| {
            fs::read(root.join(name))
                .map(|data| (name.to_owned(), data))
                .map_err(|error| format!("could not read {name}: {error}"))
        })
        .collect()
}

fn append_license_entries(
    entries: &mut Vec<ArchiveEntry>,
    root: &str,
    licenses: Vec<(String, Vec<u8>)>,
) {
    entries.extend(
        licenses
            .into_iter()
            .map(|(name, data)| ArchiveEntry::file(format!("{root}/{name}"), data, 0o644)),
    );
}

fn write_zip(output: &Path, entries: &[ArchiveEntry]) -> Result<(), String> {
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = File::create(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    let mut archive = ZipWriter::new(file);
    for entry in entries {
        let options = SimpleFileOptions::default()
            .compression_method(CompressionMethod::Deflated)
            .unix_permissions(entry.mode);
        archive
            .start_file(&entry.path, options)
            .map_err(|error| format!("could not add {} to archive: {error}", entry.path))?;
        archive.write_all(&entry.data).map_err(|error| error.to_string())?;
    }
    archive.finish().map_err(|error| error.to_string())?;
    Ok(())
}

fn write_tar_gz(output: &Path, entries: &[ArchiveEntry]) -> Result<(), String> {
    if let Some(parent) = output.parent().filter(|parent| !parent.as_os_str().is_empty()) {
        fs::create_dir_all(parent).map_err(|error| error.to_string())?;
    }
    let file = File::create(output)
        .map_err(|error| format!("could not create {}: {error}", output.display()))?;
    let encoder = GzEncoder::new(file, Compression::default());
    let mut archive = tar::Builder::new(encoder);
    for entry in entries {
        let mut header = tar::Header::new_gnu();
        header.set_size(entry.data.len() as u64);
        header.set_mode(entry.mode);
        header.set_mtime(0);
        header.set_cksum();
        archive
            .append_data(&mut header, &entry.path, Cursor::new(&entry.data))
            .map_err(|error| format!("could not add {} to archive: {error}", entry.path))?;
    }
    let encoder = archive.into_inner().map_err(|error| error.to_string())?;
    encoder.finish().map_err(|error| error.to_string())?;
    Ok(())
}

pub fn read_standalone_cartridge(executable: &Path) -> Result<Option<Vec<u8>>, String> {
    let mut file = File::open(executable).map_err(|error| error.to_string())?;
    let file_len = file.metadata().map_err(|error| error.to_string())?.len();
    if file_len < FOOTER_SIZE {
        return Ok(None);
    }
    file.seek(SeekFrom::End(-(FOOTER_SIZE as i64))).map_err(|error| error.to_string())?;
    let mut footer = [0; FOOTER_SIZE as usize];
    file.read_exact(&mut footer).map_err(|error| error.to_string())?;
    if footer[8..] != FOOTER_MAGIC {
        return Ok(None);
    }
    let cartridge_len = u64::from_le_bytes(footer[..8].try_into().expect("eight byte length"));
    if cartridge_len > file_len - FOOTER_SIZE {
        return Err("standalone player has an invalid cartridge length".to_owned());
    }
    file.seek(SeekFrom::Start(file_len - FOOTER_SIZE - cartridge_len))
        .map_err(|error| error.to_string())?;
    let mut cartridge = vec![0; cartridge_len as usize];
    file.read_exact(&mut cartridge).map_err(|error| error.to_string())?;
    Ok(Some(cartridge))
}

fn copy_licenses(kit: &RuntimeKit, output: &Path) -> Result<(), String> {
    let source = kit.root.join("licenses");
    require_file(&source.join("LICENSE-MIT"))?;
    require_file(&source.join("LICENSE-APACHE"))?;
    let destination = output.join("licenses");
    fs::create_dir_all(&destination).map_err(|error| error.to_string())?;
    for name in ["LICENSE-MIT", "LICENSE-APACHE"] {
        fs::copy(source.join(name), destination.join(name)).map_err(|error| error.to_string())?;
    }
    Ok(())
}

fn write_web_icons(metadata: &ExportMetadata, output: &Path) -> Result<(), String> {
    fs::write(output.join("icon-192.png"), resized_icon(metadata, 192)?)
        .map_err(|error| format!("could not write web icon: {error}"))?;
    fs::write(output.join("icon-512.png"), resized_icon(metadata, 512)?)
        .map_err(|error| format!("could not write web icon: {error}"))
}

fn icon_image(metadata: &ExportMetadata) -> Result<image::DynamicImage, String> {
    let data = if let Some(path) = metadata.icon.as_deref() {
        fs::read(path).map_err(|error| format!("could not read export icon: {error}"))?
    } else {
        DEFAULT_ICON_PNG.to_vec()
    };
    image::load_from_memory_with_format(&data, ImageFormat::Png)
        .map_err(|error| format!("could not decode export icon: {error}"))
}

fn resized_icon(metadata: &ExportMetadata, size: u32) -> Result<Vec<u8>, String> {
    let resized = icon_image(metadata)?.resize_to_fill(size, size, FilterType::Lanczos3);
    let mut png = Cursor::new(Vec::new());
    resized
        .write_to(&mut png, ImageFormat::Png)
        .map_err(|error| format!("could not encode export icon: {error}"))?;
    Ok(png.into_inner())
}

fn require_file(path: &Path) -> Result<(), String> {
    path.is_file().then_some(()).ok_or_else(|| format!("runtime kit is missing {}", path.display()))
}

fn html_shell(metadata: &ExportMetadata) -> String {
    let title = escape_html(&metadata.title);
    let author = metadata.author.as_deref().map(escape_html).unwrap_or_default();
    let description = metadata.description.as_deref().map(escape_html).unwrap_or_default();
    let icon = "<link rel=\"icon\" href=\"icon-192.png\">";
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>{title}</title><meta name="author" content="{author}"><meta name="description" content="{description}"><meta name="theme-color" content="{background}"><link rel="manifest" href="manifest.webmanifest">{icon}<style>
html,body{{width:100%;height:100%;margin:0;background:{background};color:{foreground};font:14px system-ui,sans-serif;overflow:hidden}}
body{{display:grid;place-items:center}}canvas{{display:block;width:min(100vw,calc(100vh * 1.6));height:auto;max-height:100vh}}
#start{{position:fixed;inset:0;display:grid;place-items:center;background:{background};z-index:2}}nav{{position:fixed;right:12px;bottom:12px;display:flex;gap:8px;opacity:.72}}button{{border:1px solid currentColor;border-radius:5px;padding:9px 14px;background:{background};color:{foreground};cursor:pointer}}
</style></head><body><div id="start"><button id="play">Play {title}</button></div><nav hidden><button id="screenshot">Screenshot</button><button id="fullscreen">Fullscreen</button></nav>
<script type="module">globalThis.FANTICON_DEFER_START=true;const runtime=await import('./fanticon.js');await runtime.default();const cartridge=new Uint8Array(await (await fetch('./game.fcn')).arrayBuffer());const play=document.getElementById('play');play.onclick=()=>{{play.disabled=true;runtime.start_fanticon(cartridge);document.getElementById('start').remove();document.querySelector('nav').hidden=false;setTimeout(()=>document.getElementById('fanticon-display')?.focus(),0)}};
document.getElementById('fullscreen').onclick=()=>document.getElementById('fanticon-display')?.requestFullscreen();document.getElementById('screenshot').onclick=()=>document.getElementById('fanticon-display')?.toBlob(blob=>{{if(!blob)return;const a=Object.assign(document.createElement('a'),{{href:URL.createObjectURL(blob),download:'fanticon-{slug}.png'}});a.click();setTimeout(()=>URL.revokeObjectURL(a.href),1000)}});if('serviceWorker' in navigator)navigator.serviceWorker.register('./service-worker.js');</script>
</body></html>"#,
        background = metadata.web_background,
        foreground = metadata.web_foreground,
        slug = file_slug(&metadata.title)
    )
}

fn web_manifest(metadata: &ExportMetadata) -> String {
    let title = escape_json(&metadata.title);
    let description = escape_json(metadata.description.as_deref().unwrap_or(&metadata.title));
    format!(
        "{{\n  \"name\": \"{title}\",\n  \"short_name\": \"{title}\",\n  \"description\": \"{description}\",\n  \"start_url\": \"./\",\n  \"scope\": \"./\",\n  \"display\": \"fullscreen\",\n  \"orientation\": \"landscape\",\n  \"background_color\": \"{}\",\n  \"theme_color\": \"{}\",\n  \"icons\": [{{\"src\":\"icon-192.png\",\"sizes\":\"192x192\",\"type\":\"image/png\",\"purpose\":\"any maskable\"}},{{\"src\":\"icon-512.png\",\"sizes\":\"512x512\",\"type\":\"image/png\",\"purpose\":\"any maskable\"}}]\n}}\n",
        metadata.web_background, metadata.web_background
    )
}

fn service_worker(metadata: &ExportMetadata, cartridge: &[u8]) -> String {
    let identity = metadata
        .cartridge_id
        .map(|id| format!("{id:016x}"))
        .unwrap_or_else(|| file_slug(&metadata.title));
    let content = cartridge.iter().fold(0xcbf29ce484222325_u64, |hash, byte| {
        (hash ^ u64::from(*byte)).wrapping_mul(0x100000001b3)
    });
    format!(
        "const CACHE='fanticon-{}-{identity}-{content:016x}';const FILES=['./','./index.html','./manifest.webmanifest','./fanticon.js','./fanticon_bg.wasm','./game.fcn','./licenses/LICENSE-MIT','./licenses/LICENSE-APACHE','./icon-192.png','./icon-512.png'];self.addEventListener('install',event=>event.waitUntil(caches.open(CACHE).then(cache=>cache.addAll(FILES)).then(()=>self.skipWaiting())));self.addEventListener('activate',event=>event.waitUntil(caches.keys().then(keys=>Promise.all(keys.filter(key=>key.startsWith('fanticon-')&&key!==CACHE).map(key=>caches.delete(key)))).then(()=>self.clients.claim())));self.addEventListener('fetch',event=>{{if(event.request.method!=='GET')return;event.respondWith(caches.match(event.request).then(hit=>hit||fetch(event.request).then(response=>{{const copy=response.clone();caches.open(CACHE).then(cache=>cache.put(event.request,copy));return response}})))}});\n",
        env!("CARGO_PKG_VERSION")
    )
}

fn package_notes(metadata: &ExportMetadata, platform: ExportPlatform) -> String {
    let mut notes = format!("{}\nPlatform: {}\n", metadata.title, platform.name());
    if let Some(author) = &metadata.author {
        notes.push_str(&format!("Author: {author}\n"));
    }
    if let Some(description) = &metadata.description {
        notes.push_str(&format!("\n{description}\n"));
    }
    notes.push_str(
        "\nPackaged by Fanticon. No Fanticon installation or development toolchain is required.\n",
    );
    notes
}

fn release_notes(metadata: &ExportMetadata) -> String {
    let mut notes = metadata.title.clone();
    if let Some(author) = &metadata.author {
        notes.push_str(&format!("\nAuthor: {author}"));
    }
    if let Some(description) = &metadata.description {
        notes.push_str(&format!("\n\n{description}"));
    }
    notes.push_str("\n\nPackaged by Fanticon. Every artifact is self-contained and requires no Fanticon or development toolchain installation.");
    notes
}

fn write_release_manifest(output: &Path, metadata: &ExportMetadata) -> Result<(), String> {
    let manifest = ReleaseManifest {
        format: 1,
        fanticon_version: env!("CARGO_PKG_VERSION").to_owned(),
        title: metadata.title.clone(),
        cartridge_id: metadata.cartridge_id.map(|id| format!("{id:016X}")),
        files: release_files(output)?,
    };
    let mut json = serde_json::to_vec_pretty(&manifest)
        .map_err(|error| format!("could not encode release manifest: {error}"))?;
    json.push(b'\n');
    fs::write(output.join("release.json"), json)
        .map_err(|error| format!("could not write release.json: {error}"))
}

fn release_files(root: &Path) -> Result<Vec<ReleaseFile>, String> {
    let mut files = Vec::new();
    collect_release_files(root, root, &mut files)?;
    files.sort_by(|left, right| left.path.cmp(&right.path));
    Ok(files)
}

fn collect_release_files(
    root: &Path,
    directory: &Path,
    files: &mut Vec<ReleaseFile>,
) -> Result<(), String> {
    let entries = fs::read_dir(directory)
        .map_err(|error| format!("could not inspect {}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| error.to_string())?;
        let path = entry.path();
        let metadata = fs::symlink_metadata(&path).map_err(|error| error.to_string())?;
        if metadata.file_type().is_symlink() {
            return Err(format!("release contains unsupported symbolic link {}", path.display()));
        }
        if metadata.is_dir() {
            collect_release_files(root, &path, files)?;
            continue;
        }
        if !metadata.is_file() || path == root.join("release.json") {
            continue;
        }
        let relative = path
            .strip_prefix(root)
            .map_err(|_| format!("{} is outside the release directory", path.display()))?
            .to_string_lossy()
            .replace('\\', "/");
        let mut source = File::open(&path).map_err(|error| error.to_string())?;
        let mut digest = Sha256::new();
        let mut buffer = [0_u8; 64 * 1024];
        loop {
            let count = source.read(&mut buffer).map_err(|error| error.to_string())?;
            if count == 0 {
                break;
            }
            digest.update(&buffer[..count]);
        }
        files.push(ReleaseFile {
            path: relative,
            bytes: metadata.len(),
            sha256: format!("{:x}", digest.finalize()),
        });
    }
    Ok(())
}

fn linux_desktop(metadata: &ExportMetadata, slug: &str) -> String {
    let comment = metadata.description.as_deref().unwrap_or("Fanticon game");
    let icon = format!("Icon={slug}\n");
    format!(
        "[Desktop Entry]\nType=Application\nName={}\nComment={}\nExec={slug}\n{icon}Terminal=false\nCategories=Game;\n",
        metadata.title, comment
    )
}

fn linux_app_run(slug: &str) -> String {
    format!(
        "#!/bin/sh\nHERE=$(CDPATH= cd -- \"$(dirname -- \"$0\")\" && pwd)\nexec \"$HERE/usr/bin/{slug}\" \"$@\"\n"
    )
}

fn macos_info_plist(metadata: &ExportMetadata, slug: &str) -> String {
    let identifier = metadata
        .cartridge_id
        .map(|id| format!("game.fanticon.id-{id:016x}"))
        .unwrap_or_else(|| format!("game.fanticon.{slug}"));
    let icon = "<key>CFBundleIconFile</key><string>GameIcon.icns</string>";
    format!(
        "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n<!DOCTYPE plist PUBLIC \"-//Apple//DTD PLIST 1.0//EN\" \"http://www.apple.com/DTDs/PropertyList-1.0.dtd\">\n<plist version=\"1.0\"><dict><key>CFBundleName</key><string>{}</string><key>CFBundleDisplayName</key><string>{}</string><key>CFBundleIdentifier</key><string>{identifier}</string><key>CFBundleVersion</key><string>{}</string><key>CFBundleShortVersionString</key><string>{}</string><key>CFBundleExecutable</key><string>{slug}</string>{icon}<key>CFBundlePackageType</key><string>APPL</string><key>LSMinimumSystemVersion</key><string>11.0</string><key>LSApplicationCategoryType</key><string>public.app-category.games</string><key>NSHighResolutionCapable</key><true/></dict></plist>\n",
        escape_xml(&metadata.title),
        escape_xml(&metadata.title),
        env!("CARGO_PKG_VERSION"),
        env!("CARGO_PKG_VERSION")
    )
}

fn make_icns(metadata: &ExportMetadata) -> Result<Vec<u8>, String> {
    let image = icon_image(metadata)?;
    let specifications = [
        (*b"icp4", 16),
        (*b"icp5", 32),
        (*b"ic11", 32),
        (*b"ic12", 64),
        (*b"ic07", 128),
        (*b"ic08", 256),
        (*b"ic13", 256),
        (*b"ic09", 512),
        (*b"ic14", 512),
        (*b"ic10", 1024),
    ];
    let mut entries = Vec::new();
    for (kind, size) in specifications {
        let resized = image.resize_to_fill(size, size, FilterType::Lanczos3);
        let mut png = Cursor::new(Vec::new());
        resized
            .write_to(&mut png, ImageFormat::Png)
            .map_err(|error| format!("could not encode macOS icon: {error}"))?;
        let png = png.into_inner();
        entries.extend_from_slice(&kind);
        entries.extend_from_slice(&((png.len() + 8) as u32).to_be_bytes());
        entries.extend_from_slice(&png);
    }
    let mut output = Vec::with_capacity(entries.len() + 8);
    output.extend_from_slice(b"icns");
    output.extend_from_slice(&((entries.len() + 8) as u32).to_be_bytes());
    output.extend_from_slice(&entries);
    Ok(output)
}

fn escape_json(value: &str) -> String {
    value.replace('\\', "\\\\").replace('"', "\\\"")
}

fn escape_xml(value: &str) -> String {
    escape_html(value).replace('\'', "&apos;")
}

fn bundle_name(value: &str) -> String {
    let name = value
        .chars()
        .map(|character| {
            if character.is_ascii_alphanumeric() || matches!(character, ' ' | '-' | '_') {
                character
            } else {
                '-'
            }
        })
        .collect::<String>();
    let name = name.trim_matches([' ', '-', '_']);
    if name.is_empty() { "Fanticon Game".to_owned() } else { name.to_owned() }
}

fn escape_html(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn file_slug(value: &str) -> String {
    let slug =
        value
            .chars()
            .map(|character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
            })
            .collect::<String>()
            .trim_matches('-')
            .to_owned();
    if slug.is_empty() { "fanticon-game".to_owned() } else { slug }
}

#[cfg(test)]
mod tests {
    use super::*;
    use flate2::read::GzDecoder;
    use std::collections::BTreeMap;

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fanticon-export-{}-{name}", std::process::id()))
    }

    fn complete_kit(name: &str) -> (PathBuf, RuntimeKit) {
        let root = temporary(name);
        let _ = fs::remove_dir_all(&root);
        for platform in ExportPlatform::ALL {
            let path = root.join(platform.template());
            fs::create_dir_all(path.parent().unwrap()).unwrap();
            fs::write(path, format!("{} runtime", platform.name())).unwrap();
        }
        fs::create_dir_all(root.join("web")).unwrap();
        fs::create_dir_all(root.join("licenses")).unwrap();
        fs::write(root.join("web/fanticon.js"), b"js").unwrap();
        fs::write(root.join("web/fanticon_bg.wasm"), b"wasm").unwrap();
        fs::write(root.join("licenses/LICENSE-MIT"), b"mit").unwrap();
        fs::write(root.join("licenses/LICENSE-APACHE"), b"apache").unwrap();
        fs::write(
            root.join(MANIFEST_NAME),
            format!("FANTICON_RUNTIME_KIT=1\nVERSION={}\n", env!("CARGO_PKG_VERSION")),
        )
        .unwrap();
        let kit = RuntimeKit::locate(Some(&root)).unwrap();
        (root, kit)
    }

    #[test]
    fn standalone_footer_round_trips_without_changing_template_prefix() {
        let template = temporary("template");
        let output = temporary("output");
        fs::write(&template, b"pretend executable").unwrap();
        write_standalone_player(&template, b"cartridge", &output).unwrap();
        assert!(fs::read(&output).unwrap().starts_with(b"pretend executable"));
        assert_eq!(read_standalone_cartridge(&output).unwrap(), Some(b"cartridge".to_vec()));
        let _ = fs::remove_file(template);
        let _ = fs::remove_file(output);
    }

    #[test]
    fn html_has_metadata_and_waits_for_user_gesture() {
        let mut metadata = ExportMetadata::from_title("A&B");
        metadata.author = Some("Author".to_owned());
        let html = html_shell(&metadata);
        assert!(html.contains("<title>A&amp;B</title>"));
        assert!(html.contains("Play A&amp;B"));
        assert!(html.contains("FANTICON_DEFER_START=true"));
        assert!(html.contains("play.onclick=()=>"));
        assert!(html.contains("start_fanticon(cartridge)"));
        assert!(html.contains("name=\"author\" content=\"Author\""));
    }

    #[test]
    fn complete_kit_exports_web_and_foreign_binary_without_tools() {
        let (root, kit) = complete_kit("kit");
        let output = temporary("kit-output");
        let _ = fs::remove_dir_all(&output);
        let metadata = ExportMetadata::from_title("Smoke Test");
        export_html(&kit, b"cart", &metadata, &output.join("web")).unwrap();
        export_binary(
            &kit,
            ExportPlatform::WindowsArm64,
            b"cart",
            &metadata,
            &output.join("foreign.exe"),
        )
        .unwrap();
        assert_eq!(
            read_standalone_cartridge(&output.join("foreign.exe")).unwrap(),
            Some(b"cart".to_vec())
        );
        assert!(output.join("web/licenses/LICENSE-MIT").is_file());
        assert!(output.join("web/manifest.webmanifest").is_file());
        assert!(output.join("web/service-worker.js").is_file());
        assert!(output.join("web/icon-192.png").is_file());
        assert!(output.join("web/icon-512.png").is_file());
        let manifest = fs::read_to_string(output.join("web/manifest.webmanifest")).unwrap();
        let parsed: serde_json::Value = serde_json::from_str(&manifest).unwrap();
        assert_eq!(parsed["display"], "fullscreen");
        assert_eq!(parsed["icons"].as_array().unwrap().len(), 2);
        let icon = image::open(output.join("web/icon-512.png")).unwrap();
        assert_eq!((icon.width(), icon.height()), (512, 512));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(output);
    }

    #[test]
    fn platform_packages_have_native_layouts_and_executable_modes() {
        let (root, kit) = complete_kit("packages-kit");
        let output = temporary("packages-output");
        let _ = fs::remove_dir_all(&output);
        fs::create_dir_all(&output).unwrap();
        let metadata = ExportMetadata::from_title("Package Test");

        let windows = output.join("windows.zip");
        export_package(&kit, ExportPlatform::WindowsX86_64, b"cart", &metadata, &windows).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(windows).unwrap()).unwrap();
        assert!(zip.by_name("package-test-windows-x86_64/package-test.exe").is_ok());
        assert_eq!(
            zip.by_name("package-test-windows-x86_64/package-test.exe").unwrap().unix_mode(),
            Some(0o100755)
        );

        let macos = output.join("macos.zip");
        export_package(&kit, ExportPlatform::MacosUniversal, b"cart", &metadata, &macos).unwrap();
        let mut zip = zip::ZipArchive::new(File::open(macos).unwrap()).unwrap();
        assert!(zip.by_name("Package Test.app/Contents/Info.plist").is_ok());
        assert_eq!(
            zip.by_name("Package Test.app/Contents/MacOS/package-test").unwrap().unix_mode(),
            Some(0o100755)
        );

        let linux = output.join("linux.tar.gz");
        export_package(&kit, ExportPlatform::LinuxArm64, b"cart", &metadata, &linux).unwrap();
        let decoder = GzDecoder::new(File::open(linux).unwrap());
        let mut archive = tar::Archive::new(decoder);
        let entries = archive
            .entries()
            .unwrap()
            .map(|entry| {
                let entry = entry.unwrap();
                (
                    entry.path().unwrap().to_string_lossy().into_owned(),
                    entry.header().mode().unwrap(),
                )
            })
            .collect::<BTreeMap<_, _>>();
        assert_eq!(entries["package-test-linux-arm64.AppDir/AppRun"], 0o755);
        assert_eq!(entries["package-test-linux-arm64.AppDir/usr/bin/package-test"], 0o755);
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(output);
    }

    #[test]
    fn export_all_creates_web_and_every_native_artifact() {
        let (root, kit) = complete_kit("all-kit");
        let output = temporary("all-output");
        let _ = fs::remove_dir_all(&output);
        let artifacts =
            export_all(&kit, b"cart", &ExportMetadata::from_title("Release Test"), &output)
                .unwrap();
        assert_eq!(artifacts.len(), 6);
        assert!(artifacts.iter().all(|path| path.exists()));
        assert!(output.join("RELEASE.txt").is_file());
        assert!(output.join("release.json").is_file());
        assert!(output.join("release-test-web/manifest.webmanifest").is_file());
        let manifest = verify_release(&output).unwrap();
        assert_eq!(manifest.format, 1);
        assert!(manifest.files.iter().any(|file| file.path == "RELEASE.txt"));
        fs::write(output.join("RELEASE.txt"), b"tampered").unwrap();
        assert!(verify_release(&output).unwrap_err().contains("SHA-256"));
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(output);
    }

    #[test]
    fn punctuation_only_titles_get_a_safe_filename() {
        assert_eq!(file_slug("?!"), "fanticon-game");
    }

    #[test]
    fn service_worker_cache_changes_when_the_cartridge_changes() {
        let metadata = ExportMetadata::from_title("Cache Test");
        assert_ne!(service_worker(&metadata, b"first"), service_worker(&metadata, b"second"));
    }

    /// Release CI supplies the assembled cross-platform kit so this exercises
    /// real PE, ELF, Mach-O, and WebAssembly templates on one Linux host.
    #[test]
    fn assembled_runtime_kit_packages_every_platform_without_target_tools() {
        let Some(root) = env::var_os("FANTICON_TEST_RUNTIME_KIT").map(PathBuf::from) else {
            return;
        };
        let kit = RuntimeKit::locate(Some(&root)).unwrap();
        let output = temporary("real-kit-output");
        let _ = fs::remove_dir_all(&output);
        let mut metadata = ExportMetadata::from_title("Release Smoke");
        metadata.author = Some("Fanticon CI".to_owned());
        metadata.description = Some("Cross-platform package smoke test".to_owned());
        metadata.icon = Some(
            PathBuf::from(env!("CARGO_MANIFEST_DIR"))
                .join("assets/branding/fanticon-icon-master.png"),
        );
        let artifacts = export_all(&kit, b"release-smoke-cartridge", &metadata, &output).unwrap();
        assert_eq!(artifacts.len(), 6);
        assert!(artifacts.iter().all(|artifact| artifact.exists()));

        let windows = output.join("release-smoke-windows-x86_64.zip");
        let mut archive = zip::ZipArchive::new(File::open(windows).unwrap()).unwrap();
        let mut executable =
            archive.by_name("release-smoke-windows-x86_64/release-smoke.exe").unwrap();
        let extracted = output.join("release-smoke.exe");
        let mut file = File::create(&extracted).unwrap();
        std::io::copy(&mut executable, &mut file).unwrap();
        drop(file);
        assert_eq!(
            read_standalone_cartridge(&extracted).unwrap(),
            Some(b"release-smoke-cartridge".to_vec())
        );
        let _ = fs::remove_dir_all(output);
    }
}
