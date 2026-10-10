//! FCPXML export: the format Final Cut Pro, Premiere and Resolve read.

use miniter_domain::{ClipKind, Timeline, Timestamp, TrackKind};

use crate::Rate;

/// The project exported as FCPXML 1.10.
pub fn export_fcpxml(
    project_name: &str,
    timeline: &Timeline,
    rate: Rate,
    source_names: &dyn Fn(&str) -> String,
) -> String {
    let rate = rate.reduced();
    let (width, height) = frame_size(timeline);
    let format_id = "r1";
    let mut out = String::new();
    out.push_str("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
    out.push_str("<!DOCTYPE fcpxml>\n");
    out.push_str("<fcpxml version=\"1.10\">\n");

    // Resources: one format, one asset per distinct source file.
    let mut assets: Vec<String> = Vec::new();
    let mut seen: Vec<&str> = Vec::new();
    for track in &timeline.tracks {
        for clip in &track.clips {
            let (path, has_video, has_audio) = match &clip.kind {
                ClipKind::Video(v) => (v.source_path.as_str(), true, true),
                ClipKind::Audio(a) => (a.source_path.as_str(), false, true),
                _ => continue,
            };
            if seen.contains(&path) {
                continue;
            }
            seen.push(path);
            let name = source_names(path);
            let id = format!("r{}", seen.len() + 1);
            let duration = max_duration_ms(timeline);
            assets.push(format!(
                "    <asset id=\"{id}\" name=\"{name}\" src=\"{src}\" start=\"0s\" duration=\"{duration}ms\" hasVideo=\"{video}\" hasAudio=\"{audio}\" format=\"{format_id}\">\n      <media-rep kind=\"original-media\" signature=\"{name}\" src=\"{src}\"/>\n    </asset>",
                name = xml_escape(&name),
                src = xml_escape(&file_url(path)),
                duration = duration,
                video = has_video as u8,
                audio = has_audio as u8,
            ));
        }
    }

    out.push_str("  <resources>\n");
    let fps = (rate.num / rate.den.max(1)).max(1);
    out.push_str(&format!(
        "    <format id=\"{format_id}\" name=\"FFVideoFormat{kind}{height}p{fps}\" frameDuration=\"1/{num}s\" width=\"{width}\" height=\"{height}\"/>\n",
        kind = if height >= 2000 { "U" } else { "" },
        num = rate.num,
    ));
    for asset in &assets {
        out.push_str(asset);
        out.push('\n');
    }
    out.push_str("  </resources>\n");

    out.push_str("  <library>\n");
    out.push_str(&format!(
        "    <event name=\"{name}\">\n      <project name=\"{name}\">\n",
        name = xml_escape(project_name),
    ));
    let total = total_frames(timeline, rate);
    out.push_str(&format!(
        "        <sequence format=\"{format_id}\" duration=\"{total}/{num}s\" tcStart=\"0s\" tcFormat=\"{tc}\" audioLayout=\"stereo\" audioRate=\"48k\">\n",
        total = total,
        num = rate.num,
        tc = "NDF",
    ));

    // The primary storyline is the first video track; later tracks attach to
    // it as lanes, audio below.
    let primary = timeline
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Video);
    let mut audio_lane = 0;
    let mut video_lane = 0;
    out.push_str("          <spine>\n");
    for (index, track) in timeline.tracks.iter().enumerate() {
        for clip in &track.clips {
            let (lane, offset) = match track.kind {
                TrackKind::Video if Some(index) == primary => (String::new(), String::new()),
                TrackKind::Video => {
                    video_lane += 1;
                    (format!(" lane=\"{video_lane}\""), String::new())
                }
                TrackKind::Audio => {
                    audio_lane -= 1;
                    (format!(" lane=\"{audio_lane}\""), String::new())
                }
                _ => (String::new(), String::new()),
            };
            match &clip.kind {
                ClipKind::Video(v) => {
                    let id = asset_id_for(&seen, &v.source_path);
                    out.push_str(&format!(
                        "            <asset-clip name=\"{name}\" ref=\"{id}\"{lane}{offset} offset=\"{start}/{num}s\" start=\"{source_start}/{num}s\" duration=\"{dur}/{num}s\" audioRole=\"dialogue\" format=\"{format_id}\"/>\n",
                        name = xml_escape(&clip_name(v.source_path.as_str(), source_names)),
                        id = id,
                        lane = lane,
                        offset = offset,
                        start = frames_at(clip.timeline_start, rate),
                        source_start = frames_at_us(clip.source_start.as_micros(), rate),
                        dur = frames_at_us(clip.timeline_duration.as_micros(), rate),
                        num = rate.num,
                    ));
                }
                ClipKind::Audio(a) => {
                    let id = asset_id_for(&seen, &a.source_path);
                    out.push_str(&format!(
                        "            <asset-clip name=\"{name}\" ref=\"{id}\"{lane} offset=\"{start}/{num}s\" start=\"{source_start}/{num}s\" duration=\"{dur}/{num}s\" audioRole=\"music\" format=\"{format_id}\"/>\n",
                        name = xml_escape(&clip_name(a.source_path.as_str(), source_names)),
                        id = id,
                        lane = lane,
                        start = frames_at(clip.timeline_start, rate),
                        source_start = frames_at_us(clip.source_start.as_micros(), rate),
                        dur = frames_at_us(clip.timeline_duration.as_micros(), rate),
                        num = rate.num,
                    ));
                }
                ClipKind::Text(t) => {
                    out.push_str(&format!(
                        "            <title name=\"{name}\" offset=\"{start}/{num}s\" duration=\"{dur}/{num}s\" lane=\"{lane}\">\n              <text><text-style ref=\"ts{index}\">{body}</text-style></text>\n            </title>\n",
                        name = xml_escape(&t.text),
                        start = frames_at(clip.timeline_start, rate),
                        dur = frames_at_us(clip.timeline_duration.as_micros(), rate),
                        lane = video_lane + 1,
                        index = index,
                        body = xml_escape(&t.text),
                        num = rate.num,
                    ));
                }
                ClipKind::Subtitle(s) => {
                    out.push_str(&format!(
                        "            <caption offset=\"{start}/{num}s\" duration=\"{dur}/{num}s\" lane=\"{lane}\" name=\"{name}\"/>\n",
                        start = frames_at(clip.timeline_start, rate),
                        dur = frames_at_us(clip.timeline_duration.as_micros(), rate),
                        lane = video_lane + 2,
                        name = xml_escape(s.source_path.as_str()),
                        num = rate.num,
                    ));
                }
                _ => {}
            }
        }
    }
    out.push_str("          </spine>\n");
    out.push_str("        </sequence>\n");
    out.push_str("      </project>\n");
    out.push_str("    </event>\n");
    out.push_str("  </library>\n");
    out.push_str("</fcpxml>\n");
    out
}

fn asset_id_for(seen: &[&str], path: &str) -> String {
    match seen.iter().position(|p| *p == path) {
        Some(index) => format!("r{}", index + 2),
        None => "r2".to_string(),
    }
}

fn clip_name(path: &str, names: &dyn Fn(&str) -> String) -> String {
    names(path)
}

fn frames_at(at: Timestamp, rate: Rate) -> i64 {
    frames_at_us(at.as_micros(), rate)
}

fn frames_at_us(us: i64, rate: Rate) -> i64 {
    let num = rate.num as i128;
    let den = rate.den as i128;
    ((us.max(0) as i128 * num) / (1_000_000 * den)) as i64
}

fn total_frames(timeline: &Timeline, rate: Rate) -> i64 {
    let end = timeline
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.timeline_end().as_micros())
        .max()
        .unwrap_or(0);
    frames_at_us(end, rate).max(1)
}

fn frame_size(timeline: &Timeline) -> (u32, u32) {
    timeline
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .find_map(|c| match &c.kind {
            ClipKind::Video(v) => Some((v.width.max(16), v.height.max(16))),
            _ => None,
        })
        .unwrap_or((1920, 1080))
}

fn max_duration_ms(_timeline: &Timeline) -> i64 {
    // Sources are probed on import; exporting "unknown" duration as a large
    // value keeps FCP from trimming the asset short.
    86_400_000
}

pub fn xml_escape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    for c in text.chars() {
        match c {
            '&' => out.push_str("&amp;"),
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            other => out.push(other),
        }
    }
    out
}

fn file_url(path: &str) -> String {
    if path.starts_with("file://") {
        return path.to_string();
    }
    format!("file://{}", path)
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniter_domain::{
        BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, TextOverlay, TextStyle,
        Timeline, Timestamp, Track, TrackKind, VideoClip,
    };

    fn timeline_with_two_clips() -> Timeline {
        let mut timeline = Timeline::new();
        let mut track = Track::new(TrackKind::Video, "V1");
        for start in [0i64, 1_000_000] {
            track.clips.push(Clip {
                id: ClipId::new(),
                timeline_start: Timestamp::from_micros(start),
                timeline_duration: MediaDuration::from_micros(1_000_000),
                source_start: MediaDuration::ZERO,
                source_end: MediaDuration::from_secs(1.0),
                source_total_duration: MediaDuration::from_secs(10.0),
                speed: 1.0,
                volume: 1.0,
                opacity: 1.0,
                muted: false,
                transition_in: None,
                transition_out: None,
                kind: ClipKind::Video(VideoClip {
                    source_path: "/tmp/clip.mp4".into(),
                    width: 1920,
                    height: 1080,
                    fps: 24.0,
                    filters: Vec::new(),
                    audio_filters: Vec::new(),
                    masks: Vec::new(),
                }),
                keyframes: KeyframeCurve::default(),
                blend_mode: BlendMode::Normal,
            });
        }
        timeline.add_track(track);
        timeline
    }

    #[test]
    fn fcpxml_is_well_formed_and_placed() {
        let timeline = timeline_with_two_clips();
        let xml = export_fcpxml("Cut", &timeline, Rate::new(24, 1), &|_| "clip.mp4".into());
        assert!(xml.starts_with("<?xml"));
        assert!(xml.contains("<fcpxml version=\"1.10\">"));
        assert!(xml.contains("</fcpxml>"));
        assert!(xml.contains("<asset-clip"));
        assert!(xml.contains("offset=\"0/24s\""));
        assert!(
            xml.contains("offset=\"24/24s\""),
            "the second clip starts at frame 24"
        );
        assert!(xml.contains("duration=\"24/24s\""));
        // Every tag opened is closed.
        for tag in [
            "fcpxml",
            "resources",
            "library",
            "event",
            "project",
            "sequence",
            "spine",
        ] {
            let open = format!("<{tag}");
            let close = format!("</{tag}>");
            assert_eq!(
                xml.matches(&open).count(),
                xml.matches(&close).count(),
                "{tag} balanced"
            );
        }
    }

    #[test]
    fn special_characters_are_escaped() {
        let mut timeline = Timeline::new();
        let track = Track::new(TrackKind::Text, "T1");
        timeline.add_track(track);
        let mut timeline = timeline;
        timeline.tracks[0].clips.push(Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::ZERO,
            timeline_duration: MediaDuration::from_secs(2.0),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_secs(2.0),
            source_total_duration: MediaDuration::from_secs(2.0),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Text(TextOverlay {
                text: "Beat & <soul>".into(),
                style: miniter_domain::TextStyle::default(),
            }),
            keyframes: KeyframeCurve::default(),
            blend_mode: BlendMode::Normal,
        });
        let xml = export_fcpxml("A & B", &timeline, Rate::new(24, 1), &|_| "x".into());
        assert!(xml.contains("A &amp; B"), "project name escaped");
        assert!(
            xml.contains("Beat &amp; &lt;soul&gt;"),
            "title body escaped"
        );
        assert!(!xml.contains("<soul>"));
    }

    #[test]
    fn fractional_rates_reduce() {
        assert_eq!(Rate::from_f64(23.976).reduced(), Rate::new(24_000, 1001));
        assert_eq!(Rate::from_f64(30.0).reduced(), Rate::new(30, 1));
    }
}
