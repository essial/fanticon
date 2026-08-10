use std::{env, ffi::OsString, fs, path::Path, path::PathBuf, process::ExitCode};

use fanticon::{
    assembler::{Diagnostic, SymbolSection, assemble_with_loader},
    project::{MANIFEST_NAME, build_project_with_loader},
};

const USAGE: &str = "\
Fanticon headless build checker

Usage:
  fanticon-check <project-directory> [--out <file.fcn>] [--check-only]
  fanticon-check <file.asm> [--out <file.bin>] [--check-only]

A project directory is any directory containing fanticon.cfg (matched
case-insensitively); it is assembled the same way the in-app BUILD command
assembles a cartridge project. A bare .asm path assembles a single raw
source file with no cartridge sections, matching the in-app BUILD command
for non-project sources.

Diagnostics print as `source:line:column: message`, one per line, and the
process exits nonzero if assembly fails. --check-only assembles without
writing the output file, for a fast compile-only feedback loop.
";

fn main() -> ExitCode {
    match run() {
        Ok(()) => ExitCode::SUCCESS,
        Err(code) => code,
    }
}

fn run() -> Result<(), ExitCode> {
    let mut arguments = env::args_os().skip(1).collect::<Vec<_>>();
    if arguments.is_empty()
        || arguments.first().is_some_and(|value| matches!(value.to_str(), Some("-h" | "--help")))
    {
        print!("{USAGE}");
        return Ok(());
    }
    let check_only = take_flag(&mut arguments, "--check-only");
    let out_path = take_option(&mut arguments, "--out")?;
    if arguments.len() != 1 {
        eprint!("{USAGE}");
        return Err(ExitCode::FAILURE);
    }
    let target = PathBuf::from(&arguments[0]);

    let result = if target.is_dir() {
        build_project(&target, out_path.as_deref(), check_only)
    } else {
        build_raw(&target, out_path.as_deref(), check_only)
    };

    match result {
        Ok(()) => Ok(()),
        Err(Failure::Diagnostics(diagnostics)) => {
            print_diagnostics(&diagnostics);
            Err(ExitCode::FAILURE)
        }
        Err(Failure::Message(message)) => {
            eprintln!("fanticon-check: {message}");
            Err(ExitCode::FAILURE)
        }
    }
}

enum Failure {
    Diagnostics(Vec<Diagnostic>),
    Message(String),
}

impl From<Vec<Diagnostic>> for Failure {
    fn from(diagnostics: Vec<Diagnostic>) -> Self {
        Failure::Diagnostics(diagnostics)
    }
}

impl From<String> for Failure {
    fn from(message: String) -> Self {
        Failure::Message(message)
    }
}

fn build_project(
    directory: &Path,
    out_path: Option<&Path>,
    check_only: bool,
) -> Result<(), Failure> {
    let manifest_path = case_insensitive_child(directory, MANIFEST_NAME)?
        .ok_or_else(|| format!("no {MANIFEST_NAME} found in {}", directory.display()))?;
    let manifest_source = fs::read_to_string(&manifest_path)
        .map_err(|error| format!("{}: {error}", manifest_path.display()))?;

    let build = build_project_with_loader(&manifest_source, |path| load_file(directory, path))?;

    let output_path =
        out_path.map(PathBuf::from).unwrap_or_else(|| directory.join(&build.manifest.output));
    if !check_only {
        fs::write(&output_path, &build.bytes)
            .map_err(|error| format!("{}: {error}", output_path.display()))?;
    }

    let banks = build.cartridge.bank_count();
    let noun = if banks == 1 { "bank" } else { "banks" };
    if check_only {
        println!(
            "OK: {} assembles cleanly ({} bytes, {banks} ROM {noun})",
            build.manifest.title,
            build.bytes.len()
        );
    } else {
        println!(
            "Built {} -> {} ({} bytes, {banks} ROM {noun})",
            build.manifest.title,
            output_path.display(),
            build.bytes.len()
        );
    }
    for usage in &build.bank_usage {
        let label = match usage.section {
            SymbolSection::Fixed => "FIXED".to_owned(),
            SymbolSection::Bank(number) => format!("BANK {number}"),
        };
        println!("  {label}: {}/{} bytes used ({} free)", usage.used, usage.capacity, usage.free());
    }
    Ok(())
}

fn build_raw(file: &Path, out_path: Option<&Path>, check_only: bool) -> Result<(), Failure> {
    let source_name =
        file.file_name().and_then(|name| name.to_str()).unwrap_or("source.asm").to_owned();
    let directory = file.parent().filter(|parent| !parent.as_os_str().is_empty());
    let source =
        fs::read_to_string(file).map_err(|error| format!("{}: {error}", file.display()))?;

    let program = assemble_with_loader(&source_name, &source, |path| {
        let Some(directory) = directory else {
            return Err("include files are unavailable without a project directory".to_owned());
        };
        load_file(directory, path)
    })?;

    let output_path = out_path.map(PathBuf::from).unwrap_or_else(|| file.with_extension("bin"));
    if !check_only {
        fs::write(&output_path, &program.bytes)
            .map_err(|error| format!("{}: {error}", output_path.display()))?;
    }

    if check_only {
        println!(
            "OK: {} assembles cleanly (origin ${:04X}, {} bytes)",
            source_name,
            program.origin,
            program.bytes.len()
        );
    } else {
        println!(
            "Built {} -> {} (origin ${:04X}, {} bytes)",
            source_name,
            output_path.display(),
            program.origin,
            program.bytes.len()
        );
    }
    Ok(())
}

/// Resolve an assembler-supplied include path (arbitrary case, `/`-or-`\`
/// separated) against files actually on disk, the same way the in-app
/// filesystem sandbox matches names case-insensitively.
fn load_file(root: &Path, relative: &str) -> Result<String, String> {
    let mut path = root.to_owned();
    for component in relative.split(['/', '\\']) {
        if component.is_empty() || component == "." {
            continue;
        }
        if component == ".." {
            return Err(format!("{relative}: cannot leave the project directory"));
        }
        path = case_insensitive_child(&path, component)
            .map_err(|error| format!("{relative}: {error}"))?
            .ok_or_else(|| format!("{relative}: not found"))?;
    }
    fs::read_to_string(&path).map_err(|error| format!("{relative}: {error}"))
}

fn case_insensitive_child(directory: &Path, name: &str) -> Result<Option<PathBuf>, String> {
    let entries =
        fs::read_dir(directory).map_err(|error| format!("{}: {error}", directory.display()))?;
    for entry in entries {
        let entry = entry.map_err(|error| format!("{}: {error}", directory.display()))?;
        if entry.file_name().to_string_lossy().eq_ignore_ascii_case(name) {
            return Ok(Some(entry.path()));
        }
    }
    Ok(None)
}

fn print_diagnostics(diagnostics: &[Diagnostic]) {
    for diagnostic in diagnostics {
        eprintln!(
            "{}:{}:{}: error: {}",
            diagnostic.source, diagnostic.line, diagnostic.column, diagnostic.message
        );
    }
    let noun = if diagnostics.len() == 1 { "error" } else { "errors" };
    eprintln!("{} {noun}", diagnostics.len());
}

fn take_flag(arguments: &mut Vec<OsString>, name: &str) -> bool {
    if let Some(index) = arguments.iter().position(|argument| argument == name) {
        arguments.remove(index);
        true
    } else {
        false
    }
}

fn take_option(arguments: &mut Vec<OsString>, name: &str) -> Result<Option<PathBuf>, ExitCode> {
    let Some(index) = arguments.iter().position(|argument| argument == name) else {
        return Ok(None);
    };
    arguments.remove(index);
    if index >= arguments.len() {
        eprintln!("{name} needs a path");
        return Err(ExitCode::FAILURE);
    }
    Ok(Some(PathBuf::from(arguments.remove(index))))
}
