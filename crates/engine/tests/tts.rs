//! Narration: the formant voice speaks a script onto the timeline with nothing
//! to install.

use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-tts-{}-{}",
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

#[tokio::test]
async fn the_built_in_voices_are_listed_and_installed() {
    let session = session().await;
    let report = session.execute("tts.voices", json!({})).await.unwrap();
    let voices = report["voices"].as_array().unwrap();
    assert_eq!(voices.len(), 2);
    assert!(voices.iter().all(|v| v["installed"].as_bool().unwrap()));
    assert!(voices.iter().any(|v| v["id"] == "basic-female"));
    assert!(voices.iter().any(|v| v["id"] == "basic-male"));
}

#[tokio::test]
async fn previewing_a_script_reports_real_audio() {
    let session = session().await;
    let report = session
        .execute(
            "tts.preview",
            json!({"text": "One two three.", "voice": "basic-male"}),
        )
        .await
        .unwrap();
    assert_eq!(report["sampleRate"], 24000);
    assert!(report["seconds"].as_f64().unwrap() > 0.5);
    assert!(report["samples"].as_u64().unwrap() > 12000);
}

#[tokio::test]
async fn a_pause_marker_stretches_the_audio() {
    let session = session().await;
    let plain = session
        .execute("tts.preview", json!({"text": "One. Two."}))
        .await
        .unwrap();
    let paused = session
        .execute("tts.preview", json!({"text": "One. [pause 1s] Two."}))
        .await
        .unwrap();
    let extra = paused["seconds"].as_f64().unwrap() - plain["seconds"].as_f64().unwrap();
    assert!(
        extra > 0.8 && extra < 1.5,
        "a one-second pause adds about one second: {extra}"
    );
}

#[tokio::test]
async fn an_unknown_voice_names_itself() {
    let session = session().await;
    let error = session
        .execute("tts.preview", json!({"text": "Hi.", "voice": "nope"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("nope"), "{error}");
}

#[tokio::test]
async fn narration_lands_on_the_timeline_as_a_clip() {
    let session = session().await;
    let report = session
        .execute(
            "tts.addToTimeline",
            json!({"text": "One two three.", "voice": "basic-female"}),
        )
        .await
        .unwrap();
    assert!(report["seconds"].as_f64().unwrap() > 0.5);
    assert_eq!(report["timeline"]["clipCount"], 1);
}
