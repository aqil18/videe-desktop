mod cache;
mod export;
mod ffmpeg;
mod metadata;
mod resolve_bridge;
mod scanner;
mod watcher;

use notify::RecommendedWatcher;
use rusqlite::Connection;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub db: Mutex<Connection>,
    pub watcher: Mutex<Option<RecommendedWatcher>>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSummary {
    pub id: String,
    pub path: String,
    pub filename: String,
    pub size: i64,
    pub content_hash: String,
    pub duration: Option<f64>,
    pub thumbnail_path: Option<String>,
    pub tags: Vec<String>,
    pub notes: String,
}

impl From<cache::ClipRow> for ClipSummary {
    fn from(row: cache::ClipRow) -> Self {
        Self {
            id: row.id,
            path: row.path,
            filename: row.filename,
            size: row.size,
            content_hash: row.content_hash,
            duration: row.duration,
            thumbnail_path: row.thumbnail_path,
            tags: row.tags,
            notes: row.notes,
        }
    }
}

/// Opens the native folder picker. Returns `None` if the user cancels.
#[tauri::command]
async fn pick_library_folder(app: AppHandle) -> Result<Option<String>, String> {
    let picked = app.dialog().file().blocking_pick_folder();
    Ok(picked.map(|p| p.to_string()))
}

/// Scans `library_root` for video files, generates thumbnails for any clip that
/// doesn't have one cached yet, and upserts everything into the local SQLite cache.
/// Returns the full, up-to-date library listing for that folder.
#[tauri::command]
async fn scan_library(
    app: AppHandle,
    state: State<'_, AppState>,
    library_root: String,
) -> Result<Vec<ClipSummary>, String> {
    let root = std::path::Path::new(&library_root);
    if !root.is_dir() {
        return Err(format!("{library_root} is not a directory"));
    }

    let scanned = scanner::scan_videos(root);
    // Sidecars on disk are the source of truth for tags/notes; a fresh cache (new
    // machine, or teammate's tags synced down before we ever scanned) must be
    // reconciled against them rather than starting every clip out untagged.
    let sidecars = metadata::read_all_metadata(root);
    let thumbnail_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("thumbnails");

    let mut still_present = Vec::with_capacity(scanned.len());

    for file in &scanned {
        let path_str = file.path.to_string_lossy().to_string();
        still_present.push(path_str.clone());

        let cached_id = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            cache::find_id_for_path(&conn, &library_root, &path_str)
        };
        let sidecar_match = metadata::find_match(&sidecars, &file.filename, &file.content_hash);
        let id = cached_id
            .or_else(|| sidecar_match.map(|m| m.id.clone()))
            .unwrap_or_else(|| uuid::Uuid::new_v4().to_string());

        let thumbnail_path = thumbnail_dir.join(format!("{id}.jpg"));
        if !thumbnail_path.exists() {
            if let Err(e) = ffmpeg::generate_thumbnail(&app, &file.path, &thumbnail_path).await {
                eprintln!("thumbnail generation failed for {path_str}: {e}");
            }
        }
        let duration = ffmpeg::probe_duration_seconds(&app, &file.path)
            .await
            .map_err(|e| {
                eprintln!("duration probe failed for {path_str}: {e}");
                e
            })
            .ok();

        let row = cache::ClipRow {
            id,
            library_root: library_root.clone(),
            path: path_str,
            filename: file.filename.clone(),
            size: file.size as i64,
            mtime: file.mtime,
            content_hash: file.content_hash.clone(),
            duration,
            thumbnail_path: thumbnail_path
                .exists()
                .then(|| thumbnail_path.to_string_lossy().to_string()),
            tags: sidecar_match.map(|m| m.tags.clone()).unwrap_or_default(),
            notes: sidecar_match.map(|m| m.notes.clone()).unwrap_or_default(),
            author: sidecar_match.map(|m| m.author.clone()).unwrap_or_default(),
            updated_at: sidecar_match.map(|m| m.updated_at.clone()).unwrap_or_default(),
        };

        let conn = state.db.lock().map_err(|e| e.to_string())?;
        cache::upsert_clip(&conn, &row)?;
    }

    {
        let conn = state.db.lock().map_err(|e| e.to_string())?;
        cache::prune_missing(&conn, &library_root, &still_present)?;
    }

    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let rows = cache::list_clips(&conn, &library_root)?;
    Ok(rows.into_iter().map(ClipSummary::from).collect())
}

/// Returns the cached library listing without touching the filesystem or ffmpeg.
/// Used on app load / folder re-selection so the grid renders instantly.
#[tauri::command]
fn get_cached_library(state: State<'_, AppState>, library_root: String) -> Result<Vec<ClipSummary>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    let rows = cache::list_clips(&conn, &library_root)?;
    Ok(rows.into_iter().map(ClipSummary::from).collect())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SaveClipMetadataInput {
    library_root: String,
    id: String,
    filename: String,
    content_hash: String,
    tags: Vec<String>,
    notes: String,
    markers: Vec<metadata::Marker>,
}

/// Writes the full editable record (tags, notes, markers) for a clip in one shot.
/// The frontend's debounced editor always sends all three together so this command
/// never has to guess which fields the caller meant to leave untouched. Takes a
/// plain `&Connection` so it's testable without spinning up a Tauri app.
fn save_clip_metadata_impl(conn: &Connection, input: &SaveClipMetadataInput) -> Result<ClipSummary, String> {
    let root = Path::new(&input.library_root);
    let sidecar_path = metadata::sidecar_path(root, &input.id);

    let mut record = metadata::read_metadata(&sidecar_path).unwrap_or_else(|_| {
        metadata::ClipMetadata::new(input.id.clone(), input.filename.clone(), Some(input.content_hash.clone()))
    });
    record.filename = input.filename.clone();
    record.tags = input.tags.clone();
    record.notes = input.notes.clone();
    record.markers = input.markers.clone();
    record.author = metadata::current_user();
    record.updated_at = metadata::now_iso8601();
    if record.content_hash.is_none() {
        record.content_hash = Some(input.content_hash.clone());
    }

    metadata::write_metadata(root, &record)?;

    cache::update_clip_metadata(conn, &input.id, &record.tags, &record.notes, &record.author, &record.updated_at)?;
    let rows = cache::list_clips(conn, &input.library_root)?;
    rows.into_iter()
        .find(|row| row.id == input.id)
        .map(ClipSummary::from)
        .ok_or_else(|| format!("clip {} not found in cache after save", input.id))
}

/// Called from the (debounced) clip editor: tags, notes, and markers together.
#[tauri::command]
fn save_clip_metadata(state: State<'_, AppState>, input: SaveClipMetadataInput) -> Result<ClipSummary, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    save_clip_metadata_impl(&conn, &input)
}

/// Reads a clip's markers straight from its sidecar (the cache doesn't index them).
/// Returns an empty list if the clip has no sidecar yet rather than erroring, since
/// "no metadata written yet" is the normal state for an untagged clip.
#[tauri::command]
fn get_clip_markers(library_root: String, id: String) -> Result<Vec<metadata::Marker>, String> {
    let root = Path::new(&library_root);
    let sidecar_path = metadata::sidecar_path(root, &id);
    match metadata::read_metadata(&sidecar_path) {
        Ok(record) => Ok(record.markers),
        Err(_) => Ok(Vec::new()),
    }
}

/// (Re)starts the `.metadata/` watcher for `library_root`. Replacing the previous
/// watcher (if any) drops it, which stops its background thread.
#[tauri::command]
fn start_watching(app: AppHandle, state: State<'_, AppState>, library_root: String) -> Result<(), String> {
    let new_watcher = watcher::start(app, PathBuf::from(library_root)).map_err(|e| e.to_string())?;
    let mut guard = state.watcher.lock().map_err(|e| e.to_string())?;
    *guard = Some(new_watcher);
    Ok(())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportClipsInput {
    library_root: String,
    clip_ids: Vec<String>,
    format: String,
}

/// Builds one `ExportRow` per marker for the given clips, or one row covering the
/// whole clip if it has no markers, so every selected clip is represented even if
/// nobody's marked it up yet. Shared by the file-export command and the "send to
/// DaVinci" bridge so the marker-gathering/fps-probing logic isn't duplicated.
async fn build_export_rows(app: &AppHandle, root: &Path, selected: &[cache::ClipRow]) -> Vec<export::ExportRow> {
    let mut rows = Vec::new();
    for clip in selected {
        let sidecar_path = metadata::sidecar_path(root, &clip.id);
        let markers = metadata::read_metadata(&sidecar_path).map(|m| m.markers).unwrap_or_default();
        let fps = ffmpeg::probe_frame_rate(app, Path::new(&clip.path)).await;

        if markers.is_empty() {
            rows.push(export::ExportRow {
                filename: clip.filename.clone(),
                tags: clip.tags.clone(),
                marker_label: String::new(),
                in_seconds: 0.0,
                out_seconds: clip.duration.unwrap_or(0.0),
                fps,
            });
        } else {
            for marker in markers {
                rows.push(export::ExportRow {
                    filename: clip.filename.clone(),
                    tags: clip.tags.clone(),
                    marker_label: marker.label,
                    in_seconds: marker.in_seconds,
                    out_seconds: marker.out_seconds,
                    fps,
                });
            }
        }
    }
    rows
}

fn selected_clip_rows(state: &State<'_, AppState>, library_root: &str, clip_ids: &[String]) -> Result<Vec<cache::ClipRow>, String> {
    let conn = state.db.lock().map_err(|e| e.to_string())?;
    Ok(cache::list_clips(&conn, library_root)?
        .into_iter()
        .filter(|row| clip_ids.contains(&row.id))
        .collect())
}

/// Exports the selected clips as CSV or EDL. Prompts for a save location and
/// returns the chosen path, or `None` if the user cancelled the dialog.
#[tauri::command]
async fn export_clips(app: AppHandle, state: State<'_, AppState>, input: ExportClipsInput) -> Result<Option<String>, String> {
    let root = Path::new(&input.library_root);
    let selected = selected_clip_rows(&state, &input.library_root, &input.clip_ids)?;
    let rows = build_export_rows(&app, root, &selected).await;

    let (content, default_name, filter_label, extension) = if input.format == "edl" {
        (export::build_edl("Videee Export", &rows), "videee-export.edl", "EDL", "edl")
    } else {
        (export::build_csv(&rows), "videee-export.csv", "CSV", "csv")
    };

    let picked = app
        .dialog()
        .file()
        .set_file_name(default_name)
        .add_filter(filter_label, &[extension])
        .blocking_save_file();

    let Some(picked) = picked else {
        return Ok(None);
    };
    let save_path = picked.as_path().ok_or("save dialog returned an invalid path")?;
    std::fs::write(save_path, content).map_err(|e| e.to_string())?;
    Ok(Some(save_path.to_string_lossy().to_string()))
}

#[tauri::command]
async fn resolve_ping() -> bool {
    resolve_bridge::ping().await
}

#[tauri::command]
fn resolve_script_status() -> Option<String> {
    resolve_bridge::script_status().map(|p| p.to_string_lossy().to_string())
}

#[tauri::command]
fn resolve_install_script(app: AppHandle) -> Result<String, String> {
    resolve_bridge::install_script(&app).map(|p| p.to_string_lossy().to_string())
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
struct SendClipsToResolveInput {
    library_root: String,
    clip_ids: Vec<String>,
}

/// Builds an EDL for the selection, same as `export_clips`, but instead of a save
/// dialog it hands the file to the `Videe.lua` bridge running inside DaVinci
/// Resolve via `ImportTimelineFromFile`, using the synced library folder as the
/// clip-relinking source.
#[tauri::command]
async fn send_clips_to_resolve(app: AppHandle, state: State<'_, AppState>, input: SendClipsToResolveInput) -> Result<(), String> {
    let root = Path::new(&input.library_root);
    let selected = selected_clip_rows(&state, &input.library_root, &input.clip_ids)?;
    let rows = build_export_rows(&app, root, &selected).await;
    let content = export::build_edl("Videee Export", &rows);

    let temp_path = std::env::temp_dir().join(format!("videee-resolve-{}.edl", uuid::Uuid::new_v4()));
    std::fs::write(&temp_path, content).map_err(|e| e.to_string())?;

    let timeline_name = format!("Videee Export {}", metadata::now_iso8601());
    let result = resolve_bridge::import_edl(&temp_path, root, &timeline_name)
        .await
        .map_err(|e| e.to_string());

    std::fs::remove_file(&temp_path).ok();
    result
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .plugin(tauri_plugin_dialog::init())
        .plugin(tauri_plugin_shell::init())
        .setup(|app| {
            let db_path = app
                .path()
                .app_local_data_dir()
                .expect("app_local_data_dir should be available")
                .join("cache.sqlite");
            let conn = cache::open(&db_path).expect("failed to open local sqlite cache");
            app.manage(AppState {
                db: Mutex::new(conn),
                watcher: Mutex::new(None),
            });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_library_folder,
            scan_library,
            get_cached_library,
            save_clip_metadata,
            get_clip_markers,
            start_watching,
            export_clips,
            resolve_ping,
            resolve_script_status,
            resolve_install_script,
            send_clips_to_resolve,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_library() -> std::path::PathBuf {
        std::env::temp_dir().join(format!("videee-lib-test-{}", uuid::Uuid::new_v4()))
    }

    #[test]
    fn save_clip_metadata_writes_sidecar_and_updates_cache() {
        let root = temp_library();
        std::fs::create_dir_all(&root).unwrap();
        let conn = Connection::open_in_memory().unwrap();
        cache::init_schema(&conn).unwrap();

        // The clip must already be in the cache from a prior scan.
        cache::upsert_clip(
            &conn,
            &cache::ClipRow {
                id: "clip-1".to_string(),
                library_root: root.to_string_lossy().to_string(),
                path: root.join("a.mp4").to_string_lossy().to_string(),
                filename: "a.mp4".to_string(),
                size: 100,
                mtime: 0,
                content_hash: "hash-a".to_string(),
                duration: Some(5.0),
                thumbnail_path: None,
                tags: vec![],
                notes: String::new(),
                author: String::new(),
                updated_at: String::new(),
            },
        )
        .unwrap();

        let input = SaveClipMetadataInput {
            library_root: root.to_string_lossy().to_string(),
            id: "clip-1".to_string(),
            filename: "a.mp4".to_string(),
            content_hash: "hash-a".to_string(),
            tags: vec!["b-roll".to_string()],
            notes: "nice shot".to_string(),
            markers: vec![metadata::Marker {
                id: "marker-1".to_string(),
                label: "Best take".to_string(),
                in_seconds: 1.0,
                out_seconds: 2.5,
                notes: String::new(),
            }],
        };

        let updated = save_clip_metadata_impl(&conn, &input).expect("save should succeed");
        assert_eq!(updated.tags, vec!["b-roll".to_string()]);
        assert_eq!(updated.notes, "nice shot");
        // Filesystem-derived fields from the original scan must survive untouched.
        assert_eq!(updated.duration, Some(5.0));

        let sidecar_path = metadata::sidecar_path(&root, "clip-1");
        assert!(sidecar_path.exists());
        let on_disk = metadata::read_metadata(&sidecar_path).unwrap();
        assert_eq!(on_disk.tags, vec!["b-roll".to_string()]);
        assert_eq!(on_disk.content_hash, Some("hash-a".to_string()));
        assert_eq!(on_disk.markers.len(), 1);
        assert_eq!(on_disk.markers[0].label, "Best take");

        std::fs::remove_dir_all(&root).ok();
    }

    #[test]
    fn get_clip_markers_returns_empty_for_untagged_clip() {
        let root = temp_library();
        std::fs::create_dir_all(&root).unwrap();
        let markers = get_clip_markers(root.to_string_lossy().to_string(), "no-such-clip".to_string())
            .expect("should not error for a clip with no sidecar yet");
        assert!(markers.is_empty());
        std::fs::remove_dir_all(&root).ok();
    }
}
