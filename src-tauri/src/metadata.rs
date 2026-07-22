//! Reads and writes the `.metadata/<clip-id>.json` sidecar files that live inside the
//! synced project folder. These files, not the SQLite cache, are the source of truth.

use serde::{Deserialize, Serialize};
use std::fs;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Marker {
    pub id: String,
    pub label: String,
    #[serde(rename = "inSeconds")]
    pub in_seconds: f64,
    #[serde(rename = "outSeconds")]
    pub out_seconds: f64,
    #[serde(default)]
    pub notes: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct ClipMetadata {
    pub id: String,
    pub filename: String,
    #[serde(default)]
    pub tags: Vec<String>,
    #[serde(default)]
    pub markers: Vec<Marker>,
    #[serde(default)]
    pub notes: String,
    #[serde(default)]
    pub author: String,
    #[serde(rename = "updatedAt")]
    pub updated_at: String,
    /// SHA-256 of the file's size plus its first few MB, stored so a clip can still be
    /// linked to its sidecar after a rename. See README for the filename-vs-hash decision.
    #[serde(rename = "contentHash", default, skip_serializing_if = "Option::is_none")]
    pub content_hash: Option<String>,
}

impl ClipMetadata {
    pub fn new(id: String, filename: String, content_hash: Option<String>) -> Self {
        Self {
            id,
            filename,
            tags: Vec::new(),
            markers: Vec::new(),
            notes: String::new(),
            author: current_user(),
            updated_at: now_iso8601(),
            content_hash,
        }
    }
}

pub fn current_user() -> String {
    std::env::var("USER")
        .or_else(|_| std::env::var("USERNAME"))
        .unwrap_or_else(|_| "unknown".to_string())
}

pub fn now_iso8601() -> String {
    chrono::Utc::now().to_rfc3339()
}

pub fn metadata_dir(library_root: &Path) -> PathBuf {
    library_root.join(".metadata")
}

pub fn sidecar_path(library_root: &Path, clip_id: &str) -> PathBuf {
    metadata_dir(library_root).join(format!("{clip_id}.json"))
}

pub fn read_metadata(path: &Path) -> Result<ClipMetadata, String> {
    let contents = fs::read_to_string(path).map_err(|e| e.to_string())?;
    serde_json::from_str(&contents).map_err(|e| e.to_string())
}

/// Writes the sidecar atomically (write to a temp file, then rename) so a concurrently
/// running sync client never observes a half-written JSON file.
pub fn write_metadata(library_root: &Path, metadata: &ClipMetadata) -> Result<(), String> {
    let dir = metadata_dir(library_root);
    fs::create_dir_all(&dir).map_err(|e| e.to_string())?;
    let final_path = dir.join(format!("{}.json", metadata.id));
    let tmp_path = dir.join(format!(".{}.json.tmp", metadata.id));
    let json = serde_json::to_string_pretty(metadata).map_err(|e| e.to_string())?;
    fs::write(&tmp_path, json).map_err(|e| e.to_string())?;
    fs::rename(&tmp_path, &final_path).map_err(|e| e.to_string())?;
    Ok(())
}

/// Reads every sidecar file in `.metadata/`, skipping files that fail to parse
/// (e.g. a zero-byte placeholder left by a cloud-sync client mid-download).
pub fn read_all_metadata(library_root: &Path) -> Vec<ClipMetadata> {
    let dir = metadata_dir(library_root);
    let Ok(entries) = fs::read_dir(&dir) else {
        return Vec::new();
    };
    entries
        .flatten()
        .map(|entry| entry.path())
        .filter(|path| path.extension().and_then(|e| e.to_str()) == Some("json"))
        .filter_map(|path| read_metadata(&path).ok())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    #[test]
    fn round_trips_metadata_through_disk() {
        let tmp = std::env::temp_dir().join(format!("videee-metadata-test-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&tmp).unwrap();

        let mut metadata = ClipMetadata::new(
            "clip-1".to_string(),
            "beach_scene.mov".to_string(),
            Some("deadbeef".to_string()),
        );
        metadata.tags = vec!["b-roll".to_string(), "sunset".to_string()];
        metadata.markers.push(Marker {
            id: "marker-1".to_string(),
            label: "Best take".to_string(),
            in_seconds: 1.5,
            out_seconds: 4.25,
            notes: "use this one".to_string(),
        });

        write_metadata(&tmp, &metadata).expect("write should succeed");

        let path = sidecar_path(&tmp, "clip-1");
        assert!(path.exists());

        let loaded = read_metadata(&path).expect("read should succeed");
        assert_eq!(loaded, metadata);

        let all = read_all_metadata(&tmp);
        assert_eq!(all.len(), 1);
        assert_eq!(all[0].id, "clip-1");

        fs::remove_dir_all(&tmp).ok();
    }

    #[test]
    fn skips_unparseable_sidecar_files() {
        let tmp = std::env::temp_dir().join(format!("videee-metadata-test-{}", uuid::Uuid::new_v4()));
        let meta_dir = metadata_dir(&tmp);
        fs::create_dir_all(&meta_dir).unwrap();
        // Simulate a zero-byte placeholder from a cloud-sync client mid-download.
        fs::write(meta_dir.join("placeholder.json"), b"").unwrap();

        let all = read_all_metadata(&tmp);
        assert!(all.is_empty());

        fs::remove_dir_all(&tmp).ok();
    }
}
