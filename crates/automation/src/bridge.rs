//! Client for the app's JSON-lines control protocol. Loopback only, one
//! connection per request in flight, reused once its reply is in.

use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;

use serde_json::{Value, json};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::sync::Mutex;

use crate::AutomationError;

/// How often a blocking export's app job is polled.
const JOB_POLL: Duration = Duration::from_millis(250);
/// Idle connections kept for reuse; the app serves a bounded number at once.
const MAX_IDLE: usize = 4;

type Conn = (
    BufReader<tokio::net::tcp::OwnedReadHalf>,
    tokio::net::tcp::OwnedWriteHalf,
);

pub struct BridgeClient {
    addr: String,
    /// Connections with no request in flight. A request takes one (or opens
    /// one) for its whole round trip and puts it back only once the reply is
    /// read, so a slow or abandoned request never holds up the others or
    /// leaves its late reply for the next one to read.
    idle: Mutex<Vec<Conn>>,
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
            idle: Mutex::new(Vec::new()),
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
        for attempt in 0..2 {
            // A retry gets a fresh connection: the failed one may have been
            // closed by an app restart.
            let idle = if attempt == 0 {
                self.idle.lock().await.pop()
            } else {
                None
            };
            let mut conn = match idle {
                Some(c) => c,
                None => self.connect().await?,
            };
            let id = self.next_id.fetch_add(1, Ordering::Relaxed);
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
                    let mut idle = self.idle.lock().await;
                    if idle.len() < MAX_IDLE {
                        idle.push(conn);
                    }
                    drop(idle);
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
                Err(_) if attempt == 0 => {}
                Err(e) => return Err(e),
            }
        }
        Err(AutomationError::Bridge("unreachable".into()))
    }

    async fn connect(&self) -> Result<Conn, AutomationError> {
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
        Ok((BufReader::new(r), w))
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

    /// A stand-in for the app that serves any number of connections and answers
    /// each request on its own: `slow` after `slow_reply` (never when `None`),
    /// anything else at once with its command id. The ids of the requests it
    /// receives are sent to the returned channel.
    async fn slow_app(
        slow_reply: Option<Duration>,
    ) -> (String, tokio::sync::mpsc::UnboundedReceiver<String>) {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap().to_string();
        let (seen, rx) = tokio::sync::mpsc::unbounded_channel();
        tokio::spawn(async move {
            while let Ok((s, _)) = listener.accept().await {
                let seen = seen.clone();
                tokio::spawn(async move {
                    let (r, w) = s.into_split();
                    let w = std::sync::Arc::new(AsyncMutex::new(w));
                    let mut lines = BufReader::new(r).lines();
                    while let Ok(Some(line)) = lines.next_line().await {
                        let req: Value = serde_json::from_str(&line).unwrap();
                        let cmd = req["params"]["command"]
                            .as_str()
                            .unwrap_or_default()
                            .to_string();
                        let _ = seen.send(cmd.clone());
                        let delay = if cmd == "slow" {
                            slow_reply
                        } else {
                            Some(Duration::ZERO)
                        };
                        let w = w.clone();
                        tokio::spawn(async move {
                            let Some(d) = delay else { return };
                            tokio::time::sleep(d).await;
                            let line = format!(
                                "{}\n",
                                json!({"id": req["id"], "ok": true, "result": cmd})
                            );
                            let _ = w.lock().await.write_all(line.as_bytes()).await;
                        });
                    }
                });
            }
        });
        (addr, rx)
    }

    /// A command the app is slow to answer holds up only itself: a later call is
    /// answered at once instead of queueing behind it, and behind every call
    /// queued after it.
    #[tokio::test]
    async fn a_slow_command_does_not_block_later_calls() {
        let (addr, mut seen) = slow_app(None).await;
        let b = Arc::new(BridgeClient::new(addr).unwrap());
        let slow = tokio::spawn({
            let b = b.clone();
            async move { b.execute("slow", json!({})).await }
        });
        assert_eq!(seen.recv().await.as_deref(), Some("slow"));
        let fast = tokio::time::timeout(Duration::from_secs(5), b.execute("fast", json!({}))).await;
        assert_eq!(
            fast.expect("the later call waited on the slow one")
                .unwrap(),
            json!("fast")
        );
        slow.abort();
    }

    /// A call given up while it waits leaves no late reply on a connection for
    /// the next call to read as its own.
    #[tokio::test]
    async fn an_abandoned_call_leaves_no_late_reply_behind() {
        let (addr, mut seen) = slow_app(Some(Duration::from_millis(100))).await;
        let b = Arc::new(BridgeClient::new(addr).unwrap());
        let slow = tokio::spawn({
            let b = b.clone();
            async move { b.execute("slow", json!({})).await }
        });
        assert_eq!(seen.recv().await.as_deref(), Some("slow"));
        slow.abort();
        tokio::time::sleep(Duration::from_millis(300)).await;
        assert_eq!(b.execute("fast", json!({})).await.unwrap(), json!("fast"));
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
