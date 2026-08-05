#[path = "build_support/code_assets.rs"]
mod code_assets;

use std::{env, path::PathBuf};

fn main() {
    embed_windows_icon();

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

/// Embed the Fanticon app icon into fanticon-app.exe so it shows up in
/// Explorer, the taskbar, and Alt-Tab. NSIS shortcuts inherit this icon
/// automatically since they point at the exe.
#[cfg(target_os = "windows")]
fn embed_windows_icon() {
    if env::var_os("CARGO_FEATURE_APP_HOST").is_none() {
        return;
    }

    let manifest_dir =
        PathBuf::from(env::var_os("CARGO_MANIFEST_DIR").expect("manifest directory"));
    let icon = manifest_dir.join("assets/branding/fanticon.ico");
    println!("cargo:rerun-if-changed={}", icon.display());

    let mut resource = winresource::WindowsResource::new();
    resource.set_icon(icon.to_str().expect("icon path must be valid UTF-8"));
    if let Err(error) = resource.compile() {
        panic!("could not embed Windows icon resource: {error}");
    }
}

#[cfg(not(target_os = "windows"))]
fn embed_windows_icon() {}
