//! The control channel: the app serves commands and UI actions to a bridge
//! client over a loopback JSON-lines socket.
//!
//! The app implements [`ControlHandler`]: `engine.execute` runs a command on
//! its session, `ui.*` performs an action on its own (UI) thread. Both reply
//! with the result or an error string, so a bridge client sees one protocol.

use std::future::Future;
use std::pin::Pin;
use std::sync::Arc;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};

use crate::AutomationError;

pub type ControlFuture<'a> = Pin<Box<dyn Future<Output = Result<Value, String>> + Send + 'a>>;

/// What the app gives the control channel.
pub trait ControlHandler: Send + Sync {
    /// Run engine command `id` with JSON params.
    fn engine_execute<'a>(&'a self, command: &'a str, params: Value) -> ControlFuture<'a>;
    /// Perform a UI action (`ui.inspect`, `ui.click`, …).
    fn ui<'a>(&'a self, method: &'a str, params: Value) -> ControlFuture<'a>;
}

pub struct ControlServer {
    handler: Arc<dyn ControlHandler>,
}

impl ControlServer {
    pub fn new(handler: Arc<dyn ControlHandler>) -> Self {
        Self { handler }
    }

    /// Serve on `addr` (loopback only) until the listener fails.
    pub async fn serve(&self, addr: &str) -> Result<(), AutomationError> {
        let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(addr);
        if !matches!(
            host,
            "127.0.0.1" | "localhost" | "[::1]" | "::1" | "0.0.0.0"
        ) {
            return Err(AutomationError::BadRequest(format!(
                "control address must be loopback, got `{addr}`"
            )));
        }
        let listener = TcpListener::bind(addr)
            .await
            .map_err(|e| AutomationError::Bridge(format!("cannot listen on {addr}: {e}")))?;
        self.serve_on(listener).await
    }

    /// Serve on an already-bound listener.
    pub async fn serve_on(&self, listener: TcpListener) -> Result<(), AutomationError> {
        loop {
            let (stream, _) = listener
                .accept()
                .await
                .map_err(|e| AutomationError::Bridge(format!("accept failed: {e}")))?;
            let handler = self.handler.clone();
            tokio::spawn(async move {
                if let Err(e) = serve_connection(handler, stream).await {
                    tracing::debug!("control connection ended: {e}");
                }
            });
        }
    }
}

async fn serve_connection(
    handler: Arc<dyn ControlHandler>,
    stream: TcpStream,
) -> Result<(), AutomationError> {
    let (reader, mut writer) = stream.into_split();
    let mut lines = BufReader::new(reader).lines();
    while let Some(line) = lines
        .next_line()
        .await
        .map_err(|e| AutomationError::Bridge(e.to_string()))?
    {
        if line.trim().is_empty() {
            continue;
        }
        let request: Value = match serde_json::from_str(&line) {
            Ok(v) => v,
            Err(e) => {
                let reply =
                    json!({"id": Value::Null, "ok": false, "error": format!("parse error: {e}")});
                writer
                    .write_all(format!("{reply}\n").as_bytes())
                    .await
                    .map_err(|e| AutomationError::Bridge(e.to_string()))?;
                writer.flush().await.ok();
                continue;
            }
        };
        let id = request.get("id").cloned().unwrap_or(Value::Null);
        let method = request
            .get("method")
            .and_then(Value::as_str)
            .unwrap_or_default();
        let params = request.get("params").cloned().unwrap_or_else(|| json!({}));

        let reply = match dispatch(&handler, method, params).await {
            Ok(result) => json!({"id": id, "ok": true, "result": result}),
            Err(error) => json!({"id": id, "ok": false, "error": error}),
        };
        writer
            .write_all(format!("{reply}\n").as_bytes())
            .await
            .map_err(|e| AutomationError::Bridge(e.to_string()))?;
        writer.flush().await.ok();
    }
    Ok(())
}

async fn dispatch(
    handler: &Arc<dyn ControlHandler>,
    method: &str,
    params: Value,
) -> Result<Value, String> {
    match method {
        "engine.execute" => {
            let command = params
                .get("command")
                .and_then(Value::as_str)
                .ok_or_else(|| "`command` is required".to_string())?
                .to_string();
            let inner = params.get("params").cloned().unwrap_or_else(|| json!({}));
            handler.engine_execute(&command, inner).await
        }
        other if other.starts_with("ui.") => handler.ui(other, params).await,
        "ping" => Ok(json!({"pong": true})),
        other => Err(format!("unknown method `{other}`")),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Echo;

    impl ControlHandler for Echo {
        fn engine_execute<'a>(&'a self, command: &'a str, params: Value) -> ControlFuture<'a> {
            Box::pin(async move { Ok(json!({"command": command, "params": params})) })
        }
        fn ui<'a>(&'a self, method: &'a str, _params: Value) -> ControlFuture<'a> {
            Box::pin(async move { Ok(json!({"method": method})) })
        }
    }

    #[tokio::test]
    async fn a_connection_gets_replies_and_survives_garbage() {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let bind = listener.local_addr().unwrap().to_string();
        let server = ControlServer::new(Arc::new(Echo));
        tokio::spawn(async move { server.serve_on(listener).await.unwrap() });

        let stream = TcpStream::connect(bind.as_str()).await.unwrap();
        let (reader, mut writer) = stream.into_split();
        let mut lines = BufReader::new(reader).lines();

        writer
            .write_all(b"{\"id\":1,\"method\":\"engine.execute\",\"params\":{\"command\":\"timeline.inspect\",\"params\":{}}}\n")
            .await
            .unwrap();
        let reply: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["id"], 1);
        assert_eq!(reply["result"]["command"], "timeline.inspect");

        writer.write_all(b"not json\n").await.unwrap();
        let reply: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["ok"], false);
        assert!(reply["error"].as_str().unwrap().contains("parse error"));

        writer
            .write_all(
                b"{\"id\":2,\"method\":\"ui.click\",\"params\":{\"id\":\"timeline.clip.x\"}}\n",
            )
            .await
            .unwrap();
        let reply: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["result"]["method"], "ui.click");

        writer
            .write_all(b"{\"id\":3,\"method\":\"nope\"}\n")
            .await
            .unwrap();
        let reply: Value =
            serde_json::from_str(&lines.next_line().await.unwrap().unwrap()).unwrap();
        assert_eq!(reply["ok"], false);
    }

    #[tokio::test]
    async fn only_loopback_is_served() {
        let server = ControlServer::new(Arc::new(Echo));
        assert!(
            server.serve("10.0.0.1:9000").await.is_err(),
            "a non-loopback address must be refused"
        );
    }
}
