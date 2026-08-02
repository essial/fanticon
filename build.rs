#[path = "build_support/code_assets.rs"]
mod code_assets;

use std::{env, path::PathBuf};

fn main() {
    println!("cargo:rerun-if-changed=code-assets");
    println!("cargo:rerun-if-env-changed=FANTICON_SKIP_CODE_ASSET_SYNC");

    if env::var_os("CARGO_FEATURE_APP_HOST").is_none()
        || env::var_os("FANTICON_SKIP_CODE_ASSET_SYNC").is_some()
        || env::var_os("HOST") != env::var_os("TARGET")
    {
        return;
    }

    let Some(documents) = dirs::document_dir() else {
        println!(
            "cargo:warning=Fanticon code assets were not synced: Documents directory not found"
        );
        return;
    };
    let source = PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"))
        .join("code-assets");
    let destination = documents.join("Fanticon");

    if let Err(error) = code_assets::sync_children(&source, &destination) {
        panic!(
            "could not sync Fanticon code assets from {} to {}: {error}",
            source.display(),
            destination.display()
        );
    }
}
