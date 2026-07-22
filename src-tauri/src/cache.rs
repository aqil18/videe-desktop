//! Local SQLite cache. This lives in the app's local data dir (never inside the synced
//! library folder) and is purely a rebuildable index for fast search/filter — if it's
//! deleted, the next scan plus a metadata reload reconstructs it from the JSON sidecars.

use rusqlite::{params, Connection};
use serde::Serialize;
use std::path::Path;

pub fn open(db_path: &Path) -> Result<Connection, String> {
    if let Some(parent) = db_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| e.to_string())?;
    }
    let conn = Connection::open(db_path).map_err(|e| e.to_string())?;
    init_schema(&conn)?;
    Ok(conn)
}

pub(crate) fn init_schema(conn: &Connection) -> Result<(), String> {
    conn.execute_batch(
        "CREATE TABLE IF NOT EXISTS clips (
            id TEXT PRIMARY KEY,
            library_root TEXT NOT NULL,
            path TEXT NOT NULL,
            filename TEXT NOT NULL,
            size INTEGER NOT NULL,
            mtime INTEGER NOT NULL,
            content_hash TEXT NOT NULL,
            duration REAL,
            thumbnail_path TEXT,
            tags TEXT NOT NULL DEFAULT '[]',
            notes TEXT NOT NULL DEFAULT '',
            author TEXT NOT NULL DEFAULT '',
            updated_at TEXT NOT NULL DEFAULT '',
            UNIQUE(library_root, path)
        );
        CREATE INDEX IF NOT EXISTS idx_clips_library_root ON clips(library_root);",
    )
    .map_err(|e| e.to_string())
}

#[derive(Debug, Clone, Serialize)]
pub struct ClipRow {
    pub id: String,
    pub library_root: String,
    pub path: String,
    pub filename: String,
    pub size: i64,
    pub mtime: i64,
    pub content_hash: String,
    pub duration: Option<f64>,
    pub thumbnail_path: Option<String>,
    pub tags: Vec<String>,
    pub notes: String,
    pub author: String,
    pub updated_at: String,
}

/// Looks up the clip id already assigned to this path (from a previous scan) so
/// re-scanning the same library doesn't mint a new id for a file we've already seen.
pub fn find_id_for_path(conn: &Connection, library_root: &str, path: &str) -> Option<String> {
    conn.query_row(
        "SELECT id FROM clips WHERE library_root = ?1 AND path = ?2",
        params![library_root, path],
        |row| row.get(0),
    )
    .ok()
}

pub fn upsert_clip(conn: &Connection, row: &ClipRow) -> Result<(), String> {
    let tags_json = serde_json::to_string(&row.tags).map_err(|e| e.to_string())?;
    conn.execute(
        "INSERT INTO clips (id, library_root, path, filename, size, mtime, content_hash, duration, thumbnail_path, tags, notes, author, updated_at)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13)
         ON CONFLICT(library_root, path) DO UPDATE SET
            id = excluded.id,
            filename = excluded.filename,
            size = excluded.size,
            mtime = excluded.mtime,
            content_hash = excluded.content_hash,
            duration = excluded.duration,
            thumbnail_path = excluded.thumbnail_path,
            tags = excluded.tags,
            notes = excluded.notes,
            author = excluded.author,
            updated_at = excluded.updated_at",
        params![
            row.id,
            row.library_root,
            row.path,
            row.filename,
            row.size,
            row.mtime,
            row.content_hash,
            row.duration,
            row.thumbnail_path,
            tags_json,
            row.notes,
            row.author,
            row.updated_at,
        ],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

/// Updates just the metadata-derived columns for a clip (used by the metadata/watcher
/// modules) without touching the filesystem-derived columns from the last scan.
pub fn update_clip_metadata(
    conn: &Connection,
    id: &str,
    tags: &[String],
    notes: &str,
    author: &str,
    updated_at: &str,
) -> Result<(), String> {
    let tags_json = serde_json::to_string(tags).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE clips SET tags = ?1, notes = ?2, author = ?3, updated_at = ?4 WHERE id = ?5",
        params![tags_json, notes, author, updated_at, id],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}

pub fn list_clips(conn: &Connection, library_root: &str) -> Result<Vec<ClipRow>, String> {
    let mut stmt = conn
        .prepare(
            "SELECT id, library_root, path, filename, size, mtime, content_hash, duration, thumbnail_path, tags, notes, author, updated_at
             FROM clips WHERE library_root = ?1 ORDER BY filename COLLATE NOCASE",
        )
        .map_err(|e| e.to_string())?;

    let rows = stmt
        .query_map(params![library_root], |row| {
            let tags_json: String = row.get(9)?;
            let tags: Vec<String> = serde_json::from_str(&tags_json).unwrap_or_default();
            Ok(ClipRow {
                id: row.get(0)?,
                library_root: row.get(1)?,
                path: row.get(2)?,
                filename: row.get(3)?,
                size: row.get(4)?,
                mtime: row.get(5)?,
                content_hash: row.get(6)?,
                duration: row.get(7)?,
                thumbnail_path: row.get(8)?,
                tags,
                notes: row.get(10)?,
                author: row.get(11)?,
                updated_at: row.get(12)?,
            })
        })
        .map_err(|e| e.to_string())?;

    rows.collect::<Result<Vec<_>, _>>().map_err(|e| e.to_string())
}

/// Removes cache rows for files that disappeared from disk since the last scan
/// (deleted or moved out of the library folder).
pub fn prune_missing(conn: &Connection, library_root: &str, still_present_paths: &[String]) -> Result<(), String> {
    let mut stmt = conn
        .prepare("SELECT path FROM clips WHERE library_root = ?1")
        .map_err(|e| e.to_string())?;
    let known: Vec<String> = stmt
        .query_map(params![library_root], |row| row.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string())?;

    for path in known {
        if !still_present_paths.contains(&path) {
            conn.execute(
                "DELETE FROM clips WHERE library_root = ?1 AND path = ?2",
                params![library_root, path],
            )
            .map_err(|e| e.to_string())?;
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_row(id: &str, path: &str) -> ClipRow {
        ClipRow {
            id: id.to_string(),
            library_root: "/library".to_string(),
            path: path.to_string(),
            filename: path.rsplit('/').next().unwrap().to_string(),
            size: 1024,
            mtime: 1_700_000_000,
            content_hash: "deadbeef".to_string(),
            duration: Some(12.5),
            thumbnail_path: Some(format!("/thumbs/{id}.jpg")),
            tags: vec![],
            notes: String::new(),
            author: String::new(),
            updated_at: String::new(),
        }
    }

    #[test]
    fn upsert_then_list_round_trips_and_reuses_id_on_rescan() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        let row = sample_row("clip-1", "/library/a.mp4");
        upsert_clip(&conn, &row).unwrap();

        let found = list_clips(&conn, "/library").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].filename, "a.mp4");

        // Re-scanning the same path should resolve back to the same id, not mint a new one.
        let reused_id = find_id_for_path(&conn, "/library", "/library/a.mp4");
        assert_eq!(reused_id, Some("clip-1".to_string()));

        // Upserting again with the same (library_root, path) updates in place.
        let mut updated = row.clone();
        updated.size = 2048;
        upsert_clip(&conn, &updated).unwrap();
        let found = list_clips(&conn, "/library").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].size, 2048);
    }

    #[test]
    fn prune_missing_removes_deleted_files_only() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();

        upsert_clip(&conn, &sample_row("clip-1", "/library/a.mp4")).unwrap();
        upsert_clip(&conn, &sample_row("clip-2", "/library/b.mp4")).unwrap();

        prune_missing(&conn, "/library", &["/library/a.mp4".to_string()]).unwrap();

        let found = list_clips(&conn, "/library").unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].id, "clip-1");
    }

    #[test]
    fn update_clip_metadata_only_touches_metadata_columns() {
        let conn = Connection::open_in_memory().unwrap();
        init_schema(&conn).unwrap();
        upsert_clip(&conn, &sample_row("clip-1", "/library/a.mp4")).unwrap();

        update_clip_metadata(
            &conn,
            "clip-1",
            &["b-roll".to_string()],
            "great take",
            "alex",
            "2026-07-21T00:00:00Z",
        )
        .unwrap();

        let found = list_clips(&conn, "/library").unwrap();
        assert_eq!(found[0].tags, vec!["b-roll".to_string()]);
        assert_eq!(found[0].notes, "great take");
        assert_eq!(found[0].size, 1024); // untouched
    }
}
