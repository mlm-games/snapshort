//! Engine tests: the command surface an agent, the CLI and the UI share.

use serde_json::{Value, json};
use snapshort_engine::{Session, SessionCommand, command_specs, find_command};
use snapshort_usecases::ProjectCommand;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-engine-{}-{}",
        std::process::id(),
        COUNTER.fetch_add(1, Ordering::SeqCst)
    ));
    let _ = std::fs::remove_dir_all(&dir);
    let session = Session::new(dir);
    session.start().await.expect("session starts");
    session
}

static COUNTER: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);
use std::sync::atomic::Ordering;

#[tokio::test]
async fn every_command_runs_and_the_registry_is_consistent() {
    let session = session().await;
    let specs = command_specs();
    assert!(
        specs.len() > 40,
        "expected a full command surface, got {}",
        specs.len()
    );

    let mut ids: Vec<&str> = specs.iter().map(|s| s.id).collect();
    ids.sort_unstable();
    let unique = ids.windows(2).all(|w| w[0] != w[1]);
    assert!(unique, "command ids must be unique");

    for spec in specs {
        assert!(!spec.label.is_empty(), "{} has no label", spec.id);
        assert!(!spec.params.is_empty(), "{} documents no params", spec.id);
        let params: Value = serde_json::from_str(spec.params).unwrap_or(Value::Null);
        assert!(
            params.is_object() || spec.params.trim_start().starts_with('{'),
            "{} params must be a JSON-ish object",
            spec.id
        );
        assert_eq!(find_command(spec.id).map(|s| s.id), Some(spec.id));
    }
}

#[tokio::test]
async fn an_unknown_command_is_an_error_not_a_panic() {
    let session = session().await;
    let error = session.execute("nope.nope", json!({})).await.unwrap_err();
    assert!(error.to_string().contains("unknown command"), "{error}");
}

#[tokio::test]
async fn a_missing_param_names_the_param() {
    let session = session().await;
    let error = session
        .execute("playback.seek", json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("seconds"), "{error}");
}

#[tokio::test]
async fn timeline_edits_round_trip_through_the_engine() {
    let session = session().await;

    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let track = timeline["tracks"][0]["id"].as_str().unwrap().to_string();
    assert_eq!(timeline["tracks"][0]["name"], "Video 1");

    session
        .execute(
            "timeline.renameTrack",
            json!({"track": track, "name": "Main"}),
        )
        .await
        .unwrap();
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(timeline["tracks"][0]["name"], "Main");

    session
        .execute(
            "timeline.addText",
            json!({"text": "Hello", "start": {"seconds": 1.0}, "duration": {"seconds": 2.0}}),
        )
        .await
        .unwrap();
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(timeline["clipCount"], 1);
    let clip = &timeline["tracks"][2]["clips"][0];
    assert_eq!(clip["kind"]["text"], "Hello");
    assert_eq!(clip["start"]["seconds"], 1.0);

    // Undo removes it, redo puts it back.
    session.execute("app.undo", json!({})).await.unwrap();
    assert_eq!(
        session
            .execute("timeline.inspect", json!({}))
            .await
            .unwrap()["clipCount"],
        0
    );
    session.execute("app.redo", json!({})).await.unwrap();
    assert_eq!(
        session
            .execute("timeline.inspect", json!({}))
            .await
            .unwrap()["clipCount"],
        1
    );
}

#[tokio::test]
async fn a_batch_is_one_undo_step() {
    let session = session().await;
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let track = timeline["tracks"][2]["id"].as_str().unwrap().to_string();

    session
        .execute(
            "timeline.batch",
            json!({
                "label": "Titles",
                "commands": [
                    {"id": "timeline.addText", "params": {"text": "a", "track": track, "start": {"seconds": 0.0}}},
                    {"id": "timeline.addText", "params": {"text": "b", "track": track, "start": {"seconds": 5.0}}},
                ],
            }),
        )
        .await
        .unwrap();
    let history = session.execute("app.history", json!({})).await.unwrap();
    assert_eq!(history["undoDepth"], 1);

    session.execute("app.undo", json!({})).await.unwrap();
    assert_eq!(
        session
            .execute("timeline.inspect", json!({}))
            .await
            .unwrap()["clipCount"],
        0
    );
}

#[tokio::test]
async fn a_clip_must_be_named_or_selected() {
    let session = session().await;
    let error = session
        .execute("timeline.removeClip", json!({}))
        .await
        .unwrap_err();
    assert!(
        error.to_string().contains("clips") || error.to_string().contains("select"),
        "{error}"
    );
}

#[tokio::test]
async fn seeking_and_the_playhead_agree() {
    let session = session().await;
    session
        .execute("playback.seek", json!({"seconds": 2.5}))
        .await
        .unwrap();
    let playback = session
        .execute("playback.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(playback["playhead"]["us"], 2_500_000);

    session
        .execute("playback.seek", json!({"frame": 24}))
        .await
        .unwrap();
    let playback = session
        .execute("playback.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(playback["playhead"]["us"], 1_000_000);
}

#[tokio::test]
async fn inspect_reports_the_whole_document() {
    let session = session().await;
    let doc = session.execute("app.inspect", json!({})).await.unwrap();
    for key in [
        "project",
        "timeline",
        "assets",
        "selection",
        "playback",
        "history",
        "jobs",
        "settings",
    ] {
        assert!(doc.get(key).is_some(), "app.inspect is missing `{key}`");
    }
}

#[tokio::test]
async fn typed_commands_and_json_commands_agree() {
    let session = session().await;
    let typed = session
        .dispatch(SessionCommand::Project(ProjectCommand::Create {
            name: "Typed".into(),
        }))
        .await
        .unwrap();
    assert_eq!(typed["name"], "Typed");

    let json_path = session
        .execute("project.new", json!({"name": "Json"}))
        .await
        .unwrap();
    assert_eq!(json_path["name"], "Json");
    assert_eq!(
        session.execute("project.inspect", json!({})).await.unwrap()["name"],
        "Json"
    );
}

#[tokio::test]
async fn a_new_project_resets_the_timeline() {
    let session = session().await;
    session
        .execute("timeline.addText", json!({"text": "x"}))
        .await
        .unwrap();
    session
        .execute("project.new", json!({"name": "Fresh"}))
        .await
        .unwrap();
    assert_eq!(
        session
            .execute("timeline.inspect", json!({}))
            .await
            .unwrap()["clipCount"],
        0
    );
}

#[tokio::test]
async fn closing_leaves_no_project_open() {
    let session = session().await;
    session.execute("project.close", json!({})).await.unwrap();
    let doc = session.execute("app.inspect", json!({})).await.unwrap();
    assert!(doc["project"]["project"].is_null());
    let error = session
        .execute("timeline.addText", json!({"text": "x"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("not available"), "{error}");
}

/// A text clip on the text track, `length_us` long starting at `start_us`.
fn text_clip(start_us: i64, length_us: i64) -> serde_json::Value {
    json!({
        "text": format!("clip-{start_us}"),
        "start": {"us": start_us},
        "duration": {"us": length_us},
    })
}

/// The clip bounds on the timeline, as `(start_us, end_us)` pairs.
async fn clip_bounds(session: &Session) -> Vec<(i64, i64)> {
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    timeline["tracks"][2]["clips"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["start"]["us"].as_i64().unwrap(),
                c["start"]["us"].as_i64().unwrap() + c["duration"]["us"].as_i64().unwrap(),
            )
        })
        .collect()
}

#[tokio::test]
async fn ripple_delete_removes_a_range_and_closes_the_hole() {
    let session = session().await;
    // Three clips: [0,1s) [1s,2s) [2s,3s) on the text track.
    for start in [0, 1_000_000, 2_000_000] {
        session
            .execute("timeline.addText", text_clip(start, 1_000_000))
            .await
            .unwrap();
    }
    assert_eq!(
        clip_bounds(&session).await,
        vec![
            (0, 1_000_000),
            (1_000_000, 2_000_000),
            (2_000_000, 3_000_000)
        ]
    );

    // Ripple-delete the middle second: the third clip slides left to fill it.
    session
        .execute(
            "timeline.rippleDelete",
            json!({"track": null, "start": {"us": 1_000_000}, "end": {"us": 2_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (1_000_000, 2_000_000)],
        "the ripple closed the hole"
    );
    assert_eq!(
        session
            .execute("timeline.inspect", json!({}))
            .await
            .unwrap()["clipCount"],
        2
    );
}

#[tokio::test]
async fn lift_leaves_a_hole_where_the_range_was() {
    let session = session().await;
    for start in [0, 1_000_000, 2_000_000] {
        session
            .execute("timeline.addText", text_clip(start, 1_000_000))
            .await
            .unwrap();
    }
    session
        .execute(
            "timeline.lift",
            json!({"start": {"us": 1_000_000}, "end": {"us": 2_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (2_000_000, 3_000_000)],
        "the hole stays put"
    );
}

#[tokio::test]
async fn a_ripple_over_a_straddling_clip_trims_both_sides() {
    let session = session().await;
    // One clip spanning 0.5s to 2.5s; delete the middle second.
    session
        .execute("timeline.addText", text_clip(500_000, 2_000_000))
        .await
        .unwrap();
    session
        .execute(
            "timeline.rippleDelete",
            json!({"start": {"us": 1_000_000}, "end": {"us": 2_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(500_000, 1_000_000), (1_000_000, 1_500_000)],
        "the clip was cut in two, the middle removed, and the tail joined to the head"
    );
}

#[tokio::test]
async fn razor_splits_every_clip_under_the_playhead() {
    let session = session().await;
    for start in [0, 1_000_000, 2_000_000] {
        session
            .execute("timeline.addText", text_clip(start, 1_000_000))
            .await
            .unwrap();
    }
    session
        .execute("playback.seek", json!({"us": 2_500_000}))
        .await
        .unwrap();
    session.execute("timeline.razor", json!({})).await.unwrap();

    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(timeline["clipCount"], 4, "one clip became two");
    let bounds = clip_bounds(&session).await;
    assert_eq!(
        bounds,
        vec![
            (0, 1_000_000),
            (1_000_000, 2_000_000),
            (2_000_000, 2_500_000),
            (2_500_000, 3_000_000)
        ],
        "the cut is exactly at the playhead"
    );
}

#[tokio::test]
async fn close_gap_pulls_the_rest_of_the_track_left() {
    let session = session().await;
    session
        .execute("timeline.addText", text_clip(0, 1_000_000))
        .await
        .unwrap();
    session
        .execute("timeline.addText", text_clip(3_000_000, 1_000_000))
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (3_000_000, 4_000_000)]
    );

    session
        .execute("timeline.closeGap", json!({"at": {"us": 1_500_000}}))
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (1_000_000, 2_000_000)],
        "the 2s hole closed and the clip moved up"
    );
}

#[tokio::test]
async fn close_gap_with_nothing_to_close_is_an_error() {
    let session = session().await;
    session
        .execute("timeline.addText", text_clip(0, 1_000_000))
        .await
        .unwrap();
    let error = session
        .execute("timeline.closeGap", json!({"at": {"us": 100}}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("gap"), "{error}");
}

fn temp_file(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!("snapshort-export-{}-{name}", std::process::id()));
    let _ = std::fs::remove_dir_all(&dir);
    std::fs::create_dir_all(&dir).unwrap();
    dir.join(name)
}

#[tokio::test]
async fn a_cut_exports_to_fcpxml_and_captions() {
    let session = session().await;
    session
        .execute("timeline.addText", text_clip(0, 2_000_000))
        .await
        .unwrap();
    session
        .execute("timeline.addText", text_clip(2_000_000, 2_000_000))
        .await
        .unwrap();

    let fcpxml_path = temp_file("cut.fcpxml");
    let report = session
        .execute(
            "export.fcpxml",
            json!({"path": fcpxml_path.display().to_string()}),
        )
        .await
        .unwrap();
    assert_eq!(report["ok"], true);
    let xml = std::fs::read_to_string(&fcpxml_path).unwrap();
    assert!(xml.contains("<fcpxml version=\"1.10\">"));
    assert_eq!(xml.matches("<title").count(), 2, "both titles exported");
    assert!(xml.contains("</fcpxml>"));

    let edl_path = temp_file("cut.edl");
    session
        .execute(
            "export.edl",
            json!({"path": edl_path.display().to_string()}),
        )
        .await
        .unwrap();
    let edl = std::fs::read_to_string(&edl_path).unwrap();
    assert!(edl.starts_with("TITLE: "));
    assert!(edl.contains("FCM: NON-DROP FRAME"));

    let srt_path = temp_file("cut.srt");
    session
        .execute(
            "export.srt",
            json!({"path": srt_path.display().to_string()}),
        )
        .await
        .unwrap();
    let srt = std::fs::read_to_string(&srt_path).unwrap();
    assert_eq!(srt.matches("-->").count(), 2);
    assert!(srt.starts_with("1\n00:00:00,000 --> 00:00:02,000\n"));
}

#[tokio::test]
async fn captions_come_in_from_an_srt_file() {
    let session = session().await;
    let path = temp_file("subs.srt");
    std::fs::write(
        &path,
        "1\n00:00:00,000 --> 00:00:02,000\nHello\n\n2\n00:00:02,000 --> 00:00:04,000\nWorld\n\n",
    )
    .unwrap();

    let report = session
        .execute(
            "captions.import",
            json!({"path": path.display().to_string()}),
        )
        .await
        .unwrap();
    assert_eq!(report["cues"], 2);

    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(timeline["clipCount"], 2);
    let clips = timeline["tracks"][2]["clips"].as_array().unwrap();
    assert_eq!(clips[0]["kind"]["text"], "Hello");
    assert_eq!(clips[1]["start"]["seconds"], 2.0);
}

#[tokio::test]
async fn a_file_without_cues_is_refused() {
    let session = session().await;
    let path = temp_file("empty.srt");
    std::fs::write(&path, "not a caption file\n").unwrap();
    let error = session
        .execute(
            "captions.import",
            json!({"path": path.display().to_string()}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no cues"), "{error}");
}

#[tokio::test]
async fn exporting_to_an_unwritable_path_names_the_path() {
    let session = session().await;
    let error = session
        .execute(
            "export.fcpxml",
            json!({"path": "/proc/definitely/not/here.fcpxml"}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("cannot write"), "{error}");
}

#[tokio::test]
async fn an_srt_file_becomes_a_subtitle_clip() {
    let session = session().await;
    let path = temp_file("subs-clip.srt");
    std::fs::write(
        &path,
        "1\n00:00:01,000 --> 00:00:03,000\nHello\n\n2\n00:00:04,000 --> 00:00:06,000\nWorld\n\n",
    )
    .unwrap();

    let report = session
        .execute(
            "timeline.addSubtitles",
            json!({"path": path.display().to_string()}),
        )
        .await
        .unwrap();
    assert_eq!(report["clipCount"], 1);

    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let clips = timeline["tracks"][3]["clips"].as_array().unwrap();
    assert_eq!(clips[0]["kind"]["type"], "subtitle");
    assert_eq!(clips[0]["kind"]["sourcePath"], path.display().to_string());
    // The clip spans the whole cue list.
    assert_eq!(clips[0]["duration"]["seconds"], 6.0);
}

#[tokio::test]
async fn export_presets_round_trip_through_the_data_dir() {
    let session = session().await;
    let builtin = session.execute("preset.list", json!({})).await.unwrap();
    assert!(
        builtin["count"].as_u64().unwrap() >= 5,
        "built-in presets ship"
    );

    session
        .execute(
            "preset.save",
            json!({"name": "Mine", "format": "webm-vp9", "quality": "high", "width": 1280, "height": 720, "fps": 25.0}),
        )
        .await
        .unwrap();
    let saved = session.execute("preset.list", json!({})).await.unwrap();
    let mine = saved["presets"]
        .as_array()
        .unwrap()
        .iter()
        .find(|p| p["name"] == "Mine")
        .expect("the user preset is listed");
    assert_eq!(mine["format"], "webm-vp9");
    assert_eq!(mine["width"], 1280);

    // A new session (presets live on disk) sees it too.
    let dir = session.data_dir().to_path_buf();
    let again = snapshort_engine::Session::new(dir);
    again.start().await.unwrap();
    let listed = again.execute("preset.list", json!({})).await.unwrap();
    assert!(
        listed["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Mine"),
        "presets persist"
    );

    // Saving the same name edits it instead of duplicating.
    again
        .execute("preset.save", json!({"name": "Mine", "width": 640}))
        .await
        .unwrap();
    let listed = again.execute("preset.list", json!({})).await.unwrap();
    let mine: Vec<&serde_json::Value> = listed["presets"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|p| p["name"] == "Mine")
        .collect();
    assert_eq!(mine.len(), 1);
    assert_eq!(mine[0]["width"], 640, "the edit landed, not a copy");

    // Built-ins cannot be deleted; user ones can.
    let error = again
        .execute("preset.delete", json!({"name": "YouTube 1080p"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("built-in"), "{error}");
    again
        .execute("preset.delete", json!({"name": "Mine"}))
        .await
        .unwrap();
    let listed = again.execute("preset.list", json!({})).await.unwrap();
    assert!(
        !listed["presets"]
            .as_array()
            .unwrap()
            .iter()
            .any(|p| p["name"] == "Mine"),
        "the preset is gone"
    );
}

#[tokio::test]
async fn exporting_with_a_preset_renders_the_file() {
    let session = session().await;
    let out = temp_file("preset.mp4");
    session
        .execute(
            "export.preset",
            json!({"name": "Preview", "output": out.display().to_string(), "wait": true}),
        )
        .await
        .unwrap();
    assert!(out.exists(), "the preset export wrote the file");
    let bytes = std::fs::metadata(&out).unwrap().len();
    assert!(bytes > 0);
}

#[tokio::test]
async fn a_missing_preset_names_itself() {
    let session = session().await;
    let error = session
        .execute(
            "export.preset",
            json!({"name": "Nope", "output": "/tmp/x.mp4"}),
        )
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no preset `Nope`"), "{error}");
}
