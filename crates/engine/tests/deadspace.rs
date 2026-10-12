//! Dead space, fillers, through edits and gaps on the transcript surface.

use std::sync::atomic::{AtomicUsize, Ordering};

use serde_json::json;
use snapshort_engine::Session;

static NEXT: AtomicUsize = AtomicUsize::new(0);

fn unique(tag: &str) -> std::path::PathBuf {
    std::env::temp_dir().join(format!(
        "snapshort-{tag}-{}-{}",
        std::process::id(),
        NEXT.fetch_add(1, Ordering::SeqCst)
    ))
}

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-transcript-{}-{}",
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

/// Every clip on the text and audio tracks, in order, as (start, end).
async fn clip_bounds(session: &Session) -> Vec<(i64, i64)> {
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let mut out = Vec::new();
    for track in timeline["tracks"].as_array().unwrap() {
        let kind = track["kind"].as_str().unwrap_or("");
        if kind != "text" && kind != "audio" {
            continue;
        }
        for c in track["clips"].as_array().unwrap() {
            let start = c["start"]["us"].as_i64().unwrap();
            let duration = c["duration"]["us"].as_i64().unwrap();
            out.push((start, start + duration));
        }
    }
    out
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

fn words_json(words: &[(&str, i64, i64)]) -> serde_json::Value {
    serde_json::to_value(
        &words
            .iter()
            .map(|(text, start, end)| {
                json!({
                    "text": text,
                    "start": *start as f64 / 1e6,
                    "end": *end as f64 / 1e6
                })
            })
            .collect::<Vec<_>>(),
    )
    .unwrap()
}

async fn import_words(session: &Session, words: &[(&str, i64, i64)]) {
    let dir = unique("twords");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("words.json");
    std::fs::write(&path, words_json(words).to_string()).unwrap();
    session
        .execute(
            "transcript.import",
            json!({"path": path.display().to_string()}),
        )
        .await
        .unwrap();
}

#[tokio::test]
async fn words_import_and_report() {
    let session = session().await;
    import_words(
        &session,
        &[("one", 1_000_000, 2_000_000), ("two", 3_000_000, 4_000_000)],
    )
    .await;
    let words = session
        .execute("transcript.words", json!({}))
        .await
        .unwrap();
    assert_eq!(words["count"], 2);
    assert_eq!(words["words"][0]["startUs"], 1_000_000);
}

#[tokio::test]
async fn dead_space_needs_words_first() {
    let session = session().await;
    let error = session
        .execute("deadspace.find", json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no words yet"), "{error}");
}

#[tokio::test]
async fn dead_space_finds_pauses_and_removes_them() {
    let session = session().await;
    text_clip(&session, 0, 10_000_000).await;
    import_words(
        &session,
        &[
            ("one", 0, 2_000_000),
            ("two", 5_000_000, 7_000_000),
            ("three", 9_000_000, 10_000_000),
        ],
    )
    .await;

    let found = session
        .execute("deadspace.find", json!({"minUs": 500_000, "keepUs": 0}))
        .await
        .unwrap();
    assert_eq!(found["count"], 2, "two gaps between three words");

    let removed = session
        .execute("deadspace.remove", json!({"minUs": 500_000, "keepUs": 0}))
        .await
        .unwrap();
    assert_eq!(removed["removed"], 2);

    // The gaps are gone and what survives is contiguous: no hole between the
    // pieces, and 5s of material in total (frame-snapped, so not exact).
    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds.len(), 3);
    for pair in bounds.windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "pieces must be contiguous");
    }
    let total: i64 = bounds.iter().map(|(s, e)| e - s).sum();
    assert!((total - 5_000_000).abs() < 5_000, "kept {total}us of 10s");
}

#[tokio::test]
async fn dead_space_keeps_a_pad_around_each_cut() {
    let session = session().await;
    text_clip(&session, 0, 10_000_000).await;
    import_words(
        &session,
        &[
            ("one", 0, 2_000_000),
            ("two", 5_000_000, 7_000_000),
            ("three", 9_000_000, 10_000_000),
        ],
    )
    .await;

    session
        .execute(
            "deadspace.remove",
            json!({"minUs": 500_000, "keepUs": 200_000}),
        )
        .await
        .unwrap();

    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds.len(), 3);
    for pair in bounds.windows(2) {
        assert_eq!(pair[0].1, pair[1].0, "pieces must be contiguous");
    }
    // The 3s gap keeps 200ms on both sides (2.6s goes) and the 2s gap likewise
    // (1.6s goes): 4.2s of 10s removed, 5.8s kept.
    let total: i64 = bounds.iter().map(|(s, e)| e - s).sum();
    assert!(
        (total - 5_800_000).abs() < 34_000,
        "kept {total}us, to a frame"
    );
}

#[tokio::test]
async fn dead_space_of_nothing_removes_nothing() {
    let session = session().await;
    text_clip(&session, 0, 10_000_000).await;
    import_words(
        &session,
        &[("one", 0, 2_000_000), ("two", 2_100_000, 4_000_000)],
    )
    .await;

    let before = clip_bounds(&session).await;
    let removed = session
        .execute("deadspace.remove", json!({"minUs": 500_000, "keepUs": 0}))
        .await
        .unwrap();
    assert_eq!(removed["removed"], 0);
    assert_eq!(clip_bounds(&session).await, before);
}

#[tokio::test]
async fn filler_words_are_removed_by_text() {
    let session = session().await;
    text_clip(&session, 0, 10_000_000).await;
    import_words(
        &session,
        &[
            ("one", 0, 1_000_000),
            ("um,", 1_200_000, 1_800_000),
            ("two", 3_000_000, 4_000_000),
        ],
    )
    .await;

    let removed = session
        .execute("fillers.remove", json!({"fillers": ["um"]}))
        .await
        .unwrap();
    assert_eq!(removed["removed"], 1);
}

#[tokio::test]
async fn through_edits_and_gaps_are_reported() {
    let session = session().await;
    let report = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(report["count"], 0);
    let gaps = session.execute("gaps.find", json!({})).await.unwrap();
    assert_eq!(gaps["count"], 0);
}

/// An 8-second mono WAV, synthesised in place so the tests carry their own
/// media fixture and need nothing from outside the repo.
fn write_tone(path: &std::path::Path) {
    let sample_rate = 48_000u32;
    let frames = sample_rate * 8;
    let mut bytes = Vec::new();
    for i in 0..frames {
        let value = (6000.0
            * (2.0 * std::f64::consts::PI * 440.0 * i as f64 / sample_rate as f64).sin())
            as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + bytes.len()) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&sample_rate.to_le_bytes());
    wav.extend_from_slice(&(sample_rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    wav.extend_from_slice(&bytes);
    std::fs::write(path, wav).unwrap();
}

async fn media_clip(session: &Session, name: &str, start_us: i64, length_us: i64) -> String {
    let dir = unique("media");
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join(format!("{name}.wav"));
    write_tone(&path);
    let imported = session
        .execute(
            "asset.import",
            json!({"paths": [path.display().to_string()]}),
        )
        .await
        .unwrap();
    let asset = imported["assets"][0]["id"].as_str().unwrap().to_string();
    session
        .execute(
            "timeline.addAsset",
            json!({
                "asset": asset,
                "start": {"us": start_us},
                "duration": {"us": length_us},
            }),
        )
        .await
        .unwrap();
    asset
}

#[tokio::test]
async fn razor_pieces_join_back_into_one_clip() {
    let session = session().await;
    media_clip(&session, "reel", 0, 6_000_000).await;

    for at in [2_000_000usize, 4_000_000usize] {
        session
            .execute("timeline.razor", json!({"at": {"us": at}}))
            .await
            .unwrap();
    }
    let found = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(found["count"], 2, "two cuts between three pieces");

    let joined = session
        .execute("timeline.joinThroughEdits", json!({}))
        .await
        .unwrap();
    assert_eq!(joined["joined"], 2);

    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds, vec![(0, 6_000_000)], "the pieces heal into one");

    let report = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(report["count"], 0, "no through edits are left");
}

#[tokio::test]
async fn two_different_text_clips_are_never_a_through_edit() {
    let session = session().await;
    text_clip(&session, 0, 2_000_000).await;
    text_clip(&session, 2_000_000, 2_000_000).await;

    let found = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(
        found["count"], 0,
        "adjacent text clips carry their own content and must not be joined"
    );

    let joined = session
        .execute("timeline.joinThroughEdits", json!({}))
        .await
        .unwrap();
    assert_eq!(joined["joined"], 0);

    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds.len(), 2, "both text clips survive untouched");
}

#[tokio::test]
async fn different_media_are_never_a_through_edit() {
    let session = session().await;
    media_clip(&session, "alpha", 0, 2_000_000).await;
    media_clip(&session, "beta", 2_000_000, 2_000_000).await;

    let found = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(
        found["count"], 0,
        "different sources are never a through edit"
    );

    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds.len(), 2);
}

#[tokio::test]
async fn joining_a_clip_with_nothing_to_join_changes_nothing() {
    let session = session().await;
    text_clip(&session, 0, 2_000_000).await;
    text_clip(&session, 3_000_000, 2_000_000).await;
    let before = clip_bounds(&session).await;

    let joined = session
        .execute("timeline.joinThroughEdits", json!({}))
        .await
        .unwrap();
    assert_eq!(joined["joined"], 0);
    assert_eq!(clip_bounds(&session).await, before);
}

#[tokio::test]
async fn dead_space_leaves_adjacent_pieces_that_are_not_through_edits() {
    let session = session().await;
    media_clip(&session, "reel", 0, 10_000_000).await;
    import_words(
        &session,
        &[
            ("one", 0, 2_000_000),
            ("two", 5_000_000, 7_000_000),
            ("three", 9_000_000, 10_000_000),
        ],
    )
    .await;

    session
        .execute("deadspace.remove", json!({"minUs": 500_000, "keepUs": 0}))
        .await
        .unwrap();

    let bounds = clip_bounds(&session).await;
    assert_eq!(bounds.len(), 3, "each word run is its own piece");
    for pair in bounds.windows(2) {
        assert_eq!(
            pair[0].1, pair[1].0,
            "the pieces are adjacent on the timeline"
        );
    }
    let total: i64 = bounds.iter().map(|(s, e)| e - s).sum();
    assert!(
        (total - 5_000_000).abs() < 34_000,
        "the dead space is gone: kept {total}us of 10s"
    );

    // Dead space skips media, so these are not through edits: joining them would
    // pull the silence back in. Detection must refuse them.
    let found = session.execute("through.find", json!({})).await.unwrap();
    assert_eq!(found["count"], 0, "skipping media is not a through edit");
    session
        .execute("timeline.joinThroughEdits", json!({}))
        .await
        .unwrap();
    assert_eq!(clip_bounds(&session).await, bounds, "nothing changed");
}
