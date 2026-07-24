# Videe

Videe is a local-first video library and tagging tool for video editing teams.
It does not upload your footage anywhere and it does not run a server. Instead,
you point it at a folder that's already being synced by Google Drive, Dropbox,
or any similar client, and it:

- scans that folder for video files and shows them as a searchable, taggable grid
- lets you tag clips, add notes, and mark in/out points on a timeline
- writes all of that metadata as small JSON files *inside the same folder*

Because the metadata lives next to your footage in the synced folder, it
propagates to your teammates automatically through whatever sync client they
already run. There's no account system, no hosted database, and no cloud
storage owned by this project — the sync provider you already trust with your
footage is doing all the work.


Nobody has stitched these three pieces together — (1) zero-infrastructure collaboration via existing Drive/Dropbox sync, (2) video-specific tagging with in/out marker logging, and (3) direct EDL/CSV export for NLEs — into one open-source tool.

**Requirement:** the folder you select must already be kept in sync by a
client like Google Drive for Desktop or the Dropbox app. Videee only reads and
writes files on disk; it has no knowledge of Drive/Dropbox accounts or APIs.

## Project status

This is an MVP built in phases. All five are done:

- [x] Phase 1 — Library scan (folder picker, recursive video scan, ffmpeg
      thumbnails, SQLite cache, library grid)
- [x] Phase 2 — Metadata read/write (tag editor, notes, debounced sidecar
      writes, filtering)
- [x] Phase 3 — File watching for collaboration
- [x] Phase 4 — In/out markers and the video player
- [x] Phase 5 — Export (CSV/EDL — see the note in the Export section below on
      why FCPXML isn't one of the options)

## Export

Select one or more clips in the library grid (checkbox in the top-left corner
of each card) and an export bar appears with **Export CSV** and **Export
EDL** buttons. A native save dialog asks where to put the file.

- **CSV**: one row per marker (filename, tags, marker label, in/out as
  `HH:MM:SS.mmm`). A clip with no markers exports as a single row covering
  its full duration, so every selected clip shows up even if untagged.
- **EDL**: a CMX3600 edit decision list with events laid back-to-back on the
  record timeline, so importing it into Premiere or Resolve gives you an
  assembled sequence of the marked ranges, not just a list. Timecodes are
  frame-accurate (`HH:MM:SS:FF`), using each clip's frame rate probed via
  ffprobe at export time (falls back to 25fps if a stream's rate can't be read).

This intentionally doesn't export FCPXML. FCPXML's resource/timecode schema
is easy to get subtly wrong, and a file that silently fails to import is
worse than one in a simpler format that's actually correct — CSV and EDL
both import cleanly into Premiere and Resolve today.

## Metadata schema

For each video file, Videee stores `.metadata/<clip-id>.json` next to it in
the library folder:

```json
{
  "id": "uuid",
  "filename": "original file name at time of tagging",
  "tags": ["string", "string"],
  "markers": [
    { "id": "uuid", "label": "string", "inSeconds": 12.5, "outSeconds": 18.2, "notes": "string" }
  ],
  "notes": "freeform string",
  "author": "local OS username, best-effort",
  "updatedAt": "ISO 8601 timestamp",
  "contentHash": "sha256 of file size + first 4MB, optional"
}
```

**Filename-vs-hash identity decision:** a clip is matched to its sidecar JSON
primarily by filename. Each sidecar also stores a `contentHash` — a SHA-256 of
the file's size plus its first 4MB — computed during the library scan. Hashing
whole multi-GB masters on every scan would be too slow to be usable, so this
partial hash is a cheap fingerprint rather than a cryptographic guarantee of
uniqueness. It exists so that when a clip gets renamed (common when editors
reorganize bins), Phase 2's metadata loader can fall back to matching by
content hash instead of silently orphaning the tags/markers already recorded
for that file. Filename stays primary because it's what teammates actually see
and reason about; the hash is a resilience mechanism, not the source of truth.

The local SQLite cache (`cache.sqlite` in the app's local data directory, never
inside the synced folder) is a rebuildable index over these JSON files — if you
delete it, Videee reconstructs it from disk on the next scan. It is not the
source of truth.

## Tech stack

- **Shell:** Tauri v2
- **Backend:** Rust — `notify` (file watching, Phase 3), `rusqlite` (local
  cache), `serde`/`serde_json` (metadata), a bundled ffmpeg sidecar
  (thumbnails, duration probing), `uuid` (clip IDs), `walkdir` (scanning)
- **Frontend:** React + TypeScript + Vite + Tailwind CSS

## Repo structure

```
src-tauri/src/
  lib.rs        Tauri commands, app state, plugin wiring
  scanner.rs    recursive video file discovery + content fingerprinting
  metadata.rs   .metadata/<clip-id>.json read/write (source of truth)
  cache.rs      SQLite cache (rebuildable index, not source of truth)
  ffmpeg.rs     thumbnail generation, duration/frame-rate probing via ffmpeg sidecar
  watcher.rs    notify-based watcher for collaborator changes
  export.rs     CSV/EDL export builders
src/
  components/   React components (library grid, clip cards, ...)
  lib/          frontend API wrapper around Tauri commands, formatting helpers
```

## Building and running locally

Prerequisites:

- [Rust](https://rustup.rs/) (stable toolchain)
- Node.js 18+
- ffmpeg + ffprobe installed locally (`brew install ffmpeg` on macOS,
  `apt install ffmpeg` on Debian/Ubuntu, `choco install ffmpeg` on Windows)

Videee bundles ffmpeg/ffprobe as Tauri "sidecar" binaries rather than shelling
out to whatever happens to be on `PATH` at runtime. Those binaries aren't
committed to the repo — they're large, platform-specific, and their
redistribution terms depend on which codecs are enabled in the build. Instead,
a setup script copies your local ffmpeg/ffprobe install into
`src-tauri/binaries/` with the naming convention Tauri's sidecar mechanism
expects:

```bash
npm install
npm run setup:ffmpeg   # copies system ffmpeg/ffprobe into src-tauri/binaries/
npm run tauri dev
```

To produce a release build:

```bash
npm run tauri build
```

## Testing

```bash
cd src-tauri && cargo test   # scanner, metadata, cache, watcher, and export tests
npx tsc --noEmit             # frontend type-checking
```
