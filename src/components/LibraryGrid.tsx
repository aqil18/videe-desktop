import type { ClipSummary } from "../types";
import { ClipCard } from "./ClipCard";

interface LibraryGridProps {
  clips: ClipSummary[];
  selectedIds: Set<string>;
  onSelect: (clip: ClipSummary) => void;
  onToggleSelect: (id: string) => void;
}

export function LibraryGrid({ clips, selectedIds, onSelect, onToggleSelect }: LibraryGridProps) {
  if (clips.length === 0) {
    return (
      <div className="flex flex-1 items-center justify-center text-sm text-neutral-500">
        No video files found in this folder.
      </div>
    );
  }

  return (
    <div className="grid flex-1 grid-cols-[repeat(auto-fill,minmax(200px,1fr))] gap-4 overflow-y-auto p-4">
      {clips.map((clip) => (
        <ClipCard
          key={clip.id}
          clip={clip}
          selected={selectedIds.has(clip.id)}
          onSelect={onSelect}
          onToggleSelect={onToggleSelect}
        />
      ))}
    </div>
  );
}
