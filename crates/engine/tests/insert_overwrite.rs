//! Three-point editing: insert pushes, overwrite replaces.

use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-insert-{}-{}",
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

async fn text_clip(session: &Session, start_us: i64, length_us: i64, text: &str) {
    session
        .execute(
            "timeline.addText",
            json!({
                "text": text,
                "start": {"us": start_us},
                "duration": {"us": length_us},
            }),
        )
        .await
        .unwrap();
}

async fn clip_id(session: &Session, index: usize) -> String {
    session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap()["tracks"][2]["clips"][index]["id"]
        .as_str()
        .unwrap()
        .to_string()
}

/// (start_us, length_us, label) per clip on the text track.
async fn clips(session: &Session) -> Vec<(i64, i64, String)> {
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
                c["duration"]["us"].as_i64().unwrap(),
                c["kind"]["text"].as_str().unwrap_or("").to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn insert_pushes_what_follows_out_of_the_way() {
    let session = session().await;
    text_clip(&session, 0, 1_000_000, "A").await;
    text_clip(&session, 2_000_000, 1_000_000, "B").await;

    // Insert a copy of the 1s clip A at 1s, leaving a 1s hole before B.
    session
        .execute(
            "timeline.insert",
            json!({"clip": clip_id(&session, 0).await, "at": {"us": 1_000_000}}),
        )
        .await
        .unwrap();

    assert_eq!(
        clips(&session).await,
        vec![
            (0, 1_000_000, "A".to_string()),
            (1_000_000, 1_000_000, "A".to_string()),
            (3_000_000, 1_000_000, "B".to_string()),
        ],
        "the insert filled the hole and pushed B right by what it added"
    );
}

#[tokio::test]
async fn insert_splits_the_clip_it_lands_in() {
    let session = session().await;
    text_clip(&session, 0, 4_000_000, "A").await;

    session
        .execute(
            "timeline.insert",
            json!({"clip": clip_id(&session, 0).await, "at": {"us": 1_000_000}}),
        )
        .await
        .unwrap();

    assert_eq!(
        clips(&session).await,
        vec![
            (0, 1_000_000, "A".to_string()),
            (1_000_000, 4_000_000, "A".to_string()),
            (5_000_000, 3_000_000, "A".to_string()),
        ],
        "the clip was cut, the insert dropped in, and the tail rejoined after it"
    );
}

#[tokio::test]
async fn overwrite_replaces_without_rippling() {
    let session = session().await;
    // A 2s clip to overwrite with, then a long clip to overwrite into.
    text_clip(&session, 0, 2_000_000, "C").await;
    text_clip(&session, 2_000_000, 6_000_000, "A").await;

    session
        .execute(
            "timeline.overwrite",
            json!({"clip": clip_id(&session, 0).await, "at": {"us": 4_000_000}}),
        )
        .await
        .unwrap();

    assert_eq!(
        clips(&session).await,
        vec![
            (0, 2_000_000, "C".to_string()),
            (2_000_000, 2_000_000, "A".to_string()),
            (4_000_000, 2_000_000, "C".to_string()),
            (6_000_000, 2_000_000, "A".to_string()),
        ],
        "the middle of A was replaced, its tail kept, and C untouched"
    );
}

#[tokio::test]
async fn insert_defaults_to_the_playhead() {
    let session = session().await;
    text_clip(&session, 0, 1_000_000, "A").await;
    text_clip(&session, 2_000_000, 1_000_000, "B").await;
    session
        .execute("playback.seek", json!({"seconds": 1}))
        .await
        .unwrap();

    session
        .execute(
            "timeline.insert",
            json!({"clip": clip_id(&session, 0).await}),
        )
        .await
        .unwrap();
    let clips = clips(&session).await;
    assert_eq!(clips.len(), 3);
    assert_eq!(clips[1].0, 1_000_000, "the insert landed at the playhead");
}

#[tokio::test]
async fn inserting_needs_material() {
    let session = session().await;
    let error = session
        .execute("timeline.insert", json!({"at": {"us": 0}}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("asset"), "{error}");
}

#[tokio::test]
async fn an_insert_undoes_as_one_step() {
    let session = session().await;
    text_clip(&session, 0, 1_000_000, "A").await;
    text_clip(&session, 2_000_000, 1_000_000, "B").await;
    let before = clips(&session).await;

    session
        .execute(
            "timeline.insert",
            json!({"clip": clip_id(&session, 0).await, "at": {"us": 1_000_000}}),
        )
        .await
        .unwrap();
    assert_eq!(clips(&session).await.len(), 3);

    session.execute("app.undo", json!({})).await.unwrap();
    assert_eq!(clips(&session).await, before, "undo restored the cut");
}
