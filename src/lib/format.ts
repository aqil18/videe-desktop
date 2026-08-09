export function formatDuration(seconds: number | null): string {
  if (seconds === null || !Number.isFinite(seconds)) return "--:--";
  const total = Math.round(seconds);
  const h = Math.floor(total / 3600);
  const m = Math.floor((total % 3600) / 60);
  const s = total % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return h > 0 ? `${h}:${pad(m)}:${pad(s)}` : `${m}:${pad(s)}`;
}

// Raw frame count at the given fps. Mirrors the Rust side's frame math
// (src-tauri/src/export.rs::format_timecode) so the number shown here always
// agrees with what an exported EDL's timecodes imply.
export function frameNumber(seconds: number, fps: number): number {
  return Math.round(Math.max(0, seconds) * fps);
}

// Frame-accurate HH:MM:SS:FF, mirroring export.rs::format_timecode exactly
// (same round-then-rollover approach) so the UI's timecode never disagrees
// with what gets written into an exported EDL.
export function formatFrameTimecode(seconds: number, fps: number): string {
  const fpsInt = Math.max(1, Math.round(fps));
  const totalFrames = frameNumber(seconds, fps);
  const frames = totalFrames % fpsInt;
  const totalSecs = Math.floor(totalFrames / fpsInt);
  const h = Math.floor(totalSecs / 3600);
  const m = Math.floor((totalSecs % 3600) / 60);
  const s = totalSecs % 60;
  const pad = (n: number) => n.toString().padStart(2, "0");
  return `${pad(h)}:${pad(m)}:${pad(s)}:${pad(frames)}`;
}

export function formatFileSize(bytes: number): string {
  if (bytes < 1024) return `${bytes} B`;
  const units = ["KB", "MB", "GB", "TB"];
  let value = bytes / 1024;
  let unitIndex = 0;
  while (value >= 1024 && unitIndex < units.length - 1) {
    value /= 1024;
    unitIndex += 1;
  }
  return `${value.toFixed(1)} ${units[unitIndex]}`;
}
