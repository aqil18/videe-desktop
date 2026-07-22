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
}

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
