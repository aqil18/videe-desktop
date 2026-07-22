mod cache;
mod ffmpeg;
mod metadata;
mod scanner;

use rusqlite::Connection;
use serde::Serialize;
use std::sync::Mutex;
use tauri::{AppHandle, Manager, State};
use tauri_plugin_dialog::DialogExt;

pub struct AppState {
    pub db: Mutex<Connection>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ClipSummary {
    pub id: String,
    pub path: String,
    pub filename: String,
    pub size: i64,
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
    let thumbnail_dir = app
        .path()
        .app_local_data_dir()
        .map_err(|e| e.to_string())?
        .join("thumbnails");

    let mut still_present = Vec::with_capacity(scanned.len());

    for file in &scanned {
        let path_str = file.path.to_string_lossy().to_string();
        still_present.push(path_str.clone());

        let id = {
            let conn = state.db.lock().map_err(|e| e.to_string())?;
            cache::find_id_for_path(&conn, &library_root, &path_str)
                .unwrap_or_else(|| uuid::Uuid::new_v4().to_string())
        };

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
            tags: Vec::new(),
            notes: String::new(),
            author: String::new(),
            updated_at: String::new(),
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
            app.manage(AppState { db: Mutex::new(conn) });
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            pick_library_folder,
            scan_library,
            get_cached_library,
        ])
        .run(tauri::generate_context!())
        .expect("error while running tauri application");
}
