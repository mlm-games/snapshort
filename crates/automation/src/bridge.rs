//! Client for the app's JSON-lines control protocol. Loopback only, one
//! connection, reconnect on failure.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::AutomationError;

/// How often a blocking export's app job is polled.
const JOB_POLL: Duration = Duration::from_millis(250);

type Conn = (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
);

pub struct BridgeClient {
    addr: String,
    conn: Mutex<Option<Conn>>,
    next_id: AtomicU64,
}

impl BridgeClient {
    /// `addr` such as `127.0.0.1:9876` (loopback only).
    pub fn new(addr: impl Into<String>) -> Result<Self, AutomationError> {
        let addr = addr.into();
        let host = addr.rsplit_once(':').map(|(h, _)| h).unwrap_or(&addr);
        if !matches!(host, "127.0.0.1" | "localhost" | "[::1]" | "::1") {
            return Err(AutomationError::BadRequest(format!(
                "bridge address must be loopback, got `{addr}`"
            )));
        }
        Ok(Self {
            addr,
            conn: Mutex::new(None),
            next_id: AtomicU64::new(1),
        })
    }

    pub fn addr(&self) -> &str {
        &self.addr
    }

    /// Run engine command `id` in the app. A blocking export (`render.export`
    /// with `wait: true`) is started as an app job and polled here until it
    /// finishes, so the app keeps repainting and answering other requests.
    pub async fn execute(&self, id: &str, mut params: Value) -> Result<Value, AutomationError> {
        if crate::long_job::is_long(id, &params) {
            if let Some(p) = params.as_object_mut() {
                p.insert("wait".into(), json!(false));
            }
            let mut start = self
                .call("engine.execute", json!({"command": id, "params": params}))
                .await?;
            let Some(job) = start.get("job").and_then(Value::as_str) else {
                return Ok(start);
            };
            let result = loop {
                tokio::time::sleep(JOB_POLL).await;
                let jobs = self
                    .call(
                        "engine.execute",
                        json!({"command": "jobs.list", "params": {}}),
                    )
                    .await?;
                let Some(j) = jobs
                    .as_array()
                    .and_then(|a| a.iter().find(|j| j["id"].as_str() == Some(job)))
                    .cloned()
                else {
                    break Value::Null;
                };
                if j["status"].as_str() != Some("queued") && j["status"].as_str() != Some("running")
                {
                    break j["result"].clone();
                }
            };
            if let Some(e) = result.get("error").and_then(Value::as_str) {
                return Err(AutomationError::App(format!("export failed: {e}")));
            }
            if let Some(o) = start.as_object_mut() {
                o.insert("result".into(), result);
            }
            Ok(start)
        } else {
            self.call("engine.execute", json!({"command": id, "params": params}))
                .await
        }
    }

    /// Call a control method; returns `result` or the app's error.
    pub async fn call(&self, method: &str, params: Value) -> Result<Value, AutomationError> {
        let mut guard = self.conn.lock().await;
        for attempt in 0..2 {
            if guard.is_none() {
                let s = tokio::time::timeout(Duration::from_secs(5), TcpStream::connect(&self.addr))
                    .await
                    .map_err(|_| AutomationError::Bridge(format!("timed out connecting to {}", self.addr)))?
                    .map_err(|e| {
                        AutomationError::Bridge(format!(
                            "cannot connect to {} ({e}); start the app with `snapshort --control <port>`",
                            self.addr
                        ))
                    })?;
                let (r, w) = s.into_split();
                *guard = Some((BufReader::new(r), w));
            }
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
            let Some(conn) = guard.as_mut() else {
                return Err(AutomationError::Bridge(format!(
                    "not connected to {}",
                    self.addr
                )));
            };
            let line = format!(
                "{}\n",
                json!({"id": id, "method": method, "params": params})
            );
            let res: Result<Value, AutomationError> = async {
                conn.1
                    .write_all(line.as_bytes())
                    .await
                    .map_err(|e| AutomationError::Bridge(e.to_string()))?;
                conn.1.flush().await.ok();
                let mut buf = String::new();
                tokio::time::timeout(Duration::from_secs(120), conn.0.read_line(&mut buf))
                    .await
                    .map_err(|_| AutomationError::Bridge("the app did not answer in time".into()))?
                    .map_err(|e| AutomationError::Bridge(e.to_string()))?;
                serde_json::from_str(&buf)
                    .map_err(|e| AutomationError::Bridge(format!("bad reply: {e}")))
            }
            .await;
            match res {
                Ok(v) => {
                    return if v.get("ok").and_then(Value::as_bool) == Some(true) {
                        Ok(v.get("result").cloned().unwrap_or(Value::Null))
                    } else {
                        Err(AutomationError::App(
                            v.get("error")
                                .and_then(Value::as_str)
                                .unwrap_or("error")
                                .to_string(),
                        ))
                    };
                }
                Err(e) if attempt == 0 => {
                    *guard = None;
                    let _ = e;
                }
                Err(e) => return Err(e),
            }
        }
        Err(AutomationError::Bridge("unreachable".into()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;
    use tokio::net::TcpListener;
    use tokio::sync::Mutex as AsyncMutex;

    async fn fake_app(export_job: Option<&str>) -> (String, Arc<AsyncMutex<Vec<Value>>>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let seen = Arc::new(AsyncMutex::new(Vec::new()));
        let log = seen.clone();
        let job = export_job.map(str::to_string);
        tokio::spawn(async move {
            let (s, _) = listener.accept().await.unwrap();
            let (r, mut w) = s.into_split();
            let mut lines = BufReader::new(r).lines();
            let mut polls = 0;
            while let Ok(Some(line)) = lines.next_line().await {
                let req: Value = serde_json::from_str(&line).unwrap();
                log.lock().await.push(req.clone());
                let reply = match req["params"]["command"].as_str() {
                    Some("render.export") => match &job {
                        Some(job) => json!({"job": job, "output": "/tmp/out.mp4"}),
                        None => json!({"ok": true}),
                    },
                    Some("jobs.list") => {
                        polls += 1;
                        let done = polls >= 2;
                        json!([{
                            "id": job,
                            "status": if done { "succeeded" } else { "running" },
                            "result": if done { json!({"ok": true}) } else { Value::Null },
                        }])
                    }
                    _ => json!({"ok": true}),
                };
                let line = format!(
                    "{}\n",
                    json!({"id": req["id"], "ok": true, "result": reply})
                );
                w.write_all(line.as_bytes()).await.unwrap();
            }
        });
        (addr, seen)
    }

    #[tokio::test]
    async fn a_blocking_export_is_polled_as_a_job() {
        let (addr, seen) = fake_app(Some("job-7")).await;
        let b = BridgeClient::new(addr).unwrap();
        let r = b
            .execute(
                "render.export",
                json!({"output": "/tmp/out.mp4", "wait": true}),
            )
            .await
            .unwrap();
        assert_eq!(r["result"], json!({"ok": true}));
        let seen = seen.lock().await;
        assert!(
            seen.iter()
                .all(|q| q["params"]["params"]["wait"] != json!(true)),
            "the app never sees wait: true"
        );
        assert_eq!(
            seen.iter()
                .filter(|q| q["params"]["command"] == "jobs.list")
                .count(),
            2
        );
    }

    #[tokio::test]
    async fn other_commands_are_forwarded() {
        let (addr, seen) = fake_app(None).await;
        let b = BridgeClient::new(addr).unwrap();
        b.execute("timeline.inspect", json!({})).await.unwrap();
        b.call("ui.inspect", json!({})).await.unwrap();
        let seen = seen.lock().await;
        assert_eq!(seen.len(), 2);
        assert_eq!(seen[0]["params"]["command"], "timeline.inspect");
        assert_eq!(seen[1]["method"], "ui.inspect");
    }

    #[test]
    fn only_loopback_is_allowed() {
        assert!(BridgeClient::new("127.0.0.1:9876").is_ok());
        assert!(BridgeClient::new("192.168.1.5:9876").is_err());
    }
}
