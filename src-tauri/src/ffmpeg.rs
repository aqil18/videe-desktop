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

/// Frame rate used when ffprobe can't tell us one (missing video stream, variable
/// frame rate reported as "0/0", etc). Only affects EDL export timecodes, which need
/// *some* fps to convert seconds to HH:MM:SS:FF -- 25 is a reasonably neutral default.
pub const FALLBACK_FPS: f64 = 25.0;

pub async fn probe_frame_rate(app: &AppHandle, video_path: &Path) -> f64 {
    let Ok(sidecar) = app.shell().sidecar("ffprobe") else {
        return FALLBACK_FPS;
    };

    let Ok(output) = sidecar
        .args([
            "-v",
            "error",
            "-select_streams",
            "v:0",
            "-show_entries",
            "stream=r_frame_rate",
            "-of",
            "default=noprint_wrappers=1:nokey=1",
            &video_path.to_string_lossy(),
        ])
        .output()
        .await
    else {
        return FALLBACK_FPS;
    };

    if !output.status.success() {
        return FALLBACK_FPS;
    }

    parse_frame_rate(&String::from_utf8_lossy(&output.stdout)).unwrap_or(FALLBACK_FPS)
}

/// ffprobe reports frame rate as a rational like "30000/1001" or "25/1".
fn parse_frame_rate(raw: &str) -> Option<f64> {
    let raw = raw.trim();
    let (num, den) = raw.split_once('/')?;
    let num: f64 = num.parse().ok()?;
    let den: f64 = den.parse().ok()?;
    if den == 0.0 || !num.is_finite() || !den.is_finite() {
        return None;
    }
    let fps = num / den;
    (fps > 0.0).then_some(fps)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_common_frame_rate_rationals() {
        assert_eq!(parse_frame_rate("25/1"), Some(25.0));
        assert!((parse_frame_rate("30000/1001").unwrap() - 29.97).abs() < 0.01);
    }

    #[test]
    fn rejects_zero_and_malformed_rates() {
        assert_eq!(parse_frame_rate("0/0"), None);
        assert_eq!(parse_frame_rate("not-a-rate"), None);
        assert_eq!(parse_frame_rate(""), None);
    }
}
