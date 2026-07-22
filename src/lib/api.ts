import { invoke } from "@tauri-apps/api/core";
import type { ClipSummary } from "../types";

export function pickLibraryFolder(): Promise<string | null> {
  return invoke("pick_library_folder");
}

export function scanLibrary(libraryRoot: string): Promise<ClipSummary[]> {
  return invoke("scan_library", { libraryRoot });
}

export function getCachedLibrary(libraryRoot: string): Promise<ClipSummary[]> {
  return invoke("get_cached_library", { libraryRoot });
}
