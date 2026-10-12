//! CMX3600 EDL export: the oldest and most widely understood cut list.

use miniter_domain::{ClipKind, Timeline, TrackKind};

use crate::Rate;

/// A CMX3600 EDL for the timeline.
pub fn export_edl(title: &str, timeline: &Timeline, rate: Rate) -> String {
    let mut out = String::new();
    out.push_str(&format!("TITLE: {}\n", sanitize(title)));
    out.push_str("FCM: NON-DROP FRAME\n\n");

    let mut event = 1u32;
    // Video events first, then each audio track, in CMX's track-letter order.
    for kind in [TrackKind::Video, TrackKind::Audio] {
        let designator = track_letter(kind);
        for track in timeline.tracks.iter().filter(|t| t.kind == kind) {
            for clip in &track.clips {
                let (reel, from, to) = match &clip.kind {
                    ClipKind::Video(v) => (
                        reel_name(&v.source_path),
                        clip.source_start.as_micros(),
                        clip.source_end.as_micros(),
                    ),
                    ClipKind::Audio(a) => (
                        reel_name(&a.source_path),
                        clip.source_start.as_micros(),
                        clip.source_end.as_micros(),
                    ),
                    // Text and subtitles are not EDL events.
                    _ => continue,
                };
                let record_in = clip.timeline_start.as_micros();
                let record_out = clip.timeline_end().as_micros();
                let transition = clip
                    .transition_in
                    .as_ref()
                    .map(|t| {
                        let kind = match t.kind {
                            miniter_domain::TransitionKind::CrossFade
                            | miniter_domain::TransitionKind::Dissolve => "D",
                            _ => "W",
                        };
                        let frames = frames_at(t.duration.as_micros(), rate);
                        format!("{kind} {frames:03}")
                    })
                    .unwrap_or_else(|| "C".to_string());
                out.push_str(&format!(
                    "{event:03}  {reel:8} {designator:5} {transition}    {src_in} {src_out} {rec_in} {rec_out}\n",
                    event = event,
                    reel = reel,
                    designator = designator,
                    transition = transition,
                    src_in = timecode(from, rate),
                    src_out = timecode(to, rate),
                    rec_in = timecode(record_in, rate),
                    rec_out = timecode(record_out, rate),
                ));
                out.push_str(&format!(
                    "* FROM CLIP NAME: {}\n",
                    name_of(match &clip.kind {
                        ClipKind::Video(v) => v.source_path.as_str(),
                        ClipKind::Audio(a) => a.source_path.as_str(),
                        _ => "",
                    })
                ));
                event += 1;
            }
        }
    }

    if event == 1 {
        out.push_str("* No cuts in the timeline.\n");
    }
    out
}

/// `HH:MM:SS:FF` at the sequence rate.
/// `HH:MM:SS:FF`. The seconds count whole seconds of *frame slots*, so a
/// 23.976 sequence labels 24 slots per second and its clock runs slightly
/// fast against wall time, exactly as NLEs write it.
pub fn timecode(us: i64, rate: Rate) -> String {
    let frames = frames_at(us, rate);
    // Slots per second, rounded: 24000/1001 labels 24 slots per second, and
    // truncating it to 23 would mislabel every frame of a 23.976 sequence.
    let fps = ((rate.num + rate.den / 2) / rate.den).max(1) as i64;
    let seconds = frames / fps;
    let f = frames % fps;
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    format!("{h:02}:{m:02}:{s:02}:{f:02}")
}

pub fn frames_at(us: i64, rate: Rate) -> i64 {
    let num = rate.num as i128;
    let den = rate.den as i128;
    ((us.max(0) as i128 * num) / (1_000_000 * den)) as i64
}

/// CMX reel names are 8 characters and uppercase.
fn reel_name(path: &str) -> String {
    let stem = name_of(path);
    let cleaned: String = stem
        .chars()
        .filter(|c| c.is_ascii_alphanumeric())
        .collect::<String>()
        .to_uppercase();
    let mut reel = cleaned;
    reel.truncate(8);
    if reel.is_empty() {
        reel.push_str("BL");
    }
    reel
}

fn name_of(path: &str) -> String {
    std::path::Path::new(path)
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("clip")
        .to_string()
}

fn track_letter(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "A",
        _ => "V",
    }
}

/// EDL titles have a fixed shape.
fn sanitize(title: &str) -> String {
    title
        .chars()
        .filter(|c| c.is_ascii_alphanumeric() || *c == ' ' || *c == '-' || *c == '_')
        .take(64)
        .collect::<String>()
        .trim()
        .to_string()
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniter_domain::{
        BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, Timeline, Timestamp,
        Track, TrackKind, VideoClip,
    };

    fn clip(start_us: i64, length_us: i64, path: &str) -> Clip {
        Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::from_micros(start_us),
            timeline_duration: MediaDuration::from_micros(length_us),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(length_us),
            source_total_duration: MediaDuration::from_micros(length_us * 4),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            markers: Vec::new(),
            kind: ClipKind::Video(VideoClip {
                source_path: path.into(),
                width: 1920,
                height: 1080,
                fps: 24.0,
                filters: Vec::new(),
                audio_filters: Vec::new(),
                masks: Vec::new(),
            }),
            keyframes: KeyframeCurve::default(),
            blend_mode: BlendMode::Normal,
        }
    }

    #[test]
    fn a_cut_list_has_one_event_per_clip() {
        let mut timeline = Timeline::new();
        let mut track = Track::new(TrackKind::Video, "V1");
        track.clips.push(clip(0, 1_000_000, "/tmp/wide shot.mp4"));
        track
            .clips
            .push(clip(1_000_000, 1_000_000, "/tmp/close up.mp4"));
        timeline.add_track(track);

        let edl = export_edl("My Cut", &timeline, Rate::new(24, 1));
        assert!(edl.starts_with("TITLE: My Cut\n"));
        assert!(
            edl.contains(
                "001  WIDESHOT V     C    00:00:00:00 00:00:01:00 00:00:00:00 00:00:01:00"
            )
        );
        assert!(
            edl.contains(
                "002  CLOSEUP  V     C    00:00:00:00 00:00:01:00 00:00:01:00 00:00:02:00"
            )
        );
        assert_eq!(edl.matches("* FROM CLIP NAME:").count(), 2);
    }

    #[test]
    fn timecode_falls_on_frame_boundaries() {
        assert_eq!(timecode(0, Rate::new(24, 1)), "00:00:00:00");
        assert_eq!(timecode(1_000_000, Rate::new(24, 1)), "00:00:01:00");
        assert_eq!(timecode(1_041_666, Rate::new(24, 1)), "00:00:01:00");
        assert_eq!(timecode(1_041_668, Rate::new(24, 1)), "00:00:01:01");
        // 23.976: 24 labelled slots per second; one real second holds 23 of them.
        assert_eq!(timecode(1_000_000, Rate::new(24_000, 1001)), "00:00:00:23");
        assert_eq!(timecode(1_041_666, Rate::new(24_000, 1001)), "00:00:01:00");
    }

    #[test]
    fn an_empty_timeline_is_still_a_valid_edl() {
        let edl = export_edl("Empty", &Timeline::new(), Rate::new(24, 1));
        assert!(edl.starts_with("TITLE: Empty"));
        assert!(edl.contains("* No cuts"));
    }

    #[test]
    fn dissolve_writes_a_transition_event() {
        let mut timeline = Timeline::new();
        let mut track = Track::new(TrackKind::Video, "V1");
        let mut first = clip(0, 2_000_000, "/tmp/one.mp4");
        first.transition_in = Some(miniter_domain::Transition::new(
            miniter_domain::TransitionKind::CrossFade,
            miniter_domain::MediaDuration::from_micros(500_000),
        ));
        track.clips.push(first);
        timeline.add_track(track);

        let edl = export_edl("Fade", &timeline, Rate::new(24, 1));
        assert!(
            edl.contains("001  ONE      V     D 012 "),
            "a dissolve carries its length: {edl}"
        );

        let mut plain = Timeline::new();
        let mut track = Track::new(TrackKind::Video, "V1");
        track.clips.push(clip(0, 1_000_000, "/tmp/two.mp4"));
        plain.add_track(track);
        assert!(export_edl("Cut", &plain, Rate::new(24, 1)).contains("001  TWO      V     C    "));
    }
}
