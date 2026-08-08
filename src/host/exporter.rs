#[cfg(not(target_arch = "wasm32"))]
use fanticon::{
    export::{ExportPlatform, RuntimeKit, export_binary, export_html},
    project::{MANIFEST_NAME, ProjectManifest},
};

use super::filesystem::SharedFilesystem;

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportTarget {
    Html,
    WindowsX86_64,
    WindowsArm64,
    LinuxX86_64,
    LinuxArm64,
    MacosUniversal,
}

impl ExportTarget {
    pub const ALL: [Self; 6] = [
        Self::Html,
        Self::WindowsX86_64,
        Self::WindowsArm64,
        Self::LinuxX86_64,
        Self::LinuxArm64,
        Self::MacosUniversal,
    ];

    pub const fn label(self) -> &'static str {
        match self {
            Self::Html => "Web",
            Self::WindowsX86_64 => "Windows x64",
            Self::WindowsArm64 => "Windows ARM",
            Self::LinuxX86_64 => "Linux x64",
            Self::LinuxArm64 => "Linux ARM",
            Self::MacosUniversal => "macOS",
        }
    }

    pub fn parse(value: &str) -> Result<Self, String> {
        match value.to_ascii_lowercase().as_str() {
            "html" | "web" => Ok(Self::Html),
            "windows-x86_64" | "win64" => Ok(Self::WindowsX86_64),
            "windows-arm64" | "winarm" => Ok(Self::WindowsArm64),
            "linux-x86_64" | "linux64" => Ok(Self::LinuxX86_64),
            "linux-arm64" | "linuxarm" => Ok(Self::LinuxArm64),
            "macos-universal" | "macos" => Ok(Self::MacosUniversal),
            _ => Err(format!("Unknown export target {value}")),
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    const fn default_output(self) -> &'static str {
        match self {
            Self::Html => "export",
            Self::WindowsX86_64 => "win64.exe",
            Self::WindowsArm64 => "winarm.exe",
            Self::LinuxX86_64 => "linux64",
            Self::LinuxArm64 => "linuxarm",
            Self::MacosUniversal => "macos",
        }
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn platform(self) -> Option<ExportPlatform> {
        match self {
            Self::Html => None,
            Self::WindowsX86_64 => Some(ExportPlatform::WindowsX86_64),
            Self::WindowsArm64 => Some(ExportPlatform::WindowsArm64),
            Self::LinuxX86_64 => Some(ExportPlatform::LinuxX86_64),
            Self::LinuxArm64 => Some(ExportPlatform::LinuxArm64),
            Self::MacosUniversal => Some(ExportPlatform::MacosUniversal),
        }
    }
}

pub fn export_project(
    filesystem: &SharedFilesystem,
    target: ExportTarget,
    output: Option<&str>,
) -> Result<String, String> {
    #[cfg(target_arch = "wasm32")]
    {
        let _ = (filesystem, target, output);
        return Err("Exports are available in native Fanticon builds".to_owned());
    }
    #[cfg(not(target_arch = "wasm32"))]
    {
        let success = super::builder::build_project(filesystem).map_err(format_diagnostics)?;
        let manifest_source = filesystem.borrow().read_text(MANIFEST_NAME)?;
        let manifest = ProjectManifest::parse(&manifest_source)?;
        let manifest_path = filesystem.borrow().host_path(MANIFEST_NAME)?;
        let directory = manifest_path.parent().ok_or_else(|| "Invalid project path".to_owned())?;
        let metadata = manifest.export_metadata(directory);
        let cartridge = filesystem.borrow().read_binary(&success.output)?;
        let output_name = output.unwrap_or_else(|| target.default_output());
        let output_path = filesystem.borrow().host_path(output_name)?;
        let kit = RuntimeKit::locate(None)?;
        match target.platform() {
            None => export_html(&kit, &cartridge, &metadata, &output_path)?,
            Some(platform) => export_binary(&kit, platform, &cartridge, &metadata, &output_path)?,
        }
        Ok(format!("Exported {} to {}", manifest.title, output_name))
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn format_diagnostics(diagnostics: Vec<fanticon::assembler::Diagnostic>) -> String {
    diagnostics
        .into_iter()
        .map(|diagnostic| {
            format!(
                "{}:{}:{} {}",
                diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
            )
        })
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn target_names_have_short_console_aliases() {
        assert_eq!(ExportTarget::parse("web"), Ok(ExportTarget::Html));
        assert_eq!(ExportTarget::parse("winarm"), Ok(ExportTarget::WindowsArm64));
        assert_eq!(ExportTarget::parse("linux64"), Ok(ExportTarget::LinuxX86_64));
        assert!(ExportTarget::parse("amiga").is_err());
    }
}
