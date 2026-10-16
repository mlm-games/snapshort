//! Timeline geometry: scale, duration, tick and coordinate calculations.
//!
//! # Units
//!
//! Everything the timeline lays out or computes is in **logical points**
//! (Dp): [`TimelineScale`], [`TRACK_HEADER_WIDTH`], scroll-adjusted content
//! offsets, `panel_origin` (repose reports it in Dp), and the control
//! channel's `ui.click`/`ui.elements` coordinates.
//!
//! Two inputs arrive in **physical pixels** and are converted exactly once,
//! here: repose pointer/drag positions ([`window_px_to_us`]) and the
//! `ScrollStateXY` offset ([`px_to_points`], the scroll state is in taffy's
//! px space). Nothing else in the module may mix the two.

use miniter_domain::{Clip, ClipId, ClipKind, Timeline, Timestamp};
use repose_core::Vec2;

pub const TRACK_HEADER_WIDTH: f32 = 128.0;
pub const TRACK_HEIGHT: f32 = 56.0;
pub const RULER_HEIGHT: f32 = 30.0;
pub const ADD_TRACK_ROW_HEIGHT: f32 = 36.0;

pub const TRIM_HANDLE_WIDTH: f32 = 12.0;
pub const MIN_CLIP_WIDTH: f32 = 12.0;

pub const MIN_TIMELINE_DURATION_US: i64 = 30_000_000;
pub const TIMELINE_PADDING_US: i64 = 5_000_000;

pub const SNAP_THRESHOLD_DP: f32 = 8.0;

/// Physical pixels → logical points, for one axis. Use on pointer positions
/// and scroll offsets; the result is the space this module computes in.
pub fn px_to_points(px: f32) -> f32 {
    repose_core::px_to_dp(repose_core::Px(px)).0
}

/// Logical points → physical pixels, for one axis. Use when handing a
/// computed offset back to a `ScrollStateXY` setter.
pub fn points_to_px(points: f32) -> f32 {
    repose_core::dp_to_px(repose_core::Dp(points)).0
}

#[derive(Debug, Clone, Copy)]
pub struct TimelineScale {
    /// Logical points per microsecond.
    pub dp_per_us: f32,
}

impl TimelineScale {
    pub fn new(zoom: f32) -> Self {
        Self {
            // Miniter: dpPerMs = zoom * 0.1
            dp_per_us: (zoom * 0.1) / 1_000.0,
        }
    }

    pub fn timestamp_to_x(self, timestamp: Timestamp) -> f32 {
        timestamp.0.max(0) as f32 * self.dp_per_us
    }

    pub fn us_to_x(self, timestamp_us: i64) -> f32 {
        timestamp_us.max(0) as f32 * self.dp_per_us
    }

    pub fn x_to_us(self, x: f32) -> i64 {
        (x.max(0.0) / self.dp_per_us.max(f32::EPSILON)).round() as i64
    }

    pub fn major_tick_us(self) -> i64 {
        let dp_per_ms = self.dp_per_us * 1_000.0;

        if dp_per_ms >= 0.50 {
            1_000_000
        } else if dp_per_ms >= 0.20 {
            2_000_000
        } else if dp_per_ms >= 0.10 {
            5_000_000
        } else if dp_per_ms >= 0.05 {
            10_000_000
        } else {
            30_000_000
        }
    }
}

pub fn timeline_duration_us(timeline: Option<&Timeline>) -> i64 {
    timeline
        .map(|t| t.duration_end().as_micros())
        .unwrap_or(MIN_TIMELINE_DURATION_US)
        .max(MIN_TIMELINE_DURATION_US)
        .saturating_add(TIMELINE_PADDING_US)
}

pub fn timeline_width(timeline: Option<&Timeline>, scale: TimelineScale) -> f32 {
    timeline_duration_us(timeline) as f32 * scale.dp_per_us
}

/// Timeline microseconds for a window-space pointer x. `window_px` comes from
/// repose (`position_in_window`, `DragOver`/`DropEvent`); `scroll_px` is the
/// raw `ScrollStateXY` offset. Both are physical pixels and converted here;
/// `panel_origin` and the result's space are logical points.
pub fn window_px_to_us(window_px: f32, panel_origin: Vec2, scroll_px: f32, scale: TimelineScale) -> i64 {
    window_points_to_us(
        px_to_points(window_px),
        panel_origin,
        px_to_points(scroll_px),
        scale,
    )
}

/// Timeline microseconds for a window-space x already in logical points
/// (the control channel's coordinates, `ui.elements` rects, and anything the
/// views compute in points).
pub fn window_points_to_us(
    window_points: f32,
    panel_origin: Vec2,
    scroll_points: f32,
    scale: TimelineScale,
) -> i64 {
    scale.x_to_us(window_points - panel_origin.x - TRACK_HEADER_WIDTH + scroll_points)
}

/// Miniter `formatRulerTime`, expressed in microseconds.
pub fn format_ruler_time(us: i64) -> String {
    let ms = us.max(0) / 1000;
    let total_sec = ms / 1000;
    let min = total_sec / 60;
    let sec = total_sec % 60;
    let tenths = (ms % 1000) / 100;
    if min > 0 {
        format!("{min}:{sec:02}.{tenths}")
    } else {
        format!("{sec}.{tenths}s")
    }
}

fn filename(path: &str) -> String {
    path.rsplit(['/', '\\']).next().unwrap_or(path).to_string()
}

pub fn clip_label(clip: &Clip) -> String {
    match &clip.kind {
        ClipKind::Video(v) => filename(&v.source_path),
        ClipKind::Audio(a) => filename(&a.source_path),
        ClipKind::Text(t) => format!("T: {}", t.text),
        ClipKind::Subtitle(s) => filename(&s.source_path),
        _ => "Clip".into(),
    }
}

#[derive(Debug, Clone, Copy)]
pub struct SnapResult {
    pub value_us: i64,
    pub guide_us: Option<i64>,
}

/// Snapshot candidates for snap: zero, playhead, markers, and every other
/// clip's start/end edges. `exclude_clip_id` is the clip being moved/trimmed.
pub fn snap_candidates(
    timeline: &Timeline,
    exclude_clip_id: ClipId,
    playhead_us: i64,
    marker_times: impl Iterator<Item = i64>,
) -> Vec<i64> {
    let mut candidates = vec![0, playhead_us];
    candidates.extend(marker_times);

    for track in &timeline.tracks {
        for clip in &track.clips {
            if clip.id == exclude_clip_id {
                continue;
            }

            candidates.push(clip.timeline_start.0);
            candidates.push(clip.timeline_end().as_micros());
        }
    }

    candidates
}

/// Snap a single moving edge to the nearest candidate within
/// `SNAP_THRESHOLD_DP` logical points.
/// Returns the snapped value (and the guide timestamp) or the raw value unchanged.
pub fn snap_edge(
    candidates: impl Iterator<Item = i64>,
    moving_us: i64,
    scale: TimelineScale,
) -> SnapResult {
    let mut best: Option<(i64, f32)> = None;

    for candidate in candidates {
        let distance_dp = ((candidate - moving_us).abs() as f32) * scale.dp_per_us;

        if distance_dp <= SNAP_THRESHOLD_DP && best.is_none_or(|(_, old)| distance_dp < old) {
            best = Some((candidate, distance_dp));
        }
    }

    match best {
        Some((value, _)) => SnapResult {
            value_us: value.max(0),
            guide_us: Some(value),
        },
        None => SnapResult {
            value_us: moving_us.max(0),
            guide_us: None,
        },
    }
}

pub fn snap_moving_clip(
    timeline: &Timeline,
    active_clip: ClipId,
    desired_start_us: i64,
    duration_us: i64,
    playhead_us: i64,
    marker_times: impl Iterator<Item = i64>,
    scale: TimelineScale,
) -> SnapResult {
    let candidates = snap_candidates(timeline, active_clip, playhead_us, marker_times);

    let moving_edges = [
        (desired_start_us, 0),
        (desired_start_us.saturating_add(duration_us), duration_us),
    ];

    let mut best: Option<(i64, i64, f32)> = None;

    for (moving_edge, edge_offset) in moving_edges {
        let edge = snap_edge(candidates.iter().copied(), moving_edge, scale);
        if let Some(guide_us) = edge.guide_us {
            let value_us = guide_us.saturating_sub(edge_offset);
            let distance_dp = ((guide_us - moving_edge).abs() as f32) * scale.dp_per_us;

            if best.is_none_or(|(_, _, old)| distance_dp < old) {
                best = Some((value_us, guide_us, distance_dp));
            }
        }
    }

    match best {
        Some((value_us, guide_us, _)) => SnapResult {
            value_us: value_us.max(0),
            guide_us: Some(guide_us),
        },
        None => SnapResult {
            value_us: desired_start_us.max(0),
            guide_us: None,
        },
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use miniter_domain::{
        Clip, ClipId, ClipKind, MediaDuration, Timeline, Timestamp, Track, TrackId, TrackKind,
    };

    fn sample_track(id: TrackId) -> Track {
        Track {
            id,
            name: "V1".into(),
            kind: TrackKind::Video,
            muted: false,
            locked: false,
            clips: vec![],
        }
    }

    fn sample_clip(id: ClipId, start_us: i64, dur_us: i64) -> Clip {
        Clip {
            id,
            timeline_start: Timestamp(start_us),
            timeline_duration: MediaDuration::from_micros(dur_us),
            source_start: MediaDuration::ZERO,
            source_end: MediaDuration::from_micros(dur_us),
            source_total_duration: MediaDuration::from_micros(dur_us),
            speed: 1.0,
            volume: 1.0,
            opacity: 1.0,
            muted: false,
            transition_in: None,
            transition_out: None,
            kind: ClipKind::Video(miniter_domain::VideoClip {
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

    #[test]
    fn scale_places_ten_seconds_correctly() {
        let scale = TimelineScale::new(2.0);
        assert!((scale.dp_per_us - 0.0002).abs() < f32::EPSILON * 100.0);
        assert!((scale.us_to_x(10_000_000) - 2000.0).abs() < 0.001);
        assert_eq!(scale.x_to_us(2000.0), 10_000_000);
    }

    /// Golden guard for the px/points boundary: at 2x density a pointer and a
    /// scroll offset are twice the physical pixels for the same logical
    /// position, and `window_px_to_us` must cancel that exactly once.
    #[test]
    fn hi_dpi_pointer_and_scroll_convert_once() {
        let scale = TimelineScale::new(2.0);
        let origin = Vec2 { x: 100.0, y: 40.0 };
        let scroll_dp = 50.0;
        let content_dp = 300.0;
        let window_dp = origin.x + TRACK_HEADER_WIDTH + content_dp - scroll_dp;
        let expected = scale.x_to_us(content_dp);

        repose_core::with_density(repose_core::Density { scale: 2.0 }, || {
            assert_eq!(
                window_px_to_us(window_dp * 2.0, origin, scroll_dp * 2.0, scale),
                expected
            );
            assert_eq!(
                window_points_to_us(window_dp, origin, scroll_dp, scale),
                expected
            );
            assert_eq!(px_to_points(200.0), 100.0);
            assert_eq!(points_to_px(100.0), 200.0);
        });
    }

    #[test]
    fn snap_adjusts_within_threshold_and_keeps_guide() {
        let mut tl = Timeline::new();
        let t1 = TrackId::new();
        let mut track = sample_track(t1);
        let a = ClipId::new();
        track.clips.push(sample_clip(a, 1_000_000, 500_000));
        tl.add_track(track);

        let scale = TimelineScale::new(2.0);
        // Neighbor clip starts at 1s; trying to land at 1.015s (~3px at zoom 2)
        // should snap to 1s exactly, returning a guide.
        let res = snap_moving_clip(
            &tl,
            ClipId::new(),
            1_015_000,
            500_000,
            0,
            std::iter::empty(),
            scale,
        );
        assert_eq!(res.value_us, 1_000_000);
        assert_eq!(res.guide_us, Some(1_000_000));
    }

    #[test]
    fn snap_noop_outside_threshold() {
        let mut tl = Timeline::new();
        let t1 = TrackId::new();
        let mut track = sample_track(t1);
        let a = ClipId::new();
        track.clips.push(sample_clip(a, 1_000_000, 500_000));
        tl.add_track(track);

        let scale = TimelineScale::new(2.0);
        // 200ms away = 40px > threshold; no snap.
        let res = snap_moving_clip(
            &tl,
            ClipId::new(),
            1_200_000,
            500_000,
            0,
            std::iter::empty(),
            scale,
        );
        assert_eq!(res.value_us, 1_200_000);
        assert_eq!(res.guide_us, None);
    }

    #[test]
    fn ruler_time_formatting() {
        assert_eq!(format_ruler_time(0), "0.0s");
        assert_eq!(format_ruler_time(2_500_000), "2.5s");
        assert_eq!(format_ruler_time(65_000_000), "1:05.0");
    }
}
