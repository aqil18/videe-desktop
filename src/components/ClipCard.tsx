import { convertFileSrc } from "@tauri-apps/api/core";
import type { ClipSummary } from "../types";
import { formatDuration, formatFileSize } from "../lib/format";

interface ClipCardProps {
  clip: ClipSummary;
  onSelect: (clip: ClipSummary) => void;
}

export function ClipCard({ clip, onSelect }: ClipCardProps) {
  return (
    <button
      onClick={() => onSelect(clip)}
      className="group flex flex-col overflow-hidden rounded-lg border border-neutral-800 bg-neutral-900 text-left transition hover:border-neutral-600"
    >
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
              <span
                key={tag}
                className="rounded-full bg-neutral-800 px-2 py-0.5 text-xs text-neutral-300"
              >
                {tag}
              </span>
            ))}
          </div>
        )}
      </div>
    </button>
  );
}
