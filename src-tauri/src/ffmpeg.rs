//! Wraps the bundled ffmpeg/ffprobe sidecar binaries for thumbnail generation and
//! duration probing. See scripts/fetch-ffmpeg.sh for how those binaries get onto disk.

use std::path::Path;
use tauri::AppHandle;
use tauri_plugin_shell::process::CommandEvent;
use tauri_plugin_shell::ShellExt;

pub async fn generate_thumbnail(
    app: &AppHandle,
    video_path: &Path,
    out_path: &Path,
) -> Result<(), String> {
    if let Some(parent) = out_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }

    let sidecar = app
        .shell()
        .sidecar("ffmpeg")
        .map_err(|e| format!("ffmpeg sidecar not available: {e}"))?;

    let (mut events, _child) = sidecar
        .args([
            "-y",
            "-ss",
            "1",
            "-i",
            &video_path.to_string_lossy(),
            "-frames:v",
            "1",
            "-update",
            "1",
            "-vf",
            "scale=320:-2",
            &out_path.to_string_lossy(),
        ])
        .spawn()
        .map_err(|e| e.to_string())?;

    let mut stderr = String::new();
    while let Some(event) = events.recv().await {
        match event {
            CommandEvent::Stderr(bytes) => stderr.push_str(&String::from_utf8_lossy(&bytes)),
            CommandEvent::Error(err) => return Err(err),
            CommandEvent::Terminated(payload) => {
                if payload.code != Some(0) {
                    return Err(format!(
                        "ffmpeg exited with code {:?}: {}",
                        payload.code, stderr
                    ));
                }
            }
            _ => {}
        }
    }

    if !out_path.exists() {
        return Err(format!("ffmpeg did not produce a thumbnail: {stderr}"));
    }
    Ok(())
}

pub async fn probe_duration_seconds(app: &AppHandle, video_path: &Path) -> Result<f64, String> {
    let sidecar = app
        .shell()
        .sidecar("ffprobe")
        .map_err(|e| format!("ffprobe sidecar not available: {e}"))?;

    let output = sidecar
        .args([
            "-v",
            "error",
            "-show_entries",
            "format=duration",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &video_path.to_string_lossy(),
        ])
        .output()
        .await
        .map_err(|e| e.to_string())?;

    if !output.status.success() {
        return Err(String::from_utf8_lossy(&output.stderr).to_string());
    }

    String::from_utf8_lossy(&output.stdout)
        .trim()
        .parse::<f64>()
        .map_err(|e| format!("could not parse ffprobe duration: {e}"))
}
