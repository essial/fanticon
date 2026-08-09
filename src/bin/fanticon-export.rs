use std::{env, fs, path::PathBuf, process::ExitCode, str::FromStr};

use fanticon::{
    cartridge::Cartridge,
    export::{
        ExportMetadata, ExportPlatform, RuntimeKit, export_all, export_binary, export_html,
        export_package,
    },
    project::{MANIFEST_NAME, ProjectManifest},
};

const USAGE: &str = "\
Fanticon toolchain-free exporter

Usage:
  fanticon-export verify [--runtime-kit <directory>]
  fanticon-export html <game.fcn> [output-directory] [--runtime-kit <directory>]
  fanticon-export binary <platform> <game.fcn> [output-file] [--runtime-kit <directory>]
  fanticon-export package <platform> <game.fcn> [output-archive] [--runtime-kit <directory>]
  fanticon-export all <game.fcn> [output-directory] [--runtime-kit <directory>]

Platforms:
  windows-x86_64  windows-arm64  linux-x86_64  linux-arm64  macos-universal
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("fanticon-export: {error}");
            ExitCode::FAILURE
        }
    }
}

fn run() -> Result<(), String> {
    let mut arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments
        .first()
        .is_some_and(|value| matches!(value.to_str(), Some("-h" | "--help" | "help")))
    {
        print!("{USAGE}");
        return Ok(());
    }
    let kit_path = take_option(&mut arguments, "--runtime-kit")?;
    let kit = RuntimeKit::locate(kit_path.as_deref())?;
    let format = take_string(&mut arguments)?;
    if format.eq_ignore_ascii_case("verify") {
        if !arguments.is_empty() {
            return Err(USAGE.to_owned());
        }
        kit.validate()?;
        println!("Runtime kit {} is complete and compatible", env!("CARGO_PKG_VERSION"));
        return Ok(());
    }
    enum Command {
        Web,
        Binary(ExportPlatform),
        Package(ExportPlatform),
        All,
    }
    let command = match format.to_ascii_lowercase().as_str() {
        "html" | "web" => Command::Web,
        "binary" | "native" | "bin" => {
            Command::Binary(ExportPlatform::from_str(&take_string(&mut arguments)?)?)
        }
        "package" | "archive" => {
            Command::Package(ExportPlatform::from_str(&take_string(&mut arguments)?)?)
        }
        "all" | "release" => Command::All,
        _ => return Err(USAGE.to_owned()),
    };
    let cartridge_path = take_path(&mut arguments)?;
    let output = arguments.first().map(PathBuf::from);
    if output.is_some() {
        arguments.remove(0);
    }
    if !arguments.is_empty() {
        return Err(USAGE.to_owned());
    }
    let bytes = fs::read(&cartridge_path)
        .map_err(|error| format!("could not read {}: {error}", cartridge_path.display()))?;
    let cartridge = Cartridge::from_bytes(&bytes).map_err(|error| error.0)?;
    let metadata = discover_metadata(&cartridge_path, &cartridge)?;
    match command {
        Command::Web => {
            let output = output.unwrap_or_else(|| {
                cartridge_path.with_file_name(format!("{}-web", stem(&cartridge_path)))
            });
            export_html(&kit, &bytes, &metadata, &output)?;
            println!("Exported {} to {}", cartridge.title, output.display());
        }
        Command::Binary(platform) => {
            let suffix = if platform.name().starts_with("windows-") { ".exe" } else { "" };
            let output = output.unwrap_or_else(|| {
                cartridge_path.with_file_name(format!(
                    "{}-{}{suffix}",
                    stem(&cartridge_path),
                    platform.name()
                ))
            });
            export_binary(&kit, platform, &bytes, &metadata, &output)?;
            println!(
                "Exported {} for {} to {}",
                cartridge.title,
                platform.name(),
                output.display()
            );
        }
        Command::Package(platform) => {
            let extension =
                if matches!(platform, ExportPlatform::LinuxX86_64 | ExportPlatform::LinuxArm64) {
                    "tar.gz"
                } else {
                    "zip"
                };
            let output = output.unwrap_or_else(|| {
                cartridge_path.with_file_name(format!(
                    "{}-{}.{extension}",
                    stem(&cartridge_path),
                    platform.name()
                ))
            });
            export_package(&kit, platform, &bytes, &metadata, &output)?;
            println!(
                "Packaged {} for {} at {}",
                cartridge.title,
                platform.name(),
                output.display()
            );
        }
        Command::All => {
            let output = output.unwrap_or_else(|| {
                cartridge_path.with_file_name(format!("{}-release", stem(&cartridge_path)))
            });
            let artifacts = export_all(&kit, &bytes, &metadata, &output)?;
            println!("Exported {} release to {}", cartridge.title, output.display());
            for artifact in artifacts {
                println!("  {}", artifact.display());
            }
        }
    }
    Ok(())
}

fn discover_metadata(
    path: &std::path::Path,
    cartridge: &Cartridge,
) -> Result<ExportMetadata, String> {
    let mut metadata = if let Some(directory) = path.parent() {
        let manifest_path = directory.join(MANIFEST_NAME);
        if manifest_path.is_file() {
            let manifest = ProjectManifest::parse(
                &fs::read_to_string(&manifest_path).map_err(|error| error.to_string())?,
            )?;
            manifest.export_metadata(directory)
        } else {
            ExportMetadata::from_title(&cartridge.title)
        }
    } else {
        ExportMetadata::from_title(&cartridge.title)
    };
    metadata.cartridge_id = Some(cartridge.id);
    Ok(metadata)
}

fn take_option(
    arguments: &mut Vec<std::ffi::OsString>,
    name: &str,
) -> Result<Option<PathBuf>, String> {
    let Some(index) = arguments.iter().position(|argument| argument == name) else {
        return Ok(None);
    };
    arguments.remove(index);
    if index >= arguments.len() {
        return Err(format!("{name} needs a directory"));
    }
    Ok(Some(PathBuf::from(arguments.remove(index))))
}

fn take_string(arguments: &mut Vec<std::ffi::OsString>) -> Result<String, String> {
    if arguments.is_empty() {
        return Err(USAGE.to_owned());
    }
    arguments.remove(0).into_string().map_err(|_| "arguments must be valid UTF-8".to_owned())
}

fn take_path(arguments: &mut Vec<std::ffi::OsString>) -> Result<PathBuf, String> {
    if arguments.is_empty() {
        return Err(USAGE.to_owned());
    }
    Ok(PathBuf::from(arguments.remove(0)))
}

fn stem(path: &std::path::Path) -> String {
    path.file_stem().and_then(|value| value.to_str()).unwrap_or("fanticon-game").to_owned()
}
