//! Title templates and the rolling autosave versions.

use serde_json::json;
use snapshort_engine::Session;

async fn session() -> Session {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-titles-{}-{}",
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
async fn templates_ship_and_style_a_clip() {
    let session = session().await;
    let list = session.execute("text.templates", json!({})).await.unwrap();
    let names: Vec<&str> = list["templates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|t| t["name"].as_str().unwrap())
        .collect();
    assert_eq!(names, vec!["title", "subtitle", "lower-third", "caption"]);

    session
        .execute(
            "timeline.addText",
            json!({"text": "Hello", "template": "lower-third"}),
        )
        .await
        .unwrap();
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    let style = &timeline["tracks"][2]["clips"][0]["kind"]["style"];
    assert_eq!(style["font_size"], 28.0);
    assert_eq!(style["alignment"], "Left");
}

#[tokio::test]
async fn an_unknown_template_names_itself() {
    let session = session().await;
    let error = session
        .execute("timeline.addText", json!({"text": "x", "template": "nope"}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no template `nope`"), "{error}");
}

#[tokio::test]
async fn recovery_lists_versions_and_restores_one() {
    let session = session().await;
    // No versions yet.
    let empty = session
        .execute("project.recovery", json!({}))
        .await
        .unwrap();
    assert_eq!(empty["count"], 0);

    // Two versions, the second newer, written directly like the autosave task.
    let dir = session.data_dir().to_path_buf();
    let project = dir.join("cut.snap");
    session
        .execute("timeline.addText", json!({"text": "one"}))
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

    for (index, text) in [(0, "one"), (1, "two")] {
        let snapshot = snapshort_usecases::ProjectSnapshot::new(
            miniter_domain::Project::new(format!("v{index}")),
            Vec::new(),
            Vec::new(),
        );
        let _ = text;
        let stamp = snapshort_usecases::now_ms() + index as i64;
        let path = snapshort_usecases::autosave_version_path(&dir, stamp);
        let meta = snapshort_usecases::AutosaveMeta {
            project_path: None,
            project_name: format!("v{index}"),
            saved_at_ms: stamp,
        };
        snapshort_usecases::write_autosave_at(&path, &snapshot, &meta).unwrap();
    }

    let list = session
        .execute("project.recovery", json!({}))
        .await
        .unwrap();
    assert_eq!(list["count"], 2, "both versions are listed");
    assert_eq!(
        list["versions"][0]["projectName"], "v1",
        "the newest version comes first"
    );

    // Restoring the older one opens that version's project.
    session
        .execute("project.restoreVersion", json!({"index": 1}))
        .await
        .unwrap();
    let project = session.execute("project.inspect", json!({})).await.unwrap();
    assert_eq!(project["name"], "v0");
}

#[tokio::test]
async fn restoring_with_no_versions_is_an_error() {
    let session = session().await;
    let error = session
        .execute("project.restoreVersion", json!({}))
        .await
        .unwrap_err();
    assert!(error.to_string().contains("no saved versions"), "{error}");
}
