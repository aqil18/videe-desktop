import { listen } from "@tauri-apps/api/event";
import { useEffect, useMemo, useState } from "react";
import { ClipDetailPanel } from "./components/ClipDetailPanel";
import { FilterBar } from "./components/FilterBar";
import { LibraryGrid } from "./components/LibraryGrid";
import { getCachedLibrary, pickLibraryFolder, scanLibrary, startWatching } from "./lib/api";
import type { ClipSummary } from "./types";

const LAST_LIBRARY_ROOT_KEY = "videee.lastLibraryRoot";

function App() {
  const [libraryRoot, setLibraryRoot] = useState<string | null>(() =>
    localStorage.getItem(LAST_LIBRARY_ROOT_KEY),
  );
  const [clips, setClips] = useState<ClipSummary[]>([]);
  const [selectedClipId, setSelectedClipId] = useState<string | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [searchQuery, setSearchQuery] = useState("");
  const [activeTags, setActiveTags] = useState<Set<string>>(new Set());

  useEffect(() => {
    if (!libraryRoot) return;

    let cancelled = false;
    getCachedLibrary(libraryRoot)
      .then((cached) => {
        if (!cancelled) setClips(cached);
      })
      .catch(() => {
        // No cache yet for this folder; the scan below will populate it.
      });

    runScan(libraryRoot).then(() => {
      if (cancelled) return;
    });

    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libraryRoot]);

  // Picks up tags/notes a teammate's sync client drops into .metadata/ without
  // requiring a manual rescan.
  useEffect(() => {
    if (!libraryRoot) return;

    let cancelled = false;
    let unlisten: (() => void) | undefined;

    listen<ClipSummary>("clip-metadata-changed", (event) => {
      handleClipSaved(event.payload);
    }).then((fn) => {
      if (cancelled) fn();
      else unlisten = fn;
    });

    startWatching(libraryRoot).catch((e) => console.error("failed to start metadata watcher:", e));

    return () => {
      cancelled = true;
      unlisten?.();
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [libraryRoot]);

  async function runScan(root: string) {
    setIsScanning(true);
    setError(null);
    try {
      const scanned = await scanLibrary(root);
      setClips(scanned);
    } catch (e) {
      setError(String(e));
    } finally {
      setIsScanning(false);
    }
  }

  async function handlePickFolder() {
    const picked = await pickLibraryFolder();
    if (!picked) return;
    localStorage.setItem(LAST_LIBRARY_ROOT_KEY, picked);
    setSelectedClipId(null);
    setClips([]);
    setSearchQuery("");
    setActiveTags(new Set());
    setLibraryRoot(picked);
  }

  function handleClipSaved(updated: ClipSummary) {
    setClips((prev) => prev.map((c) => (c.id === updated.id ? updated : c)));
  }

  function toggleTagFilter(tag: string) {
    setActiveTags((prev) => {
      const next = new Set(prev);
      if (next.has(tag)) next.delete(tag);
      else next.add(tag);
      return next;
    });
  }

  const allTags = useMemo(() => {
    const set = new Set<string>();
    clips.forEach((c) => c.tags.forEach((t) => set.add(t)));
    return Array.from(set).sort();
  }, [clips]);

  const filteredClips = useMemo(() => {
    const q = searchQuery.trim().toLowerCase();
    return clips.filter((c) => {
      const matchesQuery =
        !q || c.filename.toLowerCase().includes(q) || c.tags.some((t) => t.toLowerCase().includes(q));
      const matchesTags = activeTags.size === 0 || Array.from(activeTags).every((t) => c.tags.includes(t));
      return matchesQuery && matchesTags;
    });
  }, [clips, searchQuery, activeTags]);

  const selectedClip = clips.find((c) => c.id === selectedClipId) ?? null;

  return (
    <div className="flex h-screen flex-col bg-neutral-950 text-neutral-100">
      <header className="flex items-center justify-between gap-4 border-b border-neutral-800 px-4 py-3">
        <div className="flex items-center gap-3 overflow-hidden">
          <h1 className="shrink-0 text-sm font-semibold tracking-wide text-neutral-300">
            Videee
          </h1>
          <span className="truncate text-xs text-neutral-500">
            {libraryRoot ?? "No folder selected"}
          </span>
        </div>
        <div className="flex shrink-0 items-center gap-2">
          {isScanning && <span className="text-xs text-neutral-500">Scanning…</span>}
          <button
            onClick={handlePickFolder}
            className="rounded-md bg-neutral-800 px-3 py-1.5 text-sm text-neutral-100 transition hover:bg-neutral-700"
          >
            {libraryRoot ? "Change folder" : "Select folder"}
          </button>
          {libraryRoot && (
            <button
              onClick={() => runScan(libraryRoot)}
              disabled={isScanning}
              className="rounded-md bg-neutral-800 px-3 py-1.5 text-sm text-neutral-100 transition hover:bg-neutral-700 disabled:opacity-50"
            >
              Rescan
            </button>
          )}
        </div>
      </header>

      {error && (
        <div className="border-b border-red-900 bg-red-950/50 px-4 py-2 text-xs text-red-300">
          {error}
        </div>
      )}

      {libraryRoot && clips.length > 0 && (
        <FilterBar
          searchQuery={searchQuery}
          onSearchQueryChange={setSearchQuery}
          allTags={allTags}
          activeTags={activeTags}
          onToggleTag={toggleTagFilter}
        />
      )}

      <div className="flex flex-1 overflow-hidden">
        {libraryRoot ? (
          <LibraryGrid clips={filteredClips} onSelect={(clip) => setSelectedClipId(clip.id)} />
        ) : (
          <div className="flex flex-1 flex-col items-center justify-center gap-3 text-neutral-500">
            <p className="text-sm">Select a folder that's already synced by Drive or Dropbox.</p>
            <button
              onClick={handlePickFolder}
              className="rounded-md bg-neutral-800 px-4 py-2 text-sm text-neutral-100 transition hover:bg-neutral-700"
            >
              Select folder
            </button>
          </div>
        )}

        {selectedClip && libraryRoot && (
          <ClipDetailPanel clip={selectedClip} libraryRoot={libraryRoot} onSaved={handleClipSaved} />
        )}
      </div>
    </div>
  );
}

export default App;
