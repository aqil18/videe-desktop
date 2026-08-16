import { forwardRef, useEffect, useImperativeHandle, useRef, useState } from "react";
import WaveSurfer from "wavesurfer.js";
import RegionsPlugin, { type Region } from "wavesurfer.js/plugins/regions";
import { formatFrameTimecode, frameNumber } from "../lib/format";
import type { Marker } from "../types";
import "./VideoScrubber.css";

export interface VideoScrubberHandle {
  seek: (seconds: number) => void;
}

interface VideoScrubberProps {
  src: string;
  fps: number;
  markers: Marker[];
  onMarkersChange: (markers: Marker[]) => void;
}

// Mirrors the app's Tailwind neutral scale as JS values, since wavesurfer
// renders the waveform to <canvas> -- these genuinely can't be Tailwind classes.
const WAVE_COLOR = "#525252"; // neutral-600
const PROGRESS_COLOR = "#a3a3a3"; // neutral-400
const CURSOR_COLOR = "#e5e5e5"; // neutral-200
const REGION_COLOR = "rgba(163, 163, 163, 0.25)"; // neutral-400 @ 25%

function makeRegionLabel(label: string): HTMLElement {
  const el = document.createElement("div");
  el.className = "rounded-sm bg-black/60 px-1 text-[10px] text-neutral-100";
  el.textContent = label;
  return el;
}

export const VideoScrubber = forwardRef<VideoScrubberHandle, VideoScrubberProps>(function VideoScrubber(
  { src, fps, markers, onMarkersChange },
  ref,
) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const waveformRef = useRef<HTMLDivElement>(null);
  const wavesurferRef = useRef<WaveSurfer | null>(null);
  const regionsRef = useRef<RegionsPlugin | null>(null);
  const markersRef = useRef(markers);
  // Guards the markers->regions sync effect from having its own addRegion()
  // calls mistaken for user-created regions by the region-created listener.
  const isApplyingExternalMarkers = useRef(false);

  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);
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
      height: 64,
      waveColor: WAVE_COLOR,
      progressColor: PROGRESS_COLOR,
      cursorColor: CURSOR_COLOR,
      cursorWidth: 1,
      barWidth: 2,
      barGap: 1,
      barRadius: 1,
      normalize: true,
      dragToSeek: true,
    });
    const regions = wavesurfer.registerPlugin(RegionsPlugin.create());
    const disableDragSelection = regions.enableDragSelection({ color: REGION_COLOR });

    wavesurferRef.current = wavesurfer;
    regionsRef.current = regions;

    wavesurfer.on("timeupdate", (t) => setCurrentTime(t));
    wavesurfer.on("ready", (d) => setDuration(d));
    wavesurfer.on("play", () => setIsPlaying(true));
    wavesurfer.on("pause", () => setIsPlaying(false));

    // Fires once per completed drag/resize gesture (or once for a
    // drag-to-create) -- NOT the continuous "region-update" -- so this is the
    // only place regions ever reach back into `markers` state, and it always
    // carries the settled final start/end.
    regions.on("region-created", (region: Region) => {
      if (isApplyingExternalMarkers.current) return;
      const marker: Marker = {
        id: region.id,
        label: `Marker ${markersRef.current.length + 1}`,
        inSeconds: region.start,
        outSeconds: region.end,
        notes: "",
      };
      onMarkersChange([...markersRef.current, marker]);
    });

    regions.on("region-updated", (region: Region) => {
      if (isApplyingExternalMarkers.current) return;
      onMarkersChange(
        markersRef.current.map((m) =>
          m.id === region.id ? { ...m, inSeconds: region.start, outSeconds: region.end } : m,
        ),
      );
    });

    regions.on("region-clicked", (region: Region, e: MouseEvent) => {
      e.stopPropagation();
      wavesurfer.setTime(region.start);
    });

    return () => {
      disableDragSelection();
      wavesurfer.destroy();
      wavesurferRef.current = null;
      regionsRef.current = null;
    };
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, []);

  // Sync `markers` (the source of truth, owned by ClipDetailPanel) -> wavesurfer
  // Regions whenever markers change from outside a region gesture: the I/O
  // keyboard handler below, MarkerList's label edits/deletes, or the initial
  // async marker fetch resolving. setOptions() only emits "render" internally,
  // never "region-update"/"region-updated", so this can't loop through the
  // region-updated listener above -- the guard exists solely to stop
  // addRegion() calls here (for markers not yet mirrored as regions) from
  // being mistaken for user-created regions by the region-created listener.
  useEffect(() => {
    const regions = regionsRef.current;
    if (!regions || !hasSaneDuration) return;

    isApplyingExternalMarkers.current = true;
    try {
      const existing = new Map(regions.getRegions().map((r) => [r.id, r]));
      for (const marker of markers) {
        const region = existing.get(marker.id);
        if (!region) {
          regions.addRegion({
            id: marker.id,
            start: marker.inSeconds,
            end: marker.outSeconds,
            color: REGION_COLOR,
            content: makeRegionLabel(marker.label),
          });
        } else {
          const wasPoint = region.start === region.end;
          const willBePoint = marker.inSeconds === marker.outSeconds;
          if (wasPoint !== willBePoint) {
            // wavesurfer's Region only builds its fill color and resize handles
            // once, at construction, based on start===end at that moment --
            // setOptions() repositions but never upgrades a point marker into a
            // filled region (or vice versa). Recreate so the DOM matches reality.
            region.remove();
            regions.addRegion({
              id: marker.id,
              start: marker.inSeconds,
              end: marker.outSeconds,
              color: REGION_COLOR,
              content: makeRegionLabel(marker.label),
            });
          } else {
            if (region.start !== marker.inSeconds || region.end !== marker.outSeconds) {
              region.setOptions({ start: marker.inSeconds, end: marker.outSeconds });
            }
            if (region.getContent(false) !== marker.label) {
              region.setContent(makeRegionLabel(marker.label));
            }
          }
          existing.delete(marker.id);
        }
      }
      // Whatever's left had its marker removed elsewhere (e.g. MarkerList's
      // delete button) -- drop the now-orphaned region to match.
      for (const stale of existing.values()) {
        stale.remove();
      }
    } finally {
      isApplyingExternalMarkers.current = false;
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

  // I marks in, O marks out, ArrowLeft/ArrowRight step one frame -- all on
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

      const key = e.key.toLowerCase();
      if (key === "i") {
        e.preventDefault();
        const t = wavesurfer.getCurrentTime();
        onMarkersChange([
          ...markersRef.current,
          { id: crypto.randomUUID(), label: `Marker ${markersRef.current.length + 1}`, inSeconds: t, outSeconds: t, notes: "" },
        ]);
      } else if (key === "o") {
        e.preventDefault();
        const t = wavesurfer.getCurrentTime();
        const prev = markersRef.current;
        if (prev.length === 0) return;
        onMarkersChange(
          prev.map((m, i) => (i === prev.length - 1 ? { ...m, outSeconds: Math.max(t, m.inSeconds) } : m)),
        );
      }
    }
    window.addEventListener("keydown", onKeyDown);
    return () => window.removeEventListener("keydown", onKeyDown);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [fps, rvfcSupported]);

  return (
    <div className="flex flex-col gap-1">
      <video ref={videoRef} src={src} className="w-full rounded-t-md bg-black" />
      <div ref={waveformRef} className="videee-waveform rounded-b-md bg-neutral-900" />
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
        Press I to mark in, O to mark out. ← → step one frame. Drag on the waveform to create a marker.
      </p>
    </div>
  );
});
