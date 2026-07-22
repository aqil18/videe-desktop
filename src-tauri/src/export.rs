//! Builds export files from selected clips/markers. Deliberately sticks to CSV and
//! CMX3600 EDL rather than FCPXML: FCPXML's resource/timecode schema is easy to get
//! subtly wrong, and a file that silently fails to import is worse than one in a
//! simpler format that's actually correct. Both formats import into Premiere and
//! Resolve -- EDL as a real timeline, CSV as a shot log/spreadsheet.

/// One exportable range: either a clip's marker, or (if a clip has no markers) the
/// clip's full duration standing in for it so every selected clip is represented.
pub struct ExportRow {
    pub filename: String,
    pub tags: Vec<String>,
    pub marker_label: String,
    pub in_seconds: f64,
    pub out_seconds: f64,
    pub fps: f64,
}

pub fn build_csv(rows: &[ExportRow]) -> String {
    let mut out = String::from("filename,tags,marker,in,out\n");
    for row in rows {
        out.push_str(&csv_escape(&row.filename));
        out.push(',');
        out.push_str(&csv_escape(&row.tags.join(";")));
        out.push(',');
        out.push_str(&csv_escape(&row.marker_label));
        out.push(',');
        out.push_str(&format_hms(row.in_seconds));
        out.push(',');
        out.push_str(&format_hms(row.out_seconds));
        out.push('\n');
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

/// Builds a CMX3600 EDL with events laid out back-to-back on the record timeline
/// (so importing it gives you an assembled sequence of the marked ranges, not just
/// a list). Each event carries the source clip name, tags, and marker label as
/// comments, which Premiere/Resolve show as metadata on import.
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
        if !row.marker_label.is_empty() {
            out.push_str(&format!("* MARKER: {}\n", row.marker_label));
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

    fn row(filename: &str, marker_label: &str, in_s: f64, out_s: f64) -> ExportRow {
        ExportRow {
            filename: filename.to_string(),
            tags: vec!["b-roll".to_string()],
            marker_label: marker_label.to_string(),
            in_seconds: in_s,
            out_seconds: out_s,
            fps: 25.0,
        }
    }

    #[test]
    fn csv_has_header_and_one_row_per_export_row() {
        let csv = build_csv(&[row("a.mp4", "Best take", 1.0, 4.5)]);
        let mut lines = csv.lines();
        assert_eq!(lines.next(), Some("filename,tags,marker,in,out"));
        assert_eq!(lines.next(), Some("a.mp4,b-roll,Best take,00:00:01.000,00:00:04.500"));
    }

    #[test]
    fn csv_quotes_fields_containing_commas() {
        let mut r = row("a.mp4", "wide, then close", 0.0, 1.0);
        r.tags = vec!["a,b".to_string()];
        let csv = build_csv(&[r]);
        assert!(csv.contains("\"a,b\""));
        assert!(csv.contains("\"wide, then close\""));
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
        let edl = build_edl("Test Export", &[row("a.mp4", "", 10.0, 15.0), row("b.mp4", "", 0.0, 3.0)]);
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
