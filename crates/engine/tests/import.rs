//! Asset import is the first thing an agent does, and the CLI is one shot: a
//! command that reports an empty list it just populated is worse than an error.

use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-import-{}-{}",
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

fn tone(path: &std::path::Path) {
    let sample_rate = 48_000u32;
    let frames = sample_rate * 2;
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

#[tokio::test]
async fn an_import_reports_the_asset_it_just_added() {
    let session = session().await;
    let dir = std::env::temp_dir().join(format!("snapshort-imp-{}", std::process::id()));
    std::fs::create_dir_all(&dir).unwrap();
    let path = dir.join("tone.wav");
    tone(&path);

    let report = session
        .execute(
            "asset.import",
            json!({"paths": [path.display().to_string()]}),
        )
        .await
        .unwrap();

    assert_eq!(report["count"], 1, "the import reported its own asset");
    assert_eq!(report["assets"][0]["type"], "audio");
}

#[tokio::test]
async fn a_missing_file_imports_as_an_offline_asset() {
    let session = session().await;
    let missing = std::env::temp_dir().join(format!("snapshort-gone-{}.mp4", std::process::id()));

    let report = session
        .execute(
            "asset.import",
            json!({"paths": [missing.display().to_string()]}),
        )
        .await
        .unwrap();

    assert_eq!(
        report["count"], 1,
        "the row is kept so references stay valid"
    );
    assert!(
        report["assets"][0]["status"]
            .as_str()
            .unwrap()
            .contains("Offline")
    );
}
