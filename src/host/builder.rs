use fanticon::assembler::{AssembledProgram, Diagnostic, assemble_with_loader};

use super::filesystem::SharedFilesystem;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BuildSuccess {
    pub output: String,
    pub origin: u16,
    pub size: usize,
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
}
