#!/usr/bin/env bash
# Builds minimal, statically linked ffmpeg/ffprobe from a pinned source release and
# installs them into src-tauri/binaries/ with the Tauri "sidecar" naming convention
# (<name>-<rust-target-triple>).
#
# The build enables only what src-tauri/src/ffmpeg.rs needs (demux mp4/mov/mxf/avi,
# decode common editing codecs, scale, write a JPEG thumbnail) and no external codec
# libraries, so the result is LGPL-2.1+ and depends only on macOS system libraries.
#
# Apple Silicon macOS only. Usage: scripts/build-ffmpeg.sh [--force]
set -euo pipefail

FFMPEG_VERSION="9.0.2"
FFMPEG_SHA256="8c3850283eb25fa026482078a04051e0be17347b09ef81a0849bec15a96e002e"
FFMPEG_URL="https://ffmpeg.org/releases/ffmpeg-${FFMPEG_VERSION}.tar.xz"
MACOS_MIN="11.0"

# --- feature map -----------------------------------------------------------------
#
# Both sidecars come from one ./configure run, so they share the same component set.
# Each one uses only part of it, driven by the commands in src-tauri/src/ffmpeg.rs:
#
#   Sidecar   Called by               Command                       Components it uses
#   -------   ---------------------   ---------------------------   ---------------------------------
#   ffmpeg    generate_thumbnail      -ss 1 -i <clip> -frames:v 1   file protocol -> demuxer -> parser
#                                     -update 1 -vf scale=320:-2    -> video decoder -> buffer, scale,
#                                     <id>.jpg                      format, buffersink filters ->
#                                                                   mjpeg encoder -> image2 muxer
#   ffprobe   probe_duration_seconds  -show_entries                 file protocol -> demuxer -> parser,
#                                     format=duration               decoders (ffprobe always probes
#   ffprobe   probe_frame_rate        -select_streams v:0           stream info, nothing is fully
#                                     -show_entries                 decoded or written)
#                                     stream=r_frame_rate
#
# Input files are whatever src-tauri/src/scanner.rs VIDEO_EXTENSIONS accepts
# (mp4, mov, mxf, avi). Anything not enabled below makes that clip fail gracefully:
# no thumbnail (logged in lib.rs), and FALLBACK_FPS for the frame rate.
#
# To support a new codec or container: add it to the matching list below and run
# this script again. The stamp changes, so it rebuilds automatically. 
# These lists are passed to ./configure, and all except REQUIRED_PARSERS are checked by
# verify() after every build (ffmpeg has no command that lists its parsers).


# Video decoders for typical camera/editing footage:
#   h264, hevc      phones, mirrorless, most consumer cameras (incl. 10-bit HEVC)
#   prores          ProRes .mov from Resolve/FCP and Atomos recorders
#   dnxhd           DNxHD and DNxHR (Avid/Resolve intermediates, usually .mxf)
#   mpeg2video      XDCAM / broadcast MXF
#   mpeg4, mjpeg    older cameras and .avi files
#   vp9             web downloads in .mp4
#   dvvideo         DV / DVCPRO
#   rawvideo        uncompressed .mov/.avi
# Audio decoders, which never decode anything for Videe; they let ffprobe read
# stream info from clips that carry audio, without warnings:
#   aac             audio in phone/camera .mp4 and .mov
#   pcm_*           uncompressed audio in pro .mov/.mxf (le) and older .mov (be)
REQUIRED_DEMUXERS=(mov mxf avi)
REQUIRED_DECODERS=(
  h264 hevc prores dnxhd mpeg2video mpeg4 mjpeg vp9 dvvideo rawvideo
  aac pcm_s16le pcm_s24le pcm_s32le pcm_s16be pcm_s24be
)
REQUIRED_ENCODERS=(mjpeg)

# Parsers split a stream into individual frames for the decoders and for ffprobe's
# stream info. One per codec family in REQUIRED_DECODERS (mpegvideo covers MPEG-2).
REQUIRED_PARSERS=(h264 hevc mpegvideo mpeg4video vp9 dnxhd mjpeg aac)

# Thumbnail filter graph:
#   scale               the -vf scale=320:-2
#   buffer, buffersink  the graph's input and output
#   format              converts pixel formats for the JPEG encoder
#   null                the pass-through ffmpeg inserts for unfiltered streams
REQUIRED_FILTERS=(scale format null buffer buffersink)

join() { local IFS=,; echo "$*"; }

CONFIGURE_FLAGS=(
  # Start from nothing: no codecs/formats/filters, and no auto-linking of whatever
  # libraries happen to be installed (Homebrew x264, VideoToolbox, ...). Everything
  # below is opted into explicitly.
  --disable-everything
  --disable-autodetect

  # Static ffmpeg libraries baked into each binary, no .dylibs to ship.
  --enable-static
  --disable-shared

  # Things Videe never uses: docs, network protocols, the ffplay player, capture
  # devices (webcams/screens), debug symbols.
  --disable-doc
  --disable-network
  --disable-ffplay
  --disable-avdevice
  --disable-debug

  # zlib for compressed MOV/MP4 headers. Links /usr/lib/libz, part of macOS.
  --enable-zlib

  # Read local files only.
  --enable-protocol=file

  # Containers. `mov` also handles .mp4 (and m4a/3gp).
  --enable-demuxer="$(join "${REQUIRED_DEMUXERS[@]}")"

  # Video and audio decoders, see REQUIRED_DECODERS above.
  --enable-decoder="$(join "${REQUIRED_DECODERS[@]}")"

  # Parsers, see REQUIRED_PARSERS above.
  --enable-parser="$(join "${REQUIRED_PARSERS[@]}")"

  # Thumbnail filter graph, see REQUIRED_FILTERS above.
  --enable-filter="$(join "${REQUIRED_FILTERS[@]}")"

  # Write the thumbnail: mjpeg encodes one JPEG frame, image2 writes it to
  # <id>.jpg (and handles -update 1).
  --enable-encoder="$(join "${REQUIRED_ENCODERS[@]}")"
  --enable-muxer=image2

  # Toolchain: Apple Silicon with Xcode's clang, runnable on macOS 11+.
  --arch=arm64
  --cc=clang
  --extra-cflags="-mmacosx-version-min=${MACOS_MIN}"
  --extra-ldflags="-mmacosx-version-min=${MACOS_MIN}"
)

FORCE=0
[[ "${1:-}" == "--force" ]] && FORCE=1

REPO_ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
BIN_DIR="$REPO_ROOT/src-tauri/binaries"
BUILD_DIR="$BIN_DIR/.build"
mkdir -p "$BUILD_DIR"

die() { echo "error: $*" >&2; exit 1; }

# --- preflight -------------------------------------------------------------------

[[ "$(uname -s)" == "Darwin" ]] || die "this script only supports macOS"
[[ "$(uname -m)" == "arm64" ]] || die "this script only supports Apple Silicon (arm64)"
xcode-select -p >/dev/null 2>&1 || die "Xcode Command Line Tools not found (run: xcode-select --install)"
for tool in curl shasum make clang otool codesign strip; do
  command -v "$tool" >/dev/null 2>&1 || die "'$tool' not found on PATH"
done

# `npm run` doesn't always inherit the rustup PATH entry from the user's shell profile.
RUSTC="$(command -v rustc || true)"
[[ -z "$RUSTC" && -x "$HOME/.cargo/bin/rustc" ]] && RUSTC="$HOME/.cargo/bin/rustc"
[[ -n "$RUSTC" ]] || die "rustc not found on PATH or in ~/.cargo/bin (needed to determine the target triple)"
TARGET_TRIPLE="$("$RUSTC" -vV | awk '/^host:/ { print $2 }')"
[[ "$TARGET_TRIPLE" == "aarch64-apple-darwin" ]] || die "expected rust host aarch64-apple-darwin, got $TARGET_TRIPLE"

FFMPEG_DEST="$BIN_DIR/ffmpeg-$TARGET_TRIPLE"
FFPROBE_DEST="$BIN_DIR/ffprobe-$TARGET_TRIPLE"
LICENSE_DEST="$BIN_DIR/ffmpeg-COPYING.LGPLv2.1"
BUILD_INFO_DEST="$BIN_DIR/ffmpeg-build.txt"
STAMP_FILE="$BIN_DIR/.ffmpeg-stamp"

STAMP="$(printf '%s\n' "$FFMPEG_VERSION" "$FFMPEG_SHA256" "${CONFIGURE_FLAGS[@]}" | shasum -a 256 | awk '{ print $1 }')"

# --- verification ----------------------------------------------------------------

# Fails if a binary loads any library from outside macOS itself (e.g. /opt/homebrew).
check_system_deps_only() {
  local bin="$1" bad
  bad="$(otool -L "$bin" | tail -n +2 | awk '{ print $1 }' | grep -vE '^(/usr/lib/|/System/Library/)' || true)"
  [[ -z "$bad" ]] || die "$bin links non-system libraries:"$'\n'"$bad"
}

check_components() {
  local kind="$1"; shift
  local names name
  # Rows look like " VFS..D mjpeg  MJPEG ..." or " D   mov,mp4,m4a  QuickTime / MOV".
  names="$("$FFMPEG_DEST" -hide_banner "-$kind" 2>/dev/null | awk '{ print $2 }' | tr ',' '\n')"
  for name in "$@"; do
    grep -qx "$name" <<<"$names" || die "ffmpeg build is missing $kind entry '$name'"
  done
}

verify() {
  check_system_deps_only "$FFMPEG_DEST"
  check_system_deps_only "$FFPROBE_DEST"
  check_components demuxers "${REQUIRED_DEMUXERS[@]}"
  check_components decoders "${REQUIRED_DECODERS[@]}"
  check_components encoders "${REQUIRED_ENCODERS[@]}"
  check_components filters "${REQUIRED_FILTERS[@]}"
  "$FFMPEG_DEST" -hide_banner -L 2>/dev/null | grep -q "GNU Lesser General Public" \
    || die "ffmpeg build is not LGPL"
  "$FFPROBE_DEST" -hide_banner -version >/dev/null || die "ffprobe does not run"
}

if [[ "$FORCE" == 0 && -x "$FFMPEG_DEST" && -x "$FFPROBE_DEST" && -f "$LICENSE_DEST" \
      && -f "$STAMP_FILE" && "$(cat "$STAMP_FILE")" == "$STAMP" ]]; then
  verify
  echo "ffmpeg $FFMPEG_VERSION sidecars already up to date (use --force to rebuild)."
  exit 0
fi

# --- fetch source ----------------------------------------------------------------

TARBALL="$BUILD_DIR/ffmpeg-$FFMPEG_VERSION.tar.xz"
SRC_DIR="$BUILD_DIR/ffmpeg-$FFMPEG_VERSION"

if [[ ! -f "$TARBALL" ]] || ! shasum -a 256 -c <<<"$FFMPEG_SHA256  $TARBALL" >/dev/null 2>&1; then
  echo "downloading $FFMPEG_URL"
  curl -fL --retry 3 -o "$TARBALL.part" "$FFMPEG_URL"
  mv "$TARBALL.part" "$TARBALL"
fi
shasum -a 256 -c <<<"$FFMPEG_SHA256  $TARBALL" >/dev/null \
  || { rm -f "$TARBALL"; die "SHA256 mismatch for $TARBALL"; }

rm -rf "$SRC_DIR"
tar -xJf "$TARBALL" -C "$BUILD_DIR"

# --- build -----------------------------------------------------------------------

echo "configuring ffmpeg $FFMPEG_VERSION (minimal static LGPL build)"
(cd "$SRC_DIR" && ./configure "${CONFIGURE_FLAGS[@]}" >"$BUILD_DIR/configure.log" 2>&1) \
  || die "configure failed, see $BUILD_DIR/configure.log"

echo "building (this takes a few minutes the first time)"
(cd "$SRC_DIR" && make -j"$(sysctl -n hw.ncpu)" >"$BUILD_DIR/make.log" 2>&1) \
  || die "make failed, see $BUILD_DIR/make.log"

# --- install ---------------------------------------------------------------------

install_binary() {
  local src="$1" dest="$2"
  # Old copies may be read-only (Homebrew installs them 0555), which breaks a plain cp.
  rm -f "$dest"
  cp "$src" "$dest"
  chmod 755 "$dest"
  strip "$dest"
  # strip invalidates the linker's ad-hoc signature, and arm64 macOS refuses to run unsigned code.
  codesign --force --sign - "$dest" 2>/dev/null
  echo "installed $dest ($(du -h "$dest" | awk '{ print $1 }'))"
}

install_binary "$SRC_DIR/ffmpeg" "$FFMPEG_DEST"
install_binary "$SRC_DIR/ffprobe" "$FFPROBE_DEST"

cp "$SRC_DIR/COPYING.LGPLv2.1" "$LICENSE_DEST"
cat >"$BUILD_INFO_DEST" <<EOF
Videe bundles ffmpeg and ffprobe ${FFMPEG_VERSION}, licensed under the
GNU Lesser General Public License v2.1 or later (see COPYING.LGPLv2.1).

Source:  ${FFMPEG_URL}
SHA256:  ${FFMPEG_SHA256}
Built by scripts/build-ffmpeg.sh in https://github.com/aqil18/videe-desktop
with:

./configure ${CONFIGURE_FLAGS[*]}
EOF

verify
echo "$STAMP" >"$STAMP_FILE"
echo "done. Sidecar binaries are ready for target: $TARGET_TRIPLE"
