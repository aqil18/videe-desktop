# Spec #3: Opening large clips freezes or crashes the app

Branch: `3-video-opening-lag`. Status: **Approach A chosen and implemented on this branch.** Computing a
real audio waveform is deferred to a separate ticket.

## Summary

When you select a clip, Videe reads the **entire video file into memory several times**, and the first of
those reads happens **on the app's main thread**. The read is not needed for playback. It exists only so
wavesurfer.js can draw an audio waveform under the player:

1. `VideoScrubber` creates a WaveSurfer instance bound to the `<video>` element and gives it no
   pre-computed peaks. In that mode wavesurfer `fetch()`es the whole media URL and decodes it with
   Web Audio's `decodeAudioData`.
2. That `fetch()` sends no `Range` header. Tauri's built-in `asset://` protocol answers an unranged GET
   by `read_to_end`-ing the whole file into a `Vec<u8>`, synchronously, on the main thread.
   wry then copies that buffer again into an `NSData` to give it to WebKit.
3. On the WebKit side the bytes are buffered again as a `Blob`, again as an `ArrayBuffer`, and again by
   `decodeAudioData`.

For a clip of size *N*, the main app process holds about **2N** at its peak and the WebKit processes hold
about **2–3N**. The UI is also frozen for as long as the disk read takes. A few hundred MB only causes a
noticeable stall. Multi-GB camera masters, which is exactly what Videe is for, exhaust memory and the
app or its web content process is killed. On a cloud-synced library folder with online-only files, that
read also forces a download of the whole file before the UI unfreezes.

In practice wavesurfer is only used as the **scrub and marker surface**: seeking, the playhead, marker
regions and play/pause. All of that needs only the clip's duration, not its audio. The waveform drawing
itself has never worked reliably. When the decode fails it fails silently and leaves a plain grey bar.
**The chosen fix (Approach A) is to give wavesurfer the known duration and a flat, silent waveform**, so it
never fetches the file. The bar stays grey, as users see it today, but opening any clip no longer reads
the whole file. Computing a real waveform (Approach B) is tracked separately.

## How a clip gets opened today

```
LibraryGrid click
  └─ ClipDetailPanel renders <VideoScrubber key={clip.id} src={convertFileSrc(clip.path)} …>
       ├─ <video src="asset://localhost/<path>">          ← playback: Range requests, ≤1 MB each  (OK-ish)
       └─ WaveSurfer.create({ media: video })              ← no `peaks`, no `duration`
            └─ load(video.src)
                 └─ fetch(video.src)                       ← NO Range header
                      │
          ┌───────────┴──────────── Rust main process (main thread) ───────────────┐
          │ tauri protocol/asset.rs: Vec::with_capacity(len); file.read_to_end()   │  copy 1 (N)
          │ wry url_scheme_handler.rs: NSData::initWithBytes_length(content)       │  copy 2 (N)
          │ task.didReceiveData(data)                                              │
          └───────────┬────────────────────────────────────────────────────────────┘
                      │  IPC to WebKit networking / web content processes
                 response.clone() (progress tee) + response.blob()                    copy 3 (N)
                 blob.arrayBuffer()                                                   copy 4 (N)
                 AudioContext({sampleRate: 8000}).decodeAudioData(arrayBuffer)        copy 5 (≈N) + PCM
                 └─ emit "ready" → renderer draws waveform; VideoScrubber setDuration()
```

### Evidence (with versions locked in this repo)

| Step | Location | What it does |
|---|---|---|
| WaveSurfer bound to the video element, with no peaks | [src/components/VideoScrubber.tsx:91-105](../src/components/VideoScrubber.tsx#L91-L105) | `WaveSurfer.create({ container, media: videoRef.current, … })` |
| Auto-load on construction | `node_modules/wavesurfer.js/dist/wavesurfer.js:86-96` (7.12.11) | `initialUrl = this.options.url \|\| this.getSrc()`, so if the media element already has a `src`, `load(initialUrl)` runs automatically |
| Fetch the whole file | `wavesurfer.js:326-337`, `fetcher.js:59-70` | "Fetch the entire audio as a blob if pre-decoded data is not provided": a plain `fetch(url)`, `response.clone()` for progress, then `response.blob()` |
| Decode the whole file | `wavesurfer.js:374-380`, `decoder.js:15` | `blob.arrayBuffer()` then `new AudioContext({ sampleRate: 8000 }).decodeAudioData(...)` |
| Asset protocol registered as a *synchronous* handler | `tauri-2.11.5/src/manager/webview.rs:343` | `register_uri_scheme_protocol("asset", …)`, not the asynchronous variant |
| Handler invoked inline on the WebKit callback (main thread) | `wry-0.55.1/src/wkwebview/class/url_scheme_handler.rs:322` | `function(webview_id, final_request, responder)` is called directly from `startURLSchemeTask` |
| Unranged GET reads the whole file | `tauri-2.11.5/src/protocol/asset.rs:207-221` | `Vec::with_capacity(len as usize); file.read_to_end(&mut local_buf)` |
| Ranged GET is capped | `asset.rs:119` | `MAX_LEN = 1000 * 1024`. This is why `<video>` playback itself doesn't explode |
| Second full copy before handoff to WebKit | `wry-0.55.1/…/url_scheme_handler.rs:268-275` | `NSData::initWithBytes_length(content.as_ptr(), content.len())` (comment: "we copied the content to the NSData") |
| Every file on disk is reachable through `asset://` | [src-tauri/tauri.conf.json](../src-tauri/tauri.conf.json) | `"assetProtocol": { "enable": true, "scope": ["**"] }` |

**What was verified and what was inferred.** Every step in the table was confirmed by reading the exact
library sources pinned in `package-lock.json` / `Cargo.lock`. I have not yet reproduced the crash under a
profiler. Which process dies first (Videe's main process during the Rust read/copy, or WebKit's
networking or content process during `blob()` / `arrayBuffer()` / `decodeAudioData`) depends on file size
and free RAM, and is the first thing to confirm in the [verification plan](#verification-plan). The fix
is the same either way.

### Symptoms this explains

- **Crash or white-screen on large clips.** Memory use is roughly 4–5× the file size across processes.
  macOS overcommits, so the `Vec::with_capacity` usually succeeds and the failure arrives later as memory
  pressure, a jetsam kill, or the "Your system has run out of application memory" dialog.
- **The whole app freezes (beachball) when a clip is selected**, even for clips that don't crash. The full
  disk read and the 2N memcpy run on the main thread, which also runs the event loop and every other
  `asset://` request. This is likely the main contributor to the "video opening lag" this branch is
  named after.
- **Timecode shows `00:00:00:00` total duration for a long time.** `setDuration` is only driven by
  wavesurfer's `"ready"` event
  ([VideoScrubber.tsx:112](../src/components/VideoScrubber.tsx#L112)), and `"ready"` fires only after
  the full fetch and decode. If `decodeAudioData` rejects (clip with no audio track, or a container or
  codec WebKit's audio decoder doesn't handle, such as MXF), `"ready"` never fires and the duration stays
  zero. Nothing subscribes to wavesurfer's `"error"` event, so this fails silently.
- **Switching clips quickly makes things worse rather than better.** `key={clip.id}` remounts the scrubber
  and `wavesurfer.destroy()` aborts the JS `fetch`, but the Rust handler is already inside a blocking
  `read_to_end` and can't be cancelled. Each abandoned selection still costs a full read and copy on the
  main thread.
- **Cloud-synced libraries.** Videe targets team-synced folders. For online-only placeholders
  (Dropbox / Google Drive / iCloud "optimize storage"), reading the full file forces a full download,
  and that happens on the main thread.

## Design choices that led here

None of these were unreasonable on their own. The combination is what fails.

1. **The waveform was treated as a frontend rendering detail rather than a derived asset.** Videe already
   has the right pattern for derived media data: thumbnails are generated once by the ffmpeg sidecar during
   `scan_library` and cached under `app_local_data_dir()/thumbnails`. The waveform was instead left to
   wavesurfer's default "give me a URL and I'll work it out" mode.
2. **wavesurfer.js is an audio-file library being used on video masters.** Its default load path assumes
   a few-MB MP3/WAV: fetch it all, decode it all. It is efficient for that, and pathological for a 20 GB
   ProRes `.mov`, where the audio track is a tiny fraction of the bytes but the whole container still has
   to be read and decoded. Binding it with `media:` (to get regions, drag-to-seek and the cursor for free,
   which was a good call for #1 native scrubbing) silently opted into the auto-load at construction,
   because the `<video>` already had a `src`.
3. **Local media is served through Tauri's general-purpose `asset://` protocol.** That protocol is built
   for small app assets. wry's custom-protocol API has no streaming response body (a response is one
   `Cow<[u8]>`), so an unranged request must materialise the whole file. Tauri registers it synchronously,
   so that work happens on the main thread. Ranged requests are capped at 1 MB, which hides the problem
   for `<video>` playback. The first client that doesn't send `Range` exposes it.
4. **The UI's duration comes from the waveform pipeline instead of from data we already have.**
   `ClipSummary.duration` (from ffprobe at scan time) and the `<video>` element's `loadedmetadata` are both
   available long before wavesurfer is ready. The scrubber already gates regions on the native element's
   duration (`hasSaneDuration`), but the displayed total still waits on wavesurfer.
5. **The ffmpeg sidecar is deliberately minimal** ([spec/17](17-static-ffmpeg-sidecars.md)). It has audio
   *decoders*, but no audio filters, raw PCM encoder/muxer or `pipe:` protocol, so it currently *cannot*
   produce waveform data. That was correct for its scope at the time. It is a constraint the fix below has
   to lift.
6. **Asset scope `"**"`.** This is not a cause of the crash. It does mean any `asset://` URL can make the
   main process read any file on disk in full, which widens the impact of point 3.

## Goals / non-goals

**Goals**
- Opening any clip, of any size, never triggers a full-file read. Only the `<video>` element's own capped
  (≤1 MB) range requests touch the file.
- Selecting a clip shows video, timecode and total duration immediately.
- Scrubbing, the playhead, marker regions (drag-create, resize, click-to-seek), I/O keys, frame stepping
  and play/pause all behave exactly as before.
- Clips with no audio, or audio the webview can't decode, behave the same as every other clip.

**Non-goals (here)**
- Drawing a real audio waveform. This is deferred to a separate ticket (Approach B is the suggested design).
- Changing the media protocol (Approach C). Deferred.
- Everything under [Related findings](#related-findings-not-being-touched).
- Changing how markers/regions work.

## Approaches considered

### A. Give wavesurfer a duration and flat peaks so it never fetches (**chosen, implemented**)

Pass `duration: clip.duration` and a trivial `peaks: [[0, 0]]` to `WaveSurfer.create`. With both
present, the constructor's auto-load calls `load(src, peaks, duration)`. `loadAudio` skips
`Fetcher.fetchBlob` whenever `channelData` is given (`wavesurfer.js:326`) and builds its buffer from the
peaks (`wavesurfer.js:371-373`). `setSrc` is a no-op because the `<video>` already has that src. Drive the displayed duration from `clip.duration` / `loadedmetadata`
instead of `"ready"`.

- ✅ Removes the crash and the main-thread freeze. Roughly a 10-line frontend change, no backend work.
- ✅ Regions, drag-to-seek, the cursor and keyboard shortcuts keep working. They only need a duration.
- ❌ No waveform: just a flat bar.
- ⚠️ `clip.duration` is `Option<f64>`, and `None` if ffprobe failed. In that case leave `duration` out:
  peaks alone still skip the fetch, and wavesurfer waits for the `<video>` element's `loadedmetadata`
  instead. The displayed total falls back to the native duration once it's finite (the existing
  `hasSaneDuration` logic).

### B. Backend-generated peaks, cached (**deferred to the waveform ticket**)

Add a Tauri command `get_waveform_peaks(library_root, id) -> Peaks` that:

1. Returns cached peaks from `app_local_data_dir()/waveforms/{id}.json` (or a compact binary form) if
   present and the clip's `content_hash` matches.
2. Otherwise runs the ffmpeg sidecar off the main thread:
   `ffmpeg -v error -i <clip> -vn -map 0:a:0 -ac 1 -ar 8000 -f s16le pipe:1`.
   It streams stdout and reduces it in Rust to min/max per bucket as it goes, so memory stays constant
   regardless of file size. Writes the cache, then returns the result.
3. Returns an explicit "no audio" result if there is no audio stream, so the UI can show a flat waveform
   without retrying.

The frontend constructs WaveSurfer with `peaks` + `duration` as in A. It then calls
`wavesurfer.setOptions({ peaks, duration })` when the command resolves. Both are required: setOptions
only rebuilds the buffer when given both (`wavesurfer.js:251-254`). Do this, behind
the existing `key={clip.id}` / cancellation pattern.

Design details:
- **When to compute.** Compute lazily on first open, not during `scan_library`. Extracting audio from an
  interleaved container reads most of the file. On a synced folder that would mean downloading every
  online-only clip during a scan, and scans would get much slower. Thumbnails only need one frame, so
  they stay in the scan. Optionally add a low-priority background prefetch later.
- **Resolution.** A fixed bucket count (e.g. 100 peaks/second, capped at ~50k buckets) keeps the payload
  small. A 2-hour clip at 100/s is 720k min/max pairs, so the cap matters. Downsample further when
  serialising over IPC.
- **Concurrency.** One in-flight extraction per clip id (a map of pending futures in `AppState`), and
  kill the ffmpeg child if the user navigates away and no other caller is waiting.
- **Invalidation.** Key on `content_hash` (already computed at scan time), so a replaced file regenerates.
  Also clean up stale entries in `prune_missing`, alongside thumbnails, which are currently never cleaned
  up either.
- **ffmpeg build changes** in [scripts/build-ffmpeg.sh](../scripts/build-ffmpeg.sh): add filters
  `aresample, aformat, anull, abuffer, abuffersink`, encoder `pcm_s16le`, muxer `s16le`, protocol `pipe`,
  and extend `verify()` to check each one. Size impact is small, and it stays LGPL-only. Consider adding
  more audio decoders that turn up in camera footage (`pcm_f32le`, `ac3`, `mp3`) since they now matter.

Trade-offs:
- ✅ Real waveform, constant memory, nothing heavy in the webview, reuses existing infrastructure
  (sidecar, cache dir, `content_hash`).
- ✅ Works for MXF and other containers the webview can't decode, because ffmpeg can.
- ❌ First open of a long clip has a visible delay before the waveform appears (I/O-bound: ~20 s for
  10 GB at 500 MB/s, longer on network drives). This is acceptable because video and scrubbing are usable
  immediately.
- ❌ Requires a sidecar rebuild (`npm run setup:ffmpeg -- --force`) and a new release.

### C. Replace `asset://` for media with an async, range-only protocol (**deferred**)

Register a custom `media://` scheme with `register_asynchronous_uri_scheme_protocol` that:
- does all file I/O on a worker thread (or `tokio::task::spawn_blocking`) and calls the responder from
  there, so the main thread never touches the disk;
- **always** answers with `206 Partial Content` and a capped chunk (e.g. 1–4 MB). It treats a request
  with no `Range` header as `bytes=0-` and never materialises a whole file;
- only serves paths that are inside the current library root, or are known clip paths from the cache,
  instead of the `"**"` scope. Thumbnails can keep using `asset://` scoped to the thumbnails dir.

- ✅ Removes this whole bug class, so a future library or plain `fetch()` can't reintroduce it.
- ✅ Takes per-chunk disk reads off the main thread during normal playback too. Those 1 MB synchronous
  reads stall the UI on slow, network or synced drives today, so this also helps the general "opening
  lag".
- ✅ Tightens the asset scope.
- ❌ Doesn't help on its own if something in the webview still insists on the whole file. wavesurfer
  would page through it chunk by chunk and still decode it all in the web process. Needs A or B.
- ⚠️ Must reproduce the parts of Tauri's handler we rely on: MIME sniffing, `Accept-Ranges`,
  `Content-Range`, `HEAD`, CORS origin header. Small, but needs tests (the handler is easy to unit test
  as a pure `Request -> Response` function, like `save_clip_metadata_impl`).

### D. Decode in the webview incrementally (WebCodecs / mp4box.js in a worker), rejected

Demux with ranged fetches, decode audio with WebCodecs `AudioDecoder`, and reduce to peaks in a worker.
This avoids backend changes but is a lot of code. WebKit's WebCodecs audio support is limited and varies
by macOS version, and there's no MXF demuxer. It duplicates what the bundled ffmpeg already does well.

### E. Drop the waveform entirely, rejected as the end state

A plain timeline (wavesurfer with flat peaks, or a hand-rolled bar) is all approach A is. Reasonable if
waveforms turn out not to matter to users of video footage, but audio cues are genuinely useful for finding
takes and claps when marking in/out. In effect, Approach A is this option while keeping wavesurfer for
scrubbing and markers, so the waveform ticket can add real peaks later without replacing the component.

## Chosen approach

**Approach A only.** wavesurfer stays as the scrub and marker surface, and is never asked to load audio.

Changes on this branch (frontend only, no backend or ffmpeg changes):

- [src/components/VideoScrubber.tsx](../src/components/VideoScrubber.tsx)
  - New prop `duration: number | null`, the scan-time ffprobe value.
  - `WaveSurfer.create` gets `peaks: FLAT_PEAKS` (`[[0]]`, a single silent sample), so `loadAudio`
    never calls `Fetcher.fetchBlob`. It also gets `duration` when the probe produced a positive number,
    so loading completes immediately. When `duration` is null, wavesurfer waits for the `<video>`
    element's `loadedmetadata`, and still never fetches.
  - The displayed total duration starts from the probed value and is replaced by the native `<video>`
    duration once that is finite (the existing `hasSaneDuration` check). The `"ready"` handler only
    sets the duration when it's finite and positive.
  - Subscribes to wavesurfer's `"error"` event and logs it, so load failures are no longer silent.
- [src/components/ClipDetailPanel.tsx](../src/components/ClipDetailPanel.tsx): passes
  `duration={clip.duration}`.

Why this is safe for the rest of the scrubber:
- An all-zero waveform is handled explicitly by the renderer. `calculateVerticalScale` returns the base
  scale when the max peak is 0, so `normalize: true` can't divide by zero.
- Seeking, regions and the cursor read `wavesurfer.getDuration()`. That returns the media element's
  duration, and falls back to the supplied `duration` while the media's is still `NaN`/`Infinity`.
  Region creation remains gated on `hasSaneDuration`, as before.
- The bar options (`waveColor`, `barWidth`, `normalize`, …) are left in place so the waveform ticket only
  needs to supply real peaks.

**Deferred:**
- Approach B, a real waveform, goes to its own ticket. Its frontend half is a call to
  `wavesurfer.setOptions({ peaks, duration })` once peaks are available.
- Approach C, the media protocol, is not scheduled. With A in place nothing in the app makes an unranged
  request for a clip. The residual issue is that the `<video>` element's capped range reads still happen
  on the main thread (see Related findings).

## Verification plan

Already done on this branch: `tsc --noEmit`, `npm test` (4/4 passing) and `vite build` all pass. None of
these exercise the scrubber, so the checks below need to be run manually in the app (`npm run tauri dev`).

**Test clips.** Make these with any full ffmpeg build (e.g. Homebrew; the bundled sidecar can't encode
video):

| Clip | Command | Purpose |
|---|---|---|
| Large | `ffmpeg -f lavfi -i testsrc2=size=3840x2160:rate=25 -f lavfi -i sine=frequency=440 -t 600 -c:v prores_ks -profile:v 3 -c:a pcm_s24le big.mov` (~10–15 GB) | The crash case |
| Medium | `ffmpeg -f lavfi -i testsrc2=size=1920x1080:rate=25 -f lavfi -i sine -t 300 -c:v libx264 -c:a aac medium.mp4` | Typical clip that previously stalled |
| No audio | `ffmpeg -f lavfi -i testsrc2=rate=25 -t 30 -c:v libx264 -an silent.mp4` | Previously a stuck `00:00:00:00` |
| Moov at end | `ffmpeg -f lavfi -i testsrc2=rate=25 -f lavfi -i sine -t 60 -c:v libx264 -c:a aac moov_end.mp4` (no `-movflags +faststart`) | Duration starts as `Infinity` in the webview |

Put them in a library folder and scan it.

**1. Baseline (optional, on `main`).** Open the large clip while watching Activity Monitor (`videe`,
`com.apple.WebKit.Networking`, `com.apple.WebKit.WebContent`). Note the memory growth, the beachball, and
which process dies. This confirms the part of the analysis that was inferred rather than reproduced.

**2. No full-file read (the fix).**
- Open each clip with Web Inspector's Network tab open (right-click → Inspect in a dev build). The only
  requests for the clip should be the `<video>` element's ranged requests (status 206, ≤1 MB each).
  There should be no request without a `Range` header and no 200 response for the clip URL.
- Opening the large clip: no process grows by more than ~100 MB, there is no beachball, and the video
  frame appears within a couple of seconds.
- Click quickly between the large clip and others five or six times. The app stays responsive and memory
  doesn't climb.

**3. Duration and timecode.**
- The total duration shows immediately and correctly for every clip, including the no-audio and
  moov-at-end clips.
- To test the null-probe path, temporarily rename the ffprobe sidecar and rescan. The total should start
  at `00:00:00:00` and then fill in from the video's own metadata within a moment.

**4. Scrubber regression check** (on the medium clip, and once on the large clip):
- Clicking and dragging on the bar seeks, and the red playhead tracks playback.
- Space and the Play button toggle playback.
- ←/→ step one frame and the frame counter updates.
- I/O create a point marker and then a range. Dragging on the bar creates a marker, and dragging its edges
  resizes it.
- Clicking a region seeks to its start. Clicking a marker in MarkerList seeks the player.
- Markers saved earlier reload in the right positions when you reopen the clip.
- Renaming or deleting a marker in MarkerList updates the bar.

**5. Errors are visible.** The Web Inspector console shows no `wavesurfer error:` entries for any test
clip. If one does appear, it means a load problem that was previously silent; record it on the waveform
ticket.

**Expected, not a bug:** the bar shows no waveform, only a flat strip. That is tracked on the waveform
ticket.

## Related findings (not being touched)

These came up during the investigation. They are recorded here so they aren't lost, but this branch
**does not change any of them**.

- **The audio waveform is not calculated.** After this change the bar is always flat. This is tracked on
  its own ticket, and Approach B above is the suggested design.
- **Media range requests are still served on the main thread.** The `<video>` element's ranged requests
  go through Tauri's synchronous `asset://` handler, which does a ≤1 MB disk read per request on the main
  thread. This is harmless on a local SSD, but may cause UI stutter while playing or scrubbing from slow,
  network or cloud-synced drives. Approach C addresses it if that turns out to be a problem.
- **Asset protocol scope is `"**"`.** Any `asset://` URL can read any file on disk. This should be narrowed
  to the library root and the thumbnails dir at some point. It is not exploitable without script execution
  in the webview, so it's low priority.
- **MXF and DNxHD/DNxHR can't play in WKWebView at all.** The scanner accepts `.mxf` and the sidecar can
  thumbnail it, but `<video>` will show nothing for those clips.
- **Thumbnails are never garbage-collected.** `prune_missing` removes cache rows but leaves
  `thumbnails/{id}.jpg` on disk.
- **`scan_library` probes every clip with ffprobe on every scan** (two processes per clip), even when
  `size`/`mtime`/`content_hash` are unchanged. This is not related to the crash, but it is the next
  obvious latency win for large libraries.
