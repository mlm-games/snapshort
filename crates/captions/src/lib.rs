//! Snapshot caption formats.
//!
//! - **Files:** [SubRip](srt) (`.srt`), [WebVTT](vtt) (`.vtt`), [Scenarist SCC](scc) (`.scc`,
//!   CEA-608 pop-on, roll-up and paint-on on read; pop-on on write), [MacCaption MCC](mcc)
//!   (`.mcc`, SMPTE ST 334-2 packets with CEA-608 and CEA-708), [EBU STL](stl) (`.stl`, EBU
//!   Tech 3264) and [TTML](ttml) (`.ttml` IMSC1 Text profile, `.dfxp` DFXP) parse into a
//!   [`Document`] of [`Cue`]s and write back. Readers are forgiving (BOM, UTF-16, Windows-1252 fallback, CRLF/CR,
//!   missing SRT indexes, `,` or `.` milliseconds, missing hours, no blank line between cues);
//!   writers are strict. WebVTT cue identifiers, cue settings, `<v Speaker>` voices and
//!   STYLE/REGION/NOTE blocks are kept.
//! - **Time:** cue times are exact microseconds. SRT/VTT milliseconds and SCC 29.97 fps timecodes
//!   (drop-frame or not) convert exactly; [`Document::snap_to_frames`] snaps to a sequence's frames.

pub mod cea608;
pub mod mcc;
pub mod scc;
pub mod srt;
pub mod stl;
pub mod time;
pub mod ttml;
pub mod vtt;

use crate::time::{FrameRate, TICKS_PER_SECOND};

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Format {
    Srt,
    WebVtt,
    Scc,
    Mcc,
    Stl,
    Ttml,
    Dfxp,
}

impl Format {
    pub const ALL: [Format; 7] = [
        Format::Srt,
        Format::WebVtt,
        Format::Scc,
        Format::Mcc,
        Format::Stl,
        Format::Ttml,
        Format::Dfxp,
    ];
    pub fn name(self) -> &'static str {
        match self {
            Format::Srt => "SubRip (SRT)",
            Format::WebVtt => "WebVTT",
            Format::Scc => "Scenarist SCC (CEA-608)",
            Format::Mcc => "MacCaption MCC (CEA-608/708)",
            Format::Stl => "EBU STL",
            Format::Ttml => "TTML (IMSC1)",
            Format::Dfxp => "DFXP (TTML1)",
        }
    }
    pub fn extension(self) -> &'static str {
        match self {
            Format::Srt => "srt",
            Format::WebVtt => "vtt",
            Format::Scc => "scc",
            Format::Mcc => "mcc",
            Format::Stl => "stl",
            Format::Ttml => "ttml",
            Format::Dfxp => "dfxp",
        }
    }
    pub fn from_name(s: &str) -> Option<Format> {
        match s.trim_start_matches('.').to_ascii_lowercase().as_str() {
            "srt" | "subrip" => Some(Format::Srt),
            "vtt" | "webvtt" => Some(Format::WebVtt),
            "scc" | "cea608" | "cea-608" => Some(Format::Scc),
            "mcc" | "maccaption" | "cea708" | "cea-708" => Some(Format::Mcc),
            "stl" | "ebu-stl" | "ebustl" | "ebu" => Some(Format::Stl),
            "ttml" | "imsc" | "imsc1" | "xml" => Some(Format::Ttml),
            "dfxp" => Some(Format::Dfxp),
            _ => None,
        }
    }
    pub fn is_binary(self) -> bool {
        self == Format::Stl
    }
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum Error {
    #[error("not a {0} file")]
    NotFormat(&'static str),
    #[error("line {line}: {msg}")]
    Syntax { line: usize, msg: String },
}

pub type Result<T> = std::result::Result<T, Error>;

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Cue {
    pub start: i64,
    pub end: i64,
    pub text: String,
    pub speaker: Option<String>,
    pub id: Option<String>,
    pub settings: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Document {
    pub cues: Vec<Cue>,
    pub blocks: Vec<String>,
    pub warnings: Vec<String>,
}

impl Document {
    pub fn snap_to_frames(&mut self, rate: FrameRate) {
        for c in &mut self.cues {
            c.start = rate.snap_nearest(c.start);
            c.end = rate
                .snap_nearest(c.end)
                .max(c.start + rate.frame_duration());
        }
        self.normalize();
    }

    pub fn normalize(&mut self) {
        self.cues.retain(|c| c.end > c.start);
        self.cues.sort_by_key(|c| (c.start, c.end));
        let mut out: Vec<Cue> = Vec::with_capacity(self.cues.len());
        for c in self.cues.drain(..) {
            let mut c = c;
            if let Some(prev) = out.last_mut()
                && c.start < prev.end
            {
                if c.start > prev.start {
                    prev.end = c.start;
                } else {
                    c.start = prev.end;
                    if c.end <= c.start {
                        self.warnings.push(format!(
                            "dropped overlapping cue \"{}\"",
                            c.text.lines().next().unwrap_or("")
                        ));
                        continue;
                    }
                }
            }
            out.push(c);
        }
        self.cues = out;
    }
}

pub fn detect(bytes: &[u8], ext: Option<&str>) -> Option<Format> {
    if stl::sniff(bytes) {
        return Some(Format::Stl);
    }
    let text = decode_text(&bytes[..bytes.len().min(4096)]);
    let head = text.trim_start_matches('\u{feff}').trim_start();
    if head.starts_with("WEBVTT") {
        return Some(Format::WebVtt);
    }
    if head.starts_with("Scenarist_SCC") {
        return Some(Format::Scc);
    }
    if head.starts_with("File Format=MacCaption_MCC") {
        return Some(Format::Mcc);
    }
    let ext = ext.map(|e| e.trim_start_matches('.').to_ascii_lowercase());
    if (head.starts_with('<')
        && (head.contains("<tt ") || head.contains("<tt>") || head.contains(":tt ")))
        && (head.contains("ns/ttml") || head.contains("ttaf1"))
    {
        let legacy = head.contains("ttaf1") || head.contains("dfxp");
        return Some(
            if ext.as_deref() == Some("dfxp") || (legacy && ext.as_deref() != Some("ttml")) {
                Format::Dfxp
            } else {
                Format::Ttml
            },
        );
    }
    let has_arrow_timing = head
        .lines()
        .take(40)
        .any(|l| srt::parse_timing_line(l).is_some());
    match ext.as_deref() {
        Some("srt") if has_arrow_timing || head.is_empty() => Some(Format::Srt),
        Some("vtt") if has_arrow_timing => Some(Format::WebVtt),
        Some("srt" | "vtt" | "scc" | "mcc" | "stl" | "ttml" | "dfxp" | "xml") => None,
        _ if has_arrow_timing => Some(Format::Srt),
        _ => None,
    }
}

pub fn parse(bytes: &[u8], format: Format) -> Result<Document> {
    if format == Format::Stl {
        return stl::parse(bytes);
    }
    let text = decode_text(bytes);
    match format {
        Format::Srt => srt::parse(&text),
        Format::WebVtt => vtt::parse(&text),
        Format::Scc => scc::parse(&text),
        Format::Mcc => mcc::parse(&text),
        Format::Ttml | Format::Dfxp => ttml::parse(&text),
        Format::Stl => stl::parse(bytes),
    }
}

#[derive(Clone, Copy, Debug)]
pub struct WriteOptions {
    pub drop_frame: bool,
    pub rate: Option<FrameRate>,
}

impl Default for WriteOptions {
    fn default() -> Self {
        Self {
            drop_frame: true,
            rate: None,
        }
    }
}

pub fn write(doc: &Document, format: Format, opts: WriteOptions) -> Vec<u8> {
    match format {
        Format::Srt => srt::write(doc).into_bytes(),
        Format::WebVtt => vtt::write(doc).into_bytes(),
        Format::Scc => scc::write(doc, opts.drop_frame).into_bytes(),
        Format::Mcc => mcc::write(doc, opts.drop_frame).into_bytes(),
        Format::Stl => stl::write(doc, opts.rate),
        Format::Ttml => ttml::write(doc, ttml::Flavor::Imsc1, opts.rate, "en").into_bytes(),
        Format::Dfxp => ttml::write(doc, ttml::Flavor::Dfxp, opts.rate, "en").into_bytes(),
    }
}

pub fn decode_text(bytes: &[u8]) -> String {
    let s = if let Some(rest) = bytes.strip_prefix(&[0xef, 0xbb, 0xbf]) {
        String::from_utf8_lossy(rest).into_owned()
    } else if bytes.starts_with(&[0xff, 0xfe]) || bytes.starts_with(&[0xfe, 0xff]) {
        let le = bytes[0] == 0xff;
        let units: Vec<u16> = bytes[2..]
            .as_chunks::<2>()
            .0
            .iter()
            .map(|c| {
                if le {
                    u16::from_le_bytes([c[0], c[1]])
                } else {
                    u16::from_be_bytes([c[0], c[1]])
                }
            })
            .collect();
        String::from_utf16_lossy(&units)
    } else {
        match std::str::from_utf8(bytes) {
            Ok(s) => s.to_string(),
            Err(_) => bytes.iter().map(|&b| cp1252(b)).collect(),
        }
    };
    let s = s.strip_prefix('\u{feff}').map(str::to_string).unwrap_or(s);
    if s.contains('\r') {
        s.replace("\r\n", "\n").replace('\r', "\n")
    } else {
        s
    }
}

fn cp1252(b: u8) -> char {
    const HI: [char; 32] = [
        '€', '\u{81}', '‚', 'ƒ', '„', '…', '†', '‡', 'ˆ', '‰', 'Š', '‹', 'Œ', '\u{8d}', 'Ž',
        '\u{8f}', '\u{90}', '‘', '’', '“', '”', '•', '–', '—', '˜', '™', 'š', '›', 'œ', '\u{9d}',
        'ž', 'Ÿ',
    ];
    if (0x80..0xa0).contains(&b) {
        HI[(b - 0x80) as usize]
    } else {
        b as char
    }
}

pub(crate) const TICKS_PER_MS: i64 = TICKS_PER_SECOND / 1000;

pub fn parse_clock(s: &str) -> Option<i64> {
    let s = s.trim();
    let (hms, frac) = match s.find([',', '.']) {
        Some(i) => (&s[..i], &s[i + 1..]),
        None => (s, ""),
    };
    let parts: Vec<&str> = hms.split(':').collect();
    let num = |p: &str| -> Option<i64> {
        if !p.is_empty() && p.len() <= 9 && p.bytes().all(|b| b.is_ascii_digit()) {
            p.parse().ok()
        } else {
            None
        }
    };
    let (h, m, sec) = match parts.as_slice() {
        [h, m, s] => (num(h)?, num(m)?, num(s)?),
        [m, s] => (0, num(m)?, num(s)?),
        _ => return None,
    };
    if m >= 60 || sec >= 60 || h > 9999 {
        return None;
    }
    let mut ticks = ((h * 60 + m) * 60 + sec) * TICKS_PER_SECOND;
    if !frac.is_empty() {
        if frac.len() > 9 || !frac.bytes().all(|b| b.is_ascii_digit()) {
            return None;
        }
        let d: i64 = frac.parse().ok()?;
        ticks += (d as i128 * TICKS_PER_SECOND as i128 / 10i128.pow(frac.len() as u32)) as i64;
    }
    Some(ticks)
}

pub fn format_clock(t: i64, sep: char) -> String {
    let ms = (t.max(0) + TICKS_PER_MS / 2) / TICKS_PER_MS;
    let (h, m, s, f) = (
        ms / 3_600_000,
        (ms / 60_000) % 60,
        (ms / 1000) % 60,
        ms % 1000,
    );
    format!("{h:02}:{m:02}:{s:02}{sep}{f:03}")
}

pub fn plain_text(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    let mut chars = s.chars().peekable();
    while let Some(c) = chars.next() {
        match c {
            '<' => {
                let rest: String = chars.clone().collect();
                if let Some(end) = rest.find('>') {
                    for _ in 0..rest[..=end].chars().count() {
                        chars.next();
                    }
                } else {
                    out.push('<');
                }
            }
            '{' if chars.peek() == Some(&'\\') => {
                let rest: String = chars.clone().collect();
                if let Some(end) = rest.find('}') {
                    for _ in 0..rest[..=end].chars().count() {
                        chars.next();
                    }
                } else {
                    out.push('{');
                }
            }
            '&' => {
                let rest: String = chars.clone().take(8).collect();
                let ents = [
                    ("amp;", '&'),
                    ("lt;", '<'),
                    ("gt;", '>'),
                    ("nbsp;", '\u{a0}'),
                    ("quot;", '"'),
                    ("apos;", '\''),
                    ("lrm;", '\u{200e}'),
                    ("rlm;", '\u{200f}'),
                ];
                if let Some((name, ch)) = ents.iter().find(|(n, _)| rest.starts_with(n)) {
                    for _ in 0..name.len() {
                        chars.next();
                    }
                    out.push(*ch);
                } else {
                    out.push('&');
                }
            }
            '\r' => {}
            _ => out.push(c),
        }
    }
    out
}
