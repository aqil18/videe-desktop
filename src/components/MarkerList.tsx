import { colorForLabel, withAlpha } from "../lib/color";
import { formatDuration } from "../lib/format";
import type { Marker } from "../types";

interface MarkerListProps {
  markers: Marker[];
  onChange: (markers: Marker[]) => void;
  onSeek: (seconds: number) => void;
}

export function MarkerList({ markers, onChange, onSeek }: MarkerListProps) {
  function updateLabel(id: string, label: string) {
    onChange(markers.map((m) => (m.id === id ? { ...m, label } : m)));
  }

  function removeMarker(id: string) {
    onChange(markers.filter((m) => m.id !== id));
  }

  if (markers.length === 0) {
    return <p className="text-xs text-neutral-600">No markers yet. Press I to mark in, O to mark out.</p>;
  }

  return (
    <ul className="flex flex-col gap-1.5">
      {markers.map((marker) => {
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
              title="Jump to marker"
              className="shrink-0 text-xs tabular-nums text-neutral-500 transition hover:text-neutral-200"
            >
              {formatDuration(marker.inSeconds)}–{formatDuration(marker.outSeconds)}
            </button>
            <input
              value={marker.label}
              onChange={(e) => updateLabel(marker.id, e.currentTarget.value)}
              className="min-w-0 flex-1 bg-transparent text-xs text-neutral-100 outline-none"
            />
            <button
              onClick={() => removeMarker(marker.id)}
              aria-label={`Delete marker ${marker.label}`}
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
