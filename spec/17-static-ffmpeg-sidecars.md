# Spec #17: Self-contained ffmpeg/ffprobe sidecars for macOS

Ticket: https://github.com/aqil18/videe-desktop/issues/17 (follows the local fix for #13)

## Context

`scripts/fetch-ffmpeg.sh` copies Homebrew's `ffmpeg`/`ffprobe` into `src-tauri/binaries/`. Those are
~400 KB launchers that load 17 Homebrew dylibs by absolute path (`/opt/homebrew/Cellar/ffmpeg/<ver>/lib/...`).
Nothing outside the binary itself is bundled into `Videe.app`/the DMG, so:

- the app breaks on any Mac without that exact Homebrew ffmpeg version (thumbnails + probing fail), and
- it breaks on the dev machine after every `brew upgrade` (the "Library not loaded: libavdevice.62" error).

The Rust `videe` binary is already clean (0 non-system deps: `rusqlite` bundled, `reqwest` with rustls).
Only the sidecars need fixing.

**Outcome:** the setup script compiles a minimal, statically linked, LGPL-only ffmpeg/ffprobe from a pinned
source release, verifies with `otool -L` that they depend only on macOS system libraries, and the DMG
runs on any Apple Silicon Mac with no Homebrew.

Decisions made: **Apple Silicon only** (`aarch64-apple-darwin`), **macOS only** (drop the Linux/Windows
copy paths), custom minimal build rather than a prebuilt binary.

**About `otool`:** `otool -L <binary>` is a macOS tool that ships with Xcode CLT. It lists the shared
libraries a binary loads at launch and the exact path each one is loaded from. Paths under `/usr/lib/` or
`/System/Library/` are part of macOS and exist on every Mac. Any other absolute path (e.g. `/opt/homebrew/...`)
exists only on the build machine, and the binary fails to start without it ("Library not loaded"). This is how
the problem was diagnosed and how the fix is verified. It checks only libraries linked at launch, so it doesn't
replace testing on a clean Mac.

## What Videe needs from ffmpeg (from `src-tauri/src/ffmpeg.rs`, `scanner.rs:9`)

- Inputs: `.mp4 .mov .mxf .avi` → demuxers `mov`, `mxf`, `avi`; protocol `file`
- `generate_thumbnail`: decode 1 video frame, `-vf scale=320:-2`, write `.jpg` → encoder `mjpeg`, muxer `image2`, filter `scale`
- `probe_duration_seconds` / `probe_frame_rate`: container/stream metadata only
- Decoders for typical Resolve footage: `h264, hevc, prores, dnxhd, mpeg2video, mpeg4, mjpeg, vp9, dvvideo, rawvideo`
- Audio decoders `aac` and common `pcm_*` variants (added during implementation): Videe never decodes audio,
  but these let ffprobe read stream info from files with audio tracks, at negligible size cost
- No external codec libraries → no `--enable-gpl`, result is LGPL-2.1+

## Changes

### 1. Replace `scripts/fetch-ffmpeg.sh` with `scripts/build-ffmpeg.sh`
Keep the header-comment style of the existing script. Steps:

1. **Preflight:** require `uname -s` = Darwin and `uname -m` = arm64; Xcode CLT (`xcode-select -p`), `curl`,
   `shasum`, `make`. Resolve `rustc` from PATH, falling back to `~/.cargo/bin/rustc` (fixes the
   "rustc not found" failure under `npm run`). Assert target triple is `aarch64-apple-darwin`.
2. **Pinned source:** `FFMPEG_VERSION` (current release, e.g. 9.0.2) + `FFMPEG_SHA256`. Download
   `https://ffmpeg.org/releases/ffmpeg-$FFMPEG_VERSION.tar.xz` into `src-tauri/binaries/.build/`
   (already gitignored by `/src-tauri/binaries/*`), verify the checksum, extract.
3. **Skip if up to date:** write a stamp file (version + hash of the configure flags); if both sidecars
   exist and the stamp matches, exit early. `--force` flag to rebuild.
4. **Configure + build** with the flags from the earlier discussion, defined once in a `CONFIGURE_FLAGS` array:
   `--disable-everything --disable-autodetect --enable-static --disable-shared --disable-doc
   --disable-network --disable-ffplay --disable-debug --enable-zlib --enable-protocol=file
   --enable-demuxer=mov,mxf,avi --enable-decoder=<list above> --enable-parser=h264,hevc,mpegvideo,mpeg4video,vp9
   --enable-filter=scale,format,null,buffer,buffersink --enable-encoder=mjpeg --enable-muxer=image2`,
   plus `--arch=arm64 --cc=clang` and `-mmacosx-version-min=11.0` in extra cflags/ldflags.
   `make -j"$(sysctl -n hw.ncpu)"`.
5. **Install sidecars:** `rm -f` the old files first (they were read-only, which broke `cp`), copy
   `ffmpeg`/`ffprobe` to `src-tauri/binaries/<name>-aarch64-apple-darwin`, `strip`, then ad-hoc re-sign
   (`codesign -s - -f`), since stripping invalidates the linker's arm64 signature.
6. **Verify (fail the script on any problem):**
   - `otool -L` on both: every entry must start with `/usr/lib/` or `/System/Library/`.
   - `ffmpeg -hide_banner -decoders` / `-demuxers` / `-encoders` contain each required name.
   - `ffmpeg -L` does not report GPL.
7. **License files:** copy `COPYING.LGPLv2.1` from the source tree and write `ffmpeg-build.txt`
   (version, source URL, SHA256, full configure line) into `src-tauri/binaries/`.

### 2. `src-tauri/tauri.conf.json`
Add to `bundle.resources`: the two license files → `licenses/ffmpeg/` inside the app, so every DMG ships
ffmpeg's license and a pointer to the exact source.

### 3. `package.json`
`setup:ffmpeg` → `bash scripts/build-ffmpeg.sh` (keep the script name so existing muscle memory works).

### 4. Doc/comment updates
- `README.md` "Building and running locally": prerequisites become Xcode CLT instead of
  `brew install ffmpeg`; note first run takes a few minutes and is cached; Apple Silicon only.
  Add a short "Third-party: FFmpeg" section (LGPL-2.1+, version, link to the source tarball, configure flags
  live in `scripts/build-ffmpeg.sh`), which covers the source-availability obligation.
- `src-tauri/src/ffmpeg.rs:1-2` doc comment and the `.gitignore` comment: point to `scripts/build-ffmpeg.sh`.

No Rust logic changes: unsupported codecs already fail gracefully (`lib.rs:105` logs the thumbnail error;
`probe_frame_rate` falls back to `FALLBACK_FPS`).

## Verification

### Run by Claude during implementation (all passed, 2026-11-01)

1. **Build script:** `npm run setup:ffmpeg` from a clean `src-tauri/binaries/` succeeds (~75 s) and its verify
   step passes. A second run skips the rebuild (0.4 s).
2. **Dependencies:** `otool -L` on both sidecars shows only `/usr/lib/` and `/System/Library/` entries
   (libSystem, libz, CoreFoundation, CoreVideo, CoreMedia). Sizes: ffmpeg 6.6 MB, ffprobe 6.4 MB. Required
   components present, and the build is LGPL.
3. **Unit tests:** `cd src-tauri && cargo test`: 26 passed.
4. **Sidecar smoke test (no UI):** 3-second clips were generated into the scratchpad with Homebrew's full
   ffmpeg, used as a test tool only. Video came from `testsrc2` (1080p25 test pattern), with a 48 kHz `sine`
   audio track. The new sidecars were then run with the exact arguments from `ffmpeg.rs`:

   | Clip | Thumbnail | Duration | FPS |
   |---|---|---|---|
   | H.264 `.mp4` | 320×180 | 3.000 | 25/1 |
   | 10-bit HEVC `.mp4` | 320×180 | 3.000 | 25/1 |
   | ProRes 422 HQ `.mov` | 320×180 | 3.000 | 25/1 |
   | DNxHR HQ `.mxf` | 320×180 | 3.000 | 25/1 |
   | MPEG-2 (XDCAM-style) `.mxf` | 320×180 | 3.000 | 25/1 |
   | MPEG-4 `.avi` | 320×180 | 3.000 | 25/1 |
   | H.264 29.97 fps `.mov` | 320×180 | 3.003 | 30000/1001 |

5. **Release bundle:** `npm run tauri build` succeeds. In both `Videe.app` and the mounted DMG, `ffmpeg`,
   `ffprobe` and `videe` have no non-system dependencies, and `Contents/Resources/licenses/ffmpeg/` holds
   `COPYING.LGPLv2.1` and `BUILD.txt`. DMG size: 15 MB.

### Verified by the user

6. **Real app, real footage:** `npm run tauri dev`, scan a folder of real clips (including MXF and camera
   formats in use). Thumbnails appear and duration and fps are populated. Report any clip that fails, since its
   codec may need adding to the enable list. Synthetic clips don't cover camera-specific variants like
   XAVC MXF or variable frame rate.
7. **Clean Mac:** install `src-tauri/target/release/bundle/dmg/Videe_0.1.0_aarch64.dmg` on another Apple
   Silicon Mac or a macOS VM without Homebrew. The app opens, and thumbnails generate.
