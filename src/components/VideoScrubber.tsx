import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import RegionsPlugin, { type Region } from "wavesurfer.js/plugins/regions";
import { colorForLabel } from "../lib/color";
import { formatFrameTimecode, frameNumber } from "../lib/format";
import type { Marker } from "../types";
import "./VideoScrubber.css";

export interface VideoScrubberHandle {
  seek: (seconds: number) => void;
}

interface VideoScrubberProps {
  src: string;
  fps: number;
  /** Probed by ffprobe at scan time; null if that probe failed. */
  duration: number | null;
  markers: Marker[];
  onMarkersChange: (markers: Marker[]) => void;
}

// Mirrors the app's Tailwind neutral scale as JS values, since wavesurfer
// renders the waveform to <canvas> -- these genuinely can't be Tailwind classes.
const WAVE_COLOR = "#525252"; // neutral-600
const PROGRESS_COLOR = "#a3a3a3"; // neutral-400
const CURSOR_COLOR = "#fafafa"; // neutral-50 -- bright so the playhead reads over any region fill

// A flat, silent waveform. Without `peaks`, wavesurfer fetch()es the entire
// media file and decodes it with Web Audio just to draw the waveform. For
// multi-GB video that means several full in-memory copies of the file (and a
// full read on the Tauri main thread), which freezes and then crashes the app
// (see spec/3-video-opening-lag.md). Supplying peaks skips that fetch
// entirely; everything we actually use wavesurfer for -- seeking, the
// playhead, timestamp regions -- only needs the duration.
const FLAT_PEAKS = [[0]];

export const VideoScrubber = forwardRef<VideoScrubberHandle, VideoScrubberProps>(function VideoScrubber(
  { src, fps, duration: probedDuration, markers, onMarkersChange },
  ref,
) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const waveformRef = useRef<HTMLDivElement>(null);
  const wavesurferRef = useRef<WaveSurfer | null>(null);
  const regionsRef = useRef<RegionsPlugin | null>(null);
  const markersRef = useRef(markers);

  const [currentTime, setCurrentTime] = useState(0);
  // Seeded from the scan-time probe so the total shows immediately; replaced by
  // the native element's own duration once that's known.
  const [duration, setDuration] = useState(probedDuration ?? 0);
  const [isPlaying, setIsPlaying] = useState(false);
  // WebKitGTK (Linux) has historically lagged Chromium/Safari on newer video
  // APIs -- feature-detect rather than assume parity across platforms.
  const [rvfcSupported] = useState(() => "requestVideoFrameCallback" in HTMLVideoElement.prototype);
  // Many MP4s (moov atom at the end -- common for non-"web-optimized" exports)
  // report video.duration as Infinity until the browser performs a real seek.
  // wavesurfer.getDuration() proxies the native element's duration directly,
  // and each Region caches it once at creation as its totalDuration -- if a
  // region is created while that's Infinity, start/Infinity=0 forever, so it
  // renders collapsed at position 0 no matter what happens afterward. Gate
  // region creation on the native element itself (not wavesurfer's "ready",
  // which is tied to full peak-decode and can lag far behind) so we never hit
  // that trap.
  const [hasSaneDuration, setHasSaneDuration] = useState(false);

  useEffect(() => {
    markersRef.current = markers;
  }, [markers]);

  useEffect(() => {
    const video = videoRef.current;
    if (!video) return;
    function checkDuration() {
      if (video && Number.isFinite(video.duration) && video.duration > 0) {
        setHasSaneDuration(true);
        setDuration(video.duration);
      }
    }
    checkDuration();
    video.addEventListener("loadedmetadata", checkDuration);
    video.addEventListener("durationchange", checkDuration);
    return () => {
      video.removeEventListener("loadedmetadata", checkDuration);
      video.removeEventListener("durationchange", checkDuration);
    };
  }, []);

  useImperativeHandle(ref, () => ({
    seek(seconds: number) {
      wavesurferRef.current?.setTime(seconds);
    },
  }));

  // Create wavesurfer + the regions plugin once per mount. The component is
  // keyed by clip.id at the call site (ClipDetailPanel), so a clip switch
  // fully remounts this rather than re-pointing a live instance at new
  // media -- far less fragile than wavesurfer's load()/setMediaElement() path.
  useEffect(() => {
    if (!videoRef.current || !waveformRef.current) return;

    const wavesurfer = WaveSurfer.create({
      container: waveformRef.current,
      media: videoRef.current,
      peaks: FLAT_PEAKS,
      // With peaks present wavesurfer never fetches. A known duration also lets
      // it finish loading immediately instead of waiting for loadedmetadata.
      duration: probedDuration && probedDuration > 0 ? probedDuration : undefined,
      height: 30,
      waveColor: WAVE_COLOR,
      progressColor: PROGRESS_COLOR,
      cursorColor: CURSOR_COLOR,
      cursorWidth: 2,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      normalize: true,
      dragToSeek: true,
    });
    const regions = wavesurfer.registerPlugin(RegionsPlugin.create());

    wavesurferRef.current = wavesurfer;
    regionsRef.current = regions;

    wavesurfer.on("timeupdate", (t) => setCurrentTime(t));
    wavesurfer.on("ready", (d) => {
      if (Number.isFinite(d) && d > 0) setDuration(d);
    });
    // Load errors were previously swallowed, leaving a blank bar and a 0 total.
    wavesurfer.on("error", (err) => console.error("wavesurfer error:", err));
    wavesurfer.on("play", () => setIsPlaying(true));
    wavesurfer.on("pause", () => setIsPlaying(false));

    // Fires once per completed drag of a timestamp -- NOT the continuous
    // "region-update" -- so this is the only place regions ever reach back into
    // `markers` state, and it always carries the settled final position.
    // Timestamps are points, so in and out always move together.
    regions.on("region-updated", (region: Region) => {
      onMarkersChange(
        markersRef.current.map((m) =>
          m.id === region.id ? { ...m, inSeconds: region.start, outSeconds: region.start } : m,
        ),
      );
    });

    regions.on("region-clicked", (region: Region, e: MouseEvent) => {
      e.stopPropagation();
      wavesurfer.setTime(region.start);
    });

    return () => {
      wavesurfer.destroy();
      wavesurferRef.current = null;
      regionsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync `markers` (the source of truth, owned by ClipDetailPanel) -> wavesurfer
  // Regions whenever they change: the T key handler below, MarkerList's label
  // edits/deletes, or the initial async fetch resolving. setOptions() only emits
  // "render" internally, never "region-update"/"region-updated", so this can't
  // loop through the region-updated listener above.
  //
  // Every timestamp renders as a point region (start === end) at inSeconds.
  // That includes in/out ranges logged by older builds, which show up as a
  // timestamp at their in-point.
  useEffect(() => {
    const regions = regionsRef.current;
    if (!regions || !hasSaneDuration) return;

    const existing = new Map(regions.getRegions().map((r) => [r.id, r]));
    for (const marker of markers) {
      const region = existing.get(marker.id);
      const color = colorForLabel(marker.label);
      const t = marker.inSeconds;
      if (!region) {
        regions.addRegion({ id: marker.id, start: t, end: t, color, resize: false });
      } else {
        if (region.start !== t || region.end !== t) {
          region.setOptions({ start: t, end: t });
        }
        if (region.color !== color) {
          region.setOptions({ color });
        }
        existing.delete(marker.id);
      }
    }
    // Whatever's left had its timestamp removed elsewhere (e.g. MarkerList's
    // delete button) -- drop the now-orphaned region to match.
    for (const stale of existing.values()) {
      stale.remove();
    }
  }, [markers, hasSaneDuration]);

  function stepFrame(direction: 1 | -1) {
    const video = videoRef.current;
    if (!video) return;
    if (!video.paused) video.pause();

    const frameDuration = 1 / fps;
    const target = Math.max(0, video.currentTime + direction * frameDuration);

    if (rvfcSupported) {
      video.currentTime = target;
      video.requestVideoFrameCallback((_now, metadata) => {
        // Use the actually-painted frame's time, not the seek target we just
        // requested -- currentTime alone isn't reliable confirmation on
        // compressed/long-GOP video (see the "precise frame mode" note below).
        setCurrentTime(metadata.mediaTime);
      });
    } else {
      const onSeeked = () => {
        video.removeEventListener("seeked", onSeeked);
        setCurrentTime(video.currentTime);
      };
      video.addEventListener("seeked", onSeeked);
      video.currentTime = target;
    }
  }

  // T drops a timestamp, ArrowLeft/ArrowRight step one frame -- all on
  // whichever clip is currently loaded. No native <video controls>, so the
  // waveform plus this listener are the only scrub/seek surface; that
  // sidesteps Chromium/WebKit intercepting arrow keys for native ~5-10s
  // seeking when a focused video element has default controls.
  useEffect(() => {
    function onKeyDown(e: KeyboardEvent) {
      if (e.metaKey || e.ctrlKey || e.altKey) return;
      const tag = (e.target as HTMLElement | null)?.tagName;
      if (tag === "INPUT" || tag === "TEXTAREA") return;

      const wavesurfer = wavesurferRef.current;
      if (!wavesurfer) return;
      
      if (e.code === "Space") {
        e.preventDefault(); // stops page scroll and a focused button also firing
        if (!e.repeat) wavesurfer.playPause(); // holding space won't rapidly toggle
        return;
      }
      if (e.key === "ArrowLeft") {
        e.preventDefault();
        stepFrame(-1);
        return;
      }
      if (e.key === "ArrowRight") {
        e.preventDefault();
        stepFrame(1);
        return;
      }

      if (e.key.toLowerCase() === "t") {
        e.preventDefault();
        if (e.repeat) return; // holding T shouldn't spray timestamps
        const t = wavesurfer.getCurrentTime();
        const timestamp: Marker = {
          id: crypto.randomUUID(),
          label: `Timestamp ${markersRef.current.length + 1}`,
          inSeconds: t,
          outSeconds: t,
          notes: "",
        };
        onMarkersChange([...markersRef.current, timestamp]);
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fps, rvfcSupported]);

  return (
    <div className="flex flex-col gap-1">
      <video ref={videoRef} src={src} className="w-full rounded-t-md bg-black" />
      <div ref={waveformRef} className="videe-waveform rounded-b-md bg-neutral-900" />
      <div className="flex items-center justify-between gap-2 text-xs">
        <div className="flex items-center gap-2 text-neutral-500">
          <button
            onClick={() => wavesurferRef.current?.playPause()}
            className="rounded bg-neutral-800 px-2 py-1 text-neutral-100 transition hover:bg-neutral-700"
          >
            {isPlaying ? "Pause" : "Play"}
          </button>
          <span className="tabular-nums">
            {formatFrameTimecode(currentTime, fps)} / {formatFrameTimecode(duration, fps)}
          </span>
          <span className="tabular-nums text-neutral-600">frame {frameNumber(currentTime, fps)}</span>
        </div>
        {!rvfcSupported && (
          <span className="text-yellow-600" title="requestVideoFrameCallback isn't available on this platform">
            Approximate frame stepping
          </span>
        )}
      </div>
      <p className="text-xs text-neutral-600">
        Press T to add a timestamp. ← → step one frame. Drag a timestamp on the bar to move it.
      </p>
    </div>
  );
});
