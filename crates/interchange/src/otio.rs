//! OpenTimelineIO JSON (`.otio`).
//!
//! Schemas written and read: `Timeline.1`, `Stack.1`, `Track.1`, `Clip.1`, `Gap.1`,
//! `Transition.1`, `LinearTimeWarp.1`, `ExternalReference.1`, `MissingReference.1` and
//! `SerializableCollection.1`, with `RationalTime.1` / `TimeRange.1` times at the timeline rate.
//! The rate is the exact rational written as a float (24000/1001 comes back as 23.976023976),
//! and sub-frame source times survive through the sidecar below.
//!
//! Snapshort's clip carries no markers, labels, effect stack or nested sequences, so the fields
//! OpenTimelineIO has no schema for -- media characteristics, keyframes, text style, blend mode,
//! transitions and track state -- round-trip through `metadata.snapshort`. Import reads the first
//! timeline in the document; the rate it declared comes back as each video clip's `fps`.

use miniter_domain::{
    AudioClip, BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, SubtitleClip,
    TextOverlay, TextStyle, Timeline, Timestamp, Track, TrackKind, Transition, TransitionKind,
    VideoClip,
};
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};

use crate::{InterchangeError, Rate};

const MAX_DEPTH: usize = 64;

pub fn export_otio(
    name: &str,
    timeline: &Timeline,
    rate: Rate,
    source_names: &dyn Fn(&str) -> String,
) -> Result<String, InterchangeError> {
    let rate = rate.reduced();
    let tracks: Vec<Value> = timeline
        .tracks
        .iter()
        .map(|track| track_json(track, rate, source_names))
        .collect();
    let document = json!({
        "OTIO_SCHEMA": "Timeline.1",
        "name": name,
        "global_start_time": rational(0, rate),
        "metadata": {},
        "tracks": {
            "OTIO_SCHEMA": "Stack.1",
            "name": name,
            "children": tracks,
            "source_range": Value::Null,
            "effects": [],
            "markers": [],
            "enabled": true,
            "metadata": {},
        },
    });
    serde_json::to_string_pretty(&document)
        .map_err(|e| InterchangeError::Other(format!("cannot write OpenTimelineIO: {e}")))
}

pub fn import_otio(text: &str) -> Result<Timeline, InterchangeError> {
    let document: Value = serde_json::from_str(text)
        .map_err(|e| InterchangeError::Other(format!("OpenTimelineIO parse error: {e}")))?;
    let mut timelines = Vec::new();
    collect_timelines(&document, &mut timelines, 0);
    let root = timelines
        .first()
        .copied()
        .ok_or_else(|| InterchangeError::Other("the document holds no timeline".to_string()))?;
    let rate = first_rate(root).unwrap_or_else(|| Rate::new(24, 1));
    let mut reader = Reader { rate };
    Ok(reader.timeline(root))
}

fn collect_timelines<'v>(value: &'v Value, out: &mut Vec<&'v Value>, depth: usize) {
    if depth > MAX_DEPTH {
        return;
    }
    match schema_name(value) {
        "Timeline" | "Stack" | "Track" => out.push(value),
        "SerializableCollection" => {
            for child in value
                .get("children")
                .and_then(Value::as_array)
                .into_iter()
                .flatten()
            {
                collect_timelines(child, out, depth + 1);
            }
        }
        _ => {}
    }
}

fn first_rate(value: &Value) -> Option<Rate> {
    if let Some(span) = value.get("source_range").filter(|s| !s.is_null())
        && let Some(rate) = span.get("duration").and_then(read_rational_rate)
    {
        return Some(rate);
    }
    if let Some(rate) = value.get("global_start_time").and_then(read_rational_rate) {
        return Some(rate);
    }
    value
        .get("children")
        .and_then(Value::as_array)
        .into_iter()
        .flatten()
        .find_map(first_rate)
}

struct Reader {
    rate: Rate,
}

impl Reader {
    fn timeline(&mut self, root: &Value) -> Timeline {
        let mut timeline = Timeline::new();
        let stack = match schema_name(root) {
            "Timeline" => root.get("tracks").unwrap_or(&Value::Null),
            _ => root,
        };
        for child in stack
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            if schema_name(child) == "Track"
                && let Some(track) = self.track(child)
            {
                timeline.add_track(track);
            }
        }
        timeline
    }

    fn track(&self, node: &Value) -> Option<Track> {
        let kind = if node.get("kind").and_then(Value::as_str) == Some("Audio") {
            TrackKind::Audio
        } else {
            TrackKind::Video
        };
        let name = name_of(node);
        let mut track = Track::new(
            kind,
            if name.is_empty() {
                default_track_name(kind).to_string()
            } else {
                name
            },
        );
        track.muted = !node.get("enabled").and_then(Value::as_bool).unwrap_or(true);
        track.locked = sidecar(node)
            .and_then(|m| m.get("locked"))
            .and_then(Value::as_bool)
            .unwrap_or(false);
        let mut position = 0i64;
        let mut pending: Option<Transition> = None;
        for child in node
            .get("children")
            .and_then(Value::as_array)
            .into_iter()
            .flatten()
        {
            match schema_name(child) {
                "Gap" => {
                    if let Some((_, duration)) = read_span(child.get("source_range")) {
                        position += duration.max(0);
                    }
                }
                "Transition" => match self.transition(child) {
                    Some((transition, false)) => pending = Some(transition),
                    Some((transition, true)) => {
                        if let Some(last) = track.clips.last_mut() {
                            last.transition_out = Some(transition);
                        }
                    }
                    None => {}
                },
                "Clip" => match self.clip(child, kind, position) {
                    Some(mut clip) => {
                        clip.transition_in = pending.take();
                        position = position.max(clip.timeline_end().as_micros());
                        track.clips.push(clip);
                    }
                    None => pending = None,
                },
                _ => {}
            }
        }
        Some(track)
    }

    fn transition(&self, node: &Value) -> Option<(Transition, bool)> {
        let sidecar = sidecar(node)?;
        let tag: TransitionTag = serde_json::from_value(sidecar.clone()).ok()?;
        Some((
            Transition::new(tag.kind, MediaDuration::from_micros(tag.duration_us.max(0))),
            tag.outgoing,
        ))
    }

    fn clip(&self, node: &Value, kind: TrackKind, position: i64) -> Option<Clip> {
        let sidecar = sidecar(node);
        let meta: Option<Sidecar> = sidecar.and_then(|m| serde_json::from_value(m.clone()).ok());
        let path = media_reference(node).unwrap_or_default();
        let (native_start, native_duration) = read_span(node.get("source_range")).unwrap_or((0, 0));
        let source_start = meta
            .as_ref()
            .map(|m| m.source_start_us)
            .unwrap_or(native_start)
            .max(0);
        let duration = meta
            .as_ref()
            .map(|m| m.timeline_duration_us)
            .filter(|d| *d > 0)
            .unwrap_or(native_duration);
        if duration <= 0 {
            tracing::warn!("a clip with no duration was skipped");
            return None;
        }
        let speed = meta
            .as_ref()
            .map(|m| m.speed)
            .filter(|s| s.is_finite() && *s > 0.0)
            .unwrap_or(1.0);
        let source_end = meta
            .as_ref()
            .map(|m| m.source_end_us)
            .unwrap_or_else(|| source_start + (duration as f64 * speed).round() as i64);
        let source_total = meta
            .as_ref()
            .map(|m| m.source_total_us)
            .unwrap_or(source_end);
        let name = name_of(node);
        let tag = meta.as_ref().map(|m| m.kind).unwrap_or(match kind {
            TrackKind::Audio => ClipKindTag::Audio,
            _ => ClipKindTag::Video,
        });
        let clip_kind = match tag {
            ClipKindTag::Audio => ClipKind::Audio(AudioClip {
                source_path: path,
                sample_rate: meta
                    .as_ref()
                    .map(|m| m.sample_rate)
                    .filter(|r| *r > 0)
                    .unwrap_or(48_000),
                channels: meta
                    .as_ref()
                    .map(|m| m.channels)
                    .filter(|c| *c > 0)
                    .unwrap_or(2),
                filters: Vec::new(),
            }),
            ClipKindTag::Text => ClipKind::Text(TextOverlay {
                text: name,
                style: meta
                    .as_ref()
                    .and_then(|m| m.style.clone())
                    .unwrap_or_default(),
            }),
            ClipKindTag::Subtitle => ClipKind::Subtitle(SubtitleClip {
                source_path: path,
                font_path: None,
            }),
            ClipKindTag::Video => ClipKind::Video(VideoClip {
                source_path: path,
                width: meta
                    .as_ref()
                    .map(|m| m.width)
                    .filter(|w| *w > 0)
                    .unwrap_or(1920),
                height: meta
                    .as_ref()
                    .map(|m| m.height)
                    .filter(|h| *h > 0)
                    .unwrap_or(1080),
                fps: meta
                    .as_ref()
                    .map(|m| m.fps)
                    .filter(|f| f.is_finite() && *f > 0.0)
                    .unwrap_or_else(|| self.rate.as_f64()),
                filters: Vec::new(),
                audio_filters: Vec::new(),
                masks: Vec::new(),
            }),
        };
        let mut clip = Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::from_micros(position),
            timeline_duration: MediaDuration::from_micros(duration),
            source_start: MediaDuration::from_micros(source_start),
            source_end: MediaDuration::from_micros(source_end.max(source_start)),
            source_total_duration: MediaDuration::from_micros(source_total.max(source_end)),
            speed,
            volume: meta.as_ref().map(|m| m.volume).unwrap_or(1.0),
            opacity: meta.as_ref().map(|m| m.opacity).unwrap_or(1.0),
            muted: meta.as_ref().map(|m| m.muted).unwrap_or(false),
            transition_in: None,
            transition_out: None,
            kind: clip_kind,
            keyframes: meta
                .as_ref()
                .map(|m| m.keyframes.clone())
                .unwrap_or_default(),
            blend_mode: meta.as_ref().map(|m| m.blend_mode).unwrap_or_default(),
        };
        clip.normalize_source_bounds_in_place();
        Some(clip)
    }
}

#[derive(Serialize, Deserialize)]
struct Sidecar {
    #[serde(default)]
    kind: ClipKindTag,
    #[serde(default = "unit_speed")]
    speed: f64,
    #[serde(default = "unit_gain")]
    volume: f32,
    #[serde(default = "unit_gain")]
    opacity: f32,
    #[serde(default)]
    muted: bool,
    #[serde(default)]
    blend_mode: BlendMode,
    #[serde(default)]
    keyframes: KeyframeCurve,
    #[serde(default)]
    source_start_us: i64,
    #[serde(default)]
    source_end_us: i64,
    #[serde(default)]
    source_total_us: i64,
    #[serde(default)]
    timeline_duration_us: i64,
    #[serde(default)]
    width: u32,
    #[serde(default)]
    height: u32,
    #[serde(default)]
    fps: f64,
    #[serde(default)]
    sample_rate: u32,
    #[serde(default)]
    channels: u16,
    #[serde(default)]
    style: Option<TextStyle>,
}

fn unit_speed() -> f64 {
    1.0
}

fn unit_gain() -> f32 {
    1.0
}

impl Sidecar {
    fn of(clip: &Clip) -> Self {
        let mut sidecar = Self {
            kind: ClipKindTag::Video,
            speed: clip.speed,
            volume: clip.volume,
            opacity: clip.opacity,
            muted: clip.muted,
            blend_mode: clip.blend_mode,
            keyframes: clip.keyframes.clone(),
            source_start_us: clip.source_start.as_micros(),
            source_end_us: clip.source_end.as_micros(),
            source_total_us: clip.source_total_duration.as_micros(),
            timeline_duration_us: clip.timeline_duration.as_micros(),
            width: 0,
            height: 0,
            fps: 0.0,
            sample_rate: 0,
            channels: 0,
            style: None,
        };
        match &clip.kind {
            ClipKind::Video(video) => {
                sidecar.width = video.width;
                sidecar.height = video.height;
                sidecar.fps = video.fps;
            }
            ClipKind::Audio(audio) => {
                sidecar.kind = ClipKindTag::Audio;
                sidecar.sample_rate = audio.sample_rate;
                sidecar.channels = audio.channels;
            }
            ClipKind::Text(text) => {
                sidecar.kind = ClipKindTag::Text;
                sidecar.style = Some(text.style.clone());
            }
            ClipKind::Subtitle(_) => sidecar.kind = ClipKindTag::Subtitle,
            _ => {}
        }
        sidecar
    }
}

#[derive(Serialize, Deserialize, Clone, Copy, Default, PartialEq, Eq)]
enum ClipKindTag {
    #[default]
    Video,
    Audio,
    Text,
    Subtitle,
}

#[derive(Serialize, Deserialize, Clone, Copy)]
struct TransitionTag {
    kind: TransitionKind,
    duration_us: i64,
    outgoing: bool,
}

fn track_json(track: &Track, rate: Rate, source_names: &dyn Fn(&str) -> String) -> Value {
    let mut clips: Vec<&Clip> = track.clips.iter().collect();
    clips.sort_by_key(|c| c.timeline_start.as_micros());
    let mut children = Vec::new();
    let mut position = 0i64;
    for clip in clips {
        let start = clip.timeline_start.as_micros();
        if start > position {
            children.push(json!({
                "OTIO_SCHEMA": "Gap.1",
                "name": "",
                "source_range": span(0, start - position, rate),
                "effects": [],
                "markers": [],
                "enabled": true,
                "metadata": {},
            }));
        }
        if let Some(transition) = &clip.transition_in {
            children.push(transition_json(transition, false, rate));
        }
        children.push(clip_json(clip, rate, source_names));
        if let Some(transition) = &clip.transition_out {
            children.push(transition_json(transition, true, rate));
        }
        position = position.max(start).max(clip.timeline_end().as_micros());
    }
    json!({
        "OTIO_SCHEMA": "Track.1",
        "name": track.name,
        "kind": if track.kind == TrackKind::Audio { "Audio" } else { "Video" },
        "children": children,
        "source_range": Value::Null,
        "effects": [],
        "markers": [],
        "enabled": !track.muted,
        "metadata": {"snapshort": {"locked": track.locked}},
    })
}

fn clip_json(clip: &Clip, rate: Rate, source_names: &dyn Fn(&str) -> String) -> Value {
    let (path, name) = match &clip.kind {
        ClipKind::Video(video) => (video.source_path.as_str(), source_names(&video.source_path)),
        ClipKind::Audio(audio) => (audio.source_path.as_str(), source_names(&audio.source_path)),
        ClipKind::Subtitle(sub) => (sub.source_path.as_str(), source_names(&sub.source_path)),
        ClipKind::Text(text) => ("", first_line(&text.text)),
        _ => ("", String::new()),
    };
    let total = clip.source_total_duration.as_micros();
    let reference = if path.is_empty() {
        json!({
            "OTIO_SCHEMA": "MissingReference.1",
            "name": name,
            "available_range": Value::Null,
            "metadata": {},
        })
    } else {
        json!({
            "OTIO_SCHEMA": "ExternalReference.1",
            "name": name,
            "target_url": path_to_file_url(path),
            "available_range": if total > 0 { span(0, total, rate) } else { Value::Null },
            "available_image_bounds": Value::Null,
            "metadata": {},
        })
    };
    let mut effects = Vec::new();
    if (clip.speed - 1.0).abs() > 1e-9 {
        effects.push(json!({
            "OTIO_SCHEMA": "LinearTimeWarp.1",
            "name": "",
            "effect_name": "LinearTimeWarp",
            "time_scalar": clip.speed,
            "metadata": {},
        }));
    }
    json!({
        "OTIO_SCHEMA": "Clip.1",
        "name": name,
        "source_range": span(clip.source_start.as_micros(), clip.timeline_duration.as_micros(), rate),
        "media_references": {"DEFAULT_MEDIA": reference},
        "active_media_reference_key": "DEFAULT_MEDIA",
        "effects": effects,
        "markers": [],
        "enabled": !clip.muted,
        "metadata": {"snapshort": serde_json::to_value(Sidecar::of(clip)).unwrap_or_default()},
    })
}

fn transition_json(transition: &Transition, outgoing: bool, rate: Rate) -> Value {
    let duration = transition.duration.as_micros().max(0);
    let half = duration / 2;
    json!({
        "OTIO_SCHEMA": "Transition.1",
        "name": "Cross Dissolve",
        "transition_type": match transition.kind {
            TransitionKind::CrossFade | TransitionKind::Dissolve => "SMPTE_Dissolve",
            _ => "Custom_Transition",
        },
        "in_offset": rational(half, rate),
        "out_offset": rational(half, rate),
        "metadata": {"snapshort": TransitionTag { kind: transition.kind, duration_us: duration, outgoing }},
    })
}

fn default_track_name(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Audio => "Audio 1",
        TrackKind::Text => "Text 1",
        TrackKind::Subtitle => "Subtitle 1",
        _ => "Video 1",
    }
}

fn first_line(text: &str) -> String {
    text.lines().next().unwrap_or_default().trim().to_string()
}

fn schema_name(value: &Value) -> &str {
    value
        .get("OTIO_SCHEMA")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .split('.')
        .next()
        .unwrap_or_default()
}

fn sidecar(value: &Value) -> Option<&Value> {
    value.get("metadata").and_then(|m| m.get("snapshort"))
}

fn name_of(value: &Value) -> String {
    value
        .get("name")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .to_string()
}

fn media_reference(node: &Value) -> Option<String> {
    let key = node
        .get("active_media_reference_key")
        .and_then(Value::as_str)
        .unwrap_or("DEFAULT_MEDIA");
    let reference = match node.get("media_references") {
        Some(refs) => refs
            .get(key)
            .or_else(|| refs.as_object().and_then(|o| o.values().next()))?,
        None => node.get("media_reference")?,
    };
    match schema_name(reference) {
        "ExternalReference" | "ImageSequenceReference" => reference
            .get("target_url")
            .and_then(Value::as_str)
            .map(url_to_path),
        _ => None,
    }
}

fn frames_of(micros: i64, rate: Rate) -> f64 {
    micros as f64 * rate.num as f64 / (1_000_000.0 * rate.den as f64)
}

fn micros_of_frames(value: f64, rate: Rate) -> i64 {
    if !value.is_finite() {
        return 0;
    }
    (value * 1_000_000.0 * rate.den as f64 / rate.num as f64).round() as i64
}

fn rational(micros: i64, rate: Rate) -> Value {
    json!({
        "OTIO_SCHEMA": "RationalTime.1",
        "rate": rate.as_f64(),
        "value": frames_of(micros, rate),
    })
}

fn span(start_us: i64, duration_us: i64, rate: Rate) -> Value {
    json!({
        "OTIO_SCHEMA": "TimeRange.1",
        "start_time": rational(start_us, rate),
        "duration": rational(duration_us, rate),
    })
}

fn read_rational_rate(value: &Value) -> Option<Rate> {
    let rate = value.get("rate")?.as_f64()?;
    if rate.is_finite() && rate > 0.0 {
        Some(Rate::from_f64(rate))
    } else {
        None
    }
}

fn read_rational(value: &Value) -> Option<i64> {
    let rate = read_rational_rate(value)?;
    let value = value.get("value")?.as_f64()?;
    Some(micros_of_frames(value, rate))
}

fn read_span(value: Option<&Value>) -> Option<(i64, i64)> {
    let value = value?;
    if value.is_null() {
        return None;
    }
    Some((
        read_rational(value.get("start_time")?)?,
        read_rational(value.get("duration")?)?,
    ))
}

fn is_windows_absolute(path: &str) -> bool {
    let bytes = path.as_bytes();
    bytes.len() >= 3
        && bytes[0].is_ascii_alphabetic()
        && bytes[1] == b':'
        && (bytes[2] == b'/' || bytes[2] == b'\\')
}

fn percent_decode(text: &str) -> String {
    let bytes = text.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            let hex = std::str::from_utf8(&bytes[i + 1..i + 3])
                .ok()
                .and_then(|h| u8::from_str_radix(h, 16).ok());
            if let Some(byte) = hex {
                out.push(byte);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn percent_encode_path(path: &str) -> String {
    let mut out = String::with_capacity(path.len());
    for byte in path.as_bytes() {
        if byte.is_ascii_alphanumeric() || b"/-._~!$&'()*+,;=:@".contains(byte) {
            out.push(*byte as char);
        } else {
            out.push_str(&format!("%{byte:02X}"));
        }
    }
    out
}

fn path_to_file_url(path: &str) -> String {
    let path = path.replace('\\', "/");
    if path.starts_with('/') || is_windows_absolute(&path) {
        if is_windows_absolute(&path) {
            format!("file:///{}", percent_encode_path(&path))
        } else if let Some(unc) = path.strip_prefix("//") {
            format!("file://{}", percent_encode_path(unc))
        } else {
            format!("file://{}", percent_encode_path(&path))
        }
    } else {
        path
    }
}

fn url_to_path(url: &str) -> String {
    if url.get(..7).map(|s| s.eq_ignore_ascii_case("file://")) != Some(true) {
        return url.to_string();
    }
    let rest = &url[7..];
    let rest = if rest
        .get(..9)
        .is_some_and(|host| host.eq_ignore_ascii_case("localhost"))
    {
        &rest[9..]
    } else {
        rest
    };
    let decoded = percent_decode(rest);
    if decoded.len() >= 3 && decoded.starts_with('/') && is_windows_absolute(&decoded[1..]) {
        return decoded[1..].to_string();
    }
    if !decoded.starts_with('/') {
        return format!("//{decoded}");
    }
    decoded
}
