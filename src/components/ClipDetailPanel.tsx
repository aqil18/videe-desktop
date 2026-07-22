import { useEffect, useRef, useState } from "react";
import { saveClipMetadata } from "../lib/api";
import type { ClipSummary } from "../types";
import { TagInput } from "./TagInput";

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
  const [status, setStatus] = useState<SaveStatus>("idle");
  const skipNextSave = useRef(true);

  // Switching clips: sync local state from the newly selected clip without
  // triggering a save of the clip we just navigated away from.
  useEffect(() => {
    setTags(clip.tags);
    setNotes(clip.notes);
    setStatus("idle");
    skipNextSave.current = true;
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
  }, [tags, notes]);

  return (
    <aside className="flex w-80 shrink-0 flex-col gap-4 overflow-y-auto border-l border-neutral-800 p-4">
      <div>
        <h2 className="truncate text-sm font-medium text-neutral-100" title={clip.filename}>
          {clip.filename}
        </h2>
        <p className="mt-1 truncate text-xs text-neutral-500" title={clip.path}>
          {clip.path}
        </p>
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
