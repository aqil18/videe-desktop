import { invoke } from "@tauri-apps/api/core";
import type { ClipSummary, SaveClipMetadataInput } from "../types";

export function pickLibraryFolder(): Promise<string | null> {
  return invoke("pick_library_folder");
}

export function scanLibrary(libraryRoot: string): Promise<ClipSummary[]> {
  return invoke("scan_library", { libraryRoot });
}

export function getCachedLibrary(libraryRoot: string): Promise<ClipSummary[]> {
  return invoke("get_cached_library", { libraryRoot });
}

export function saveClipMetadata(input: SaveClipMetadataInput): Promise<ClipSummary> {
  return invoke("save_clip_metadata", { input });
}

// Starts (or restarts) the backend's `.metadata/` file watcher for this folder.
// Updates arrive as `clip-metadata-changed` events.
export function startWatching(libraryRoot: string): Promise<void> {
  return invoke("start_watching", { libraryRoot });
}
