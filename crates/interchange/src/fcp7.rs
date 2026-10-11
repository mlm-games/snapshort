//! Final Cut Pro 7 XML interchange (`xmeml` versions 4 and 5), the dialect Premiere Pro imports
//! and exports.
//!
//! Supported: `project`/`bin`/`clip`/`sequence` structure, video and audio tracks (enabled,
//! locked), `clipitem` (start/end/in/out, `pproTicksIn/Out` for tick-exact source times, enabled,
//! labels, links), `file` (pathurl, rate, duration, timecode, media characteristics,
//! references by id), `generatoritem` (Slug, Color), `transitionitem` (alignment, Cross Dissolve
//! and friends), and the filters Basic Motion (scale, rotation, center, anchor, anti-flicker),
//! Opacity, Time Remap (constant speed, reverse, freeze), Audio Levels and Audio Pan, all with
//! keyframes. Anything with no miniter equivalent (nested sequences, clip groups, markers, labels,
//! channel volume, non-uniform scale) is reported through `tracing` rather than silently dropped.
//!
//! Coordinates: Basic Motion `center` is written relative to the frame centre as a fraction of the
//! frame width/height (0,0 = centred); `centerOffset` (anchor) likewise relative to the source.
//! Keyframe `when` values and clip markers are source (media) frames, like `in`/`out`.

use std::collections::HashMap;

use miniter_domain::{
    AudioClip, BlendMode, Clip, ClipId, ClipKind, Easing, Keyframe, KeyframeCurve, MediaDuration,
    Timeline, Timestamp, Track, TrackKind, Transition, TransitionKind, VideoClip, VideoFilter,
};

use crate::fcpxml::xml_escape;
use crate::{InterchangeError, Rate};

const MAX_DEPTH: usize = 64;

const BLEND_XML: &[(&str, BlendMode)] = &[
    ("normal", BlendMode::Normal),
    ("multiply", BlendMode::Multiply),
    ("screen", BlendMode::Screen),
    ("overlay", BlendMode::Overlay),
];

fn frames_round(rate: Rate, micros: i64) -> i64 {
    let micros = micros.max(0) as i128;
    let num = rate.num as i128;
    let den = rate.den as i128;
    ((micros * num * 2 + 1_000_000 * den) / (2 * 1_000_000 * den)) as i64
}

fn micros_of_frames(frames: i64, rate: Rate) -> i64 {
    let frames = frames as i128;
    let num = rate.num as i128;
    let den = rate.den as i128;
    ((frames * 1_000_000 * den) / num) as i64
}

fn timebase(rate: Rate) -> (i64, bool) {
    let ntsc = rate.den == 1001;
    let base = if ntsc {
        rate.num / 1000
    } else {
        rate.num / rate.den.max(1)
    };
    (base.max(1) as i64, ntsc)
}

fn rate_from_timebase(timebase: i64, ntsc: bool) -> Rate {
    let timebase = timebase.clamp(1, 600);
    if ntsc {
        Rate::new(timebase as u32 * 1000, 1001)
    } else {
        Rate::new(timebase as u32, 1)
    }
}

fn timecode_frames(frames: i64, rate: Rate) -> String {
    let frames = frames.max(0);
    let fps = ((rate.num + rate.den / 2) / rate.den.max(1)).max(1) as i64;
    let seconds = frames / fps;
    let f = frames % fps;
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    format!("{h:02}:{m:02}:{s:02}:{f:02}")
}

fn bool_str(value: bool) -> &'static str {
    if value { "TRUE" } else { "FALSE" }
}

fn num(value: f64) -> String {
    if !value.is_finite() {
        return "0".to_string();
    }
    if value == value.trunc() && value.abs() < 1e15 {
        return format!("{}", value as i64);
    }
    format!("{value}")
}

fn scaled(micros: i64, speed: f64) -> i64 {
    if speed == 0.0 || !speed.is_finite() {
        return micros;
    }
    (micros as f64 * speed).round() as i64
}

fn unscaled(micros: i64, speed: f64) -> i64 {
    if speed == 0.0 || !speed.is_finite() {
        return micros;
    }
    (micros as f64 / speed).round() as i64
}

fn on_frame(rate: Rate, micros: i64) -> bool {
    let micros = micros.max(0) as i128;
    let num = rate.num as i128;
    let den = rate.den as i128;
    (micros * num) % (1_000_000 * den) == 0
}

fn file_name(path: &str) -> &str {
    path.rsplit(['/', '\\']).next().unwrap_or(path)
}

fn file_stem(path: &str) -> String {
    let name = file_name(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
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

fn path_to_file_url(path: &str, localhost: bool) -> String {
    let path = path.replace('\\', "/");
    let host = if localhost { "localhost" } else { "" };
    if let Some(unc) = path.strip_prefix("//") {
        format!("file://{}", percent_encode_path(unc))
    } else {
        format!("file://{host}{}", percent_encode_path(&path))
    }
}

fn file_url_to_path(url: &str) -> String {
    if url.get(..7).map(|s| s.eq_ignore_ascii_case("file://")) != Some(true) {
        return url.to_string();
    }
    let rest = &url[7..];
    let rest = if rest.len() >= 9 && rest[..9].eq_ignore_ascii_case("localhost") {
        &rest[9..]
    } else {
        rest
    };
    percent_decode(rest)
}

fn label_name(index: usize) -> &'static str {
    const LABELS: [&str; 8] = [
        "Iris",
        "Caribbean",
        "Rose",
        "Mango",
        "Tan",
        "Forest",
        "Teal",
        "Lavender",
    ];
    LABELS[index % LABELS.len()]
}

pub fn export(timeline: &Timeline, rate: Rate) -> String {
    export_version(timeline, rate, 4)
}

pub fn export_version(timeline: &Timeline, rate: Rate, xmeml_version: u32) -> String {
    let rate = rate.reduced();
    let mut exp = Export {
        rate,
        writer: XmlWriter::new(Some("xmeml")),
        file_ids: HashMap::new(),
        next_clip: 1,
    };
    let version = if xmeml_version == 5 { "5" } else { "4" };
    exp.writer.open("xmeml", &[("version", version)]);
    exp.write_sequence("Snapshort", timeline, (1920, 1080));
    exp.writer.close();
    exp.writer.finish()
}

struct XmlWriter {
    out: String,
    stack: Vec<String>,
}

impl XmlWriter {
    fn new(doctype: Option<&str>) -> Self {
        let mut out = String::from("<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n");
        if let Some(doctype) = doctype {
            out.push_str(&format!("<!DOCTYPE {doctype}>\n"));
        }
        Self {
            out,
            stack: Vec::new(),
        }
    }

    fn indent(&mut self) {
        for _ in 0..self.stack.len() {
            self.out.push_str("  ");
        }
    }

    fn start_tag(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.indent();
        self.out.push('<');
        self.out.push_str(name);
        for (key, value) in attrs {
            self.out
                .push_str(&format!(" {key}=\"{}\"", xml_escape(value)));
        }
    }

    fn open(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.start_tag(name, attrs);
        self.out.push_str(">\n");
        self.stack.push(name.to_string());
    }

    fn close(&mut self) {
        let Some(name) = self.stack.pop() else { return };
        self.indent();
        self.out.push_str(&format!("</{name}>\n"));
    }

    fn empty(&mut self, name: &str, attrs: &[(&str, &str)]) {
        self.start_tag(name, attrs);
        self.out.push_str("/>\n");
    }

    fn text(&mut self, name: &str, value: impl std::fmt::Display) {
        self.start_tag(name, &[]);
        let value = value.to_string();
        self.out
            .push_str(&format!(">{}</{name}>\n", xml_escape(&value)));
    }

    fn finish(mut self) -> String {
        while !self.stack.is_empty() {
            self.close();
        }
        self.out
    }
}

#[derive(Clone, Debug, Default)]
struct Element {
    name: String,
    children: Vec<Element>,
    text: String,
}

fn parse_xml(text: &str) -> Result<Element, InterchangeError> {
    let chars: Vec<char> = text.chars().collect();
    let mut pos = 0usize;
    skip_space(&chars, &mut pos);
    while chars.get(pos) == Some(&'<') && matches!(chars.get(pos + 1), Some(&'?') | Some(&'!')) {
        skip_markup(&chars, &mut pos);
        skip_space(&chars, &mut pos);
    }
    let root = read_element(&chars, &mut pos, 0)?;
    Ok(root)
}

fn read_element(
    chars: &[char],
    pos: &mut usize,
    depth: usize,
) -> Result<Element, InterchangeError> {
    if depth > MAX_DEPTH {
        return Err(InterchangeError::Other("XML nested too deeply".into()));
    }
    skip_space(chars, pos);
    if chars.get(*pos) != Some(&'<') {
        return Err(InterchangeError::Other("expected an element".into()));
    }
    *pos += 1;
    let name = read_name(chars, pos)?;
    loop {
        skip_space(chars, pos);
        match chars.get(*pos) {
            Some('/') => {
                *pos += 1;
                skip_space(chars, pos);
                expect(chars, pos, '>')?;
                return Ok(Element {
                    name,
                    children: Vec::new(),
                    text: String::new(),
                });
            }
            Some('>') => {
                *pos += 1;
                break;
            }
            Some(c) if is_name_start(*c) => {
                let _key = read_name(chars, pos)?;
                skip_space(chars, pos);
                expect(chars, pos, '=')?;
                skip_space(chars, pos);
                let _value = read_quoted(chars, pos)?;
            }
            _ => return Err(InterchangeError::Other("malformed element tag".into())),
        }
    }
    let mut children = Vec::new();
    let mut text = String::new();
    loop {
        if *pos >= chars.len() {
            return Err(InterchangeError::Other("unterminated element".into()));
        }
        if chars[*pos] == '<' {
            if chars.get(*pos + 1) == Some(&'/') {
                *pos += 2;
                let closing = read_name(chars, pos)?;
                skip_space(chars, pos);
                expect(chars, pos, '>')?;
                if closing != name {
                    return Err(InterchangeError::Other(format!(
                        "close tag </{closing}> does not match <{name}>"
                    )));
                }
                return Ok(Element {
                    name,
                    children,
                    text,
                });
            }
            if chars.get(*pos + 1) == Some(&'!') || chars.get(*pos + 1) == Some(&'?') {
                skip_markup(chars, pos);
                continue;
            }
            children.push(read_element(chars, pos, depth + 1)?);
        } else {
            text.push(chars[*pos]);
            *pos += 1;
        }
    }
}

fn skip_space(chars: &[char], pos: &mut usize) {
    while chars.get(*pos).is_some_and(|c| c.is_whitespace()) {
        *pos += 1;
    }
}

fn is_name_start(c: char) -> bool {
    c.is_ascii_alphabetic() || c == '_' || c == ':'
}

fn is_name_char(c: char) -> bool {
    c.is_ascii_alphanumeric() || matches!(c, '_' | ':' | '-' | '.')
}

fn read_name(chars: &[char], pos: &mut usize) -> Result<String, InterchangeError> {
    let start = *pos;
    while chars.get(*pos).is_some_and(|c| is_name_char(*c)) {
        *pos += 1;
    }
    if start == *pos {
        return Err(InterchangeError::Other("expected a name".into()));
    }
    Ok(chars[start..*pos].iter().collect())
}

fn read_quoted(chars: &[char], pos: &mut usize) -> Result<String, InterchangeError> {
    let quote = match chars.get(*pos) {
        Some(q @ ('"' | '\'')) => *q,
        _ => return Err(InterchangeError::Other("expected a quoted value".into())),
    };
    *pos += 1;
    let mut out = String::new();
    loop {
        match chars.get(*pos) {
            Some(c) if *c == quote => {
                *pos += 1;
                return Ok(unescape(&out));
            }
            Some(c) => {
                out.push(*c);
                *pos += 1;
            }
            None => return Err(InterchangeError::Other("unterminated value".into())),
        }
    }
}

fn expect(chars: &[char], pos: &mut usize, want: char) -> Result<(), InterchangeError> {
    if chars.get(*pos) == Some(&want) {
        *pos += 1;
        Ok(())
    } else {
        Err(InterchangeError::Other(format!("expected `{want}`")))
    }
}

fn skip_markup(chars: &[char], pos: &mut usize) {
    let mut angle = 0usize;
    while *pos < chars.len() {
        match chars[*pos] {
            '<' => angle += 1,
            '>' => {
                angle = angle.saturating_sub(1);
                if angle == 0 {
                    *pos += 1;
                    return;
                }
            }
            _ => {}
        }
        *pos += 1;
    }
}

fn unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(index) = rest.find('&') {
        out.push_str(&rest[..index]);
        let after = &rest[index..];
        let end = after.find(';').filter(|s| *s <= 10);
        match end {
            Some(end) => {
                let entity = &after[1..end];
                match entity {
                    "amp" => out.push('&'),
                    "lt" => out.push('<'),
                    "gt" => out.push('>'),
                    "quot" => out.push('"'),
                    "apos" => out.push('\''),
                    _ => {
                        if let Some(digits) = entity.strip_prefix('#') {
                            let code = digits.parse::<u32>().ok().and_then(char::from_u32);
                            match code {
                                Some(c) => out.push(c),
                                None => out.push_str(&after[..end + 1]),
                            }
                        } else {
                            out.push_str(&after[..end + 1]);
                        }
                    }
                }
                rest = &after[end + 1..];
            }
            None => {
                out.push_str(after);
                return out;
            }
        }
    }
    out.push_str(rest);
    out
}

fn child<'a>(node: &'a Element, name: &str) -> Option<&'a Element> {
    node.children.iter().find(|c| c.name == name)
}

fn children<'a>(node: &'a Element, name: &str) -> impl Iterator<Item = &'a Element> {
    node.children.iter().filter(move |c| c.name == name)
}

fn path_text<'a>(node: &'a Element, path: &[&str]) -> Option<&'a str> {
    let mut current = node;
    for name in path {
        current = child(current, name)?;
    }
    Some(current.text.trim())
}

fn child_text<'a>(node: &'a Element, name: &str) -> Option<&'a str> {
    child(node, name).map(|c| c.text.trim())
}

fn child_i64(node: &Element, name: &str) -> Option<i64> {
    child_text(node, name).and_then(|t| {
        t.parse::<i64>()
            .ok()
            .or_else(|| t.parse::<f64>().ok().map(|f| f.round() as i64))
    })
}

fn child_f64(node: &Element, name: &str) -> Option<f64> {
    child_text(node, name).and_then(|t| t.parse::<f64>().ok())
}

fn child_bool(node: &Element, name: &str) -> Option<bool> {
    child_text(node, name).map(|t| t.eq_ignore_ascii_case("true") || t == "1")
}

fn read_rate(node: &Element) -> Option<Rate> {
    let rate = child(node, "rate")?;
    let timebase = child_i64(rate, "timebase")?;
    Some(rate_from_timebase(
        timebase,
        child_bool(rate, "ntsc").unwrap_or(false),
    ))
}

fn write_rate(writer: &mut XmlWriter, rate: Rate) {
    let (timebase, ntsc) = timebase(rate);
    writer.open("rate", &[]);
    writer.text("timebase", timebase);
    writer.text("ntsc", bool_str(ntsc));
    writer.close();
}

fn write_timecode(writer: &mut XmlWriter, frame: i64, rate: Rate, drop_frame: bool) {
    writer.open("timecode", &[]);
    write_rate(writer, rate);
    writer.text("string", timecode_frames(frame, rate));
    writer.text("frame", frame);
    writer.text("displayformat", if drop_frame { "DF" } else { "NDF" });
    writer.close();
}

fn total_frames(timeline: &Timeline, rate: Rate) -> i64 {
    let end = timeline
        .tracks
        .iter()
        .flat_map(|t| &t.clips)
        .map(|c| c.timeline_end().as_micros())
        .max()
        .unwrap_or(0);
    frames_round(rate, end).max(1)
}

fn source_path_of(clip: &Clip) -> Option<String> {
    match &clip.kind {
        ClipKind::Video(v) if !v.source_path.is_empty() => Some(v.source_path.clone()),
        ClipKind::Audio(a) if !a.source_path.is_empty() => Some(a.source_path.clone()),
        ClipKind::Subtitle(s) if !s.source_path.is_empty() => Some(s.source_path.clone()),
        _ => None,
    }
}

fn is_audio(kind: TrackKind) -> bool {
    kind == TrackKind::Audio
}

struct Export {
    rate: Rate,
    writer: XmlWriter,
    file_ids: HashMap<String, String>,
    next_clip: usize,
}

impl Export {
    fn write_sequence(&mut self, name: &str, timeline: &Timeline, frame: (u32, u32)) {
        let rate = self.rate;
        self.writer.open("sequence", &[]);
        self.writer.text("name", name);
        self.writer.text("duration", total_frames(timeline, rate));
        write_rate(&mut self.writer, rate);
        write_timecode(&mut self.writer, 0, rate, false);
        self.writer.open("labels", &[]);
        self.writer.text("label2", label_name(0));
        self.writer.close();
        self.writer.open("media", &[]);
        for kind in [TrackKind::Video, TrackKind::Audio] {
            let tag = if kind == TrackKind::Video {
                "video"
            } else {
                "audio"
            };
            self.writer.open(tag, &[]);
            self.writer.open("format", &[]);
            self.writer.open("samplecharacteristics", &[]);
            if kind == TrackKind::Video {
                write_rate(&mut self.writer, rate);
                self.writer.text("width", frame.0);
                self.writer.text("height", frame.1);
                self.writer.text("anamorphic", "FALSE");
                self.writer.text("pixelaspectratio", "square");
                self.writer.text("fielddominance", "none");
            } else {
                self.writer.text("depth", 16);
                self.writer.text("samplerate", 48_000);
            }
            self.writer.close();
            self.writer.close();
            for track in timeline.tracks.iter().filter(|t| t.kind == kind) {
                self.writer.open("track", &[]);
                let mut order: Vec<(i64, u8, usize)> = track
                    .clips
                    .iter()
                    .enumerate()
                    .map(|(i, c)| (c.timeline_start.as_micros(), 1, i))
                    .collect();
                order.extend(track.clips.iter().enumerate().filter_map(|(i, c)| {
                    c.transition_in.as_ref().map(|t| {
                        (
                            c.timeline_start.as_micros() - t.duration.as_micros() / 2,
                            0,
                            i,
                        )
                    })
                }));
                order.sort();
                for (_, tag_kind, index) in order {
                    let clip = &track.clips[index];
                    if tag_kind == 1 {
                        self.write_clip(clip, kind, rate, frame);
                    } else if let Some(transition) = &clip.transition_in {
                        self.write_transition(transition, kind, rate, clip, true);
                    }
                }
                self.writer.text("enabled", bool_str(!track.muted));
                self.writer.text("locked", bool_str(track.locked));
                self.writer.close();
            }
            self.writer.close();
        }
        self.writer.close();
        self.writer.close();
    }

    fn write_clip(&mut self, clip: &Clip, kind: TrackKind, rate: Rate, frame: (u32, u32)) {
        if !on_frame(rate, clip.timeline_start.as_micros())
            || !on_frame(rate, clip.timeline_duration.as_micros())
        {
            tracing::warn!("sub-frame clip positions were rounded to frames");
        }
        let path = source_path_of(clip);
        let name = match &path {
            Some(path) => file_stem(path),
            None => "Clip".to_string(),
        };
        let start_f = frames_round(rate, clip.timeline_start.as_micros());
        let end_f = frames_round(rate, clip.timeline_end().as_micros());
        let in_f = frames_round(rate, clip.source_start.as_micros());
        let src_in = clip.source_start.as_micros();
        let src_out = clip.source_end.as_micros();
        let out_f = if src_out > src_in {
            frames_round(rate, src_out)
        } else {
            in_f.saturating_add(end_f.saturating_sub(start_f))
        };
        let duration = clip
            .source_total_duration
            .as_micros()
            .max(src_out)
            .max(src_in);
        let xid = format!("clipitem-{}", self.next_clip);
        self.next_clip += 1;
        let tag = if path.is_none() {
            "generatoritem"
        } else {
            "clipitem"
        };
        self.writer.open(tag, &[("id", xid.as_str())]);
        self.writer.text("name", name.as_str());
        self.writer.text("enabled", bool_str(!clip.muted));
        self.writer
            .text("duration", frames_round(rate, duration).max(out_f));
        write_rate(&mut self.writer, rate);
        self.writer.text("start", start_f);
        self.writer.text("end", end_f);
        self.writer.text("in", in_f);
        self.writer.text("out", out_f);
        self.writer.text("pproTicksIn", src_in);
        self.writer.text("pproTicksOut", src_out);
        self.writer
            .text("pproTicksDuration", end_f.saturating_sub(start_f));
        match &path {
            Some(path) => self.write_file(path, clip, kind, rate),
            None => {
                self.writer.open("effect", &[]);
                self.writer.text("name", "Slug");
                self.writer.text("effectid", "slug");
                self.writer.text("effectcategory", "Matte");
                self.writer.text("effecttype", "generator");
                self.writer.text("mediatype", "video");
                self.writer.close();
            }
        }
        if kind == TrackKind::Audio {
            self.writer.open("sourcetrack", &[]);
            self.writer.text("mediatype", "audio");
            self.writer.text("trackindex", 1);
            self.writer.close();
        }
        if clip.blend_mode != BlendMode::Normal {
            let xml = BLEND_XML
                .iter()
                .find(|(_, mode)| *mode == clip.blend_mode)
                .map(|(xml, _)| *xml)
                .unwrap_or("normal");
            self.writer.text("compositemode", xml);
        }
        self.write_filters(clip, kind, rate, frame);
        self.writer.open("labels", &[]);
        self.writer.text("label2", label_name(0));
        self.writer.close();
        self.writer.close();
    }

    fn write_file(&mut self, path: &str, clip: &Clip, kind: TrackKind, rate: Rate) {
        let key = format!("file:{path}");
        let existing = self.file_ids.get(&key).cloned();
        let id = match existing {
            Some(id) => {
                self.writer.empty("file", &[("id", id.as_str())]);
                return;
            }
            None => {
                let id = format!("file-{}", self.file_ids.len() + 1);
                self.file_ids.insert(key, id.clone());
                id
            }
        };
        let (width, height, fps) = match &clip.kind {
            ClipKind::Video(v) => (v.width, v.height, v.fps),
            _ => (1920, 1080, rate.as_f64()),
        };
        let fps = if fps.is_finite() && fps > 0.0 {
            Rate::from_f64(fps).reduced()
        } else {
            rate
        };
        self.writer.open("file", &[("id", id.as_str())]);
        self.writer.text("name", file_stem(path));
        self.writer.text("pathurl", path_to_file_url(path, true));
        write_rate(&mut self.writer, fps);
        self.writer.text(
            "duration",
            frames_round(fps, clip.source_total_duration.as_micros()),
        );
        write_timecode(
            &mut self.writer,
            frames_round(fps, clip.source_start.as_micros()),
            fps,
            false,
        );
        self.writer.open("media", &[]);
        if kind != TrackKind::Audio {
            self.writer.open("video", &[]);
            self.writer.open("samplecharacteristics", &[]);
            write_rate(&mut self.writer, fps);
            self.writer.text("width", width.max(16));
            self.writer.text("height", height.max(16));
            self.writer.text("anamorphic", "FALSE");
            self.writer.text("pixelaspectratio", "square");
            self.writer.text("fielddominance", "none");
            self.writer.close();
            self.writer.close();
        }
        if let ClipKind::Audio(audio) = &clip.kind {
            self.writer.open("audio", &[]);
            self.writer.open("samplecharacteristics", &[]);
            self.writer.text("depth", 16);
            self.writer.text("samplerate", audio.sample_rate.max(1));
            self.writer.close();
            self.writer.text("channelcount", audio.channels.max(1));
            self.writer.close();
        } else {
            self.writer.open("audio", &[]);
            self.writer.open("samplecharacteristics", &[]);
            self.writer.text("depth", 16);
            self.writer.text("samplerate", 48_000);
            self.writer.close();
            self.writer.text("channelcount", 2);
            self.writer.close();
        }
        self.writer.close();
        self.writer.close();
    }

    fn write_transition(
        &mut self,
        transition: &Transition,
        kind: TrackKind,
        rate: Rate,
        clip: &Clip,
        incoming: bool,
    ) {
        let duration = transition.duration.as_micros();
        let (start_us, end_us) = if incoming {
            (
                clip.timeline_start.as_micros() - duration / 2,
                clip.timeline_start.as_micros() + duration / 2,
            )
        } else {
            (
                clip.timeline_end().as_micros() - duration / 2,
                clip.timeline_end().as_micros() + duration / 2,
            )
        };
        let start_f = frames_round(rate, start_us).max(0);
        let end_f = frames_round(rate, end_us).max(start_f);
        self.writer.open("transitionitem", &[]);
        write_rate(&mut self.writer, rate);
        self.writer.text("start", start_f);
        self.writer.text("end", end_f);
        self.writer.text("alignment", "center");
        self.writer.open("effect", &[]);
        self.writer.text("name", "Cross Dissolve");
        self.writer.text("effectid", "Cross Dissolve");
        self.writer.text("effectcategory", "Dissolve");
        self.writer.text("effecttype", "transition");
        self.writer
            .text("mediatype", if is_audio(kind) { "audio" } else { "video" });
        self.writer.close();
        self.writer.close();
    }

    fn write_filters(&mut self, clip: &Clip, kind: TrackKind, rate: Rate, frame: (u32, u32)) {
        let has_motion = clip
            .keyframes
            .keyframes
            .iter()
            .any(|k| k.param.starts_with("transform."));
        if has_motion {
            self.writer.open("filter", &[]);
            self.writer.open("effect", &[]);
            self.writer.text("name", "Basic Motion");
            self.writer.text("effectid", "basic");
            self.writer.text("effectcategory", "motion");
            self.writer.text("effecttype", "motion");
            self.writer.text("mediatype", "video");
            let (fw, fh) = (frame.0 as f64, frame.1 as f64);
            self.float_param(clip, &SCALE_AXIS, rate, |v| v * 100.0);
            self.float_param(clip, &ROTATE_AXIS, rate, |v| v);
            let center = PointAxis {
                size: (fw, fh),
                ..CENTER_AXIS
            };
            self.point_param(clip, &center, rate, |x, y| {
                ((x - fw / 2.0) / fw, (y - fh / 2.0) / fh)
            });
            self.writer.close();
            self.writer.close();
        }
        if (clip.opacity - 1.0).abs() > 0.001 {
            self.writer.open("filter", &[]);
            self.writer.open("effect", &[]);
            self.writer.text("name", "Opacity");
            self.writer.text("effectid", "opacity");
            self.writer.text("effectcategory", "motion");
            self.writer.text("effecttype", "motion");
            self.writer.text("mediatype", "video");
            self.writer.open("parameter", &[]);
            self.writer.text("parameterid", "opacity");
            self.writer.text("name", "Opacity");
            self.writer.text("valuemin", 0.0);
            self.writer.text("valuemax", 100.0);
            self.writer.text("value", num(clip.opacity as f64 * 100.0));
            self.writer.close();
            self.writer.close();
            self.writer.close();
        }
        if clip.speed > 0.0 && (clip.speed - 1.0).abs() > 0.001 {
            let reverse = matches!(&clip.kind, ClipKind::Video(v) if v.filters.iter().any(|f| matches!(&f.filter, VideoFilter::Speed { factor } if *factor < 0.0)));
            self.writer.open("filter", &[]);
            self.writer.open("effect", &[]);
            self.writer.text("name", "Time Remap");
            self.writer.text("effectid", "timeremap");
            self.writer.text("effectcategory", "motion");
            self.writer.text("effecttype", "motion");
            self.writer.text("mediatype", "video");
            self.simple_param("speed", "speed", num(clip.speed * 100.0).as_str());
            self.simple_param("variablespeed", "variablespeed", "0");
            self.simple_param("reverse", "reverse", bool_str(reverse));
            self.simple_param("frameblending", "frameblending", "FALSE");
            self.writer.close();
            self.writer.close();
        }
        if is_audio(kind) && (clip.volume - 1.0).abs() > 0.001 {
            self.writer.open("filter", &[]);
            self.writer.open("effect", &[]);
            self.writer.text("name", "Audio Levels");
            self.writer.text("effectid", "audiolevels");
            self.writer.text("effectcategory", "audiolevels");
            self.writer.text("effecttype", "audiolevels");
            self.writer.text("mediatype", "audio");
            self.writer.open("parameter", &[]);
            self.writer.text("parameterid", "level");
            self.writer.text("name", "Level");
            self.writer.text("valuemin", 0.0);
            self.writer.text("valuemax", 3.98109);
            self.writer.text("value", num(clip.volume as f64));
            self.writer.close();
            self.writer.close();
            self.writer.close();
        }
    }

    fn simple_param(&mut self, id: &str, name: &str, value: &str) {
        self.writer.open("parameter", &[]);
        self.writer.text("parameterid", id);
        self.writer.text("name", name);
        self.writer.text("value", value);
        self.writer.close();
    }

    fn float_param(&mut self, clip: &Clip, axis: &Axis, rate: Rate, map: impl Fn(f64) -> f64) {
        let keys: Vec<&Keyframe> = clip
            .keyframes
            .keyframes
            .iter()
            .filter(|k| k.param == axis.domain)
            .collect();
        if keys.is_empty() {
            return;
        }
        let base = keys.iter().map(|k| k.value as f64).next().unwrap_or(0.0);
        self.writer.open("parameter", &[]);
        self.writer.text("parameterid", axis.xml);
        self.writer.text("name", axis.label);
        self.writer.text("valuemin", num(axis.min));
        self.writer.text("valuemax", num(axis.max));
        self.writer.text("value", num(map(base)));
        for key in keys {
            self.writer.open("keyframe", &[]);
            self.writer
                .text("when", frames_round(rate, key.offset.as_micros()));
            self.writer.text("value", num(map(key.value as f64)));
            self.writer.close();
        }
        self.writer.close();
    }

    fn point_param(
        &mut self,
        clip: &Clip,
        axis: &PointAxis,
        rate: Rate,
        map: impl Fn(f64, f64) -> (f64, f64),
    ) {
        let mut offsets: Vec<i64> = clip
            .keyframes
            .keyframes
            .iter()
            .filter(|k| k.param == axis.x || k.param == axis.y)
            .map(|k| k.offset.as_micros())
            .collect();
        offsets.sort_unstable();
        offsets.dedup();
        if offsets.is_empty() {
            return;
        }
        let value_at = |offset: i64, param: &str| -> f64 {
            clip.keyframes
                .keyframes
                .iter()
                .find(|k| k.param == param && k.offset.as_micros() == offset)
                .map(|k| k.value as f64)
                .unwrap_or(if param == axis.x {
                    axis.size.0 / 2.0
                } else {
                    axis.size.1 / 2.0
                })
        };
        self.writer.open("parameter", &[]);
        self.writer.text("parameterid", axis.xml);
        self.writer.text("name", axis.label);
        let first = offsets[0];
        let (bx, by) = map(value_at(first, axis.x), value_at(first, axis.y));
        self.writer.open("value", &[]);
        self.writer.text("horiz", num(bx));
        self.writer.text("vert", num(by));
        self.writer.close();
        for offset in offsets {
            let (x, y) = map(value_at(offset, axis.x), value_at(offset, axis.y));
            self.writer.open("keyframe", &[]);
            self.writer.text("when", frames_round(rate, offset));
            self.writer.open("value", &[]);
            self.writer.text("horiz", num(x));
            self.writer.text("vert", num(y));
            self.writer.close();
            self.writer.close();
        }
        self.writer.close();
    }
}

struct Axis {
    domain: &'static str,
    xml: &'static str,
    label: &'static str,
    min: f64,
    max: f64,
}

const SCALE_AXIS: Axis = Axis {
    domain: "transform.scale",
    xml: "scale",
    label: "Scale",
    min: 0.0,
    max: 1000.0,
};

const ROTATE_AXIS: Axis = Axis {
    domain: "transform.rotate",
    xml: "rotation",
    label: "Rotation",
    min: -8640.0,
    max: 8640.0,
};

struct PointAxis {
    x: &'static str,
    y: &'static str,
    xml: &'static str,
    label: &'static str,
    size: (f64, f64),
}

const CENTER_AXIS: PointAxis = PointAxis {
    x: "transform.translate_x",
    y: "transform.translate_y",
    xml: "center",
    label: "Center",
    size: (0.0, 0.0),
};

struct Reader {
    nodes: Vec<Element>,
}

pub fn import(text: &str) -> Result<Timeline, InterchangeError> {
    let root = parse_xml(text)?;
    if root.name != "xmeml" {
        return Err(InterchangeError::Other(format!(
            "root element is <{}>, expected <xmeml>",
            root.name
        )));
    }
    let mut nodes: Vec<Element> = Vec::new();
    flatten(&root, &mut nodes);
    let sequence = nodes
        .iter()
        .position(|n| n.name == "sequence" && child(n, "media").is_some())
        .ok_or_else(|| InterchangeError::Other("no sequence in the document".into()))?;
    let mut reader = Reader { nodes };
    reader.read_sequence(sequence)
}

fn flatten(node: &Element, out: &mut Vec<Element>) {
    for kid in &node.children {
        out.push(kid.clone());
        flatten(kid, out);
    }
}

impl Reader {
    fn read_sequence(&mut self, position: usize) -> Result<Timeline, InterchangeError> {
        let node = self
            .nodes
            .get(position)
            .cloned()
            .ok_or_else(|| InterchangeError::Other("sequence vanished".into()))?;
        let rate = read_rate(&node).unwrap_or_else(|| Rate::new(24, 1));
        let mut timeline = Timeline::new();
        let Some(media) = child(&node, "media") else {
            return Err(InterchangeError::Other(
                "the sequence has no media element".into(),
            ));
        };
        for (kind, tag) in [(TrackKind::Video, "video"), (TrackKind::Audio, "audio")] {
            let Some(container) = child(media, tag) else {
                continue;
            };
            for track_node in children(container, "track") {
                self.read_track(track_node, kind, rate, &mut timeline);
            }
        }
        if timeline.tracks.is_empty() {
            return Err(InterchangeError::Other("the sequence has no tracks".into()));
        }
        Ok(timeline)
    }

    fn read_track(&mut self, node: &Element, kind: TrackKind, rate: Rate, timeline: &mut Timeline) {
        let enabled = child_bool(node, "enabled").unwrap_or(true);
        let locked = child_bool(node, "locked").unwrap_or(false);
        let mut track = Track::new(kind, child_text(node, "name").unwrap_or("Track"));
        track.muted = !enabled;
        track.locked = locked;
        let mut pending: Option<(Transition, bool)> = None;
        for item in &node.children {
            match item.name.as_str() {
                "clipitem" | "generatoritem" => match self.clip_item(item, kind, rate) {
                    Some(mut clip) => {
                        if let Some((transition, incoming)) = pending.take() {
                            if incoming {
                                clip.transition_in = Some(transition);
                            } else if let Some(previous) = track.clips.last_mut() {
                                previous.transition_out = Some(transition);
                            }
                        }
                        track.clips.push(clip);
                    }
                    None => pending = None,
                },
                "transitionitem" => {
                    pending = self.transition_item(item, rate);
                }
                _ => {}
            }
        }
        track.clips.sort_by_key(|c| c.timeline_start.as_micros());
        timeline.add_track(track);
    }

    fn clip_item(&mut self, node: &Element, kind: TrackKind, seq_rate: Rate) -> Option<Clip> {
        let name = child_text(node, "name").unwrap_or("").to_string();
        let rate = read_rate(node).unwrap_or(seq_rate);
        let generator = node.name == "generatoritem";
        let path = if generator {
            None
        } else if let Some(file) = child(node, "file") {
            match child_text(file, "pathurl") {
                Some(url) => Some(file_url_to_path(url)),
                None => {
                    tracing::warn!("file \"{name}\" has no pathurl; imported offline");
                    None
                }
            }
        } else {
            None
        };
        let start = child_i64(node, "start").unwrap_or(-1);
        let end = child_i64(node, "end").unwrap_or(-1);
        let in_f = child_i64(node, "in").unwrap_or(0);
        let out_f = child_i64(node, "out").unwrap_or(in_f);
        let src_in = child_i64(node, "pproTicksIn")
            .map(|v| v.max(0))
            .unwrap_or_else(|| micros_of_frames(in_f, rate));
        let src_out = child_i64(node, "pproTicksOut")
            .map(|v| v.max(src_in))
            .unwrap_or_else(|| micros_of_frames(out_f, rate).max(src_in));
        let mut speed = 1.0;
        let mut reverse = false;
        let mut hold = false;
        for effect in node
            .children
            .iter()
            .filter(|c| c.name == "filter")
            .filter_map(|f| child(f, "effect"))
        {
            if child_text(effect, "effectid") == Some("timeremap") {
                for p in children(effect, "parameter") {
                    match child_text(p, "parameterid") {
                        Some("speed") => {
                            let v = child_f64(p, "value").unwrap_or(100.0);
                            if v == 0.0 {
                                hold = true;
                            } else {
                                speed = v.abs() / 100.0;
                                reverse |= v < 0.0;
                            }
                            if child(p, "keyframe").is_some() {
                                tracing::warn!(
                                    "variable speed (time remap keyframes) is not supported; a constant speed was used"
                                );
                            }
                        }
                        Some("reverse") => reverse |= child_bool(p, "value").unwrap_or(false),
                        Some("variablespeed")
                            if child_bool(p, "value") == Some(true)
                                || child_f64(p, "value").is_some_and(|v| v != 0.0) =>
                        {
                            tracing::warn!(
                                "variable speed (time remap keyframes) is not supported; a constant speed was used"
                            );
                        }
                        _ => {}
                    }
                }
            }
        }
        let src_len = (src_out - src_in).abs();
        let (start_us, dur_us) = match (start >= 0, end >= 0) {
            (true, true) => {
                let s = micros_of_frames(start, seq_rate);
                (s, micros_of_frames(end, seq_rate) - s)
            }
            (true, false) => {
                let d = if hold {
                    micros_of_frames(out_f - in_f, rate)
                } else {
                    unscaled(src_len, speed)
                };
                (micros_of_frames(start, seq_rate), d)
            }
            (false, true) => {
                let d = if hold {
                    micros_of_frames(out_f - in_f, rate)
                } else {
                    unscaled(src_len, speed)
                };
                (micros_of_frames(end, seq_rate) - d, d)
            }
            (false, false) => {
                tracing::warn!("clip \"{name}\" has neither start nor end; skipped");
                return None;
            }
        };
        if dur_us <= 0 {
            return None;
        }
        if !hold && src_len != dur_us && src_len > 0 {
            speed = (src_len as f64 / dur_us as f64 * 10_000.0).round() / 10_000.0;
        }
        let source_in = src_in.min(src_out).max(0);
        let source_out = if hold {
            source_in + dur_us
        } else {
            source_in + scaled(dur_us, speed)
        };
        let mut clip = base_clip(kind, start_us, dur_us, source_in, source_out, path);
        clip.speed = speed.max(0.0);
        clip.muted = !child_bool(node, "enabled").unwrap_or(true);
        if let ClipKind::Video(video) = &mut clip.kind {
            let reversed = reverse || speed < 0.0;
            if reversed {
                video.filters.retain(
                    |f| !matches!(&f.filter, VideoFilter::Speed { factor } if *factor < 0.0),
                );
            }
        }
        if let Some(mode) = child_text(node, "compositemode") {
            match BLEND_XML
                .iter()
                .find(|(xml, _)| xml.eq_ignore_ascii_case(mode))
            {
                Some((_, mode)) => clip.blend_mode = *mode,
                None => tracing::warn!("composite mode \"{mode}\" is not supported"),
            }
        }
        self.filters(node, &mut clip, rate);
        Some(clip)
    }

    fn transition_item(&self, node: &Element, rate: Rate) -> Option<(Transition, bool)> {
        let start = child_i64(node, "start").unwrap_or(0);
        let end = child_i64(node, "end").unwrap_or(start);
        let start_us = micros_of_frames(start, rate);
        let end_us = micros_of_frames(end, rate);
        if end_us <= start_us {
            return None;
        }
        let effect = child(node, "effect");
        let name = effect
            .and_then(|e| child_text(e, "name"))
            .or_else(|| effect.and_then(|e| child_text(e, "effectid")))
            .unwrap_or("Cross Dissolve");
        let kind = transition_kind(name);
        let align = child_text(node, "alignment")
            .unwrap_or("")
            .to_ascii_lowercase();
        let incoming = match align.as_str() {
            "start-black" | "start" => true,
            "end-black" | "end" => false,
            _ => true,
        };
        Some((
            Transition::new(kind, MediaDuration::from_micros(end_us - start_us)),
            incoming,
        ))
    }

    fn filters(&mut self, node: &Element, clip: &mut Clip, rate: Rate) {
        let frame = clip_frame(clip);
        for filter in node.children.iter().filter(|c| c.name == "filter") {
            let Some(effect) = child(filter, "effect") else {
                continue;
            };
            let id = child_text(effect, "effectid").unwrap_or("");
            let name = child_text(effect, "name").unwrap_or(id);
            match id {
                "basic" => self.basic_motion(effect, clip, rate, frame),
                "opacity" => self.opacity(effect, clip),
                "timeremap" => {}
                "audiolevels" => self.audio_levels(effect, clip),
                "audiopan" => self.audio_pan(effect, clip),
                _ => tracing::warn!("effect \"{name}\" is not supported and was skipped"),
            }
        }
    }

    fn basic_motion(&mut self, effect: &Element, clip: &mut Clip, rate: Rate, frame: (f64, f64)) {
        let params: Vec<&Element> = children(effect, "parameter").collect();
        let find = |id: &str| {
            params.iter().copied().find(|p| {
                child_text(p, "parameterid") == Some(id)
                    || child_text(p, "name").is_some_and(|n| n.eq_ignore_ascii_case(id))
            })
        };
        if let Some(p) = find("scale") {
            let keys = read_param(p, rate);
            push_float(clip, "transform.scale", &keys, |v| v / 100.0);
        }
        if let Some(p) = find("rotation") {
            let keys = read_param(p, rate);
            push_float(clip, "transform.rotate", &keys, |v| v);
        }
        if let Some(p) = find("center") {
            let keys = read_point_param(p, rate);
            push_point(
                clip,
                "transform.translate_x",
                "transform.translate_y",
                &keys,
                frame,
            );
        }
        if let Some(p) = find("centerOffset") {
            let keys = read_point_param(p, rate);
            let source = source_frame(clip).unwrap_or(frame);
            push_point(
                clip,
                "transform.anchor_x",
                "transform.anchor_y",
                &keys,
                source,
            );
        }
        if let Some(p) = find("antiflicker") {
            let keys = read_param(p, rate);
            push_float(clip, "transform.anti_flicker", &keys, |v| v);
        }
    }

    fn opacity(&mut self, effect: &Element, clip: &mut Clip) {
        let params: Vec<&Element> = children(effect, "parameter").collect();
        let find = |id: &str| {
            params.iter().copied().find(|p| {
                child_text(p, "parameterid") == Some(id)
                    || child_text(p, "name").is_some_and(|n| n.eq_ignore_ascii_case(id))
            })
        };
        if let Some(p) = find("opacity") {
            let keys = read_param(p, rate_of(clip));
            push_float(clip, "opacity", &keys, |v| v / 100.0);
            if let Some(first) = clip
                .keyframes
                .keyframes
                .iter()
                .find(|k| k.param == "opacity")
            {
                clip.opacity = first.value;
            }
        }
    }

    fn audio_levels(&mut self, effect: &Element, clip: &mut Clip) {
        let params: Vec<&Element> = children(effect, "parameter").collect();
        let find = |id: &str| {
            params.iter().copied().find(|p| {
                child_text(p, "parameterid") == Some(id)
                    || child_text(p, "name").is_some_and(|n| n.eq_ignore_ascii_case(id))
            })
        };
        if let Some(p) = find("level") {
            let keys = read_param(p, rate_of(clip));
            push_float(clip, "volume", &keys, |v| v);
            if let Some(first) = clip
                .keyframes
                .keyframes
                .iter()
                .find(|k| k.param == "volume")
            {
                clip.volume = first.value.clamp(0.0, 4.0);
            }
        }
    }

    fn audio_pan(&mut self, effect: &Element, clip: &mut Clip) {
        let params: Vec<&Element> = children(effect, "parameter").collect();
        let find = |id: &str| {
            params.iter().copied().find(|p| {
                child_text(p, "parameterid") == Some(id)
                    || child_text(p, "name").is_some_and(|n| n.eq_ignore_ascii_case(id))
            })
        };
        if let Some(p) = find("pan") {
            let keys = read_param(p, rate_of(clip));
            push_float(clip, "pan", &keys, |v| v / 100.0);
        }
    }
}

fn clip_frame(clip: &Clip) -> (f64, f64) {
    match &clip.kind {
        ClipKind::Video(v) => (v.width.max(16) as f64, v.height.max(16) as f64),
        _ => (1920.0, 1080.0),
    }
}

fn source_frame(clip: &Clip) -> Option<(f64, f64)> {
    match &clip.kind {
        ClipKind::Video(v) if v.width > 0 && v.height > 0 => {
            Some((v.width as f64, v.height as f64))
        }
        _ => None,
    }
}

fn rate_of(clip: &Clip) -> Rate {
    match &clip.kind {
        ClipKind::Video(v) if v.fps.is_finite() && v.fps > 0.0 => Rate::from_f64(v.fps).reduced(),
        _ => Rate::new(24, 1),
    }
}

fn transition_kind(name: &str) -> TransitionKind {
    let lower = name.to_ascii_lowercase();
    match lower.as_str() {
        "cross fade" | "crossfade" => TransitionKind::CrossFade,
        "dip to color" | "dip" => TransitionKind::Dissolve,
        "cross dissolve" | "dissolve" => TransitionKind::Dissolve,
        _ if lower.contains("wipe") => TransitionKind::SlideLeft,
        _ if lower.contains("slide") => TransitionKind::SlideLeft,
        _ => TransitionKind::Dissolve,
    }
}

fn base_clip(
    kind: TrackKind,
    start_us: i64,
    dur_us: i64,
    source_in: i64,
    source_out: i64,
    path: Option<String>,
) -> Clip {
    let clip_kind = match kind {
        TrackKind::Audio => ClipKind::Audio(AudioClip {
            source_path: path.unwrap_or_default(),
            sample_rate: 48_000,
            channels: 2,
            filters: Vec::new(),
        }),
        _ => ClipKind::Video(VideoClip {
            source_path: path.unwrap_or_default(),
            width: 1920,
            height: 1080,
            fps: 24.0,
            filters: Vec::new(),
            audio_filters: Vec::new(),
            masks: Vec::new(),
        }),
    };
    let mut clip = Clip {
        id: ClipId::new(),
        timeline_start: Timestamp::from_micros(start_us),
        timeline_duration: MediaDuration::from_micros(dur_us.max(1)),
        source_start: MediaDuration::from_micros(source_in.max(0)),
        source_end: MediaDuration::from_micros(source_out.max(source_in.max(0))),
        source_total_duration: MediaDuration::from_micros(source_out.max(source_in.max(0))),
        speed: 1.0,
        volume: 1.0,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        kind: clip_kind,
        keyframes: KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    };
    clip.normalize_source_bounds_in_place();
    clip
}

fn read_param(p: &Element, rate: Rate) -> Vec<(i64, f32, Easing)> {
    let base = child_f64(p, "value").unwrap_or(0.0);
    let mut out = Vec::new();
    for keyframe in children(p, "keyframe") {
        let when = child_i64(keyframe, "when").unwrap_or(0);
        let value = child_f64(keyframe, "value").unwrap_or(base);
        out.push((
            micros_of_frames(when, rate),
            value as f32,
            keyframe_interp(keyframe),
        ));
    }
    if out.is_empty() {
        out.push((0, base as f32, Easing::Linear));
    }
    out
}

fn read_point_param(p: &Element, rate: Rate) -> Vec<(i64, (f32, f32), Easing)> {
    let (bx, by) = child(p, "value")
        .map(|v| {
            (
                child_f64(v, "horiz").unwrap_or(0.0),
                child_f64(v, "vert").unwrap_or(0.0),
            )
        })
        .unwrap_or((0.0, 0.0));
    let mut out = Vec::new();
    for keyframe in children(p, "keyframe") {
        let when = child_i64(keyframe, "when").unwrap_or(0);
        let (x, y) = keyframe
            .children
            .iter()
            .find(|c| c.name == "value")
            .map(|v| {
                (
                    child_f64(v, "horiz").unwrap_or(0.0),
                    child_f64(v, "vert").unwrap_or(0.0),
                )
            })
            .unwrap_or((bx, by));
        out.push((
            micros_of_frames(when, rate),
            (x as f32, y as f32),
            keyframe_interp(keyframe),
        ));
    }
    if out.is_empty() {
        out.push((0, (bx as f32, by as f32), Easing::Linear));
    }
    out
}

fn keyframe_interp(keyframe: &Element) -> Easing {
    match path_text(keyframe, &["interpolation", "name"])
        .map(str::to_ascii_lowercase)
        .as_deref()
    {
        Some("hold") => Easing::EaseInOut,
        Some("bezier") | Some("fcpcurve") => Easing::EaseInOut,
        _ => Easing::Linear,
    }
}

fn sort_keyframes(clip: &mut Clip) {
    clip.keyframes
        .keyframes
        .sort_by_key(|k| (k.param.clone(), k.offset.as_micros()));
}

fn push_float(clip: &mut Clip, param: &str, keys: &[(i64, f32, Easing)], map: impl Fn(f32) -> f32) {
    if keys.is_empty() {
        return;
    }
    clip.keyframes.keyframes.retain(|k| k.param != param);
    for (offset, value, easing) in keys {
        clip.keyframes.keyframes.push(Keyframe {
            param: param.to_string(),
            offset: MediaDuration::from_micros(*offset),
            value: map(*value),
            easing: *easing,
        });
    }
    sort_keyframes(clip);
}

fn push_point(
    clip: &mut Clip,
    param_x: &str,
    param_y: &str,
    keys: &[(i64, (f32, f32), Easing)],
    size: (f64, f64),
) {
    if keys.is_empty() {
        return;
    }
    for param in [param_x, param_y] {
        clip.keyframes.keyframes.retain(|k| k.param != param);
    }
    for (offset, (x, y), easing) in keys {
        clip.keyframes.keyframes.push(Keyframe {
            param: param_x.to_string(),
            offset: MediaDuration::from_micros(*offset),
            value: *x + (size.0 / 2.0) as f32,
            easing: *easing,
        });
        clip.keyframes.keyframes.push(Keyframe {
            param: param_y.to_string(),
            offset: MediaDuration::from_micros(*offset),
            value: *y + (size.1 / 2.0) as f32,
            easing: *easing,
        });
    }
    sort_keyframes(clip);
}
