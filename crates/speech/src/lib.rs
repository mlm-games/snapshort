//! Speech-to-text for snapshort's transcript.
//!
//! - [`Transcriber`]: the trait every speech recogniser implements. It takes mono 16 kHz samples
//!   and returns a [`Transcript`] whose words carry media-time bounds (microseconds; one 16 kHz
//!   sample is exactly 62.5 µs, so sample ↔ time stays rational).
//! - [`models`]: the catalogue of downloadable Whisper models (source URL pinned to a revision,
//!   SHA-256, size, licence), where they live in the per-user data directory, and (feature
//!   `download`) the verified downloader. Weights are **never** bundled or committed.
//! - [`whisper`] (feature `whisper`): Whisper inference in pure Rust on candle, with timestamp
//!   decoding, language detection and word-level timestamps from cross-attention alignment (DTW).
//! - [`diarize`]: speaker labelling by clustering per-chunk MFCC statistics (classical, no model).
//! - [`mel`]: the log-mel front end shared by Whisper and diarization.
//! - [`vad`]: energy-based tightening of word bounds (keeps pauses out of words).
//!
//! [`snapshort_edit::Word`] is the word type: a transcript hands its words straight to the
//! editorial layer, which is why a word's speaker is a label and not an index.

#![cfg_attr(
    not(test),
    deny(
        clippy::unwrap_used,
        clippy::expect_used,
        clippy::panic,
        clippy::unimplemented,
        clippy::todo,
        clippy::unreachable
    )
)]

pub mod diarize;
pub mod mel;
pub mod models;
pub mod vad;
#[cfg(feature = "whisper")]
pub mod whisper;

pub use snapshort_edit::Word;

use snapshort_edit::normalize_word;

pub const US_PER_SECOND: i64 = 1_000_000;

pub const SAMPLE_RATE: u32 = 16_000;

pub fn sample_us(n: i64) -> i64 {
    (n as i128 * 1_000_000 / SAMPLE_RATE as i128) as i64
}

pub fn seconds_us(s: f64) -> i64 {
    sample_us((s * SAMPLE_RATE as f64).round() as i64)
}

#[derive(Debug, Clone, PartialEq, thiserror::Error)]
pub enum SpeechError {
    #[error("stopped")]
    Cancelled,
    #[error("the speech model `{0}` is not downloaded")]
    NotInstalled(String),
    #[error("unknown speech model `{0}`")]
    UnknownModel(String),
    #[error("speech-to-text is not available in this build ({0})")]
    Unavailable(String),
    #[error("model: {0}")]
    Model(String),
    #[error("download: {0}")]
    Download(String),
    #[error("{0}")]
    Io(String),
}

impl From<std::io::Error> for SpeechError {
    fn from(e: std::io::Error) -> Self {
        SpeechError::Io(e.to_string())
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub language: Option<String>,
    pub diarize: bool,
    pub max_speakers: usize,
}

impl Default for Options {
    fn default() -> Self {
        Self {
            language: None,
            diarize: true,
            max_speakers: 6,
        }
    }
}

pub type ProgressFn<'a> = &'a mut dyn FnMut(f32, &str) -> bool;

pub trait Transcriber: Send + Sync {
    fn id(&self) -> String;
    fn transcribe(
        &self,
        audio: &[f32],
        opts: &Options,
        progress: ProgressFn,
    ) -> Result<Transcript, SpeechError>;
}

pub struct FixedTranscriber {
    pub transcript: Transcript,
    pub id: String,
}

impl Transcriber for FixedTranscriber {
    fn id(&self) -> String {
        self.id.clone()
    }
    fn transcribe(
        &self,
        audio: &[f32],
        _: &Options,
        progress: ProgressFn,
    ) -> Result<Transcript, SpeechError> {
        if !progress(0.5, "Transcribing") {
            return Err(SpeechError::Cancelled);
        }
        let end = sample_us(audio.len() as i64);
        let mut t = self.transcript.clone();
        t.words.retain(|w| w.start_us < end);
        t.source = self.id.clone();
        progress(1.0, "Done");
        Ok(t)
    }
}

pub fn load(
    models_dir: &std::path::Path,
    id: &str,
) -> Result<std::sync::Arc<dyn Transcriber>, SpeechError> {
    #[cfg(feature = "whisper")]
    {
        let m = models::find(id).ok_or_else(|| SpeechError::UnknownModel(id.into()))?;
        if !models::installed(models_dir, m) {
            return Err(SpeechError::NotInstalled(id.into()));
        }
        Ok(std::sync::Arc::new(whisper::Whisper::load(
            &models::model_dir(models_dir, m),
            m.id,
        )?))
    }
    #[cfg(not(feature = "whisper"))]
    {
        let _ = (models_dir, id);
        Err(SpeechError::Unavailable(
            "built without the `whisper` feature".into(),
        ))
    }
}

pub const fn available() -> bool {
    cfg!(feature = "whisper")
}

pub fn downmix(channels: &[Vec<f32>]) -> Vec<f32> {
    let n = channels.iter().map(Vec::len).min().unwrap_or(0);
    if channels.len() == 1 {
        return channels[0][..n].to_vec();
    }
    let k = 1.0 / channels.len().max(1) as f32;
    (0..n)
        .map(|i| channels.iter().map(|c| c[i]).sum::<f32>() * k)
        .collect()
}

pub fn word_error_rate(reference: &str, hypothesis: &str) -> f64 {
    let norm = |s: &str| -> Vec<String> {
        s.split(|c: char| c.is_whitespace() || c == '-')
            .map(normalize_word)
            .filter(|w| !w.is_empty())
            .collect()
    };
    let (r, h) = (norm(reference), norm(hypothesis));
    if r.is_empty() {
        return if h.is_empty() { 0.0 } else { 1.0 };
    }
    let mut prev: Vec<usize> = (0..=h.len()).collect();
    for i in 1..=r.len() {
        let mut cur = vec![i; h.len() + 1];
        for j in 1..=h.len() {
            let sub = prev[j - 1] + usize::from(r[i - 1] != h[j - 1]);
            cur[j] = sub.min(prev[j] + 1).min(cur[j - 1] + 1);
        }
        prev = cur;
    }
    prev[h.len()] as f64 / r.len() as f64
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Speaker {
    pub name: String,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Transcript {
    pub language: String,
    pub source: String,
    pub speakers: Vec<Speaker>,
    pub words: Vec<Word>,
}

impl Transcript {
    pub fn text(&self) -> String {
        self.words
            .iter()
            .map(|w| w.text.as_str())
            .collect::<Vec<_>>()
            .join(" ")
    }

    pub fn normalize(&mut self) {
        self.words.retain(|w| !w.text.trim().is_empty());
        for w in &mut self.words {
            w.text = w.text.trim().to_string();
            if w.end_us < w.start_us {
                w.end_us = w.start_us;
            }
            w.confidence = w.confidence.clamp(0.0, 1.0);
        }
        self.words.sort_by_key(|w| (w.start_us, w.end_us));
        for i in 1..self.words.len() {
            let s = self.words[i].start_us;
            if self.words[i - 1].end_us > s {
                self.words[i - 1].end_us = s.max(self.words[i - 1].start_us);
            }
        }
        for w in &self.words {
            if let Some(label) = &w.speaker
                && !self.speakers.iter().any(|s| &s.name == label)
            {
                self.speakers.push(Speaker {
                    name: label.clone(),
                });
            }
        }
    }

    pub fn check(&self) -> Result<(), String> {
        for (i, w) in self.words.iter().enumerate() {
            if w.end_us < w.start_us {
                return Err(format!("word {i} ({:?}) ends before it starts", w.text));
            }
            if i > 0 && self.words[i - 1].end_us > w.start_us {
                return Err(format!(
                    "word {i} ({:?}) overlaps the word before it",
                    w.text
                ));
            }
            if let Some(s) = &w.speaker
                && !self.speakers.iter().any(|k| &k.name == s)
            {
                return Err(format!(
                    "word {i} ({:?}) has speaker {s:?}, but there are {} speakers",
                    w.text,
                    self.speakers.len()
                ));
            }
        }
        Ok(())
    }
}

impl From<Transcript> for Vec<Word> {
    fn from(t: Transcript) -> Self {
        t.words
    }
}

impl From<Vec<Word>> for Transcript {
    fn from(words: Vec<Word>) -> Self {
        let mut t = Self {
            words,
            ..Default::default()
        };
        t.normalize();
        t
    }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sample_times_are_exact() {
        assert_eq!(sample_us(SAMPLE_RATE as i64), 1_000_000);
        assert_eq!(seconds_us(1.5), 1_000_000 * 3 / 2);
    }

    #[test]
    fn wer_counts_edits() {
        assert_eq!(word_error_rate("the cat sat", "The cat, sat."), 0.0);
        assert!(
            (word_error_rate("the cat sat on the mat", "the cat sat on mat") - 1.0 / 6.0).abs()
                < 1e-9
        );
        assert!((word_error_rate("a b", "a x b y") - 1.0).abs() < 1e-9);
    }

    #[test]
    fn fixed_transcriber_trims_and_cancels() {
        let mut t = Transcript::default();
        t.words
            .push(Word::new("hi", seconds_us(0.1), seconds_us(0.3)));
        t.words
            .push(Word::new("late", seconds_us(5.0), seconds_us(5.5)));
        let f = FixedTranscriber {
            transcript: t,
            id: "fixed".into(),
        };
        let out = f
            .transcribe(&vec![0.0; 16_000], &Options::default(), &mut |_, _| true)
            .unwrap();
        assert_eq!(out.words.len(), 1);
        assert_eq!(out.source, "fixed");
        assert_eq!(
            f.transcribe(&[], &Options::default(), &mut |_, _| false),
            Err(SpeechError::Cancelled)
        );
    }

    #[test]
    fn downmix_averages() {
        assert_eq!(
            downmix(&[vec![1.0, 0.0], vec![0.0, 0.0, 9.0]]),
            vec![0.5, 0.0]
        );
    }
}
