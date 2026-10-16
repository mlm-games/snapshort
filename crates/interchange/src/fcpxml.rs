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
    out.push_str(&format!(
        "    <format id=\"{format_id}\" name=\"FFVideoFormat{kind}{height}p{fps}\" frameDuration=\"{den}/{num}s\" width=\"{width}\" height=\"{height}\"/>\n",
        kind = if height >= 2000 { "U" } else { "" },
        fps = fps_label(rate),
        num = rate.num,
        den = rate.den,
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
    let total = fcpxml_time(total_frames(timeline, rate), rate);
    out.push_str(&format!(
        "        <sequence format=\"{format_id}\" duration=\"{total}\" tcStart=\"0s\" tcFormat=\"{tc}\" audioLayout=\"stereo\" audioRate=\"48k\">\n",
        total = total,
        tc = "NDF",
    ));

    // The primary storyline is the first video track; later tracks attach to
    // it as lanes, audio below.
    let primary = timeline
        .tracks
        .iter()
        .position(|t| t.kind == TrackKind::Video);
    // Every clip off the primary storyline needs its own lane: a title on a
    // lane an attached video clip later reuses would overlap it and make the
    // document invalid.
    let mut audio_lane = 0;
    let mut video_lane = 0;
    let mut title_style_n = 0usize;
    out.push_str("          <spine>\n");
    for (index, track) in timeline.tracks.iter().enumerate() {
        for clip in &track.clips {
            let lane = match track.kind {
                TrackKind::Video if Some(index) == primary => String::new(),
                TrackKind::Video | TrackKind::Text | TrackKind::Subtitle => {
                    video_lane += 1;
                    format!(" lane=\"{video_lane}\"")
                }
                TrackKind::Audio => {
                    audio_lane -= 1;
                    format!(" lane=\"{audio_lane}\"")
                }
                _ => String::new(),
            };
            match &clip.kind {
                ClipKind::Video(v) => {
                    let id = asset_id_for(&seen, &v.source_path);
                    out.push_str(&format!(
                        "            <asset-clip name=\"{name}\" ref=\"{id}\"{lane} offset=\"{start}\" start=\"{source_start}\" duration=\"{dur}\" audioRole=\"dialogue\" format=\"{format_id}\"/>\n",
                        name = xml_escape(&clip_name(v.source_path.as_str(), source_names)),
                        id = id,
                        lane = lane,
                        start = fcpxml_time(frames_at(clip.timeline_start, rate), rate),
                        source_start =
                            fcpxml_time(frames_at_us(clip.source_start.as_micros(), rate), rate),
                        dur = fcpxml_time(
                            frames_at_us(clip.timeline_duration.as_micros(), rate),
                            rate
                        ),
                    ));
                }
                ClipKind::Audio(a) => {
                    let id = asset_id_for(&seen, &a.source_path);
                    out.push_str(&format!(
                        "            <asset-clip name=\"{name}\" ref=\"{id}\"{lane} offset=\"{start}\" start=\"{source_start}\" duration=\"{dur}\" audioRole=\"music\" format=\"{format_id}\"/>\n",
                        name = xml_escape(&clip_name(a.source_path.as_str(), source_names)),
                        id = id,
                        lane = lane,
                        start = fcpxml_time(frames_at(clip.timeline_start, rate), rate),
                        source_start =
                            fcpxml_time(frames_at_us(clip.source_start.as_micros(), rate), rate),
                        dur = fcpxml_time(
                            frames_at_us(clip.timeline_duration.as_micros(), rate),
                            rate
                        ),
                    ));
                }
                ClipKind::Text(t) => {
                    // The text-style the title references must exist, in a
                    // `text-style-def` of its own, or the document is invalid.
                    title_style_n += 1;
                    let style_id = format!("ts{title_style_n}");
                    out.push_str(&format!(
                        "            <title name=\"{name}\" offset=\"{start}\" duration=\"{dur}\"{lane}>\n              <text><text-style ref=\"{style_id}\">{body}</text-style></text>\n              <text-style-def id=\"{style_id}\">\n                <text-style font=\"{font}\" fontFace=\"{face}\" fontSize=\"{size}\"/>\n              </text-style-def>\n            </title>\n",
                        name = xml_escape(&t.text),
                        start = fcpxml_time(frames_at(clip.timeline_start, rate), rate),
                        dur = fcpxml_time(
                            frames_at_us(clip.timeline_duration.as_micros(), rate),
                            rate
                        ),
                        lane = lane,
                        style_id = style_id,
                        body = xml_escape(&t.text),
                        font = xml_escape(&t.style.font_family),
                        face = if t.style.bold {
                            "Bold"
                        } else if t.style.italic {
                            "Italic"
                        } else {
                            "Regular"
                        },
                        size = t.style.font_size.max(1.0),
                    ));
                }
                ClipKind::Subtitle(s) => {
                    out.push_str(&format!(
                        "            <caption offset=\"{start}\" duration=\"{dur}\"{lane} name=\"{name}\"/>\n",
                        start = fcpxml_time(frames_at(clip.timeline_start, rate), rate),
                        dur = fcpxml_time(
                            frames_at_us(clip.timeline_duration.as_micros(), rate),
                            rate
                        ),
                        lane = lane,
                        name = xml_escape(s.source_path.as_str()),
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

/// A time value: the frame count over the format's frame duration, so
/// `24/24000s` becomes `24*1001/24000s` at 23.976 and one second stays one
/// second.
fn fcpxml_time(frames: i64, rate: Rate) -> String {
    format!(
        "{}/{}s",
        frames.max(0) * i64::from(rate.den),
        rate.num
    )
}

/// The fps part of an FCP format name: `25` for 25, `2398` for 23.976.
fn fps_label(rate: Rate) -> String {
    if rate.den == 1 {
        rate.num.to_string()
    } else {
        (rate.as_f64() * 100.0).round().to_string()
    }
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
