import { convertFileSrc } from "@tauri-apps/api/core";
import { useEffect, useRef, useState } from "react";
import { getClipMarkers, saveClipMetadata } from "../lib/api";
import type { ClipSummary, Marker } from "../types";
import { MarkerList } from "./MarkerList";
import { TagInput } from "./TagInput";
import { VideoPlayer, type VideoPlayerHandle } from "./VideoPlayer";

const SAVE_DEBOUNCE_MS = 1500;

interface ClipDetailPanelProps {
  clip: ClipSummary;
  libraryRoot: string;
  onSaved: (clip: ClipSummary) => void;
}

type SaveStatus = "idle" | "pending" | "saving" | "saved" | "error";

export function ClipDetailPanel({ clip, libraryRoot, onSaved }: ClipDetailPanelProps) {
  const [tags, setTags] = useState(clip.tags);
  const [notes, setNotes] = useState(clip.notes);
  const [markers, setMarkers] = useState<Marker[]>([]);
  const [status, setStatus] = useState<SaveStatus>("idle");
  const skipNextSave = useRef(true);
  const currentTimeRef = useRef(0);
  const playerRef = useRef<VideoPlayerHandle>(null);

  // Switching clips: sync local state from the newly selected clip without
  // triggering a save of the clip we just navigated away from. Markers aren't
  // part of ClipSummary (the cache doesn't index them), so they're fetched
  // separately straight from the sidecar.
  useEffect(() => {
    setTags(clip.tags);
    setNotes(clip.notes);
    setMarkers([]);
    setStatus("idle");
    skipNextSave.current = true;

    let cancelled = false;
    getClipMarkers(libraryRoot, clip.id).then((fetched) => {
      if (cancelled) return;
      skipNextSave.current = true;
      setMarkers(fetched);
    });
    return () => {
      cancelled = true;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [clip.id]);

  useEffect(() => {
    if (skipNextSave.current) {
      skipNextSave.current = false;
      return;
    }
    setStatus("pending");
    const timer = setTimeout(async () => {
      setStatus("saving");
      try {
        const updated = await saveClipMetadata({
          libraryRoot,
          id: clip.id,
          filename: clip.filename,
          contentHash: clip.contentHash,
          tags,
          notes,
          markers,
        });
        onSaved(updated);
        setStatus("saved");
      } catch (e) {
        console.error(e);
        setStatus("error");
      }
    }, SAVE_DEBOUNCE_MS);
    return () => clearTimeout(timer);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [tags, notes, markers]);

  // I marks in, O marks out, on whichever clip is currently loaded in the player.
  // Uses functional state updates so the handler never closes over a stale
  // `markers` array and doesn't need to be re-subscribed on every marker edit.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;

      const key = e.key.toLowerCase();
      if (key === "i") {
        e.preventDefault();
        const t = currentTimeRef.current;
        setMarkers((prev) => [
          ...prev,
          { id: crypto.randomUUID(), label: `Marker ${prev.length + 1}`, inSeconds: t, outSeconds: t, notes: "" },
        ]);
      } else if (key === "o") {
        e.preventDefault();
        const t = currentTimeRef.current;
        setMarkers((prev) =>
          prev.length === 0
            ? prev
            : prev.map((m, i) => (i === prev.length - 1 ? { ...m, outSeconds: Math.max(t, m.inSeconds) } : m)),
        );
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
  }, []);

  return (
    <aside className="flex w-[420px] shrink-0 flex-col gap-4 overflow-y-auto border-l border-neutral-800 p-4">
      <div>
        <h2 className="truncate text-sm font-medium text-neutral-100" title={clip.filename}>
          {clip.filename}
        </h2>
        <p className="mt-1 truncate text-xs text-neutral-500" title={clip.path}>
          {clip.path}
        </p>
      </div>

      <div>
        <VideoPlayer
          key={clip.id}
          ref={playerRef}
          src={convertFileSrc(clip.path)}
          onTimeUpdate={(t) => {
            currentTimeRef.current = t;
          }}
        />
        <p className="mt-1 text-xs text-neutral-600">Press I to mark in, O to mark out.</p>
      </div>

      <div>
        <label className="mb-1.5 block text-xs font-medium text-neutral-400">Markers</label>
        <MarkerList markers={markers} onChange={setMarkers} onSeek={(t) => playerRef.current?.seek(t)} />
      </div>

      <div>
        <label className="mb-1.5 block text-xs font-medium text-neutral-400">Tags</label>
        <TagInput tags={tags} onChange={setTags} />
      </div>

      <div className="flex flex-1 flex-col">
        <label className="mb-1.5 block text-xs font-medium text-neutral-400">Notes</label>
        <textarea
          value={notes}
          onChange={(e) => setNotes(e.currentTarget.value)}
          placeholder="Freeform notes about this clip…"
          className="min-h-[120px] flex-1 resize-none rounded-md border border-neutral-800 bg-neutral-900 p-2 text-xs text-neutral-100 outline-none placeholder:text-neutral-600 focus:border-neutral-600"
        />
      </div>

      <SaveStatusLabel status={status} />
    </aside>
  );
}

function SaveStatusLabel({ status }: { status: SaveStatus }) {
  switch (status) {
    case "pending":
      return <p className="text-xs text-neutral-600">Editing…</p>;
    case "saving":
      return <p className="text-xs text-neutral-500">Saving…</p>;
    case "saved":
      return <p className="text-xs text-emerald-600">Saved</p>;
    case "error":
      return <p className="text-xs text-red-500">Failed to save — edit again to retry</p>;
    default:
      return null;
  }
}
