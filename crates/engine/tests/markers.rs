//! Timeline markers: the engine owns them, the UI mirrors them.

use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-markers-{}-{}",
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

async fn marker_list(session: &Session) -> Vec<(i64, String)> {
    let list = session.execute("marker.list", json!({})).await.unwrap();
    list["markers"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| {
            (
                m["us"].as_i64().unwrap(),
                m["label"].as_str().unwrap().to_string(),
            )
        })
        .collect()
}

#[tokio::test]
async fn markers_land_at_the_given_time_and_stay_sorted() {
    let session = session().await;
    session
        .execute(
            "marker.add",
            json!({"seconds": 3.0, "label": "third", "color": "FFFFC428"}),
        )
        .await
        .unwrap();
    session
        .execute("marker.add", json!({"seconds": 1.0, "label": "first"}))
        .await
        .unwrap();

    assert_eq!(
        marker_list(&session).await,
        vec![
            (1_000_000, "first".to_string()),
            (3_000_000, "third".to_string())
        ]
    );

    let list = session.execute("marker.list", json!({})).await.unwrap();
    let first = &list["markers"][0];
    assert_eq!(first["index"], 0);
    assert_eq!(first["seconds"], 1.0);
    assert_eq!(list["markers"][1]["color"], "FFFFC428");
}

#[tokio::test]
async fn a_marker_without_a_time_uses_the_playhead() {
    let session = session().await;
    session
        .execute("playback.seek", json!({"seconds": 4.5}))
        .await
        .unwrap();
    session.execute("marker.add", json!({})).await.unwrap();
    assert_eq!(
        marker_list(&session).await,
        vec![(4_500_000, "Marker".to_string())]
    );
}

#[tokio::test]
async fn markers_remove_by_index_or_time() {
    let session = session().await;
    session
        .execute("marker.add", json!({"seconds": 1.0, "label": "a"}))
        .await
        .unwrap();
    session
        .execute("marker.add", json!({"seconds": 2.0, "label": "b"}))
        .await
        .unwrap();

    session
        .execute("marker.remove", json!({"at": {"seconds": 1.2}}))
        .await
        .unwrap();
    assert_eq!(
        marker_list(&session).await,
        vec![(2_000_000, "b".to_string())]
    );

    session
        .execute("marker.remove", json!({"index": 0}))
        .await
        .unwrap();
    assert!(marker_list(&session).await.is_empty());
    let error = session
        .execute("marker.remove", json!({"index": 3}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no such marker"), "{error}");
}

#[tokio::test]
async fn markers_rename_and_clear() {
    let session = session().await;
    session
        .execute("marker.add", json!({"seconds": 1.0, "label": "a"}))
        .await
        .unwrap();
    session
        .execute("marker.rename", json!({"index": 0, "label": "beat"}))
        .await
        .unwrap();
    assert_eq!(
        marker_list(&session).await,
        vec![(1_000_000, "beat".to_string())]
    );

    session.execute("marker.clear", json!({})).await.unwrap();
    assert!(marker_list(&session).await.is_empty());
}

#[tokio::test]
async fn markers_survive_a_save_and_reopen() {
    let session = session().await;
    let dir = session.data_dir().to_path_buf();
    let project = dir.join("cut.snap");
    session
        .execute(
            "marker.add",
            json!({"seconds": 2.0, "label": "cut", "color": "FFFF0000"}),
        )
        .await
        .unwrap();
    session
        .dispatch(snapshort_engine::SessionCommand::Project(
            snapshort_usecases::ProjectCommand::SaveAs {
                path: project.clone(),
                markers: Vec::new(),
            },
        ))
        .await
        .unwrap();

    let again = Session::new(dir);
    again.start().await.unwrap();
    again
        .dispatch(snapshort_engine::SessionCommand::Project(
            snapshort_usecases::ProjectCommand::Open { path: project },
        ))
        .await
        .unwrap();
    let list = again.execute("marker.list", json!({})).await.unwrap();
    assert_eq!(list["count"], 1);
    assert_eq!(list["markers"][0]["label"], "cut");
    assert_eq!(list["markers"][0]["color"], "FFFF0000");
    assert_eq!(list["markers"][0]["seconds"], 2.0);
}

#[tokio::test]
async fn a_new_project_clears_the_markers() {
    let session = session().await;
    session
        .execute("marker.add", json!({"seconds": 1.0}))
        .await
        .unwrap();
    session
        .execute("project.new", json!({"name": "Fresh"}))
        .await
        .unwrap();
    let list = session.execute("marker.list", json!({})).await.unwrap();
    assert_eq!(list["count"], 0, "a new project starts with no markers");
}
