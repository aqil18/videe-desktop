export interface ClipSummary {
  id: string;
  path: string;
  filename: string;
  size: number;
  duration: number | null;
  thumbnailPath: string | null;
  tags: string[];
  notes: string;
}
