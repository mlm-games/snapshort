//! SubRip captions: cues in, timed text clips out, and back again.

use miniter_domain::{
    BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, TextOverlay, TextStyle,
    Timestamp, TrackKind,
};

/// One caption cue.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cue {
    pub index: usize,
    /// Microseconds.
    pub start_us: i64,
    /// Microseconds.
    pub end_us: i64,
    pub text: String,
}

/// Parse an SRT document. Tolerant of BOM, CRLF, and missing sequence numbers,
/// like the player-side parser the renderer uses.
pub fn parse_srt(content: &str) -> Vec<Cue> {
    let content = content.trim_start_matches('\u{feff}');
    let mut cues = Vec::new();
    let mut block: Vec<&str> = Vec::new();
    for line in content.lines() {
        let line = line.trim_end_matches('\r');
        if line.trim().is_empty() {
            flush(&mut block, &mut cues);
            continue;
        }
        block.push(line);
    }
    flush(&mut block, &mut cues);
    cues
}

fn flush(block: &mut Vec<&str>, cues: &mut Vec<Cue>) {
    if block.is_empty() {
        return;
    }
    let mut lines = block.drain(..).collect::<Vec<_>>();
    // A leading index line is optional; the timing line is the anchor.
    let Some(timing_at) = lines.iter().position(|l| l.contains("-->")) else {
        return;
    };
    let Some((start_us, end_us)) = parse_timing(lines[timing_at]) else {
        return;
    };
    let text = lines[timing_at + 1..].join("\n");
    cues.push(Cue {
        index: cues.len() + 1,
        start_us,
        end_us,
        text,
    });
    lines.clear();
}

/// `00:00:01,000 --> 00:00:02,500` (comma or dot, spaces optional).
fn parse_timing(line: &str) -> Option<(i64, i64)> {
    let (from, to) = line.split_once("-->")?;
    Some((parse_stamp(from)?, parse_stamp(to)?))
}

/// `HH:MM:SS,mmm` to microseconds.
pub fn parse_stamp(stamp: &str) -> Option<i64> {
    let stamp = stamp.trim();
    let (time, millis) = match stamp.split_once([',', '.']) {
        Some((time, millis)) => (time, millis),
        None => (stamp, "0"),
    };
    let mut parts = time.split(':');
    let hours: i64 = parts.next()?.parse().ok()?;
    let minutes: i64 = parts.next()?.parse().ok()?;
    let seconds: i64 = parts.next()?.parse().ok()?;
    let millis: i64 = {
        // Char-bounded: slicing bytes panicked on multi-byte millis text.
        let digits: String = millis.chars().take(3).collect();
        format!("{digits:0<3}").parse().ok()?
    };
    Some(((hours * 3600 + minutes * 60 + seconds) * 1000 + millis) * 1000)
}

/// SRT text for one cue.
pub fn cue_to_srt(index: usize, cue: &Cue) -> String {
    format!(
        "{index}\n{} --> {}\n{}\n",
        stamp(cue.start_us),
        stamp(cue.end_us),
        cue.text
    )
}

/// Microseconds to `HH:MM:SS,mmm`.
pub fn stamp(us: i64) -> String {
    let us = us.max(0);
    let millis = (us / 1000) % 1000;
    let seconds = (us / 1_000_000) % 60;
    let minutes = (us / 60_000_000) % 60;
    let hours = us / 3_600_000_000;
    format!("{hours:02}:{minutes:02}:{seconds:02},{millis:03}")
}

/// Cues as timed text clips, ready for `EditCommand::AddClip`.
pub fn clips_from_cues(cues: &[Cue], style: TextStyle) -> Vec<Clip> {
    cues.iter()
        .map(|cue| Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::from_micros(cue.start_us),
            timeline_duration: MediaDuration::from_micros((cue.end_us - cue.start_us).max(1_000)),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros((cue.end_us - cue.start_us).max(1_000)),
            source_total_duration: MediaDuration::from_micros(
                (cue.end_us - cue.start_us).max(1_000),
            ),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Text(TextOverlay {
                text: cue.text.clone(),
                style: style.clone(),
            }),
            keyframes: KeyframeCurve::default(),
            blend_mode: BlendMode::Normal,
        })
        .collect()
}

/// The cues a timeline's timed text clips describe, for caption export.
pub fn export_srt(timeline: &miniter_domain::Timeline) -> String {
    let mut cues: Vec<Cue> = timeline
        .tracks
        .iter()
        .filter(|t| t.kind == TrackKind::Text)
        .flat_map(|t| &t.clips)
        .filter_map(|clip| match &clip.kind {
            ClipKind::Text(text) => Some(Cue {
                index: 0,
                start_us: clip.timeline_start.as_micros(),
                end_us: clip.timeline_end().as_micros(),
                text: text.text.clone(),
            }),
            _ => None,
        })
        .collect();
    cues.sort_by_key(|c| c.start_us);
    cues.iter()
        .enumerate()
        .map(|(index, cue)| cue_to_srt(index + 1, cue))
        .collect::<Vec<_>>()
        .join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE: &str = "\u{feff}1\r\n00:00:01,000 --> 00:00:02,500\r\nHello there\r\n\r\n2\r\n00:00:03,000 --> 00:00:04,000\r\nGeneral Kenobi\r\n\r\n";

    #[test]
    fn cues_parse_from_a_messy_file() {
        let cues = parse_srt(SAMPLE);
        assert_eq!(cues.len(), 2);
        assert_eq!(cues[0].start_us, 1_000_000);
        assert_eq!(cues[0].end_us, 2_500_000);
        assert_eq!(cues[0].text, "Hello there");
        assert_eq!(cues[1].start_us, 3_000_000);
        assert_eq!(cues[1].text, "General Kenobi");
    }

    #[test]
    fn multi_line_and_dot_separators_are_tolerated() {
        let cues = parse_srt("00:00:02.500 --> 00:00:03.000\nline one\nline two\n");
        assert_eq!(cues.len(), 1);
        assert_eq!(cues[0].text, "line one\nline two");
        assert_eq!(cues[0].start_us, 2_500_000);
    }

    #[test]
    fn broken_cues_are_dropped_not_fatal() {
        assert!(parse_srt("not a cue").is_empty());
        assert!(parse_srt("00:00:99,000 --> nonsense\nx").is_empty());
        assert!(parse_srt("").is_empty());
    }

    #[test]
    fn stamps_round_trip_at_millisecond_precision() {
        // SRT only carries milliseconds, so that is the round-trip granularity.
        for us in [0, 1_000, 999_000, 61_000_000, 3_661_999_000] {
            assert_eq!(parse_stamp(&stamp(us)), Some(us));
        }
        assert_eq!(parse_stamp(&stamp(999)), Some(0));
    }

    #[test]
    fn cues_become_clips_on_the_timeline() {
        let cues = parse_srt(SAMPLE);
        let clips = clips_from_cues(&cues, TextStyle::default());
        assert_eq!(clips.len(), 2);
        assert_eq!(clips[0].timeline_start.as_micros(), 1_000_000);
        assert_eq!(clips[0].timeline_duration.as_micros(), 1_500_000);
        match &clips[1].kind {
            ClipKind::Text(text) => assert_eq!(text.text, "General Kenobi"),
            other => panic!("expected a text clip, got {other:?}"),
        }
    }

    #[test]
    fn export_writes_one_block_per_cue() {
        let srt = export_srt(&timeline_with_titles());
        assert_eq!(srt.matches("-->").count(), 2);
        assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:02,000\nFirst\n"));
        assert!(srt.contains("2\n00:00:03,000 --> 00:00:05,000\nSecond\n"));
    }

    fn timeline_with_titles() -> miniter_domain::Timeline {
        let mut timeline = miniter_domain::Timeline::new();
        let mut track = miniter_domain::Track::new(TrackKind::Text, "T1");
        for (start, text) in [(0, "First"), (3_000_000, "Second")] {
            track.clips.push(Clip {
                id: ClipId::new(),
                timeline_start: Timestamp::from_micros(start),
                timeline_duration: MediaDuration::from_micros(2_000_000),
                source_start: MediaDuration::ZERO,
                source_end: MediaDuration::from_micros(2_000_000),
                source_total_duration: MediaDuration::from_micros(2_000_000),
                speed: 1.0,
                volume: 1.0,
                opacity: 1.0,
                muted: false,
                transition_in: None,
                transition_out: None,
                kind: ClipKind::Text(TextOverlay {
                    text: text.into(),
                    style: TextStyle::default(),
                }),
                keyframes: KeyframeCurve::default(),
                blend_mode: BlendMode::Normal,
            });
        }
        timeline.add_track(track);
        timeline
    }
}
