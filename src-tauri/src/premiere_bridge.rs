//! Installs the `com.videe.bridge` CEP panel into Adobe's per-user extensions
//! folder. The panel itself is the server half of the Premiere bridge (see
//! integrations/premiere/com.videe.bridge/); Videe's client half lives in
//! `bridge.rs`.
//!
//! The panel is unsigned, so Premiere only loads it with CEP's `PlayerDebugMode`
//! turned on. Installing sets that per-user preference for every recent CSXS
//! runtime version, the same way other Tauri apps with a Premiere panel do it
//! (e.g. AutoSubs). It's a per-user setting that only relaxes signature
//! checking for CEP extensions; it doesn't touch Premiere itself.

use std::path::{Path, PathBuf};
use std::process::Command;
use tauri::AppHandle;

const BUNDLE_ID: &str = "com.videe.bridge";

/// Where the panel's files ship inside the app (see `resources` in
/// tauri.conf.json), relative to the resource directory.
const BUNDLED_DIR: &str = "premiere/com.videe.bridge";

/// CSXS runtime versions to enable `PlayerDebugMode` for. Each Premiere release
/// reads the key for its own CEP version (Premiere 2020 is CSXS 9, 2024-2025
/// are CSXS 11-12), so this covers 2020 onward plus a little headroom.
const CSXS_VERSIONS: std::ops::RangeInclusive<u32> = 9..=14;

fn bundled_extension_dir(app: &AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .resolve(BUNDLED_DIR, tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())
}

/// Adobe's per-user folder: `~/Library/Application Support/Adobe` on macOS,
/// `%APPDATA%\Adobe` on Windows. Only macOS is exercised in practice so far;
/// the Windows path follows Adobe's documented CEP layout but is unverified.
fn adobe_dir() -> Result<PathBuf, String> {
    if !cfg!(any(target_os = "macos", target_os = "windows")) {
        return Err("Premiere Pro only runs on macOS and Windows".to_string());
    }
    let data_dir = dirs::data_dir().ok_or("could not determine this OS's application data directory")?;
    Ok(data_dir.join("Adobe"))
}

pub fn install_target_path() -> Result<PathBuf, String> {
    Ok(adobe_dir()?.join("CEP").join("extensions").join(BUNDLE_ID))
}

/// Copies the bundled panel into Adobe's CEP extensions folder, replacing any
/// previous copy so files dropped from a newer version don't linger, and turns
/// on `PlayerDebugMode`. Errors rather than creating a fake Adobe folder if
/// there isn't one, since that almost always means no Adobe app is installed.
pub fn install_extension(app: &AppHandle) -> Result<PathBuf, String> {
    let adobe = adobe_dir()?;
    if !adobe.is_dir() {
        return Err(format!(
            "Adobe's application support folder wasn't found at {}. Is Premiere Pro installed?",
            adobe.display()
        ));
    }
    let target = install_target_path()?;
    install_into(&bundled_extension_dir(app)?, &target)?;
    enable_player_debug_mode()?;
    Ok(target)
}

fn install_into(source: &Path, target: &Path) -> Result<(), String> {
    if !source.join("CSXS").join("manifest.xml").is_file() {
        return Err(format!("the bundled Premiere extension is missing from {}", source.display()));
    }
    if target.exists() {
        std::fs::remove_dir_all(target).map_err(|e| format!("couldn't remove the old extension at {}: {e}", target.display()))?;
    }
    copy_dir(source, target).map_err(|e| format!("couldn't copy the extension to {}: {e}", target.display()))
}

fn copy_dir(source: &Path, target: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(target)?;
    for entry in std::fs::read_dir(source)? {
        let entry = entry?;
        let dest = target.join(entry.file_name());
        if entry.file_type()?.is_dir() {
            copy_dir(&entry.path(), &dest)?;
        } else {
            std::fs::copy(entry.path(), dest)?;
        }
    }
    Ok(())
}

fn enable_player_debug_mode() -> Result<(), String> {
    for version in CSXS_VERSIONS {
        let output = if cfg!(target_os = "windows") {
            Command::new("reg")
                .args(["add", &format!(r"HKCU\Software\Adobe\CSXS.{version}"), "/v", "PlayerDebugMode", "/t", "REG_SZ", "/d", "1", "/f"])
                .output()
        } else {
            Command::new("defaults")
                .args(["write", &format!("com.adobe.CSXS.{version}"), "PlayerDebugMode", "1"])
                .output()
        }
        .map_err(|e| format!("couldn't enable unsigned CEP extensions: {e}"))?;
        if !output.status.success() {
            return Err(format!(
                "couldn't enable unsigned CEP extensions (CSXS.{version}): {}",
                String::from_utf8_lossy(&output.stderr).trim()
            ));
        }
    }
    Ok(())
}

pub fn extension_status() -> Option<PathBuf> {
    let target = install_target_path().ok()?;
    target.join("CSXS").join("manifest.xml").is_file().then_some(target)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_dir() -> PathBuf {
        std::env::temp_dir().join(format!("videe-premiere-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn install_copies_tree_and_replaces_stale_files() {
        let root = temp_dir();
        let source = root.join("source");
        let target = root.join("extensions").join(BUNDLE_ID);
        std::fs::create_dir_all(source.join("CSXS")).unwrap();
        std::fs::write(source.join("CSXS").join("manifest.xml"), "<manifest/>").unwrap();
        std::fs::write(source.join("main.js"), "new").unwrap();

        std::fs::create_dir_all(&target).unwrap();
        std::fs::write(target.join("main.js"), "old").unwrap();
        std::fs::write(target.join("stale.js"), "left over").unwrap();

        install_into(&source, &target).unwrap();

        assert_eq!(std::fs::read_to_string(target.join("CSXS").join("manifest.xml")).unwrap(), "<manifest/>");
        assert_eq!(std::fs::read_to_string(target.join("main.js")).unwrap(), "new");
        assert!(!target.join("stale.js").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn install_refuses_a_source_without_a_manifest() {
        let root = temp_dir();
        std::fs::create_dir_all(root.join("source")).unwrap();
        assert!(install_into(&root.join("source"), &root.join("target")).is_err());
        assert!(!root.join("target").exists());
        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn bundled_extension_matches_the_resource_layout() {
        // tauri.conf.json bundles each file of the repo's extension folder under
        // BUNDLED_DIR, so the repo copy must have everything the manifest names.
        let repo_ext = Path::new(env!("CARGO_MANIFEST_DIR")).join("../integrations/premiere").join(BUNDLE_ID);
        for file in ["CSXS/manifest.xml", "index.html", "main.js", "host.jsx"] {
            assert!(repo_ext.join(file).is_file(), "missing {file}");
        }
        let conf = std::fs::read_to_string(Path::new(env!("CARGO_MANIFEST_DIR")).join("tauri.conf.json")).unwrap();
        for file in ["CSXS/manifest.xml", "index.html", "main.js", "host.jsx"] {
            assert!(conf.contains(&format!("\"{BUNDLED_DIR}/{file}\"")), "tauri.conf.json doesn't bundle {file}");
        }
    }
}
