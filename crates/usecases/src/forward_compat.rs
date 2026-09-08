//! Forward-tolerant project parsing.
//!
//! Effect payloads are sibling-owned tagged enums with no fallback: one
//! unknown variant (a newer build's filter, mask, transition, or clip kind)
//! would fail the whole typed parse and brick the project at open. This
//! module validates the raw JSON first and strips only the unrecognizable
//! shapes.

use crate::{AppError, AppResult, ProjectSnapshot};
use tracing::{info, warn};

/// Parse snapshot bytes with forward tolerance (see [`strip_unknown_effects`]).
/// Shared by file opens and the web JSON-import path.
pub fn parse_snapshot_bytes(bytes: &[u8]) -> AppResult<(ProjectSnapshot, StrippedSummary)> {
    let mut value: serde_json::Value = serde_json::from_slice(bytes)?;
    let summary = strip_unknown_effects(&mut value);
    let mut snapshot: ProjectSnapshot = serde_json::from_value(value)?;
    if snapshot.schema_version > ProjectSnapshot::SCHEMA_VERSION {
        return Err(AppError::InvalidInput(format!(
            "Unsupported project file schema version: {}",
            snapshot.schema_version
        )));
    }
    snapshot.schema_version = ProjectSnapshot::SCHEMA_VERSION;
    Ok((snapshot, summary))
}

/// What the forward-tolerance pass removed. Empty means the file parsed
/// exactly as written — today's behavior, byte for byte.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct StrippedSummary {
    /// Unknown video/audio filters and masks dropped from surviving clips.
    pub filters: usize,
    /// Clips whose kind is unrecognizable (placement lost with them).
    pub clips: usize,
    /// Transitions reset to none.
    pub transitions: usize,
}

impl StrippedSummary {
    pub fn is_empty(&self) -> bool {
        self.filters == 0 && self.clips == 0 && self.transitions == 0
    }

    pub fn total(&self) -> usize {
        self.filters + self.clips + self.transitions
    }
}

/// Walk every clip, validating effect payloads against the linked sibling
/// types. Each entry is tried with `from_value`: known variants (including
/// ones with new optional fields, via serde defaults) pass through
/// untouched; only truly unknown shapes are removed. No hardcoded tag
/// lists, so this tracks sibling additions automatically.
pub fn strip_unknown_effects(value: &mut serde_json::Value) -> StrippedSummary {
    let mut summary = StrippedSummary::default();
    let Some(tracks) = value
        .pointer_mut("/project/timeline/tracks")
        .and_then(|t| t.as_array_mut())
    else {
        return summary;
    };
    for (track_idx, track) in tracks.iter_mut().enumerate() {
        let Some(clips) = track.get_mut("clips").and_then(|c| c.as_array_mut()) else {
            continue;
        };
        for (clip_idx, clip) in clips.iter_mut().enumerate() {
            strip_clip_effects(clip, &mut summary, track_idx, clip_idx);
        }
        // Unrecognizable clip kinds drop by index (retain has no position):
        // the placement is lost with them, but the rest opens.
        let mut drop_idx = Vec::new();
        for (clip_idx, clip) in clips.iter().enumerate() {
            let kind_valid = clip
                .get("kind")
                .map(|k| serde_json::from_value::<miniter_domain::ClipKind>(k.clone()).is_ok())
                .unwrap_or(false);
            if !kind_valid {
                summary.clips += 1;
                warn!(
                    "Stripped clip #{clip_idx} (track #{track_idx}, id={}) with unrecognizable kind type={}: placement lost, rest of timeline kept",
                    clip_id_of(clip),
                    kind_type_of(clip),
                );
                drop_idx.push(clip_idx);
            }
        }
        for i in drop_idx.into_iter().rev() {
            clips.remove(i);
        }
    }
    if !summary.is_empty() {
        info!(
            "Project forward-tolerance: stripped {} filter(s), {} clip(s), {} transition(s) written by a newer version; media and cuts intact",
            summary.filters, summary.clips, summary.transitions,
        );
    }
    summary
}

/// Clip identifier for logs: the serialized id, else the source path, else ?.
fn clip_id_of(clip: &serde_json::Value) -> String {
    clip.get("id")
        .and_then(|id| match id {
            serde_json::Value::String(s) => Some(s.clone()),
            _ => Some(id.to_string()),
        })
        .unwrap_or_else(|| "?".to_string())
}

/// The kind's `type` tag for logs (`?` when the shape has none).
fn kind_type_of(clip: &serde_json::Value) -> &str {
    clip.get("kind")
        .and_then(|k| k.get("type"))
        .and_then(|t| t.as_str())
        .unwrap_or("?")
}

/// A filter entry's `type` tag for logs, falling back to the wrapper's tag.
fn effect_type_of(entry: &serde_json::Value) -> &str {
    entry
        .get("type")
        .or_else(|| entry.get("filter").and_then(|f| f.get("type")))
        .and_then(|t| t.as_str())
        .unwrap_or("?")
}

/// Strip one clip's effect arrays in place, logging every drop with its
/// identity (track/clip position, clip id, effect type tag). Runs before
/// kind validation so an unknown filter costs one filter, not the clip.
/// Note `filters` holds VideoEffect wrappers (not bare VideoFilter) —
/// including the legacy bare shape, which VideoEffect's deserializer
/// still accepts.
fn strip_clip_effects(
    clip: &mut serde_json::Value,
    summary: &mut StrippedSummary,
    track_idx: usize,
    clip_idx: usize,
) {
    use serde_json::Value;
    // Identity first: kind borrows below are &mut into the same clip.
    let clip_id = clip_id_of(clip);
    if let Some(kind) = clip.get_mut("kind") {
        summary.filters += retain_valid::<miniter_domain::filter::VideoEffect>(
            kind, "filters", track_idx, clip_idx, &clip_id,
        );
        summary.filters += retain_valid::<miniter_domain::AudioFilter>(
            kind,
            "audio_filters",
            track_idx,
            clip_idx,
            &clip_id,
        );
        summary.filters += retain_valid::<miniter_domain::MaskEffect>(
            kind, "masks", track_idx, clip_idx, &clip_id,
        );
    }
    for key in ["transition_in", "transition_out"] {
        let Some(t) = clip.get_mut(key) else {
            continue;
        };
        if !t.is_null() && serde_json::from_value::<miniter_domain::Transition>(t.clone()).is_err()
        {
            warn!(
                "Stripped {key} (kind={}) on clip #{clip_idx} (track #{track_idx}, id={clip_id}): reset to none",
                t.get("kind").and_then(|k| k.as_str()).unwrap_or("?"),
            );
            *t = Value::Null;
            summary.transitions += 1;
        }
    }
}

/// Drop array entries that don't deserialize as `T`, logging each drop with
/// its effect type tag and clip identity; returns the drop count.
fn retain_valid<T>(
    obj: &mut serde_json::Value,
    key: &str,
    track_idx: usize,
    clip_idx: usize,
    clip_id: &str,
) -> usize
where
    T: for<'de> serde::Deserialize<'de>,
{
    let Some(arr) = obj.get_mut(key).and_then(|a| a.as_array_mut()) else {
        return 0;
    };
    let before = arr.len();
    arr.retain(|v| {
        let valid = serde_json::from_value::<T>(v.clone()).is_ok();
        if !valid {
            warn!(
                "Stripped unknown {key} entry type={} on clip #{clip_idx} (track #{track_idx}, id={clip_id})",
                effect_type_of(v),
            );
        }
        valid
    });
    before - arr.len()
}

#[cfg(test)]
pub(crate) static FORWARD_COMPAT_TEST_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

#[cfg(test)]
pub(crate) mod forward_compat_tests {
    use super::*;
    use miniter_domain::clip::{Clip, ClipId, ClipKind, VideoClip};
    use miniter_domain::filter::{AudioFilter, VideoEffect, VideoFilter};
    use miniter_domain::mask::{MaskEffect, MaskShape};
    use miniter_domain::time::{MediaDuration, Timestamp};
    use miniter_domain::track::{Track, TrackKind};
    use miniter_domain::transition::{Transition, TransitionKind};
    use serde_json::{Value, json};
    use std::path::{Path, PathBuf};

    pub(crate) fn clip_at(start_us: i64) -> Clip {
        Clip {
            id: ClipId::new(),
            timeline_start: Timestamp::from_micros(start_us),
            timeline_duration: MediaDuration::from_micros(1_000_000),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(1_000_000),
            source_total_duration: MediaDuration::from_micros(1_000_000),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: Some(Transition::new(
                TransitionKind::CrossFade,
                MediaDuration::from_micros(250_000),
            )),
            transition_out: None,
            kind: ClipKind::Video(VideoClip {
                source_path: "/media/clip.mp4".into(),
                width: 1920,
                height: 1080,
                fps: 30.0,
                filters: vec![VideoEffect::new(VideoFilter::Brightness { value: 0.1 })],
                audio_filters: vec![AudioFilter::Volume { value: 0.8 }],
                masks: vec![MaskEffect::shape(MaskShape::Rectangle {
                    left: 0.0,
                    top: 0.0,
                    right: 1.0,
                    bottom: 1.0,
                })],
            }),
            keyframes: Default::default(),
            blend_mode: Default::default(),
        }
    }

    /// Golden project + two clips, then newer-version payloads injected as
    /// raw JSON (shapes today's deserializer has never seen).
    pub(crate) fn future_file(dir: &Path) -> PathBuf {
        let mut track = Track::new(TrackKind::Video, "V1");
        track.insert_clip(clip_at(0)).unwrap();
        track.insert_clip(clip_at(1_000_000)).unwrap();
        let mut project = miniter_domain::Project::new("Future");
        project.timeline = miniter_domain::Timeline {
            tracks: vec![track],
        };
        let mut root =
            serde_json::to_value(&ProjectSnapshot::new(project, vec![], vec![])).unwrap();

        let clips = root
            .pointer_mut("/project/timeline/tracks/0/clips")
            .and_then(|c| c.as_array_mut())
            .unwrap();
        // Clip 0: one unknown entry per effect array + bad transition_out.
        // transition_out mirrors the valid transition_in shape, then swaps
        // the kind tag for one that doesn't exist.
        let mut bad_transition = clips[0]["transition_in"].clone();
        bad_transition["kind"] = Value::String("Teleport".into());
        clips[0]["transition_out"] = bad_transition;
        for (key, unknown) in [
            ("filters", json!({"type": "QuantumGlow", "strength": 9.0})),
            ("audio_filters", json!({"type": "MegaBass", "gain": 12.0})),
            ("masks", json!({"enabled": true})),
        ] {
            clips[0]["kind"][key].as_array_mut().unwrap().push(unknown);
        }
        // Clip 1: unrecognizable kind — the placement is lost with it.
        clips[1]["kind"] = json!({"type": "Hologram", "density": 1.0});

        let path = dir.join("future-effects.snap");
        std::fs::write(&path, serde_json::to_vec_pretty(&root).unwrap()).unwrap();
        path
    }

    #[test]
    fn unknown_variants_strip_with_exact_counts() {
        let _guard = FORWARD_COMPAT_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bytes = std::fs::read(future_file(dir.path())).unwrap();

        // Sanity: today's typed parse really does choke on this file…
        assert!(serde_json::from_slice::<ProjectSnapshot>(&bytes).is_err());

        // …while the tolerant path opens it with exact counts.
        let (snapshot, summary) = parse_snapshot_bytes(&bytes).unwrap();
        assert_eq!(
            summary,
            StrippedSummary {
                filters: 3,
                clips: 1,
                transitions: 1,
            }
        );

        // Surviving clip: known effects intact, bad transition nulled.
        let clips = &snapshot.project.timeline.tracks[0].clips;
        assert_eq!(clips.len(), 1);
        let ClipKind::Video(v) = &clips[0].kind else {
            panic!("expected surviving video clip");
        };
        assert!(matches!(
            v.filters.as_slice(),
            [f] if matches!(f.filter, VideoFilter::Brightness { .. })
        ));
        assert!(matches!(
            v.audio_filters.as_slice(),
            [f] if matches!(f, AudioFilter::Volume { .. })
        ));
        assert_eq!(v.masks.len(), 1);
        assert!(clips[0].transition_in.is_some());
        assert!(clips[0].transition_out.is_none());
    }

    #[test]
    fn clean_files_parse_with_empty_summary() {
        let _guard = FORWARD_COMPAT_TEST_LOCK.lock().unwrap();
        // Regression: files written by this build behave exactly as before.
        let golden = std::fs::read(
            PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/project-v4.snap"),
        )
        .unwrap();
        let (snapshot, summary) = parse_snapshot_bytes(&golden).unwrap();
        assert!(summary.is_empty());
        assert_eq!(snapshot.project.meta.name, "Golden");
    }
}

#[cfg(test)]
mod legacy_shape_tests {
    use super::forward_compat_tests::clip_at;
    use super::{FORWARD_COMPAT_TEST_LOCK, ProjectSnapshot, parse_snapshot_bytes};
    use miniter_domain::clip::ClipKind;
    use miniter_domain::filter::VideoFilter;
    use miniter_domain::track::{Track, TrackKind};
    use serde_json::json;

    #[test]
    fn bare_filter_shape_without_wrapper_survives() {
        let _guard = FORWARD_COMPAT_TEST_LOCK.lock().unwrap();
        // Old files predate the {enabled, filter} wrapper: VideoEffect's
        // deserializer accepts the bare filter, and the strip pass must too.
        let mut track = Track::new(TrackKind::Video, "V1");
        track.insert_clip(clip_at(0)).unwrap();
        let mut project = miniter_domain::Project::new("Legacy");
        project.timeline = miniter_domain::Timeline {
            tracks: vec![track],
        };
        let mut root =
            serde_json::to_value(&ProjectSnapshot::new(project, vec![], vec![])).unwrap();
        let filters = root
            .pointer_mut("/project/timeline/tracks/0/clips/0/kind/filters")
            .and_then(|f| f.as_array_mut())
            .unwrap();
        filters.clear();
        filters.push(json!({"type": "Grayscale"}));

        let bytes = serde_json::to_vec_pretty(&root).unwrap();
        let (snapshot, summary) = parse_snapshot_bytes(&bytes).unwrap();
        assert!(summary.is_empty());
        let ClipKind::Video(v) = &snapshot.project.timeline.tracks[0].clips[0].kind else {
            panic!("expected video clip");
        };
        assert!(matches!(
            v.filters.as_slice(),
            [f] if matches!(f.filter, VideoFilter::Grayscale) && f.enabled
        ));
    }
}

#[cfg(test)]
mod strip_logging_tests {
    use super::forward_compat_tests::future_file;
    use super::{FORWARD_COMPAT_TEST_LOCK, parse_snapshot_bytes};
    use std::fmt;
    use std::sync::{Arc, Mutex};
    use tracing::field::{Field, Visit};
    use tracing::{Event, Metadata};

    /// Minimal capturing subscriber: records event messages for assertions.
    #[derive(Clone, Default)]
    struct Capture {
        messages: Arc<Mutex<Vec<String>>>,
    }

    struct MessageVisitor {
        out: Vec<String>,
    }

    impl Visit for MessageVisitor {
        fn record_debug(&mut self, field: &Field, value: &dyn fmt::Debug) {
            if field.name() == "message" {
                self.out.push(format!("{value:?}"));
            }
        }
    }

    impl tracing::Subscriber for Capture {
        fn enabled(&self, _: &Metadata<'_>) -> bool {
            true
        }
        fn new_span(&self, _: &tracing::span::Attributes<'_>) -> tracing::span::Id {
            tracing::span::Id::from_u64(1)
        }
        fn record(&self, _: &tracing::span::Id, _: &tracing::span::Record<'_>) {}
        fn record_follows_from(&self, _: &tracing::span::Id, _: &tracing::span::Id) {}
        fn event(&self, event: &Event<'_>) {
            let mut visitor = MessageVisitor { out: Vec::new() };
            event.record(&mut visitor);
            self.messages.lock().unwrap().extend(visitor.out);
        }
        fn enter(&self, _: &tracing::span::Id) {}
        fn exit(&self, _: &tracing::span::Id) {}
    }

    #[test]
    fn stripped_items_log_their_identities() {
        let _guard = FORWARD_COMPAT_TEST_LOCK.lock().unwrap();
        let dir = tempfile::tempdir().unwrap();
        let bytes = std::fs::read(future_file(dir.path())).unwrap();
        let capture = Capture::default();
        let messages = capture.messages.clone();
        tracing::dispatcher::with_default(&tracing::Dispatch::new(capture), || {
            let (_, summary) = parse_snapshot_bytes(&bytes).unwrap();
            assert_eq!(summary.total(), 5);
        });
        let log = messages.lock().unwrap().join("\n");
        // Every dropped shape is named: filter tags, clip kind, transition.
        for needle in [
            "QuantumGlow",
            "MegaBass",
            "Hologram",
            "Teleport",
            "forward-tolerance",
        ] {
            assert!(
                log.contains(needle),
                "expected log to name {needle}:\n{log}"
            );
        }
    }

    #[test]
    fn clean_files_log_nothing() {
        let _guard = FORWARD_COMPAT_TEST_LOCK.lock().unwrap();
        let golden = std::fs::read(
            std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("testdata/project-v4.snap"),
        )
        .unwrap();
        let capture = Capture::default();
        let messages = capture.messages.clone();
        tracing::dispatcher::with_default(&tracing::Dispatch::new(capture), || {
            let (_, summary) = parse_snapshot_bytes(&golden).unwrap();
            assert!(summary.is_empty());
        });
        assert!(messages.lock().unwrap().is_empty());
    }
}
