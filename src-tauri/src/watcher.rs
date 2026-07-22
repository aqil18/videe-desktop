//! Watches a library's `.metadata/` folder so tags/notes a teammate wrote on another
//! machine show up here as soon as their sync client drops the updated JSON file,
//! without the user having to manually rescan.

use crate::{cache, metadata, AppState};
use notify::{Event, EventKind, RecommendedWatcher, RecursiveMode, Watcher};
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};
use std::time::Duration;
use tauri::{AppHandle, Emitter, Manager};

/// How many times to retry reading a sidecar before giving up. Cloud-sync clients
/// (Drive's "files on demand" in particular) can briefly leave a zero-byte or
/// partially-written placeholder in place of the real file while it downloads.
const MAX_READ_ATTEMPTS: u32 = 6;
const INITIAL_RETRY_DELAY_MS: u64 = 150;
const SETTLE_DELAY_MS: u64 = 200;

/// Per-path generation counters, used to let a newer event cancel an in-flight
/// retry loop from an older event on the same path (a quick save often fires
/// several filesystem events for the same logical write).
type Generations = Arc<Mutex<HashMap<PathBuf, u64>>>;

pub fn start(app: AppHandle, library_root: PathBuf) -> notify::Result<RecommendedWatcher> {
    let meta_dir = metadata::metadata_dir(&library_root);
    std::fs::create_dir_all(&meta_dir).ok();

    let generations: Generations = Arc::new(Mutex::new(HashMap::new()));

    let mut watcher = notify::recommended_watcher(move |res: notify::Result<Event>| {
        let Ok(event) = res else { return };
        if !matches!(
            event.kind,
            EventKind::Create(_) | EventKind::Modify(_) | EventKind::Remove(_)
        ) {
            return;
        }

        for path in &event.paths {
            if path.extension().and_then(|e| e.to_str()) != Some("json") {
                continue;
            }

            let generation = {
                let mut map = generations.lock().unwrap();
                let entry = map.entry(path.clone()).or_insert(0);
                *entry += 1;
                *entry
            };

            let app = app.clone();
            let library_root = library_root.clone();
            let path = path.clone();
            let generations = generations.clone();

            tauri::async_runtime::spawn(async move {
                handle_sidecar_change(app, library_root, path, generation, generations).await;
            });
        }
    })?;

    watcher.watch(&meta_dir, RecursiveMode::NonRecursive)?;
    Ok(watcher)
}

/// What became of a sidecar file after we finished waiting for it to settle.
#[derive(Debug, PartialEq)]
enum ReadOutcome {
    Ready(metadata::ClipMetadata),
    Deleted,
    Superseded,
    GaveUp,
}

async fn handle_sidecar_change(
    app: AppHandle,
    library_root: PathBuf,
    path: PathBuf,
    generation: u64,
    generations: Generations,
) {
    tokio::time::sleep(Duration::from_millis(SETTLE_DELAY_MS)).await;

    let Some(id) = path.file_stem().and_then(|s| s.to_str()) else {
        return;
    };
    let id = id.to_string();

    let is_superseded = |p: &Path| superseded(p, generation, &generations);
    match wait_for_readable_sidecar(&path, is_superseded, MAX_READ_ATTEMPTS, INITIAL_RETRY_DELAY_MS).await {
        ReadOutcome::Ready(record) => apply_or_clear(&app, &library_root, &id, Some(record)),
        // Deleted (locally or by a teammate's sync client): the sidecar no longer
        // claims any tags/notes for this clip, so the cache shouldn't either.
        ReadOutcome::Deleted => apply_or_clear(&app, &library_root, &id, None),
        ReadOutcome::Superseded => {}
        ReadOutcome::GaveUp => {
            eprintln!("videee: gave up waiting for {path:?} to become readable after {MAX_READ_ATTEMPTS} attempts");
        }
    }
}

/// Polls `path` with exponential backoff until it parses as valid `ClipMetadata`,
/// is confirmed deleted, or `max_attempts` is exhausted. A zero-byte or
/// unparseable-but-nonempty file (a cloud-sync "files on demand" placeholder, or a
/// half-written JSON body) is treated as "not ready yet" rather than valid content.
async fn wait_for_readable_sidecar(
    path: &Path,
    is_superseded: impl Fn(&Path) -> bool,
    max_attempts: u32,
    initial_delay_ms: u64,
) -> ReadOutcome {
    let mut delay = initial_delay_ms;
    for attempt in 0..max_attempts {
        if is_superseded(path) {
            return ReadOutcome::Superseded;
        }
        if !path.exists() {
            return ReadOutcome::Deleted;
        }

        let looks_ready = std::fs::metadata(path).map(|m| m.len() > 0).unwrap_or(false);
        if looks_ready {
            if let Ok(record) = metadata::read_metadata(path) {
                return ReadOutcome::Ready(record);
            }
        }

        if attempt + 1 < max_attempts {
            tokio::time::sleep(Duration::from_millis(delay)).await;
            delay = (delay * 2).min(5_000);
        }
    }
    ReadOutcome::GaveUp
}

fn superseded(path: &Path, generation: u64, generations: &Generations) -> bool {
    let map = generations.lock().unwrap();
    map.get(path).copied() != Some(generation)
}

/// Applies a successfully parsed sidecar (or clears the clip's metadata if `record`
/// is `None`, e.g. the sidecar was deleted) and, if the clip is already known to the
/// cache, emits `clip-metadata-changed` so the frontend can patch just that card.
fn apply_or_clear(app: &AppHandle, library_root: &Path, id: &str, record: Option<metadata::ClipMetadata>) {
    let Some(state) = app.try_state::<AppState>() else {
        return;
    };
    let Ok(conn) = state.db.lock() else {
        return;
    };

    let (tags, notes, author, updated_at) = match &record {
        Some(r) => (r.tags.clone(), r.notes.clone(), r.author.clone(), r.updated_at.clone()),
        None => (Vec::new(), String::new(), String::new(), String::new()),
    };

    if cache::update_clip_metadata(&conn, id, &tags, &notes, &author, &updated_at).is_err() {
        return;
    }

    let library_root_str = library_root.to_string_lossy().to_string();
    let Ok(rows) = cache::list_clips(&conn, &library_root_str) else {
        return;
    };
    if let Some(row) = rows.into_iter().find(|r| r.id == id) {
        let _ = app.emit("clip-metadata-changed", crate::ClipSummary::from(row));
    }
    // If the clip isn't in the cache yet (a brand new video+sidecar pair synced down
    // together), there's nothing to patch — it'll pick up the tags on the next scan.
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_sidecar_path() -> PathBuf {
        std::env::temp_dir().join(format!("videee-watcher-test-{}.json", uuid::Uuid::new_v4()))
    }

    #[tokio::test]
    async fn treats_zero_byte_placeholder_as_not_ready_then_succeeds_once_filled() {
        let path = temp_sidecar_path();
        // Simulate a cloud-sync "files on demand" placeholder: the file exists
        // (so a Create event fires) but has no content yet.
        fs::write(&path, b"").unwrap();

        // Fill it in shortly after the retry loop starts, like a sync client
        // finishing the download mid-poll.
        let fill_path = path.clone();
        tokio::spawn(async move {
            tokio::time::sleep(Duration::from_millis(30)).await;
            let record = metadata::ClipMetadata::new(
                "clip-1".to_string(),
                "a.mp4".to_string(),
                Some("hash".to_string()),
            );
            fs::write(&fill_path, serde_json::to_string(&record).unwrap()).unwrap();
        });

        let outcome = wait_for_readable_sidecar(&path, |_| false, 8, 10).await;
        assert!(matches!(outcome, ReadOutcome::Ready(m) if m.id == "clip-1"));

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn gives_up_after_max_attempts_on_a_permanently_empty_file() {
        let path = temp_sidecar_path();
        fs::write(&path, b"").unwrap();

        let outcome = wait_for_readable_sidecar(&path, |_| false, 3, 5).await;
        assert_eq!(outcome, ReadOutcome::GaveUp);

        fs::remove_file(&path).ok();
    }

    #[tokio::test]
    async fn reports_deleted_when_file_never_existed() {
        let path = temp_sidecar_path();
        let outcome = wait_for_readable_sidecar(&path, |_| false, 3, 5).await;
        assert_eq!(outcome, ReadOutcome::Deleted);
    }

    #[tokio::test]
    async fn stops_immediately_once_superseded() {
        let path = temp_sidecar_path();
        fs::write(&path, b"").unwrap();

        let outcome = wait_for_readable_sidecar(&path, |_| true, 8, 10).await;
        assert_eq!(outcome, ReadOutcome::Superseded);

        fs::remove_file(&path).ok();
    }
}
