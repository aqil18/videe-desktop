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

// Library-wide tag list (from tag templates), stored in .metadata/project.json.
export function getProjectTags(libraryRoot: string): Promise<string[]> {
  return invoke("get_project_tags", { libraryRoot });
}

export function saveProjectTags(libraryRoot: string, tags: string[]): Promise<void> {
  return invoke("save_project_tags", { libraryRoot, tags });
}

export type ExportFormat = "csv" | "edl";

// Prompts for a save location and writes the export there. Resolves to the chosen
// path, or null if the user cancelled the save dialog.
export function exportClips(libraryRoot: string, clipIds: string[], format: ExportFormat): Promise<string | null> {
  return invoke("export_clips", { input: { libraryRoot, clipIds, format } });
}

// Pure status check for the DaVinci Resolve bridge -- true only if the
// Videe.lua script is actually running inside Resolve and reachable.
export function resolvePing(): Promise<boolean> {
  return invoke("resolve_ping");
}

// Path the bridge script is installed at, if it's been installed at all.
export function resolveScriptStatus(): Promise<string | null> {
  return invoke("resolve_script_status");
}

// Copies the bundled Videe.lua into Resolve's Utility scripts folder.
// Resolves to the installed path, or rejects with a user-facing message if
// Resolve's scripts folder wasn't found (e.g. Resolve isn't installed).
export function resolveInstallScript(): Promise<string> {
  return invoke("resolve_install_script");
}

// Hands the selection to the running Videe.lua bridge, which imports the files
// into the open Resolve project, builds a timeline, and adds timestamp markers.
export function sendClipsToResolve(libraryRoot: string, clipIds: string[]): Promise<void> {
  return invoke("send_clips_to_resolve", { input: { libraryRoot, clipIds } });
}

// Pure status check for the Premiere Pro bridge -- true only if the Videe CEP
// panel is open inside Premiere and reachable.
export function premierePing(): Promise<boolean> {
  return invoke("premiere_ping");
}

// Path the CEP extension is installed at, if it's been installed at all.
export function premiereExtensionStatus(): Promise<string | null> {
  return invoke("premiere_extension_status");
}

// Copies the bundled CEP extension into Adobe's extensions folder and enables
// PlayerDebugMode so Premiere loads it. Resolves to the installed path.
export function premiereInstallExtension(): Promise<string> {
  return invoke("premiere_install_extension");
}

// Same as sendClipsToResolve, but to the Videe panel in Premiere Pro: imports
// the files, builds a sequence, and adds timestamp markers.
export function sendClipsToPremiere(libraryRoot: string, clipIds: string[]): Promise<void> {
  return invoke("send_clips_to_premiere", { input: { libraryRoot, clipIds } });
}
