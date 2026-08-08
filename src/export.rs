//! Toolchain-free HTML and standalone-player packaging.

use std::{
    env, fs,
    fs::File,
    io::{Read, Seek, SeekFrom, Write},
    path::{Path, PathBuf},
};

const FOOTER_MAGIC: [u8; 16] = *b"FANTICON-EXPORT\x01";
const FOOTER_SIZE: u64 = 24;
const MANIFEST_NAME: &str = "manifest.txt";

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
    pub author: Option<String>,
    pub description: Option<String>,
    pub icon: Option<PathBuf>,
    pub web_background: String,
    pub web_foreground: String,
}

impl ExportMetadata {
    pub fn from_title(title: impl Into<String>) -> Self {
        Self {
            title: title.into(),
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
    copy_icon(metadata, output)?;
    copy_licenses(kit, output)?;
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
    write_standalone_player(&template, cartridge, output)?;
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
    fs::copy(template, output).map_err(|error| {
        format!("could not copy runtime template {}: {error}", template.display())
    })?;
    let mut file = fs::OpenOptions::new()
        .append(true)
        .open(output)
        .map_err(|error| format!("could not open {}: {error}", output.display()))?;
    file.write_all(cartridge).map_err(|error| error.to_string())?;
    file.write_all(&(cartridge.len() as u64).to_le_bytes()).map_err(|error| error.to_string())?;
    file.write_all(&FOOTER_MAGIC).map_err(|error| error.to_string())?;
    file.sync_all().map_err(|error| error.to_string())
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

fn copy_icon(metadata: &ExportMetadata, output: &Path) -> Result<(), String> {
    let Some(icon) = &metadata.icon else { return Ok(()) };
    fs::copy(icon, output.join("icon.png"))
        .map(|_| ())
        .map_err(|error| format!("could not copy export icon: {error}"))
}

fn require_file(path: &Path) -> Result<(), String> {
    path.is_file().then_some(()).ok_or_else(|| format!("runtime kit is missing {}", path.display()))
}

fn html_shell(metadata: &ExportMetadata) -> String {
    let title = escape_html(&metadata.title);
    let author = metadata.author.as_deref().map(escape_html).unwrap_or_default();
    let description = metadata.description.as_deref().map(escape_html).unwrap_or_default();
    let icon =
        metadata.icon.as_ref().map(|_| "<link rel=\"icon\" href=\"icon.png\">").unwrap_or("");
    format!(
        r#"<!doctype html>
<html lang="en"><head><meta charset="utf-8"><meta name="viewport" content="width=device-width,initial-scale=1,viewport-fit=cover">
<title>{title}</title><meta name="author" content="{author}"><meta name="description" content="{description}">{icon}<style>
html,body{{width:100%;height:100%;margin:0;background:{background};color:{foreground};font:14px system-ui,sans-serif;overflow:hidden}}
body{{display:grid;place-items:center}}canvas{{display:block;width:min(100vw,calc(100vh * 1.6));height:auto;max-height:100vh}}
#start{{position:fixed;inset:0;display:grid;place-items:center;background:{background};z-index:2}}nav{{position:fixed;right:12px;bottom:12px;display:flex;gap:8px;opacity:.72}}button{{border:1px solid currentColor;border-radius:5px;padding:9px 14px;background:{background};color:{foreground};cursor:pointer}}
</style></head><body><div id="start"><button id="play">Play {title}</button></div><nav hidden><button id="screenshot">Screenshot</button><button id="fullscreen">Fullscreen</button></nav>
<script type="module">globalThis.FANTICON_DEFER_START=true;const runtime=await import('./fanticon.js');await runtime.default();const cartridge=new Uint8Array(await (await fetch('./game.fcn')).arrayBuffer());const play=document.getElementById('play');play.onclick=()=>{{play.disabled=true;runtime.start_fanticon(cartridge);document.getElementById('start').remove();document.querySelector('nav').hidden=false;setTimeout(()=>document.getElementById('fanticon-display')?.focus(),0)}};
document.getElementById('fullscreen').onclick=()=>document.getElementById('fanticon-display')?.requestFullscreen();document.getElementById('screenshot').onclick=()=>document.getElementById('fanticon-display')?.toBlob(blob=>{{if(!blob)return;const a=Object.assign(document.createElement('a'),{{href:URL.createObjectURL(blob),download:'fanticon-{slug}.png'}});a.click();setTimeout(()=>URL.revokeObjectURL(a.href),1000)}});</script>
</body></html>"#,
        background = metadata.web_background,
        foreground = metadata.web_foreground,
        slug = file_slug(&metadata.title)
    )
}

fn escape_html(value: &str) -> String {
    value.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;").replace('"', "&quot;")
}

fn file_slug(value: &str) -> String {
    value
        .chars()
        .map(
            |character| {
                if character.is_ascii_alphanumeric() { character.to_ascii_lowercase() } else { '-' }
            },
        )
        .collect::<String>()
        .trim_matches('-')
        .to_owned()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temporary(name: &str) -> PathBuf {
        std::env::temp_dir().join(format!("fanticon-export-{}-{name}", std::process::id()))
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
        let root = temporary("kit");
        let output = temporary("kit-output");
        let _ = fs::remove_dir_all(&root);
        let _ = fs::remove_dir_all(&output);
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
        let _ = fs::remove_dir_all(root);
        let _ = fs::remove_dir_all(output);
    }
}
