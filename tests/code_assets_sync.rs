#[path = "../build_support/code_assets.rs"]
mod code_assets;

use std::path::Path;

#[test]
fn demo_comments_and_text_fit_the_55_column_editor_width() {
    let demos = Path::new(env!("CARGO_MANIFEST_DIR")).join("code-assets/demos");
    check_demo_width(&demos);
}

fn check_demo_width(directory: &Path) {
    for entry in std::fs::read_dir(directory).unwrap() {
        let path = entry.unwrap().path();
        if path.is_dir() {
            check_demo_width(&path);
            continue;
        }
        let extension = path.extension().and_then(|value| value.to_str()).unwrap_or("");
        if !matches!(extension.to_ascii_lowercase().as_str(), "asm" | "txt") {
            continue;
        }
        let source = std::fs::read_to_string(&path).unwrap();
        for (index, line) in source.lines().enumerate() {
            assert!(
                line.chars().count() <= 55,
                "{}:{} exceeds 55 columns: {line}",
                path.display(),
                index + 1
            );
        }
    }
}
