export interface ClipSummary {
  id: string;
  path: string;
  filename: string;
  size: number;
  contentHash: string;
  duration: number | null;
  thumbnailPath: string | null;
  tags: string[];
  notes: string;
  // Probed once during scan; always a usable number (real probe or the
  // backend's fallback default), never absent.
  fps: number;
}

// A timestamp on a clip, positioned at `inSeconds`. New ones are always points
// (outSeconds === inSeconds); the in/out shape is kept so sidecars stay
// compatible with older builds, and in/out ranges those builds logged load as a
// timestamp at their in-point.
export interface Marker {
  id: string;
  label: string;
  inSeconds: number;
  outSeconds: number;
  notes: string;
}

export interface SaveClipMetadataInput {
  libraryRoot: string;
  id: string;
  filename: string;
  contentHash: string;
  tags: string[];
  notes: string;
  markers: Marker[];
}
