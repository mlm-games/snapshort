//! Structural timeline invariants.
//!
//! The reducer enforces these on the way in (`can_insert_clip`,
//! `ensure_clip_source_bounds`, …), but every layer that reads a timeline —
//! render-graph construction, export, cache keys — silently assumes them.
//! [`validate_timeline`] re-checks a fully built timeline so tests and
//! `debug_assert`s catch regressions at the point of use, not three layers
//! down as a corrupt render.
//!
//! Rules (per track, clips in stored order):
//! - clips sorted by `timeline_start`, no overlaps (touching edges allowed);
//! - `timeline_start >= 0`, `timeline_duration > 0`;
//! - `source_start >= 0`, `source_end >= source_start`;
//! - `source_end <= source_total_duration` when a total is known
//!   (text/subtitle clips are exempt, mirroring the reducer);
//! - `speed` finite and positive;
//! - track ids unique.

use miniter_domain::clip::ClipKind;
use miniter_domain::timeline::Timeline;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum TimelineViolation {
    DuplicateTrackId { id: Uuid },
    ClipUnsorted { track_index: usize, clip_id: Uuid },
    ClipOverlap { track_index: usize, a: Uuid, b: Uuid },
    NegativeTimelineStart { clip_id: Uuid },
    NonPositiveDuration { clip_id: Uuid },
    NegativeSourceStart { clip_id: Uuid },
    SourceEndBeforeStart { clip_id: Uuid },
    SourceEndBeyondTotal { clip_id: Uuid },
    NonPositiveSpeed { clip_id: Uuid },
}

pub fn validate_timeline(timeline: &Timeline) -> Vec<TimelineViolation> {
    let mut out = Vec::new();
    let mut seen_tracks = std::collections::HashSet::new();
    for (ti, track) in timeline.tracks.iter().enumerate() {
        if !seen_tracks.insert(track.id.0) {
            out.push(TimelineViolation::DuplicateTrackId { id: track.id.0 });
        }
        let mut prev_start: Option<i64> = None;
        let mut prev_end: Option<(i64, Uuid)> = None;
        for clip in &track.clips {
            let id = clip.id.0;
            let start = clip.timeline_start.as_micros();
            let end = clip.timeline_end().as_micros();

            if let Some(ps) = prev_start {
                if start < ps {
                    out.push(TimelineViolation::ClipUnsorted {
                        track_index: ti,
                        clip_id: id,
                    });
                }
            }
            // Consecutive-pair overlap check is sufficient once sorted; on
            // unsorted input it still reports true overlaps, just possibly
            // with a different partner. Touching edges are legal.
            if let Some((pe, pid)) = prev_end {
                if start < pe {
                    out.push(TimelineViolation::ClipOverlap {
                        track_index: ti,
                        a: pid,
                        b: id,
                    });
                }
            }
            prev_start = Some(start);
            prev_end = Some((end, id));

            if start < 0 {
                out.push(TimelineViolation::NegativeTimelineStart { clip_id: id });
            }
            if !clip.timeline_duration.is_positive() {
                out.push(TimelineViolation::NonPositiveDuration { clip_id: id });
            }
            if !(clip.speed.is_finite() && clip.speed > 0.0) {
                out.push(TimelineViolation::NonPositiveSpeed { clip_id: id });
            }

            let text_like = matches!(
                clip.kind,
                ClipKind::Text(_) | ClipKind::Subtitle(_)
            );
            if clip.source_start.is_negative() {
                out.push(TimelineViolation::NegativeSourceStart { clip_id: id });
            }
            if clip.source_end < clip.source_start {
                out.push(TimelineViolation::SourceEndBeforeStart { clip_id: id });
            }
            if !text_like
                && !clip.source_total_duration.is_zero()
                && clip.source_end > clip.source_total_duration
            {
                out.push(TimelineViolation::SourceEndBeyondTotal { clip_id: id });
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniter_domain::clip::{AudioClip, Clip, ClipId, VideoClip};
    use miniter_domain::time::{MediaDuration, Timestamp};
    use miniter_domain::track::{Track, TrackKind};

    fn video_clip(start_us: i64, dur_us: i64, source_us: i64) -> Clip {
        Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::from_micros(start_us),
            timeline_duration: MediaDuration::from_micros(dur_us),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(source_us),
            source_total_duration: MediaDuration::from_micros(source_us.max(1)),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Video(VideoClip {
                source_path: "/tmp/a.mp4".into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                filters: vec![],
                audio_filters: vec![],
                masks: vec![],
            }),
            keyframes: Default::default(),
            blend_mode: Default::default(),
        }
    }

    fn audio_clip(start_us: i64, dur_us: i64) -> Clip {
        let mut c = video_clip(start_us, dur_us, dur_us);
        c.kind = ClipKind::Audio(AudioClip {
            source_path: "/tmp/a.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            filters: vec![],
        });
        c
    }

    fn timeline_with(clips: Vec<Clip>) -> Timeline {
        let mut track = Track::new(TrackKind::Video, "V1");
        track.clips = clips;
        Timeline {
            tracks: vec![track],
        }
    }

    #[test]
    fn clean_timeline_passes() {
        let tl = timeline_with(vec![
            video_clip(0, 1_000_000, 1_000_000),
            video_clip(1_000_000, 1_000_000, 1_000_000),
        ]);
        assert!(validate_timeline(&tl).is_empty());
    }

    #[test]
    fn overlap_and_unsorted_detected() {
        let a = video_clip(0, 2_000_000, 2_000_000);
        let b = video_clip(1_000_000, 1_000_000, 1_000_000);
        let violations = validate_timeline(&timeline_with(vec![a.clone(), b.clone()]));
        assert_eq!(
            violations,
            vec![TimelineViolation::ClipOverlap {
                track_index: 0,
                a: a.id.0,
                b: b.id.0,
            }]
        );

        let c = video_clip(5_000_000, 1_000_000, 1_000_000);
        let d = video_clip(0, 1_000_000, 1_000_000);
        let violations = validate_timeline(&timeline_with(vec![c, d]));
        assert!(violations.iter().any(|v| matches!(
            v,
            TimelineViolation::ClipUnsorted { .. }
        )));
    }

    #[test]
    fn source_and_speed_bounds_detected() {
        let mut bad = video_clip(0, 1_000_000, 1_000_000);
        bad.source_end = MediaDuration::from_micros(2_000_000);
        bad.speed = 0.0;
        let violations = validate_timeline(&timeline_with(vec![bad.clone()]));
        assert!(violations.contains(&TimelineViolation::SourceEndBeyondTotal {
            clip_id: bad.id.0,
        }));
        assert!(violations
            .iter()
            .any(|v| matches!(v, TimelineViolation::NonPositiveSpeed { .. })));

        let mut neg = audio_clip(0, 1_000_000);
        neg.timeline_start = Timestamp::from_micros(-5);
        neg.timeline_duration = MediaDuration::ZERO;
        let violations = validate_timeline(&timeline_with(vec![neg]));
        assert!(violations
            .iter()
            .any(|v| matches!(v, TimelineViolation::NegativeTimelineStart { .. })));
        assert!(violations
            .iter()
            .any(|v| matches!(v, TimelineViolation::NonPositiveDuration { .. })));
    }

    #[test]
    fn duplicate_track_ids_detected() {
        use miniter_domain::track::TrackId;
        let id = TrackId::new();
        let mut t1 = Track::new(TrackKind::Video, "V1");
        let mut t2 = Track::new(TrackKind::Video, "V2");
        t1.id = id;
        t2.id = id;
        let tl = Timeline {
            tracks: vec![t1, t2],
        };
        assert_eq!(
            validate_timeline(&tl),
            vec![TimelineViolation::DuplicateTrackId { id: id.0 }]
        );
    }
}

/// Timeline source files that no longer exist on disk.
///
/// The export pre-flight check uses this to fail fast with an actionable
/// "relink N files" message instead of dying mid-encode on a missing input.
/// Text/subtitle clips carry no media file and are skipped. Pure over the
/// timeline except for the existence probes (inherently IO).
pub fn missing_timeline_sources(timeline: &Timeline) -> Vec<std::path::PathBuf> {
    use std::collections::BTreeSet;
    let mut missing: BTreeSet<std::path::PathBuf> = BTreeSet::new();
    for track in &timeline.tracks {
        for clip in &track.clips {
            let source = match &clip.kind {
                ClipKind::Video(v) => Some(v.source_path.as_str()),
                ClipKind::Audio(a) => Some(a.source_path.as_str()),
                ClipKind::Text(_) | ClipKind::Subtitle(_) => None,
                _ => None,
            };
            if let Some(path) = source {
                let path = std::path::PathBuf::from(path);
                if !path.exists() {
                    missing.insert(path);
                }
            }
        }
    }
    missing.into_iter().collect()
}

#[cfg(test)]
mod missing_sources_tests {
    use super::*;
    use miniter_domain::clip::{AudioClip, Clip, ClipId, VideoClip};
    use miniter_domain::time::{MediaDuration, Timestamp};
    use miniter_domain::track::{Track, TrackKind};

    fn video_clip(source: &str) -> Clip {
        Clip {
            id: ClipId(uuid::Uuid::new_v4()),
            timeline_start: Timestamp::ZERO,
            timeline_duration: MediaDuration::from_micros(1_000_000),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(1_000_000),
            source_total_duration: MediaDuration::from_micros(1_000_000),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Video(VideoClip {
                source_path: source.into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                filters: vec![],
                audio_filters: vec![],
                masks: vec![],
            }),
            keyframes: Default::default(),
            blend_mode: Default::default(),
        }
    }

    fn audio_clip(source: &str) -> Clip {
        let mut clip = video_clip(source);
        clip.kind = ClipKind::Audio(AudioClip {
            source_path: source.into(),
            sample_rate: 48000,
            channels: 2,
            filters: vec![],
        });
        clip
    }

    #[test]
    fn reports_each_missing_source_once_and_ignores_present() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("here.mp4");
        std::fs::write(&present, b"x").unwrap();
        let gone = dir.path().join("gone.mp4");

        let mut v = Track::new(TrackKind::Video, "V1");
        v.insert_clip(video_clip(present.to_str().unwrap())).unwrap();
        let mut second = video_clip(gone.to_str().unwrap());
        second.timeline_start = Timestamp::from_micros(1_000_000);
        v.insert_clip(second).unwrap();
        // Duplicate reference: still reported once.
        let mut a = Track::new(TrackKind::Audio, "A1");
        a.insert_clip(audio_clip(gone.to_str().unwrap())).unwrap();
        let timeline = Timeline {
            tracks: vec![v, a],
        };

        let missing = missing_timeline_sources(&timeline);
        assert_eq!(missing, vec![gone]);
    }

    #[test]
    fn clean_timeline_reports_nothing() {
        let dir = tempfile::tempdir().unwrap();
        let present = dir.path().join("here.mp4");
        std::fs::write(&present, b"x").unwrap();
        let mut v = Track::new(TrackKind::Video, "V1");
        v.insert_clip(video_clip(present.to_str().unwrap())).unwrap();
        let timeline = Timeline {
            tracks: vec![v],
        };
        assert!(missing_timeline_sources(&timeline).is_empty());
    }
}
