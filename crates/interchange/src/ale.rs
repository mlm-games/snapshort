//! Avid Log Exchange (`.ale`): a tab-delimited clip log (File ▸ Export ▸ Avid Log Exchange…).
//!
//! Written from Avid's published description of the format: three sections introduced by a line
//! holding only `Heading`, `Column` or `Data`, separated by blank lines.
//!
//! ```text
//! Heading
//! FIELD_DELIM  TABS
//! VIDEO_FORMAT  1080
//! AUDIO_FORMAT  48khz
//! FPS  23.976
//!
//! Column
//! (fields are separated by tab characters)
//! Name  Tracks  Start  End  Tape  Source File  Description  …
//!
//! Data
//! Beach  VA1A2  01:00:00:00  01:00:10:00  …
//! ```
//!
//! `Tracks` lists the clip's tracks (`V`, `A1`…`A16`); `Start` and `End` are SMPTE timecode at the
//! heading's `FPS`, `End` exclusive. Values never contain tabs or line breaks (they are replaced by
//! spaces). Lines end with CR LF (Avid's own files); the reader accepts LF and CR too.

use miniter_domain::{ClipKind, Timeline, TrackKind};

use crate::Rate;

/// A parsed or to-be-written ALE file.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct AleDoc {
    /// Heading fields in order (`FIELD_DELIM`, `VIDEO_FORMAT`, `AUDIO_FORMAT`, `FPS`, …).
    pub heading: Vec<(String, String)>,
    pub columns: Vec<String>,
    pub rows: Vec<Vec<String>>,
}

impl AleDoc {
    pub fn heading(&self, key: &str) -> Option<&str> {
        self.heading
            .iter()
            .find(|(k, _)| k.eq_ignore_ascii_case(key))
            .map(|(_, v)| v.as_str())
    }

    /// The value of `column` in row `row`.
    pub fn get(&self, row: usize, column: &str) -> Option<&str> {
        let c = self
            .columns
            .iter()
            .position(|x| x.eq_ignore_ascii_case(column))?;
        self.rows.get(row)?.get(c).map(String::as_str)
    }

    /// The heading's frame rate (`FPS`).
    pub fn rate(&self) -> Option<Rate> {
        self.heading("FPS")
            .and_then(|f| f.trim().parse::<f64>().ok())
            .map(Rate::from_f64)
    }
}

/// Metadata columns written after the standard ones (from the clips' overlay text).
const EXTRA: [&str; 6] = [
    "Description",
    "Scene",
    "Take",
    "Shot",
    "Log Note",
    "Comment",
];

fn clean(s: &str) -> String {
    s.replace(['\t', '\r', '\n'], " ")
}

/// `FPS` heading text for a rate.
pub fn fps_text(r: Rate) -> String {
    let f = r.as_f64();
    if (f - f.round()).abs() < 1e-6 {
        format!("{}", f.round() as i64)
    } else {
        format!("{:.3}", f).trim_end_matches('0').to_string()
    }
}

/// `VIDEO_FORMAT` heading text for a frame height.
fn video_format(height: u32) -> &'static str {
    match height {
        1080 => "1080",
        720 => "720",
        480 | 486 => "NTSC",
        576 => "PAL",
        _ => "CUSTOM",
    }
}

/// Build the log of `timeline`'s media clips (video, audio and text; other clips are skipped).
/// The heading's frame rate is the first video clip's (24 fps when there is none).
pub fn export_timeline(timeline: &Timeline, rate: Rate) -> AleDoc {
    let clips: Vec<&miniter_domain::Clip> = timeline
        .tracks
        .iter()
        .filter(|t| t.kind != TrackKind::Subtitle)
        .flat_map(|t| &t.clips)
        .collect();
    let first_video = clips.iter().find_map(|c| match &c.kind {
        ClipKind::Video(v) => Some((v.fps, v.height)),
        _ => None,
    });
    let (rate, height) = match first_video {
        Some((fps, h)) if fps.is_finite() && fps > 0.0 => (Rate::from_f64(fps), h.max(1)),
        _ => (rate, 1080),
    };
    let sample_rate = clips
        .iter()
        .find_map(|c| match &c.kind {
            ClipKind::Audio(a) => Some(a.sample_rate),
            _ => None,
        })
        .unwrap_or(48_000);
    let heading = vec![
        ("FIELD_DELIM".to_string(), "TABS".to_string()),
        ("VIDEO_FORMAT".to_string(), video_format(height).to_string()),
        (
            "AUDIO_FORMAT".to_string(),
            if sample_rate == 44_100 {
                "44khz"
            } else {
                "48khz"
            }
            .to_string(),
        ),
        ("FPS".to_string(), fps_text(rate)),
    ];
    let mut columns: Vec<String> = ["Name", "Tracks", "Start", "End", "Tape", "Source File"]
        .iter()
        .map(|s| s.to_string())
        .collect();
    columns.extend(EXTRA.iter().map(|s| s.to_string()));
    let mut rows = Vec::new();
    for clip in clips {
        let mut tracks = String::new();
        match &clip.kind {
            ClipKind::Video(_) => tracks.push('V'),
            ClipKind::Audio(audio) => {
                for c in 1..=audio.channels.min(16) {
                    tracks.push_str(&format!("A{c}"));
                }
            }
            ClipKind::Text(_) => tracks.push('V'),
            ClipKind::Subtitle(_) => tracks.push('V'),
            _ => {}
        }
        if tracks.is_empty() {
            continue;
        }
        let path = source_path(clip);
        let start = clip.timeline_start.as_micros().max(0);
        let end = clip.timeline_end().as_micros().max(start);
        let mut row = vec![
            clean(clip_name(clip).as_str()),
            tracks,
            timecode(start, rate),
            timecode(end, rate),
            String::new(),
            clean(&path),
        ];
        row.extend(EXTRA.iter().map(|_| String::new()));
        rows.push(row);
    }
    AleDoc {
        heading,
        columns,
        rows,
    }
}

/// Serialise (CR LF line ends).
pub fn write(doc: &AleDoc) -> String {
    let mut out = String::from("Heading\r\n");
    for (k, v) in &doc.heading {
        out.push_str(&format!("{}\t{}\r\n", clean(k), clean(v)));
    }
    out.push_str("\r\nColumn\r\n");
    out.push_str(
        &doc.columns
            .iter()
            .map(|c| clean(c))
            .collect::<Vec<_>>()
            .join("\t"),
    );
    out.push_str("\r\n\r\nData\r\n");
    for r in &doc.rows {
        out.push_str(&r.iter().map(|c| clean(c)).collect::<Vec<_>>().join("\t"));
        out.push_str("\r\n");
    }
    out
}

/// Read an ALE file.
pub fn parse(text: &str) -> Result<AleDoc, String> {
    #[derive(PartialEq)]
    enum Sec {
        None,
        Heading,
        Column,
        Data,
    }
    let mut doc = AleDoc::default();
    let mut sec = Sec::None;
    let text = text.replace("\r\n", "\n").replace('\r', "\n");
    for line in text.lines() {
        match line.trim() {
            "Heading" => {
                sec = Sec::Heading;
                continue;
            }
            "Column" => {
                sec = Sec::Column;
                continue;
            }
            "Data" => {
                sec = Sec::Data;
                continue;
            }
            "" => continue,
            _ => {}
        }
        match sec {
            Sec::None => return Err("not an ALE file (no Heading section)".into()),
            Sec::Heading => {
                let mut it = line.splitn(2, '\t');
                let k = it.next().unwrap_or_default().trim().to_string();
                doc.heading
                    .push((k, it.next().unwrap_or_default().trim().to_string()));
            }
            Sec::Column => doc
                .columns
                .extend(line.split('\t').map(|c| c.trim().to_string())),
            Sec::Data => doc
                .rows
                .push(line.split('\t').map(str::to_string).collect()),
        }
    }
    if doc.columns.is_empty() {
        return Err("the ALE file has no Column section".into());
    }
    Ok(doc)
}

/// Start and end (exclusive) of a data row, in microseconds at the heading's rate.
pub fn row_range(doc: &AleDoc, row: usize) -> Option<(i64, i64)> {
    let rate = doc.rate()?;
    let a = parse_timecode(doc.get(row, "Start")?, rate).ok()?;
    let b = parse_timecode(doc.get(row, "End")?, rate).ok()?;
    Some((us_of(a, rate), us_of(b, rate)))
}

/// `HH:MM:SS:FF` (or `HH:MM:SS;FF`) to a frame count at `rate`.
pub fn parse_timecode(stamp: &str, rate: Rate) -> Result<i64, String> {
    let (time, frames) = stamp
        .trim()
        .rsplit_once([':', ';'])
        .ok_or_else(|| format!("not a timecode: {stamp}"))?;
    let frames: i64 = frames
        .parse()
        .map_err(|_| format!("not a timecode: {stamp}"))?;
    let mut parts = time.split([':', ';']);
    let hours: i64 = parts
        .next()
        .ok_or_else(|| format!("not a timecode: {stamp}"))?
        .parse()
        .map_err(|_| format!("not a timecode: {stamp}"))?;
    let minutes: i64 = parts
        .next()
        .ok_or_else(|| format!("not a timecode: {stamp}"))?
        .parse()
        .map_err(|_| format!("not a timecode: {stamp}"))?;
    let seconds: i64 = parts
        .next()
        .ok_or_else(|| format!("not a timecode: {stamp}"))?
        .parse()
        .map_err(|_| format!("not a timecode: {stamp}"))?;
    let fps = frames_per_second(rate);
    let total = frames + seconds * fps + minutes * 60 * fps + hours * 3600 * fps;
    Ok(total)
}

fn us_of(frames: i64, rate: Rate) -> i64 {
    let num = rate.num as i128;
    let den = rate.den as i128;
    ((frames as i128 * 1_000_000 * den) / num) as i64
}

fn frames_per_second(rate: Rate) -> i64 {
    ((rate.num + rate.den / 2) / rate.den.max(1)).max(1) as i64
}

/// Microseconds at `rate` to `HH:MM:SS:FF`.
pub fn timecode(us: i64, rate: Rate) -> String {
    let us = us.max(0);
    let frames = ((us as i128 * rate.num as i128) / (1_000_000 * rate.den as i128)) as i64;
    let fps = frames_per_second(rate);
    let seconds = frames / fps;
    let f = frames % fps;
    let (h, m, s) = (seconds / 3600, (seconds / 60) % 60, seconds % 60);
    format!("{h:02}:{m:02}:{s:02}:{f:02}")
}

fn source_path(clip: &miniter_domain::Clip) -> String {
    match &clip.kind {
        ClipKind::Video(v) => v.source_path.clone(),
        ClipKind::Audio(a) => a.source_path.clone(),
        ClipKind::Subtitle(s) => s.source_path.clone(),
        ClipKind::Text(_) => String::new(),
        _ => String::new(),
    }
}

fn clip_name(clip: &miniter_domain::Clip) -> String {
    let path = source_path(clip);
    if path.is_empty() {
        return match &clip.kind {
            ClipKind::Text(t) => t.text.clone(),
            _ => "Clip".to_string(),
        };
    }
    file_stem(&path)
}

fn file_stem(path: &str) -> String {
    let name = path.rsplit(['/', '\\']).next().unwrap_or(path);
    match name.rfind('.') {
        Some(i) if i > 0 => name[..i].to_string(),
        _ => name.to_string(),
    }
}
