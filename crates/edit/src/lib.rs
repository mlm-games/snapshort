//! Editorial logic over a word list and a track of clips: pause and filler
//! removal, through edits, gaps, paragraphs and word search.
//!
//! Everything here is pure and domain-free: words carry sequence time in
//! microseconds, clips are described by a plain [`Segment`]. The engine turns
//! these into timeline edits.

use std::ops::Range;

/// One recognised word, in sequence time (microseconds).
#[derive(Clone, Debug, PartialEq)]
pub struct Word {
    pub text: String,
    pub start_us: i64,
    pub end_us: i64,
    /// Speaker label, when the recogniser provides one.
    pub speaker: Option<String>,
    /// Recogniser confidence, 0–1.
    pub confidence: f32,
}

impl Word {
    pub fn new(text: impl Into<String>, start_us: i64, end_us: i64) -> Self {
        Self {
            text: text.into(),
            start_us,
            end_us,
            speaker: None,
            confidence: 0.0,
        }
    }

    pub fn normalized(&self) -> String {
        normalize_word(&self.text)
    }
}

/// Lowercase, trimmed to alphanumerics only, as word comparison needs.
pub fn normalize_word(s: &str) -> String {
    s.trim_matches(|c: char| !c.is_alphanumeric())
        .to_lowercase()
}

/// A time range in sequence microseconds, half open: `start..end`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Span {
    pub start_us: i64,
    pub end_us: i64,
}

impl Span {
    pub fn new(start_us: i64, end_us: i64) -> Self {
        Self { start_us, end_us }
    }

    pub fn len_us(&self) -> i64 {
        (self.end_us - self.start_us).max(0)
    }

    pub fn is_empty(&self) -> bool {
        self.len_us() == 0
    }

    pub fn contains(&self, at_us: i64) -> bool {
        at_us >= self.start_us && at_us < self.end_us
    }
}

/// Snap `at_us` to the nearest frame boundary at `fps`.
pub fn snap(at_us: i64, fps: f64) -> i64 {
    if !(fps.is_finite() && fps > 0.0) {
        return at_us;
    }
    let frame_us = (1_000_000.0 / fps).round().max(1.0) as i64;
    ((at_us as f64 / frame_us as f64).round() as i64 * frame_us).max(0)
}

/// Snap `at_us` up to the next frame boundary.
pub fn snap_after(at_us: i64, fps: f64) -> i64 {
    let snapped = snap(at_us, fps);
    if snapped < at_us {
        snapped + frame_us(fps)
    } else {
        snapped
    }
}

fn frame_us(fps: f64) -> i64 {
    if !(fps.is_finite() && fps > 0.0) {
        return 1;
    }
    (1_000_000.0 / fps).round().max(1.0) as i64
}

/// The default filler words and phrases.
pub const DEFAULT_FILLERS: &[&str] = &[
    "um", "uh", "umm", "uhm", "erm", "er", "ah", "hmm", "mm", "mhm",
];

/// Word index ranges matching one of `fillers` (each a word or a phrase such as
/// "you know"; case and punctuation ignored).
pub fn find_fillers(words: &[Word], fillers: &[String]) -> Vec<Range<usize>> {
    let norm: Vec<String> = words.iter().map(Word::normalized).collect();
    let mut phrases: Vec<Vec<String>> = fillers
        .iter()
        .map(|f| {
            f.split_whitespace()
                .map(normalize_word)
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
        })
        .filter(|p| !p.is_empty())
        .collect();
    phrases.sort_by_key(|p| std::cmp::Reverse(p.len()));

    let mut out = Vec::new();
    let mut i = 0;
    while i < norm.len() {
        let hit = phrases.iter().find(|p| {
            i + p.len() <= norm.len() && p.iter().enumerate().all(|(k, w)| norm[i + k] == *w)
        });
        match hit {
            Some(p) => {
                out.push(i..i + p.len());
                i += p.len();
            }
            None => i += 1,
        }
    }
    out
}

/// Pauses between consecutive words of at least `min_us`, as the ranges to
/// remove: each keeps `keep_us` of silence next to both words and is snapped
/// outward to frames.
pub fn find_pauses(words: &[Word], min_us: i64, keep_us: i64, fps: f64) -> Vec<Span> {
    let mut out = Vec::new();
    for pair in words.windows(2) {
        let (a, b) = (pair[0].end_us, pair[1].start_us);
        if b - a < min_us || b <= a {
            continue;
        }
        let s = snap_after(a + keep_us, fps);
        let e = snap(b - keep_us, fps);
        if e > s {
            out.push(Span::new(s, e));
        }
    }
    out
}

/// Timeline ranges removing the filler words `hits`, never cut into the
/// neighbouring words' frames.
pub fn filler_ranges(words: &[Word], hits: &[Range<usize>], fps: f64) -> Vec<Span> {
    let mut out = Vec::new();
    for hit in hits {
        let (a, b) = (hit.start, hit.end - 1);
        let mut s = snap(words[a].start_us, fps);
        let mut e = snap(words[b].end_us, fps);
        if a > 0 && s < words[a - 1].end_us {
            s = snap_after(words[a - 1].end_us, fps);
        }
        if let Some(next) = words.get(b + 1)
            && e > next.start_us
        {
            e = snap(next.start_us, fps);
        }
        if e > s {
            out.push(Span::new(s, e));
        }
    }
    out
}

/// Sort and merge ranges that overlap or touch.
pub fn merge_spans(mut spans: Vec<Span>) -> Vec<Span> {
    spans.sort_by_key(|s| s.start_us);
    let mut out: Vec<Span> = Vec::new();
    for span in spans {
        match out.last_mut() {
            Some(last) if span.start_us <= last.end_us => {
                last.end_us = last.end_us.max(span.end_us);
            }
            _ => out.push(span),
        }
    }
    out
}

/// The word under `at_us`, if any.
pub fn word_at(words: &[Word], at_us: i64) -> Option<usize> {
    words
        .iter()
        .position(|w| at_us >= w.start_us && at_us < w.end_us)
}

/// Paragraph runs: words split where the speaker changes or after a gap.
pub fn paragraphs(words: &[Word], gap_us: i64) -> Vec<Range<usize>> {
    let mut out = Vec::new();
    let mut start = 0;
    for i in 1..words.len() {
        let split = words[i].speaker != words[i - 1].speaker
            || words[i].start_us - words[i - 1].end_us >= gap_us;
        if split {
            out.push(start..i);
            start = i;
        }
    }
    if start < words.len() {
        out.push(start..words.len());
    }
    out
}

/// Word index ranges matching a query (whitespace-separated words, in order).
pub fn search(words: &[Word], query: &str) -> Vec<Range<usize>> {
    let q: Vec<String> = query
        .split_whitespace()
        .map(normalize_word)
        .filter(|s| !s.is_empty())
        .collect();
    if q.is_empty() {
        return Vec::new();
    }
    let mut out = Vec::new();
    for i in 0..words.len() {
        if i + q.len() <= words.len()
            && q.iter()
                .enumerate()
                .all(|(k, w)| words[i + k].normalized() == *w)
        {
            out.push(i..i + q.len());
        }
    }
    out
}

/// The span covering words `a..=b`, snapped to frames.
pub fn word_span(words: &[Word], a: usize, b: usize, fps: f64) -> Option<Span> {
    let first = words.get(a)?;
    let last = words.get(b)?;
    Some(Span::new(snap(first.start_us, fps), snap(last.end_us, fps)))
}

/// The total time the spans cover.
pub fn total_removed_us(spans: &[Span]) -> i64 {
    spans.iter().map(Span::len_us).sum()
}

/// One clip on a track, described for editing purposes only.
#[derive(Clone, Debug, PartialEq)]
pub struct Segment {
    pub source: String,
    pub start_us: i64,
    /// Duration on the timeline.
    pub duration_us: i64,
    /// In-point in the source media.
    pub source_in_us: i64,
    /// Out-point in the source media.
    pub source_out_us: i64,
    /// Playback speed; negative means reversed.
    pub speed: f64,
}

/// Whether the cut between adjacent segments `a` (left) and `b` (right) is a
/// through edit: same source, same speed, and `b` continues `a` in media time.
pub fn is_through_edit(a: &Segment, b: &Segment) -> bool {
    // No media source means no media time to continue: text and subtitle clips
    // carry content of their own, and joining them would silently discard it.
    if a.source.is_empty() || b.source.is_empty() {
        return false;
    }
    if a.source != b.source || a.speed != b.speed || a.speed == 0.0 {
        return false;
    }
    // Speed changes round each piece's source range, so the comparison carries a
    // little slack; filmcraft's equivalent is two ticks of its own timebase.
    let slack = (a.speed.abs() * 1.0).max(0.0) as i64;
    let expected = if a.speed > 0.0 {
        a.source_out_us
    } else {
        a.source_in_us
    };
    let actual = if a.speed > 0.0 {
        b.source_in_us
    } else {
        b.source_out_us
    };
    (actual - expected).abs() <= slack.max(1)
}

/// A cut between two pieces of the same source that continue each other.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ThroughEdit {
    /// Index of the left piece.
    pub left: usize,
    /// Index of the right piece.
    pub right: usize,
    /// Where the cut sits in sequence time.
    pub at_us: i64,
}

/// Every through edit on a track, in order.
pub fn track_through_edits(segments: &[Segment]) -> Vec<ThroughEdit> {
    let mut out = Vec::new();
    for i in 1..segments.len() {
        if is_through_edit(&segments[i - 1], &segments[i]) {
            out.push(ThroughEdit {
                left: i - 1,
                right: i,
                at_us: segments[i].start_us,
            });
        }
    }
    out
}

/// Empty time followed by material on a track.
pub fn track_gaps(segments: &[Segment]) -> Vec<Span> {
    let mut out = Vec::new();
    for pair in segments.windows(2) {
        let end = pair[0].start_us + pair[0].duration_us;
        if pair[1].start_us > end {
            out.push(Span::new(end, pair[1].start_us));
        }
    }
    out
}

/// The clip indices each span cuts through, for a ripple or lift.
pub fn segments_in(segments: &[Segment], span: Span) -> Vec<usize> {
    segments
        .iter()
        .enumerate()
        .filter(|(_, segment)| {
            let start = segment.start_us;
            let end = segment.start_us + segment.duration_us;
            span.contains(start)
                || span.contains(end - 1)
                || (span.start_us <= start && span.end_us >= end)
        })
        .map(|(i, _)| i)
        .collect()
}
