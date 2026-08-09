import { describe, expect, test } from "vitest";
import { formatFrameTimecode, frameNumber } from "./format";

// Mirrors src-tauri/src/export.rs's timecode_rolls_over_correctly_at_fps_boundary
// test so the frontend's displayed timecode never disagrees with what an
// exported EDL actually contains.
describe("formatFrameTimecode", () => {
  test("rolls over correctly at the fps boundary", () => {
    expect(formatFrameTimecode(1.0, 25)).toBe("00:00:01:00");
    expect(formatFrameTimecode(0.0, 25)).toBe("00:00:00:00");
    expect(formatFrameTimecode(61.52, 25)).toBe("00:01:01:13");
  });

  test("matches export.rs's non-drop-frame rollover at 29.97fps", () => {
    // 30000/1001 truncated to a plain fps value the way ffprobe reports it;
    // frame math still rounds fps to the nearest integer for the FF field.
    expect(formatFrameTimecode(1.0, 29.97)).toBe("00:00:01:00");
  });

  test("clamps negative input to zero rather than producing a negative timecode", () => {
    expect(formatFrameTimecode(-5, 25)).toBe("00:00:00:00");
  });
});

describe("frameNumber", () => {
  test("computes the rounded frame count at the given fps", () => {
    expect(frameNumber(1.0, 25)).toBe(25);
    expect(frameNumber(0.5, 24)).toBe(12);
  });
});
