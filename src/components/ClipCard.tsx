import { convertFileSrc } from "@tauri-apps/api/core";
import type { ClipSummary } from "../types";
import { formatDuration, formatFileSize } from "../lib/format";

interface ClipCardProps {
  clip: ClipSummary;
  selected: boolean;
  onSelect: (clip: ClipSummary) => void;
  onToggleSelect: (id: string) => void;
}

export function ClipCard({ clip, selected, onSelect, onToggleSelect }: ClipCardProps) {
  return (
    <div
      className={`group relative flex flex-col overflow-hidden rounded-lg border text-left transition ${
        selected ? "border-neutral-400" : "border-neutral-800 hover:border-neutral-600"
      } bg-neutral-900`}
    >
      <button
        onClick={(e) => {
          e.stopPropagation();
          onToggleSelect(clip.id);
        }}
        aria-label={selected ? `Deselect ${clip.filename}` : `Select ${clip.filename}`}
        className={`absolute left-2 top-2 z-10 flex h-5 w-5 items-center justify-center rounded border text-xs transition ${
          selected
            ? "border-neutral-100 bg-neutral-100 text-neutral-900"
            : "border-neutral-500 bg-black/50 text-transparent group-hover:text-neutral-300"
        }`}
      >
        ✓
      </button>
      <button onClick={() => onSelect(clip)} className="flex flex-col text-left">
        <div className="relative aspect-video w-full bg-neutral-950">
          {clip.thumbnailPath ? (
            <img
              src={convertFileSrc(clip.thumbnailPath)}
              alt={clip.filename}
              className="h-full w-full object-cover"
              loading="lazy"
            />
          ) : (
            <div className="flex h-full w-full items-center justify-center text-xs text-neutral-600">
              no preview
            </div>
          )}
          <span className="absolute bottom-1 right-1 rounded bg-black/70 px-1.5 py-0.5 text-xs text-neutral-200">
            {formatDuration(clip.duration)}
          </span>
        </div>
        <div className="flex flex-col gap-1 px-3 py-2">
          <span className="truncate text-sm text-neutral-100" title={clip.filename}>
            {clip.filename}
          </span>
          <span className="text-xs text-neutral-500">{formatFileSize(clip.size)}</span>
          {clip.tags.length > 0 && (
            <div className="flex flex-wrap gap-1 pt-1">
              {clip.tags.map((tag) => (
                <span key={tag} className="rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300">
                  {tag}
                </span>
              ))}
            </div>
          )}
        </div>
      </button>
    </div>
  );
}
