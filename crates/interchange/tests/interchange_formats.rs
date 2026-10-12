//! Round-trip tests for the interchange formats ported from filmcraft.

use miniter_domain::{
    AudioClip, BlendMode, Clip, ClipId, ClipKind, KeyframeCurve, MediaDuration, Timeline,
    Timestamp, Track, TrackKind, Transition, TransitionKind, VideoClip,
};

use snapshort_interchange::{Rate, ale, fcp7, otio};

fn rate() -> Rate {
    Rate::new(24, 1)
}

fn timeline() -> Timeline {
    let mut timeline = Timeline::new();
    let mut video = Track::new(TrackKind::Video, "V1");
    video
        .clips
        .push(clip(0, 1_000_000, "/media/wide.mp4", 1920, 1080));
    video
        .clips
        .push(clip(1_000_000, 1_000_000, "/media/close.mp4", 1920, 1080));
    timeline.add_track(video);
    let mut audio = Track::new(TrackKind::Audio, "A1");
    audio.clips.push(Clip {
        id: ClipId::new(),
        timeline_start: Timestamp::from_micros(0),
        timeline_duration: MediaDuration::from_micros(2_000_000),
        source_start: MediaDuration::ZERO,
        source_end: MediaDuration::from_micros(2_000_000),
        source_total_duration: MediaDuration::from_micros(2_000_000),
        speed: 1.0,
        volume: 0.5,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        kind: ClipKind::Audio(AudioClip {
            source_path: "/media/score.wav".into(),
            sample_rate: 48_000,
            channels: 2,
            filters: Vec::new(),
        }),
        keyframes: KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    });
    timeline.add_track(audio);
    timeline
}

fn clip(start: i64, length: i64, path: &str, width: u32, height: u32) -> Clip {
    Clip {
        id: ClipId::new(),
        timeline_start: Timestamp::from_micros(start),
        timeline_duration: MediaDuration::from_micros(length),
        source_start: MediaDuration::ZERO,
        source_end: MediaDuration::from_micros(length),
        source_total_duration: MediaDuration::from_micros(length * 4),
        speed: 1.0,
        volume: 1.0,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        kind: ClipKind::Video(VideoClip {
            source_path: path.into(),
            width,
            height,
            fps: 24.0,
            filters: Vec::new(),
            audio_filters: Vec::new(),
            masks: Vec::new(),
        }),
        keyframes: KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    }
}

#[test]
fn otio_round_trips_a_timeline() {
    let text = otio::export_otio("Cut", &timeline(), rate(), &|p: &str| p.to_string()).unwrap();
    assert!(text.contains("\"OTIO_SCHEMA\": \"Timeline.1\""), "{text}");
    let back = otio::import_otio(&text).unwrap();

    assert_eq!(back.tracks.len(), 2, "video and audio survive");
    let video = &back.tracks[0];
    assert_eq!(video.kind, TrackKind::Video);
    assert_eq!(video.clips.len(), 2);
    assert_eq!(video.clips[0].timeline_start.as_micros(), 0);
    assert_eq!(video.clips[1].timeline_start.as_micros(), 1_000_000);
    assert_eq!(video.clips[0].timeline_duration.as_micros(), 1_000_000);
    match &video.clips[0].kind {
        ClipKind::Video(v) => {
            assert_eq!(v.source_path, "/media/wide.mp4");
            assert_eq!(v.width, 1920);
            assert_eq!(v.height, 1080);
        }
        other => panic!("expected a video clip, got {other:?}"),
    }
    assert_eq!(video.clips[0].source_start.as_micros(), 0);
    assert_eq!(video.clips[0].source_end.as_micros(), 1_000_000);

    let audio = &back.tracks[1];
    assert_eq!(audio.kind, TrackKind::Audio);
    assert_eq!(audio.clips.len(), 1);
    match &audio.clips[0].kind {
        ClipKind::Audio(a) => {
            assert_eq!(a.source_path, "/media/score.wav");
            assert_eq!(a.sample_rate, 48_000);
            assert_eq!(a.channels, 2);
        }
        other => panic!("expected an audio clip, got {other:?}"),
    }
    assert_eq!(audio.clips[0].volume, 0.5);
}

#[test]
fn otio_reads_another_tools_document() {
    let text = r#"{
        "OTIO_SCHEMA": "Timeline.1",
        "name": "Foreign",
        "global_start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25.0, "value": 0.0},
        "tracks": {"OTIO_SCHEMA": "Stack.1", "children": [
            {"OTIO_SCHEMA": "Track.1", "name": "V1", "kind": "Video", "children": [
                {"OTIO_SCHEMA": "Clip.1", "name": "Shot", "source_range":
                    {"OTIO_SCHEMA": "TimeRange.1",
                     "start_time": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25.0, "value": 12.0},
                     "duration": {"OTIO_SCHEMA": "RationalTime.1", "rate": 25.0, "value": 48.0}},
                 "media_references": {"DEFAULT_MEDIA":
                    {"OTIO_SCHEMA": "ExternalReference.1", "target_url": "file:///media/foreign.mov"}}}
            ]}
        ]}
    }"#;
    let back = otio::import_otio(text).unwrap();
    assert_eq!(back.tracks.len(), 1);
    let clip = &back.tracks[0].clips[0];
    assert_eq!(clip.timeline_start.as_micros(), 0);
    assert_eq!(clip.timeline_duration.as_micros(), 1_920_000);
    assert_eq!(clip.source_start.as_micros(), 480_000);
    match &clip.kind {
        ClipKind::Video(v) => {
            assert_eq!(v.source_path, "/media/foreign.mov");
            assert_eq!(v.fps, 25.0);
        }
        other => panic!("expected a video clip, got {other:?}"),
    }
}

#[test]
fn fcp7_round_trips_a_timeline() {
    let text = fcp7::export(&timeline(), rate());
    assert!(text.starts_with("<?xml"), "{text}");
    assert!(text.contains("<xmeml version=\"4\">"), "{text}");
    let back = fcp7::import(&text).unwrap();

    assert_eq!(back.tracks.len(), 2, "video and audio survive");
    let video = &back.tracks[0];
    assert_eq!(video.kind, TrackKind::Video);
    assert_eq!(video.clips.len(), 2);
    assert_eq!(video.clips[0].timeline_start.as_micros(), 0);
    assert_eq!(video.clips[1].timeline_start.as_micros(), 1_000_000);
    assert_eq!(video.clips[0].timeline_duration.as_micros(), 1_000_000);
    assert_eq!(video.clips[1].timeline_duration.as_micros(), 1_000_000);
    match &video.clips[0].kind {
        ClipKind::Video(v) => {
            assert_eq!(v.source_path, "/media/wide.mp4");
            assert_eq!(v.width, 1920);
            assert_eq!(v.height, 1080);
        }
        other => panic!("expected a video clip, got {other:?}"),
    }
    assert_eq!(video.clips[0].source_start.as_micros(), 0);
    assert_eq!(video.clips[0].source_end.as_micros(), 1_000_000);

    let audio = &back.tracks[1];
    assert_eq!(audio.kind, TrackKind::Audio);
    assert_eq!(audio.clips.len(), 1);
    match &audio.clips[0].kind {
        ClipKind::Audio(a) => {
            assert_eq!(a.source_path, "/media/score.wav");
            assert_eq!(a.sample_rate, 48_000);
            assert_eq!(a.channels, 2);
        }
        other => panic!("expected an audio clip, got {other:?}"),
    }
    assert_eq!(audio.clips[0].volume, 0.5);
}

#[test]
fn fcp7_rejects_a_document_that_is_not_xmeml() {
    assert!(fcp7::import("<other><sequence/></other>").is_err());
    assert!(fcp7::import("<xmeml version=\"4\"><sequence/></xmeml>").is_err());
    assert!(fcp7::import("").is_err());
}

#[test]
fn ale_writes_the_three_sections() {
    let timeline = timeline();
    let doc = ale::export_timeline(&timeline, rate());
    let text = ale::write(&doc);
    assert!(
        text.starts_with(
            "Heading\r\nFIELD_DELIM\tTABS\r\nVIDEO_FORMAT\t1080\r\nAUDIO_FORMAT\t48khz\r\nFPS\t24"
        ),
        "{text}"
    );
    assert!(
        text.contains("\r\nColumn\r\nName\tTracks\tStart\tEnd\tTape"),
        "{text}"
    );
    assert!(text.contains("\r\nData\r\n"), "{text}");
    assert_eq!(doc.rows.len(), 3, "every clip is logged");
    assert_eq!(doc.get(0, "Tracks"), Some("V"));
    assert_eq!(doc.get(0, "Source File"), Some("/media/wide.mp4"));
    assert_eq!(doc.get(1, "Start"), Some("00:00:01:00"));
    assert_eq!(doc.get(2, "Tracks"), Some("A1A2"));
    assert_eq!(doc.get(2, "Start"), Some("00:00:00:00"));
    assert_eq!(doc.get(2, "End"), Some("00:00:02:00"));
}

#[test]
fn ale_parses_back_what_it_writes() {
    let doc = ale::export_timeline(&timeline(), rate());
    let text = ale::write(&doc);
    let back = ale::parse(&text).unwrap();
    assert_eq!(back, doc);
    assert_eq!(back.rate(), Some(rate()));
    let (start, end) = ale::row_range(&back, 1).unwrap();
    assert_eq!(start, 1_000_000);
    assert_eq!(end, 2_000_000);
    assert!(ale::parse("hello").is_err());
}

#[test]
fn otio_relative_paths_survive_a_round_trip() {
    let mut timeline = Timeline::new();
    let mut video = Track::new(TrackKind::Video, "V1");
    video
        .clips
        .push(clip(0, 1_000_000, "media/shot.mp4", 1920, 1080));
    timeline.add_track(video);
    let text = otio::export_otio("Cut", &timeline, rate(), &|p: &str| p.to_string()).unwrap();
    assert!(
        !text.contains("file://media/shot.mp4"),
        "a relative path must not gain a host-like prefix: {text}"
    );
    let back = otio::import_otio(&text).unwrap();
    match &back.tracks[0].clips[0].kind {
        ClipKind::Video(v) => assert_eq!(v.source_path, "media/shot.mp4"),
        other => panic!("expected a video clip, got {other:?}"),
    }
}

#[test]
fn fcp7_transition_out_survives_a_round_trip() {
    let mut timeline = Timeline::new();
    let mut video = Track::new(TrackKind::Video, "V1");
    let mut first = clip(0, 2_000_000, "/media/a.mp4", 1920, 1080);
    first.transition_out = Some(Transition::new(
        TransitionKind::Dissolve,
        MediaDuration::from_micros(500_000),
    ));
    video.clips.push(first);
    video
        .clips
        .push(clip(2_000_000, 2_000_000, "/media/b.mp4", 1920, 1080));
    timeline.add_track(video);
    let text = fcp7::export(&timeline, rate());
    let back = fcp7::import(&text).unwrap();
    let out = &back.tracks[0].clips[0].transition_out;
    assert!(
        out.is_some(),
        "transition_out must survive export and import"
    );
    assert_eq!(out.as_ref().unwrap().duration.as_micros(), 500_000);
}

#[test]
fn ale_rejects_out_of_range_timecodes() {
    let rate = Rate::new(24, 1);
    assert!(ale::parse_timecode("01:00:00:00", rate).is_ok());
    assert!(
        ale::parse_timecode("01:00:00:75", rate).is_err(),
        "75 frames at 24fps is not a timecode"
    );
    assert!(
        ale::parse_timecode("01:00:-5:00", rate).is_err(),
        "negative components are not a timecode"
    );
    assert!(
        ale::parse_timecode("01:75:00:00", rate).is_err(),
        "75 minutes is not a timecode"
    );
}
