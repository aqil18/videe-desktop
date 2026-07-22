import { invoke } from "@tauri-apps/api/core";
import type { ClipSummary, Marker, SaveClipMetadataInput } from "../types";

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

// The cache doesn't index markers, so they're read straight from the sidecar.
export function getClipMarkers(libraryRoot: string, id: string): Promise<Marker[]> {
  return invoke("get_clip_markers", { libraryRoot, id });
}

export type ExportFormat = "csv" | "edl";

// Prompts for a save location and writes the export there. Resolves to the chosen
// path, or null if the user cancelled the save dialog.
export function exportClips(libraryRoot: string, clipIds: string[], format: ExportFormat): Promise<string | null> {
  return invoke("export_clips", { input: { libraryRoot, clipIds, format } });
}
