//! Recursively finds video files under a library root and fingerprints each one.

use sha2::{Digest, Sha256};
use std::io::Read;
use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;
use walkdir::WalkDir;

const VIDEO_EXTENSIONS: [&str; 4] = ["mp4", "mov", "mxf", "avi"];

/// How many bytes of the file to hash. Hashing whole multi-GB masters on every scan
/// would be far too slow; the first few MB plus the total size is enough to tell
/// distinct clips apart while staying fast on network/synced drives.
const HASH_SAMPLE_BYTES: u64 = 4 * 1024 * 1024;

#[derive(Debug, Clone)]
pub struct ScannedFile {
    pub path: PathBuf,
    pub filename: String,
    pub size: u64,
    pub mtime: i64,
    pub content_hash: String,
}

pub fn is_video_file(path: &Path) -> bool {
    path.extension()
        .and_then(|ext| ext.to_str())
        .map(|ext| VIDEO_EXTENSIONS.contains(&ext.to_lowercase().as_str()))
        .unwrap_or(false)
}

pub fn scan_videos(root: &Path) -> Vec<ScannedFile> {
    WalkDir::new(root)
        .into_iter()
        .filter_map(|entry| entry.ok())
        // Skip the .metadata sidecar directory and any hidden/dot directories.
        .filter(|entry| {
            !entry
                .path()
                .components()
                .any(|c| c.as_os_str().to_string_lossy().starts_with('.'))
        })
        .filter(|entry| entry.file_type().is_file())
        .filter(|entry| is_video_file(entry.path()))
        .filter_map(|entry| fingerprint_file(entry.path()).ok())
        .collect()
}

fn fingerprint_file(path: &Path) -> std::io::Result<ScannedFile> {
    let meta = std::fs::metadata(path)?;
    let size = meta.len();
    let mtime = meta
        .modified()?
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0);
    let filename = path
        .file_name()
        .map(|f| f.to_string_lossy().to_string())
        .unwrap_or_default();
    let content_hash = hash_prefix(path, size)?;

    Ok(ScannedFile {
        path: path.to_path_buf(),
        filename,
        size,
        mtime,
        content_hash,
    })
}

fn hash_prefix(path: &Path, size: u64) -> std::io::Result<String> {
    let file = std::fs::File::open(path)?;
    let mut handle = file.take(HASH_SAMPLE_BYTES);
    let mut hasher = Sha256::new();
    let mut buf = [0u8; 64 * 1024];
    loop {
        let n = handle.read(&mut buf)?;
        if n == 0 {
            break;
        }
        hasher.update(&buf[..n]);
    }
    hasher.update(size.to_le_bytes());
    let digest = hasher.finalize();
    Ok(digest.iter().map(|b| format!("{b:02x}")).collect())
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_dir(name: &str) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("videee-scanner-test-{name}-{}", uuid::Uuid::new_v4()));
        fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn finds_video_files_by_extension_and_ignores_others() {
        let dir = temp_dir("ext");
        fs::write(dir.join("clip.mp4"), b"fake mp4 bytes").unwrap();
        fs::write(dir.join("clip.MOV"), b"fake mov bytes").unwrap();
        fs::write(dir.join("notes.txt"), b"not a video").unwrap();

        let sub = dir.join("subfolder");
        fs::create_dir_all(&sub).unwrap();
        fs::write(sub.join("nested.avi"), b"fake avi bytes").unwrap();

        let hidden_metadata = dir.join(".metadata");
        fs::create_dir_all(&hidden_metadata).unwrap();
        fs::write(hidden_metadata.join("should-be-ignored.mp4"), b"sidecar dir, not a clip").unwrap();

        let found = scan_videos(&dir);
        let mut names: Vec<String> = found.iter().map(|f| f.filename.clone()).collect();
        names.sort();

        assert_eq!(names, vec!["clip.MOV", "clip.mp4", "nested.avi"]);

        fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn same_content_produces_same_hash_different_content_differs() {
        let dir = temp_dir("hash");
        fs::write(dir.join("a.mp4"), b"identical bytes").unwrap();
        fs::write(dir.join("b.mp4"), b"identical bytes").unwrap();
        fs::write(dir.join("c.mp4"), b"different bytes!").unwrap();

        let found = scan_videos(&dir);
        let hash_of = |name: &str| {
            found
                .iter()
                .find(|f| f.filename == name)
                .unwrap()
                .content_hash
                .clone()
        };

        assert_eq!(hash_of("a.mp4"), hash_of("b.mp4"));
        assert_ne!(hash_of("a.mp4"), hash_of("c.mp4"));

        fs::remove_dir_all(&dir).ok();
    }
}
