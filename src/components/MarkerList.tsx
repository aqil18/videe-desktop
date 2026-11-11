import { colorForLabel, withAlpha } from "../lib/color";
import { formatFrameTimecode } from "../lib/format";
import type { Marker } from "../types";

interface MarkerListProps {
  markers: Marker[];
  fps: number;
  onChange: (markers: Marker[]) => void;
  onSeek: (seconds: number) => void;
}

/** Lists a clip's timestamps in time order (not creation order). */
export function MarkerList({ markers, fps, onChange, onSeek }: MarkerListProps) {
  function updateLabel(id: string, label: string) {
    onChange(markers.map((m) => (m.id === id ? { ...m, label } : m)));
  }

  function removeMarker(id: string) {
    onChange(markers.filter((m) => m.id !== id));
  }

  if (markers.length === 0) {
    return <p className="text-xs text-neutral-600">No timestamps yet. Press T to add one.</p>;
  }

  const sorted = [...markers].sort((a, b) => a.inSeconds - b.inSeconds);

  return (
    <ul className="flex flex-col gap-1.5">
      {sorted.map((marker) => {
        const color = colorForLabel(marker.label);
        return (
          <li
            key={marker.id}
            className="flex items-center gap-2 rounded-md border bg-neutral-900 px-2 py-1.5"
            style={{ borderColor: withAlpha(color, 0.45) }}
          >
            <span className="h-2 w-2 shrink-0 rounded-full" style={{ backgroundColor: color }} />
            <button
              onClick={() => onSeek(marker.inSeconds)}
              title="Jump to timestamp"
              className="shrink-0 text-xs tabular-nums text-neutral-500 transition hover:text-neutral-200"
            >
              {formatFrameTimecode(marker.inSeconds, fps)}
            </button>
            <input
              value={marker.label}
              onChange={(e) => updateLabel(marker.id, e.currentTarget.value)}
              className="min-w-0 flex-1 bg-transparent text-xs text-neutral-100 outline-none"
            />
            <button
              onClick={() => removeMarker(marker.id)}
              aria-label={`Delete timestamp ${marker.label}`}
              className="shrink-0 text-neutral-500 transition hover:text-red-400"
            >
              ×
            </button>
          </li>
        );
      })}
    </ul>
  );
}
