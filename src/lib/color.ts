// Fixed-order categorical palette (validated for CVD-safe adjacent contrast on
// a dark surface). Never cycle/generate hues -- labels hash into these slots so
// the same label always renders the same color across tags, markers, and the
// waveform.
const CATEGORICAL_PALETTE = [
  "#3987e5", // blue
  "#d95926", // orange
  "#199e70", // aqua
  "#c98500", // yellow
  "#d55181", // magenta
  "#008300", // green
  "#9085e9", // violet
  "#e66767", // red
];

function hashString(value: string): number {
  let hash = 0;
  for (let i = 0; i < value.length; i++) {
    hash = (hash * 31 + value.charCodeAt(i)) >>> 0;
  }
  return hash;
}

export function colorForLabel(label: string): string {
  const key = label.trim().toLowerCase();
  if (!key) return CATEGORICAL_PALETTE[0];
  return CATEGORICAL_PALETTE[hashString(key) % CATEGORICAL_PALETTE.length];
}

export function withAlpha(hex: string, alpha: number): string {
  const value = hex.replace("#", "");
  const r = parseInt(value.slice(0, 2), 16);
  const g = parseInt(value.slice(2, 4), 16);
  const b = parseInt(value.slice(4, 6), 16);
  return `rgba(${r}, ${g}, ${b}, ${alpha})`;
}
