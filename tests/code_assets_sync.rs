#[path = "../build_support/code_assets.rs"]
mod code_assets;

use std::path::Path;

#[test]
fn code_assets_fit_the_55_column_editor_width() {
    let assets = Path::new(env!("CARGO_MANIFEST_DIR")).join("code-assets");
    check_file_width(&assets.join("fanticon.inc"));
    check_source_width(&assets.join("demos"));
}

fn check_source_width(directory: &Path) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            check_source_width(&path);
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("");
        if !matches!(extension.to_ascii_lowercase().as_str(), "asm" | "txt") {
            continue;
        }
        check_file_width(&path);
    }
}

fn check_file_width(path: &Path) {
    let source = std::fs::read_to_string(path).unwrap();
    for (index, line) in source.lines().enumerate() {
        assert!(
            line.chars().count() <= 55,
            "{}:{} exceeds 55 columns: {line}",
            path.display(),
            index + 1
        );
    }
}
