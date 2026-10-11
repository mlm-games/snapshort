//! Transcripts, dead space and filler removal, through edits and gaps.
//!
//! A transcript is a word list in sequence time. Words come either from a
//! recogniser (imported as JSON) or from the audio's own speech runs, so
//! dead-space removal needs no model: pause detection works on the gaps
//! between speech, which is what a listening pass removes anyway.

use serde_json::{Value, json};
use snapshort_edit::{Word, find_pauses, merge_spans};
use snapshort_infra_audio::AudioDspService;

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("transcript.words", "Transcript Words", "{}", run_words),
        cmd!(
            "transcript.derive",
            "Derive Speech Runs",
            [],
            None::<&'static str>,
            "{\"thresholdDb\"?: number, \"minSpeechMs\"?: number, \"minSilenceMs\"?: number}",
            has_project,
            run_derive
        ),
        cmd!(
            "transcript.import",
            "Import a Transcript",
            [],
            None::<&'static str>,
            "{\"path\": string}",
            has_project,
            run_import
        ),
        query!(
            "deadspace.find",
            "Find Dead Space",
            "{}",
            run_deadspace_find
        ),
        cmd!(
            "deadspace.remove",
            "Remove Dead Space",
            [],
            None::<&'static str>,
            "{\"minUs\"?: int, \"keepUs\"?: int}",
            has_project,
            run_deadspace_remove
        ),
        cmd!(
            "fillers.remove",
            "Remove Filler Words",
            [],
            None::<&'static str>,
            "{\"fillers\"?: [string]}",
            has_project,
            run_fillers
        ),
        query!("gaps.find", "Find Gaps", "{}", run_gaps),
        query!("through.find", "Find Through Edits", "{}", run_through),
    ]
}

fn run_words<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let words = s.transcript().await;
        Ok(json!({
            "count": words.len(),
            "words": words.iter().map(|w| json!({
                "text": w.text,
                "startUs": w.start_us,
                "endUs": w.end_us,
                "speaker": w.speaker,
                "confidence": w.confidence,
            })).collect::<Vec<_>>(),
        }))
    })
}

fn run_derive<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let chain = snapshort_infra_audio::AudioDspService::chain(
            &s.services().project.audio_chain().await,
        )?;
        let (volumes, master) = super::audio::volumes_of(p);
        let mixed = s
            .services()
            .audio
            .mix(&timeline, &volumes, master)
            .map_err(EngineError::from)?;
        let threshold = params::f64_p(p, "thresholdDb").unwrap_or(-45.0);
        let min_speech = params::f64_p(p, "minSpeechMs").unwrap_or(150.0);
        let min_silence = params::f64_p(p, "minSilenceMs").unwrap_or(300.0);
        let words = snapshort_infra_audio::speech_segments(
            &mixed.samples,
            mixed.channels,
            mixed.sample_rate,
            threshold,
            min_speech,
            min_silence,
        );
        s.set_transcript(words.clone()).await;
        let chain_json = AudioDspService::chain_report(&chain);
        Ok(json!({
            "count": words.len(),
            "source": "speech runs",
            "thresholdDb": threshold,
            "chainApplied": chain_json["count"],
            "durationSeconds": mixed.samples.len() as f64
                / (mixed.sample_rate as f64 * mixed.channels.max(1) as f64),
        }))
    })
}

fn run_import<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = params::require_file(p, "path", "transcript.import")?;
        let text = std::fs::read_to_string(&path)
            .map_err(|why| EngineError::bad("transcript.import", why.to_string()))?;
        let raw: Value = serde_json::from_str(&text)
            .map_err(|why| EngineError::bad("transcript.import", why.to_string()))?;
        let entries = raw.as_array().ok_or_else(|| {
            EngineError::bad("transcript.import", "expected a JSON array of words")
        })?;
        let mut words = Vec::with_capacity(entries.len());
        for entry in entries {
            words.push(Word::new(
                entry["text"].as_str().unwrap_or_default(),
                entry["start"].as_f64().map(us_from_seconds).unwrap_or(0),
                entry["end"].as_f64().map(us_from_seconds).unwrap_or(0),
            ));
        }
        s.set_transcript(words.clone()).await;
        Ok(json!({"count": words.len(), "source": path.display().to_string()}))
    })
}

fn us_from_seconds(seconds: f64) -> i64 {
    (seconds * 1_000_000.0).round() as i64
}

fn run_deadspace_find<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let words = s.transcript().await;
        if words.len() < 2 {
            return Err(EngineError::bad(
                "deadspace.find",
                "no words yet; run `transcript.derive` or `transcript.import` first".to_string(),
            ));
        }
        let min_us = params::i64_p(p, "minUs").unwrap_or(300_000);
        let keep_us = params::i64_p(p, "keepUs").unwrap_or(120_000);
        let spans = merge_spans(find_pauses(&words, min_us, keep_us, s.fps() as f64));
        Ok(json!({
            "count": spans.len(),
            "removedSeconds": spans.iter().map(|span| span.len_us()).sum::<i64>() as f64 / 1e6,
            "spans": spans.iter().map(|span| json!({
                "startUs": span.start_us,
                "endUs": span.end_us,
            })).collect::<Vec<_>>(),
        }))
    })
}

fn run_deadspace_remove<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let report = deadspace_find(s, p).await?;
        let spans = report["spans"].as_array().cloned().unwrap_or_default();
        if spans.is_empty() {
            return Ok(json!({"removed": 0, "message": "no dead space to remove"}));
        }
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let ranges: Vec<super::timeline::Range> = spans
            .iter()
            .map(|span| {
                super::timeline::Range::new(
                    miniter_domain::Timestamp::from_micros(span["startUs"].as_i64().unwrap_or(0)),
                    miniter_domain::Timestamp::from_micros(span["endUs"].as_i64().unwrap_or(0)),
                )
            })
            .collect();
        let commands = super::timeline::remove_spans_commands(
            &timeline,
            &ranges,
            true,
            params::track_p(p, "track"),
            &[],
        );
        super::apply_batch(s, commands, "Remove Dead Space").await?;
        Ok(json!({
            "removed": spans.len(),
            "removedSeconds": report["removedSeconds"],
        }))
    })
}

async fn deadspace_find(s: &Session, p: &Value) -> crate::Result<Value> {
    let words = s.transcript().await;
    if words.len() < 2 {
        return Err(EngineError::bad(
            "deadspace.remove",
            "no words yet; run `transcript.derive` or `transcript.import` first".to_string(),
        ));
    }
    let min_us = params::i64_p(p, "minUs").unwrap_or(300_000);
    let keep_us = params::i64_p(p, "keepUs").unwrap_or(120_000);
    let spans = merge_spans(find_pauses(&words, min_us, keep_us, s.fps() as f64));
    Ok(json!({
        "count": spans.len(),
        "removedSeconds": spans.iter().map(|span| span.len_us()).sum::<i64>() as f64 / 1e6,
        "spans": spans.iter().map(|span| json!({
            "startUs": span.start_us,
            "endUs": span.end_us,
        })).collect::<Vec<_>>(),
    }))
}

fn run_fillers<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let words = s.transcript().await;
        let fillers: Vec<String> = params::optional_object(p, "fillers", "fillers.remove")?
            .unwrap_or_else(|| {
                snapshort_edit::DEFAULT_FILLERS
                    .iter()
                    .map(|f| f.to_string())
                    .collect()
            });
        let hits = snapshort_edit::find_fillers(&words, &fillers);
        if hits.is_empty() {
            return Ok(json!({"removed": 0, "message": "no filler words in the transcript"}));
        }
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let spans = merge_spans(snapshort_edit::filler_ranges(&words, &hits, s.fps() as f64));
        let ranges: Vec<super::timeline::Range> = spans
            .iter()
            .map(|span| {
                super::timeline::Range::new(
                    miniter_domain::Timestamp::from_micros(span.start_us),
                    miniter_domain::Timestamp::from_micros(span.end_us),
                )
            })
            .collect();
        let commands = super::timeline::remove_spans_commands(&timeline, &ranges, true, None, &[]);
        super::apply_batch(s, commands, "Remove Fillers").await?;
        Ok(json!({
            "removed": hits.len(),
            "removedSeconds": spans.iter().map(|span| span.len_us()).sum::<i64>() as f64 / 1e6,
        }))
    })
}

fn run_gaps<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let track = params::track_p(p, "track");
        let spans = gaps_of(&timeline, track);
        Ok(json!({
            "count": spans.len(),
            "gaps": spans.iter().map(|span| json!({
                "startUs": span.start_us,
                "endUs": span.end_us,
            })).collect::<Vec<_>>(),
        }))
    })
}

fn run_through<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let mut edits = Vec::new();
        for track in &timeline.tracks {
            let segments = segments_of(&track.clips);
            for edit in snapshort_edit::track_through_edits(&segments) {
                edits.push(json!({
                    "track": format!("{:?}", track.id.0),
                    "trackName": track.name,
                    "left": edit.left,
                    "right": edit.right,
                    "atUs": edit.at_us,
                }));
            }
        }
        Ok(json!({"count": edits.len(), "throughEdits": edits}))
    })
}

fn segments_of(clips: &[miniter_domain::Clip]) -> Vec<snapshort_edit::Segment> {
    clips
        .iter()
        .map(|clip| {
            let (source, in_us, out_us) = match &clip.kind {
                miniter_domain::ClipKind::Video(video) => (
                    video.source_path.clone(),
                    clip.source_start.as_micros(),
                    clip.source_end.as_micros(),
                ),
                miniter_domain::ClipKind::Audio(audio) => (
                    audio.source_path.clone(),
                    clip.source_start.as_micros(),
                    clip.source_end.as_micros(),
                ),
                _ => (String::new(), 0, 0),
            };
            snapshort_edit::Segment {
                source,
                start_us: clip.timeline_start.as_micros(),
                duration_us: clip.timeline_duration.as_micros(),
                source_in_us: in_us,
                source_out_us: out_us,
                speed: clip.speed as f64,
            }
        })
        .collect()
}

fn gaps_of(
    timeline: &miniter_domain::Timeline,
    track: Option<miniter_domain::TrackId>,
) -> Vec<snapshort_edit::Span> {
    timeline
        .tracks
        .iter()
        .filter(|t| track.is_none_or(|id| t.id == id))
        .flat_map(|t| snapshort_edit::track_gaps(&segments_of(&t.clips)))
        .collect()
}
