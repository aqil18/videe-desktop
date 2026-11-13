//! Installs the `Videe.lua` bridge script into DaVinci Resolve's Utility scripts
//! folder. The script itself is the server half of the bridge; Videe's client
//! half lives in `bridge.rs`.

use std::path::PathBuf;
use tauri::AppHandle;

/// Where the bundled `Videe.lua` ships inside the app, so it can be copied out
/// to Resolve's scripts folder.
pub fn bundled_script_path(app: &AppHandle) -> Result<PathBuf, String> {
    use tauri::Manager;
    app.path()
        .resolve("Videe.lua", tauri::path::BaseDirectory::Resource)
        .map_err(|e| e.to_string())
}

/// DaVinci Resolve's per-user "Utility" scripts folder. Only macOS is exercised in
/// practice so far (matching the ffmpeg sidecar's current aarch64-apple-darwin-only
/// coverage) -- Windows/Linux paths follow Blackmagic's documented layout but are
/// unverified.
pub fn install_target_path() -> Result<PathBuf, String> {
    let data_dir = dirs::data_dir().ok_or("could not determine this OS's application data directory")?;
    let path = if cfg!(target_os = "windows") {
        // dirs::data_dir() on Windows is %APPDATA%\Roaming.
        data_dir
            .join("Blackmagic Design")
            .join("DaVinci Resolve")
            .join("Support")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    } else if cfg!(target_os = "macos") {
        data_dir
            .join("Blackmagic Design")
            .join("DaVinci Resolve")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    } else {
        data_dir
            .join("DaVinciResolve")
            .join("Fusion")
            .join("Scripts")
            .join("Utility")
            .join("Videe.lua")
    };
    Ok(path)
}

/// Copies the bundled script to Resolve's Utility scripts folder. Errors rather
/// than creating a fake directory tree if Resolve's own parent folder doesn't
/// exist yet, since that almost always means Resolve itself isn't installed.
pub fn install_script(app: &AppHandle) -> Result<PathBuf, String> {
    let target = install_target_path()?;
    let scripts_utility_dir = target.parent().ok_or("invalid install target path")?;
    let scripts_dir = scripts_utility_dir
        .parent()
        .ok_or("invalid install target path")?;

    if !scripts_dir.exists() {
        return Err(format!(
            "DaVinci Resolve's scripts folder wasn't found at {}. Is DaVinci Resolve installed?",
            scripts_dir.display()
        ));
    }

    std::fs::create_dir_all(scripts_utility_dir).map_err(|e| e.to_string())?;

    let source = bundled_script_path(app)?;
    std::fs::copy(&source, &target).map_err(|e| e.to_string())?;
    Ok(target)
}

pub fn script_status() -> Option<PathBuf> {
    let target = install_target_path().ok()?;
    target.exists().then_some(target)
}
