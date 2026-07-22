#!/usr/bin/env bash
# Copies the system-installed ffmpeg/ffprobe binaries into src-tauri/binaries/
# with the Tauri "sidecar" naming convention (<name>-<rust-target-triple>).
#
# We don't commit ffmpeg/ffprobe binaries to git: they're large, platform-specific,
# and redistribution terms vary by build (GPL vs LGPL, which codecs are enabled).
# Instead each developer/CI runner points this script at a locally installed
# ffmpeg (brew/apt/choco/scoop) before running `tauri dev` or `tauri build`.
set -euo pipefail

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/binaries"
mkdir -p "$BIN_DIR"

if ! command -v rustc >/dev/null 2>&1; then
  echo "error: rustc not found on PATH (needed to determine the target triple)" >&2
  exit 1
fi
TARGET_TRIPLE="$(rustc -vV | awk '/^host:/ { print $2 }')"

EXT=""
if [[ "$TARGET_TRIPLE" == *"windows"* ]]; then
  EXT=".exe"
fi

copy_binary() {
  local name="$1"
  local src
  src="$(command -v "$name" || true)"
  if [[ -z "$src" ]]; then
    echo "error: '$name' not found on PATH." >&2
    echo "  macOS:   brew install ffmpeg" >&2
    echo "  Linux:   sudo apt install ffmpeg" >&2
    echo "  Windows: choco install ffmpeg  (or scoop install ffmpeg)" >&2
    exit 1
  fi
  local dest="$BIN_DIR/${name}-${TARGET_TRIPLE}${EXT}"
  cp "$src" "$dest"
  chmod +x "$dest"
  echo "copied $src -> $dest"
}

copy_binary ffmpeg
copy_binary ffprobe

echo "done. Sidecar binaries are ready for target: $TARGET_TRIPLE"
