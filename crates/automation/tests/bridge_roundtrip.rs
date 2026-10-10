//! The agent's path end to end: a bridge client, the control channel, and a
//! real session behind it.

use std::sync::Arc;

use serde_json::{Value, json};
use snapshort_automation::{BridgeClient, ControlFuture, ControlHandler, ControlServer};
use snapshort_engine::Session;
use tokio::net::TcpListener;

struct App {
    session: Arc<Session>,
}

impl ControlHandler for App {
    fn engine_execute<'a>(&'a self, command: &'a str, params: Value) -> ControlFuture<'a> {
        Box::pin(async move {
            self.session
                .execute(command, params)
                .await
                .map_err(|e| e.to_string())
        })
    }

    fn ui<'a>(&'a self, method: &'a str, params: Value) -> ControlFuture<'a> {
        Box::pin(async move { Ok(json!({"method": method, "echo": params})) })
    }
}

async fn spawn_app() -> (String, Arc<Session>) {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-bridge-{}-{}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let session = Arc::new(Session::new(dir));
    session.start().await.expect("session starts");
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap().to_string();
    let server = ControlServer::new(Arc::new(App {
        session: session.clone(),
    }));
    tokio::spawn(async move { server.serve_on(listener).await.unwrap() });
    (addr, session)
}

#[tokio::test]
async fn an_agent_drives_a_real_session_over_the_bridge() {
    let (addr, session) = spawn_app().await;
    let bridge = BridgeClient::new(&addr).unwrap();

    let clip = bridge
        .execute(
            "timeline.addText",
            json!({"text": "bridge", "start": {"seconds": 1}}),
        )
        .await
        .expect("the command ran");
    assert_eq!(clip["clipCount"], 1);

    // The session behind the handler holds the edit the bridge just made.
    let timeline = session
        .execute("timeline.inspect", json!({}))
        .await
        .unwrap();
    assert_eq!(timeline["clipCount"], 1);
    assert_eq!(timeline["tracks"][2]["clips"][0]["kind"]["text"], "bridge");

    // Unknown commands come back as errors, not crashes.
    let error = bridge.execute("nope", json!({})).await.unwrap_err();
    assert!(error.to_string().contains("unknown command"), "{error}");
}

#[tokio::test]
async fn ui_actions_round_trip() {
    let (addr, _session) = spawn_app().await;
    let bridge = BridgeClient::new(&addr).unwrap();
    let reply = bridge
        .call("ui.elements", json!({"prefix": "panel."}))
        .await
        .unwrap();
    assert_eq!(reply["method"], "ui.elements");
    assert_eq!(reply["echo"]["prefix"], "panel.");
}
