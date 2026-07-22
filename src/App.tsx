import { useEffect, useState } from "react";
import { LibraryGrid } from "./components/LibraryGrid";
import { getCachedLibrary, pickLibraryFolder, scanLibrary } from "./lib/api";
import type { ClipSummary } from "./types";

const LAST_LIBRARY_ROOT_KEY = "videee.lastLibraryRoot";

function App() {
  const [libraryRoot, setLibraryRoot] = useState<string | null>(() =>
    localStorage.getItem(LAST_LIBRARY_ROOT_KEY),
  );
  const [clips, setClips] = useState<ClipSummary[]>([]);
  const [selectedClip, setSelectedClip] = useState<ClipSummary | null>(null);
  const [isScanning, setIsScanning] = useState(false);
  const [error, setError] = useState<string | null>(null);

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
    setSelectedClip(null);
    setClips([]);
    setLibraryRoot(picked);
  }

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

      <div className="flex flex-1 overflow-hidden">
        {libraryRoot ? (
          <LibraryGrid clips={clips} onSelect={setSelectedClip} />
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

        {selectedClip && (
          <aside className="w-72 shrink-0 border-l border-neutral-800 p-4">
            <h2 className="truncate text-sm font-medium text-neutral-100">
              {selectedClip.filename}
            </h2>
            <p className="mt-1 truncate text-xs text-neutral-500">{selectedClip.path}</p>
            <p className="mt-4 text-xs text-neutral-600">
              Tag editing and notes arrive in Phase 2.
            </p>
          </aside>
        )}
      </div>
    </div>
  );
}

export default App;
