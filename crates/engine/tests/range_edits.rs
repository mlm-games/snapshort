use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-range-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let session = Session::new(dir);
    session.start().await.expect("session starts");
    session
}

async fn text_clip(session: &Session, start_us: i64, length_us: i64) {
    session
        .execute(
            "timeline.addText",
            json!({
                "text": format!("clip-{start_us}"),
                "start": {"us": start_us},
                "duration": {"us": length_us},
            }),
        )
        .await
        .unwrap();
}

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
async fn a_ripple_undoes_back_to_the_original_cut() {
    let session = session().await;
    for start in [0, 1_000_000, 2_000_000, 3_000_000] {
        text_clip(&session, start, 1_000_000).await;
    }
    let before = clip_bounds(&session).await;

    session
        .execute(
            "timeline.rippleDelete",
            json!({"start": {"us": 1_000_000}, "end": {"us": 3_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (1_000_000, 2_000_000)],
        "two seconds of ripple closed up"
    );

    session.execute("app.undo", json!({})).await.unwrap();
    assert_eq!(clip_bounds(&session).await, before, "undo restored the cut");

    session.execute("app.redo", json!({})).await.unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (1_000_000, 2_000_000)]
    );
}

#[tokio::test]
async fn a_ripple_over_an_empty_part_of_a_track_is_a_no_op() {
    let session = session().await;
    text_clip(&session, 0, 1_000_000).await;
    session
        .execute(
            "timeline.rippleDelete",
            json!({"start": {"us": 5_000_000}, "end": {"us": 6_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(clip_bounds(&session).await, vec![(0, 1_000_000)]);
}

#[tokio::test]
async fn a_ripple_only_touches_the_named_track() {
    let session = session().await;
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let video_track = timeline["tracks"][0]["id"].as_str().unwrap().to_string();
    let text_track = timeline["tracks"][2]["id"].as_str().unwrap().to_string();
    text_clip(&session, 0, 1_000_000).await;
    text_clip(&session, 4_000_000, 1_000_000).await;

    // Ripple on the video track: nothing there, so nothing moves.
    session
        .execute(
            "timeline.rippleDelete",
            json!({"track": video_track, "start": {"us": 0}, "end": {"us": 1_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(0, 1_000_000), (4_000_000, 5_000_000)],
        "the text track was left alone"
    );

    // Ripple on the text track: the later clip moves up.
    session
        .execute(
            "timeline.rippleDelete",
            json!({"track": text_track, "start": {"us": 0}, "end": {"us": 1_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(3_000_000, 4_000_000)],
        "the clip after the range moved left by the removed second"
    );
}

#[tokio::test]
async fn a_ripple_from_a_named_clip_uses_that_clips_extent() {
    let session = session().await;
    text_clip(&session, 0, 1_000_000).await;
    text_clip(&session, 2_000_000, 1_000_000).await;
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let first = timeline["tracks"][2]["clips"][0]["id"]
        .as_str()
        .unwrap()
        .to_string();

    session
        .execute("timeline.rippleDelete", json!({"clips": [first]}))
        .await
        .unwrap();
    assert_eq!(
        clip_bounds(&session).await,
        vec![(1_000_000, 2_000_000)],
        "the follower moved left by the removed second"
    );
}
