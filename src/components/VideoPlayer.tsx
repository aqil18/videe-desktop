import { forwardRef, useImperativeHandle, useRef, useState } from "react";
import { formatDuration } from "../lib/format";

export interface VideoPlayerHandle {
  seek: (seconds: number) => void;
}

interface VideoPlayerProps {
  src: string;
  onTimeUpdate?: (seconds: number) => void;
}

export const VideoPlayer = forwardRef<VideoPlayerHandle, VideoPlayerProps>(function VideoPlayer(
  { src, onTimeUpdate },
  ref,
) {
  const videoRef = useRef<HTMLVideoElement>(null);
  const [currentTime, setCurrentTime] = useState(0);
  const [duration, setDuration] = useState(0);

  useImperativeHandle(ref, () => ({
    seek(seconds: number) {
      if (videoRef.current) {
        videoRef.current.currentTime = seconds;
      }
    },
  }));

  return (
    <div className="flex flex-col gap-1">
      <video
        ref={videoRef}
        src={src}
        controls
        className="w-full rounded-md bg-black"
        onTimeUpdate={(e) => {
          const t = e.currentTarget.currentTime;
          setCurrentTime(t);
          onTimeUpdate?.(t);
        }}
        onLoadedMetadata={(e) => setDuration(e.currentTarget.duration)}
      />
      <div className="text-right text-xs tabular-nums text-neutral-500">
        {formatDuration(currentTime)} / {formatDuration(duration)}
      </div>
    </div>
  );
});
