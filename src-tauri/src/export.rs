//! Builds export files from selected clips and their timestamps. Deliberately sticks
//! to CSV and CMX3600 EDL rather than FCPXML: FCPXML's resource/timecode schema is
//! easy to get subtly wrong, and a file that silently fails to import is worse than
//! one in a simpler format that's actually correct. Both formats import into
//! Premiere and Resolve -- EDL as a real timeline, CSV as a shot log/spreadsheet.

/// A point in time within a clip, in seconds from the clip's own start.
#[derive(Debug, Clone, PartialEq)]
pub struct ExportTimestamp {
    pub seconds: f64,
    pub label: String,
}

/// One exported clip: a single source range (the whole clip) plus the timestamps
/// logged on it, sorted by time.
pub struct ExportRow {
    pub filename: String,
    pub tags: Vec<String>,
    pub in_seconds: f64,
    pub out_seconds: f64,
    pub fps: f64,
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

/// Frame-accurate HH:MM:SS:FF for EDL, which NLEs parse as non-drop-frame timecode.
fn format_timecode(seconds: f64, fps: f64) -> String {
    let fps_int = fps.round().max(1.0) as i64;
    let total_frames = (seconds.max(0.0) * fps).round() as i64;
    let frames = total_frames % fps_int;
    let total_secs = total_frames / fps_int;
    let (h, m, s) = (total_secs / 3600, (total_secs % 3600) / 60, total_secs % 60);
    format!("{h:02}:{m:02}:{s:02}:{frames:02}")
}

/// CMX3600 reel names are conventionally short and alphanumeric; NLEs use this to
/// match the EDL event back to the source clip by filename via the comment below,
/// so accuracy of the reel name itself doesn't matter beyond "not empty".
fn reel_name(filename: &str) -> String {
    let stem = filename.rsplit_once('.').map(|(s, _)| s).unwrap_or(filename);
    let cleaned: String = stem.chars().filter(|c| c.is_ascii_alphanumeric()).collect();
    let upper = cleaned.to_uppercase();
    if upper.is_empty() {
        "CLIP".to_string()
    } else {
        upper.chars().take(8).collect()
    }
}

/// Builds a CMX3600 EDL with one event per clip, laid out back-to-back on the
/// record timeline (so importing it gives you an assembled sequence, not just a
/// list). Each event carries the source clip name, tags, and timestamps as
/// comments. CMX3600 has no real marker syntax, so the "Send to DaVinci" bridge
/// adds the timestamps as actual Resolve markers separately after import.
pub fn build_edl(title: &str, rows: &[ExportRow]) -> String {
    let mut out = String::new();
    out.push_str(&format!("TITLE: {title}\n"));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    let mut record_seconds = 0.0_f64;
    for (i, row) in rows.iter().enumerate() {
        let event_num = i + 1;
        let duration = (row.out_seconds - row.in_seconds).max(0.0);
        let src_in = format_timecode(row.in_seconds, row.fps);
        let src_out = format_timecode(row.out_seconds, row.fps);
        let rec_in = format_timecode(record_seconds, row.fps);
        let rec_out = format_timecode(record_seconds + duration, row.fps);
        record_seconds += duration;

        out.push_str(&format!(
            "{event_num:03}  {:<8} V     C        {src_in} {src_out} {rec_in} {rec_out}\n",
            reel_name(&row.filename)
        ));
        out.push_str(&format!("* FROM CLIP NAME: {}\n", row.filename));
        for ts in &row.timestamps {
            out.push_str(&format!("* MARKER: {} {}\n", format_timecode(ts.seconds, row.fps), ts.label));
        }
        if !row.tags.is_empty() {
            out.push_str(&format!("* TAGS: {}\n", row.tags.join(", ")));
        }
        out.push('\n');
    }
    out
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
            timestamps: Vec::new(),
        }
    }

    fn ts(seconds: f64, label: &str) -> ExportTimestamp {
        ExportTimestamp { seconds, label: label.to_string() }
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
    fn edl_lists_timestamps_as_comments_on_their_clip() {
        let mut r = row("a.mp4", 0.0, 10.0);
        r.timestamps = vec![ts(2.0, "Best take")];
        let edl = build_edl("Test Export", &[r]);
        assert!(edl.contains("* MARKER: 00:00:02:00 Best take\n"));
    }

    #[test]
    fn timecode_rolls_over_correctly_at_fps_boundary() {
        // 25fps: frame 25 rolls into the next second, not "00:00:00:25".
        assert_eq!(format_timecode(1.0, 25.0), "00:00:01:00");
        assert_eq!(format_timecode(0.0, 25.0), "00:00:00:00");
        assert_eq!(format_timecode(61.52, 25.0), "00:01:01:13");
    }

    #[test]
    fn edl_lays_events_back_to_back_on_the_record_timeline() {
        let edl = build_edl("Test Export", &[row("a.mp4", 10.0, 15.0), row("b.mp4", 0.0, 3.0)]);
        assert!(edl.starts_with("TITLE: Test Export\n"));
        // First event: source 10-15s, record starts at 0 -> 0-5s.
        assert!(edl.contains("00:00:10:00 00:00:15:00 00:00:00:00 00:00:05:00"));
        // Second event: source 0-3s, but record continues from where the first left off (5s).
        assert!(edl.contains("00:00:00:00 00:00:03:00 00:00:05:00 00:00:08:00"));
        assert!(edl.contains("* FROM CLIP NAME: a.mp4"));
        assert!(edl.contains("* FROM CLIP NAME: b.mp4"));
    }

    #[test]
    fn reel_name_strips_extension_and_punctuation() {
        // "interview_01" -> alphanumeric-only, uppercased, truncated to 8 chars.
        assert_eq!(reel_name("interview_01.mov"), "INTERVIE");
        assert_eq!(reel_name("....mp4"), "CLIP");
    }
}
