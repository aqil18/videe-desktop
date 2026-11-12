//! Builds export files from selected clips and their timestamps. Deliberately sticks
//! to CSV and CMX3600 EDL rather than FCPXML: FCPXML's resource/timecode schema is
//! easy to get subtly wrong, and a file that silently fails to import is worse than
//! one in a simpler format that's actually correct. Both formats import into
//! Premiere and Resolve -- EDL as a real timeline, CSV as a shot log/spreadsheet.
//!
//! EDL conform matches each event to media by source timecode, so the source
//! columns must use the clip's *embedded* timecode (an iPhone clip starts at
//! time-of-day, e.g. 14:32:10;05), not zero. Getting that wrong is what makes an
//! NLE show "Media Offline" for a clip whose file it found perfectly well.

use std::collections::HashSet;

/// A timestamp within a clip, in seconds from the clip's own start.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTimestamp {
    pub seconds: f64,
    /// Zero for a point timestamp.
    pub duration_seconds: f64,
    pub label: String,
    pub notes: String,
}

/// A clip's embedded start timecode as a frame count at its timecode base
/// (fps rounded to a whole number), plus whether its labels are drop-frame.
#[derive(Debug, Clone, Copy, PartialEq, Default)]
pub struct StartTimecode {
    pub frames: i64,
    pub drop_frame: bool,
}

/// One exported clip: a single source range (the whole clip) plus the timestamps
/// logged on it, sorted by time.
pub struct ExportRow {
    pub filename: String,
    pub tags: Vec<String>,
    pub in_seconds: f64,
    pub out_seconds: f64,
    pub fps: f64,
    /// Defaults to 00:00:00:00 when the file carries no timecode.
    pub start_timecode: StartTimecode,
    pub timestamps: Vec<ExportTimestamp>,
}

/// One line per timestamp. A clip with no timestamps still gets one line (with
/// the timestamp columns empty) so every selected clip shows up in the log.
pub fn build_csv(rows: &[ExportRow]) -> String {
    let mut out = String::from("filename,tags,timestamp,label\n");
    for row in rows {
        let prefix = format!("{},{}", csv_escape(&row.filename), csv_escape(&row.tags.join(";")));
        if row.timestamps.is_empty() {
            out.push_str(&prefix);
            out.push_str(",,\n");
        }
        for ts in &row.timestamps {
            out.push_str(&prefix);
            out.push(',');
            out.push_str(&format_hms(ts.seconds));
            out.push(',');
            out.push_str(&csv_escape(&ts.label));
            out.push('\n');
        }
    }
    out
}

fn csv_escape(field: &str) -> String {
    if field.contains(',') || field.contains('"') || field.contains('\n') {
        format!("\"{}\"", field.replace('"', "\"\""))
    } else {
        field.to_string()
    }
}

/// Human-readable HH:MM:SS.mmm -- CSV is a data export, not tied to a frame rate.
fn format_hms(seconds: f64) -> String {
    let seconds = seconds.max(0.0);
    let millis = ((seconds.fract()) * 1000.0).round() as i64;
    let total_secs = seconds.trunc() as i64;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    format!("{h:02}:{m:02}:{s:02}.{millis:03}")
}

// ---- Timecode ----

/// Timecode counts whole frames per second: 29.97 counts at 30, 23.976 at 24.
fn timebase(fps: f64) -> i64 {
    fps.round().max(1.0) as i64
}

fn seconds_to_frames(seconds: f64, fps: f64) -> i64 {
    (seconds.max(0.0) * fps).round() as i64
}

/// Drop-frame only exists for 29.97 and 59.94 (timebase 30/60): it skips 2 (or 4)
/// frame *labels* at the start of every minute except each tenth minute.
fn drop_frame_applies(timebase: i64) -> bool {
    timebase == 30 || timebase == 60
}

/// Parses an embedded timecode tag ("14:32:10:05", or "14:32:10;05" for
/// drop-frame) into a frame count. `None` for anything malformed, so the caller
/// can fall back to zero rather than writing nonsense into the EDL.
pub fn parse_timecode(raw: &str, fps: f64) -> Option<StartTimecode> {
    let raw = raw.trim();
    let tb = timebase(fps);
    let drop_frame = drop_frame_applies(tb) && (raw.contains(';') || raw.contains('.'));
    let parts: Vec<i64> = raw
        .split([':', ';', '.'])
        .map(|p| p.parse::<i64>().ok())
        .collect::<Option<_>>()?;
    let [h, m, s, f] = parts[..] else { return None };
    if h < 0 || !(0..60).contains(&m) || !(0..60).contains(&s) || !(0..tb).contains(&f) {
        return None;
    }

    let mut frames = ((h * 60 + m) * 60 + s) * tb + f;
    if drop_frame {
        let drop = tb / 15;
        let total_minutes = h * 60 + m;
        frames -= drop * (total_minutes - total_minutes / 10);
    }
    Some(StartTimecode { frames, drop_frame })
}

/// HH:MM:SS:FF, or HH:MM:SS;FF for drop-frame.
fn format_frames(frames: i64, timebase: i64, drop_frame: bool) -> String {
    let mut frames = frames.max(0);
    let drop_frame = drop_frame && drop_frame_applies(timebase);
    if drop_frame {
        let drop = timebase / 15;
        let per_minute = timebase * 60 - drop;
        let per_ten_minutes = timebase * 600 - drop * 9;
        let tens = frames / per_ten_minutes;
        let rem = frames % per_ten_minutes;
        frames += drop * 9 * tens;
        if rem > drop {
            frames += drop * ((rem - drop) / per_minute);
        }
    }
    let f = frames % timebase;
    let total_secs = frames / timebase;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    let sep = if drop_frame { ';' } else { ':' };
    format!("{h:02}:{m:02}:{s:02}{sep}{f:02}")
}

// ---- EDL layout ----

/// Record timelines conventionally start at 01:00:00:00 (Resolve's and
/// Premiere's default), which keeps the marker EDL's positions lined up with a
/// timeline imported from the clip EDL.
const TIMELINE_START_HOURS: i64 = 1;

/// An EDL has a single record frame rate. Use the most common clip rate in the
/// selection (first one wins a tie) -- set the NLE project to match it.
fn timeline_fps(rows: &[ExportRow]) -> f64 {
    let mut best: Option<(f64, usize)> = None;
    for row in rows {
        let count = rows.iter().filter(|r| (r.fps - row.fps).abs() < 0.001).count();
        if best.is_none_or(|(_, c)| count > c) {
            best = Some((row.fps, count));
        }
    }
    best.map(|(fps, _)| fps).unwrap_or(25.0)
}

/// Where each clip lands on the record timeline: (record in, record out), in
/// timeline frames, laid back-to-back from TIMELINE_START_HOURS.
fn record_layout(rows: &[ExportRow], fps: f64) -> Vec<(i64, i64)> {
    let mut cursor = TIMELINE_START_HOURS * 3600 * timebase(fps);
    rows.iter()
        .map(|row| {
            let len = seconds_to_frames(row.out_seconds - row.in_seconds, fps);
            let span = (cursor, cursor + len);
            cursor += len;
            span
        })
        .collect()
}

/// CMX3600 reels are at most 8 characters. Keeps the filename stem's letters,
/// digits and underscores (so "IMG_1628.MOV" stays "IMG_1628"), and makes each
/// reel unique -- two clips sharing a truncated reel ("interview_01" and
/// "interview_02" both -> "INTERVIE") could be linked to the wrong file by an
/// NLE that matches on reel names.
fn reel_names(rows: &[ExportRow]) -> Vec<String> {
    let mut used = HashSet::new();
    rows.iter()
        .map(|row| {
            let stem = row.filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(&row.filename);
            let cleaned: String = stem
                .chars()
                .filter(|c| c.is_ascii_alphanumeric() || *c == '_')
                .map(|c| c.to_ascii_uppercase())
                .collect();
            let base = if cleaned.is_empty() { "CLIP".to_string() } else { cleaned };
            let mut candidate: String = base.chars().take(8).collect();
            let mut n = 2;
            while !used.insert(candidate.clone()) {
                let suffix = n.to_string();
                candidate = base.chars().take(8 - suffix.len()).collect::<String>() + &suffix;
                n += 1;
            }
            candidate
        })
        .collect()
}

/// Builds a CMX3600 EDL with one event per clip, laid out back-to-back on the
/// record timeline (so importing it gives you an assembled sequence, not just a
/// list). Source timecodes are offset by each clip's embedded start timecode so
/// conform can find the frames; record timecodes use one timeline frame rate.
/// Timestamps go in the separate marker EDL (see `build_marker_edl`).
pub fn build_edl(title: &str, rows: &[ExportRow]) -> String {
    let fps = timeline_fps(rows);
    let rec_tb = timebase(fps);
    let layout = record_layout(rows, fps);
    let reels = reel_names(rows);

    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    for (i, row) in rows.iter().enumerate() {
        let src_tb = timebase(row.fps);
        let start = row.start_timecode;
        let src_in = start.frames + seconds_to_frames(row.in_seconds, row.fps);
        let src_out = start.frames + seconds_to_frames(row.out_seconds, row.fps);
        let (rec_in, rec_out) = layout[i];

        out.push_str(&format!(
            "{:03}  {:<8} V     C        {} {} {} {}\n",
            i + 1,
            reels[i],
            format_frames(src_in, src_tb, start.drop_frame),
            format_frames(src_out, src_tb, start.drop_frame),
            format_frames(rec_in, rec_tb, false),
            format_frames(rec_out, rec_tb, false),
        ));
        out.push_str(&format!("* FROM CLIP NAME: {}\n", row.filename));
        if !row.tags.is_empty() {
            out.push_str(&format!("* TAGS: {}\n", row.tags.join(", ")));
        }
        out.push('\n');
    }
    out
}

/// `|` delimits fields in Resolve's marker comment line and a newline ends it.
fn marker_field(text: &str) -> String {
    text.chars().map(|c| if c.is_control() || c == '|' { ' ' } else { c }).collect()
}

/// Colour for exported timestamps. Blue matches the markers the "Send to
/// DaVinci" bridge adds, so both export paths look the same.
const MARKER_COLOR: &str = "ResolveColorBlue";

/// Timestamps as a DaVinci Resolve timeline-marker EDL (Timeline > Import >
/// Timeline Markers from EDL), in the exact shape Resolve itself exports: one
/// event per marker spanning its duration (at least one frame), followed by
/// `<notes> |C:<color> |M:<name> |D:<frames>`. Positions are on the record
/// timeline laid out by `build_edl`, so it lines up with that EDL's timeline.
/// `None` when no clip has a timestamp.
pub fn build_marker_edl(title: &str, rows: &[ExportRow]) -> Option<String> {
    if rows.iter().all(|r| r.timestamps.is_empty()) {
        return None;
    }
    let fps = timeline_fps(rows);
    let tb = timebase(fps);
    let layout = record_layout(rows, fps);

    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    let mut event = 0;
    for (row, (rec_in, _)) in rows.iter().zip(layout) {
        for ts in &row.timestamps {
            event += 1;
            let at = rec_in + seconds_to_frames(ts.seconds - row.in_seconds, fps);
            let len = seconds_to_frames(ts.duration_seconds, fps).max(1);
            let (tc_in, tc_out) = (format_frames(at, tb, false), format_frames(at + len, tb, false));
            out.push_str(&format!("{event:03}  001      V     C        {tc_in} {tc_out} {tc_in} {tc_out}  \n"));
            out.push_str(&format!(
                "{} |C:{MARKER_COLOR} |M:{} |D:{len}\n\n",
                marker_field(&ts.notes),
                marker_field(&ts.label)
            ));
        }
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn row(filename: &str, in_s: f64, out_s: f64) -> ExportRow {
        ExportRow {
            filename: filename.to_string(),
            tags: vec!["b-roll".to_string()],
            in_seconds: in_s,
            out_seconds: out_s,
            fps: 25.0,
            start_timecode: StartTimecode::default(),
            timestamps: Vec::new(),
        }
    }

    fn ts(seconds: f64, label: &str) -> ExportTimestamp {
        ExportTimestamp { seconds, duration_seconds: 0.0, label: label.to_string(), notes: String::new() }
    }

    #[test]
    fn csv_has_header_and_one_line_per_timestamp() {
        let mut r = row("a.mp4", 0.0, 10.0);
        r.timestamps = vec![ts(1.0, "Best take"), ts(4.5, "Clap")];
        let csv = build_csv(&[r]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some("filename,tags,timestamp,label"));
        assert_eq!(lines.next(), Some("a.mp4,b-roll,00:00:01.000,Best take"));
        assert_eq!(lines.next(), Some("a.mp4,b-roll,00:00:04.500,Clap"));
        assert_eq!(lines.next(), None);
    }

    #[test]
    fn csv_still_lists_clips_without_timestamps() {
        let csv = build_csv(&[row("a.mp4", 0.0, 10.0)]);
        assert_eq!(csv.lines().nth(1), Some("a.mp4,b-roll,,"));
    }

    #[test]
    fn csv_quotes_fields_containing_commas() {
        let mut r = row("a.mp4", 0.0, 1.0);
        r.tags = vec!["a,b".to_string()];
        r.timestamps = vec![ts(0.5, "wide, then close")];
        let csv = build_csv(&[r]);
        assert!(csv.contains("\"a,b\""));
        assert!(csv.contains("\"wide, then close\""));
    }

    #[test]
    fn timecode_rolls_over_correctly_at_fps_boundary() {
        assert_eq!(format_frames(25, 25, false), "00:00:01:00");
        assert_eq!(format_frames(0, 25, false), "00:00:00:00");
        assert_eq!(format_frames(seconds_to_frames(61.52, 25.0), 25, false), "00:01:01:13");
    }

    #[test]
    fn parses_non_drop_timecode_tags() {
        let tc = parse_timecode("01:00:00:00", 25.0).unwrap();
        assert_eq!(tc, StartTimecode { frames: 90_000, drop_frame: false });
        assert_eq!(format_frames(tc.frames, 25, false), "01:00:00:00");
    }

    #[test]
    fn drop_frame_timecode_round_trips() {
        // 29.97 DF skips labels ;00 and ;01 at each minute except every tenth.
        let tc = parse_timecode("00:01:00;02", 29.97).unwrap();
        assert_eq!(tc, StartTimecode { frames: 1800, drop_frame: true });
        assert_eq!(format_frames(1800, 30, true), "00:01:00;02");
        assert_eq!(format_frames(17_982, 30, true), "00:10:00;00");
        let iphone = parse_timecode("14:32:10;05", 29.97).unwrap();
        assert_eq!(format_frames(iphone.frames, 30, true), "14:32:10;05");
    }

    #[test]
    fn rejects_malformed_timecode_tags() {
        assert_eq!(parse_timecode("", 25.0), None);
        assert_eq!(parse_timecode("01:00:00", 25.0), None);
        assert_eq!(parse_timecode("01:00:00:25", 25.0), None);
        assert_eq!(parse_timecode("aa:00:00:00", 25.0), None);
    }

    #[test]
    fn edl_source_timecodes_start_at_the_clips_embedded_timecode() {
        let mut r = row("IMG_1628.MOV", 0.0, 10.0);
        r.fps = 30.0;
        r.start_timecode = parse_timecode("14:32:10:05", 30.0).unwrap();
        let edl = build_edl("Test Export", &[r]);
        assert!(edl.contains("14:32:10:05 14:32:20:05 01:00:00:00 01:00:10:00"), "{edl}");
    }

    #[test]
    fn edl_lays_events_back_to_back_from_one_hour() {
        let edl = build_edl("Test Export", &[row("a.mp4", 10.0, 15.0), row("b.mp4", 0.0, 3.0)]);
        assert!(edl.starts_with("TITLE: Test Export\n"));
        assert!(edl.contains("00:00:10:00 00:00:15:00 01:00:00:00 01:00:05:00"), "{edl}");
        assert!(edl.contains("00:00:00:00 00:00:03:00 01:00:05:00 01:00:08:00"), "{edl}");
        assert!(edl.contains("* FROM CLIP NAME: a.mp4"));
        assert!(edl.contains("* FROM CLIP NAME: b.mp4"));
    }

    #[test]
    fn edl_record_side_uses_one_frame_rate_for_mixed_clips() {
        // Two 25fps clips set the timeline rate; the 30fps clip's 2s still
        // occupies 50 timeline frames, keeping the record side contiguous.
        let mut b = row("b.mp4", 0.0, 2.0);
        b.fps = 30.0;
        let edl = build_edl("T", &[row("a.mp4", 0.0, 1.0), b, row("c.mp4", 0.0, 1.0)]);
        assert!(edl.contains("00:00:00:00 00:00:02:00 01:00:01:00 01:00:03:00"), "{edl}");
        assert!(edl.contains("01:00:03:00 01:00:04:00"), "{edl}");
    }

    #[test]
    fn edl_has_no_marker_lines() {
        let mut r = row("a.mp4", 0.0, 10.0);
        r.timestamps = vec![ts(2.0, "Best take")];
        let edl = build_edl("T", &[r]);
        assert!(!edl.contains("Best take"));
    }

    #[test]
    fn marker_edl_matches_resolves_own_format() {
        let first = row("a.mp4", 0.0, 10.0);
        let mut second = row("IMG_1628.MOV", 0.0, 9.56);
        second.timestamps = vec![
            ExportTimestamp {
                seconds: 1.6,
                duration_seconds: 0.0,
                label: "this is my name".to_string(),
                notes: "this is a note".to_string(),
            },
            ExportTimestamp { seconds: 2.0, duration_seconds: 2.0, label: "a|b".to_string(), notes: String::new() },
        ];
        let edl = build_marker_edl("marked", &[first, second]).unwrap();
        assert!(edl.contains(
            "001  001      V     C        01:00:11:15 01:00:11:16 01:00:11:15 01:00:11:16  \n\
             this is a note |C:ResolveColorBlue |M:this is my name |D:1\n"
        ), "{edl}");
        assert!(edl.contains(
            "002  001      V     C        01:00:12:00 01:00:14:00 01:00:12:00 01:00:14:00  \n \
             |C:ResolveColorBlue |M:a b |D:50\n"
        ), "{edl}");
    }

    #[test]
    fn marker_edl_is_none_without_timestamps() {
        assert!(build_marker_edl("T", &[row("a.mp4", 0.0, 1.0)]).is_none());
    }

    #[test]
    fn reel_names_keep_underscores_and_are_unique() {
        let reels = reel_names(&[
            row("IMG_1628.MOV", 0.0, 1.0),
            row("interview_01.mov", 0.0, 1.0),
            row("interview_02.mov", 0.0, 1.0),
            row("....mp4", 0.0, 1.0),
        ]);
        assert_eq!(reels, vec!["IMG_1628", "INTERVIE", "INTERVI2", "CLIP"]);
    }
}
