//! The MCP server. Tool names are snake_case; every tool runs a command, so
//! the tool surface and the command surface never drift apart.

use std::sync::{Arc, Mutex, PoisonError};

use base64::Engine as _;
use rmcp::handler::server::router::tool::ToolRouter;
use rmcp::handler::server::wrapper::Parameters;
use rmcp::model::{CallToolResult, ContentBlock as Content};
use rmcp::service::RequestContext;
use rmcp::{
    ErrorData as McpError, RoleServer, ServerHandler, ServiceExt, tool, tool_handler, tool_router,
};
use schemars::JsonSchema;
use serde::Deserialize;
use serde_json::{Value, json};

use snapshort_engine::{EngineError, Session};

use crate::{AutomationError, BridgeClient};

pub enum Backend {
    Headless(Arc<Mutex<Session>>),
    Bridge(Arc<BridgeClient>),
}

fn panic_message(p: Box<dyn std::any::Any + Send>) -> String {
    p.downcast_ref::<&str>()
        .map(|s| s.to_string())
        .or_else(|| p.downcast_ref::<String>().cloned())
        .unwrap_or_else(|| "panic".into())
}

fn join_error(e: tokio::task::JoinError) -> AutomationError {
    match e.try_into_panic() {
        Ok(p) => AutomationError::Other(format!("internal error: {}", panic_message(p))),
        Err(e) => AutomationError::Other(e.to_string()),
    }
}

#[derive(Clone)]
pub struct SnapshortMcp {
    backend: Arc<Backend>,
    #[allow(dead_code)]
    tool_router: ToolRouter<Self>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ListParams {
    /// Only commands whose id or label contains this text (case-insensitive).
    #[serde(default)]
    pub filter: Option<String>,
    /// Only commands that can run right now.
    #[serde(default)]
    pub enabled_only: Option<bool>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct RunParams {
    /// Command id, e.g. `timeline.addAsset`, `effects.apply`, `asset.import`.
    pub id: String,
    /// Parameters object (see `params` in `command_list`).
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchStep {
    pub id: String,
    #[serde(default)]
    pub params: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct BatchParams {
    /// Commands to run in order.
    pub steps: Vec<BatchStep>,
    /// Stop at the first failing step (default true).
    #[serde(default)]
    pub stop_on_error: Option<bool>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct FrameParams {
    /// Timeline time in seconds (default: the playhead).
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Longest side of the PNG (default 960).
    #[serde(default)]
    pub max_side: Option<u32>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TimeParams {
    #[serde(default)]
    pub seconds: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PathParams {
    /// Absolute path of the file to write.
    pub path: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct CaptionParams {
    /// Absolute path of the SRT file.
    pub path: String,
    /// Text track to place the cues on (default: the first unlocked one).
    #[serde(default)]
    pub track: Option<String>,
    /// Offset the cues by this many seconds (default: 0).
    #[serde(default)]
    pub start: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct MarkerParams {
    /// Time in seconds (default: the playhead).
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Time in microseconds; wins over `seconds`.
    #[serde(default)]
    pub us: Option<i64>,
    /// Marker label.
    #[serde(default)]
    pub label: Option<String>,
    /// ARGB hex colour, e.g. `FFFFC428`.
    #[serde(default)]
    pub color: Option<String>,
    /// How long the marker covers, in seconds (default: a point marker).
    #[serde(default)]
    pub duration: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct MarkerRemoveParams {
    /// Marker index from `marker_list`.
    #[serde(default)]
    pub index: Option<usize>,
    /// Remove the marker at this time instead (seconds).
    #[serde(default)]
    pub at: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct PlaceParams {
    /// Clip id to insert, or an asset id to place.
    #[serde(default)]
    pub clip: Option<String>,
    #[serde(default)]
    pub asset: Option<String>,
    /// Track to place on (default: the preferred track for the material).
    #[serde(default)]
    pub track: Option<String>,
    /// Time in seconds (default: the playhead).
    #[serde(default)]
    pub at: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SecondsParams {
    /// Time in seconds (default: the playhead).
    #[serde(default)]
    pub seconds: Option<f64>,
    /// Time in microseconds; wins over `seconds`.
    #[serde(default)]
    pub us: Option<i64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct WaveformParams {
    #[serde(default)]
    pub seconds: Option<f64>,
    #[serde(default)]
    pub us: Option<i64>,
    /// Width in cells (default 72).
    #[serde(default)]
    pub width: Option<u32>,
    /// Height in cells (default 24).
    #[serde(default)]
    pub height: Option<u32>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct VersionParams {
    /// Version index from `recovery_versions` (default: the newest).
    #[serde(default)]
    pub index: Option<usize>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct MixerGainParams {
    /// Track id.
    pub track: String,
    /// Linear gain (default 1.0).
    #[serde(default)]
    pub gain: Option<f64>,
    /// Gain in dB; wins over `gain`.
    #[serde(default)]
    pub gain_db: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct MixerMasterParams {
    /// Linear gain (default 1.0).
    #[serde(default)]
    pub gain: Option<f64>,
    /// Gain in dB; wins over `gain`.
    #[serde(default)]
    pub gain_db: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct NarrationParams {
    /// The script to speak. Supports `[pause 1s]` / `[pause 500ms]` markers.
    pub text: String,
    /// Voice id from `tts_voices` (default `basic-female`).
    #[serde(default)]
    pub voice: Option<String>,
    /// Speaking rate, 0.5-2.0 (default 1.0).
    #[serde(default)]
    pub pace: Option<f64>,
    /// Pitch shift in semitones (default 0.0).
    #[serde(default)]
    pub semitones: Option<f64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct SpeechRunParams {
    /// Frame RMS threshold in dB (default -45).
    #[serde(default)]
    pub threshold_db: Option<f64>,
    /// Shortest run of speech, in milliseconds (default 150).
    #[serde(default)]
    pub min_speech_ms: Option<f64>,
    /// Silence at least this long separates two runs, in milliseconds (default 300).
    #[serde(default)]
    pub min_silence_ms: Option<f64>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TranscriptImportParams {
    /// Path to a JSON file: an array of `{"text", "start", "end"}`, times in seconds.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct PauseParams {
    /// Only gaps of at least this many microseconds count (default 300000).
    #[serde(default)]
    pub min_us: Option<i64>,
    /// Silence kept on both sides of each cut, in microseconds (default 120000).
    #[serde(default)]
    pub keep_us: Option<i64>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct FillerParams {
    /// Filler words or phrases to cut (default: um, uh, umm, erm, er, ah, hmm, mm, mhm).
    #[serde(default)]
    pub fillers: Option<Vec<String>>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AudioTrackParams {
    /// Track id; omit for every track.
    #[serde(default)]
    pub track: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AudioEffectParams {
    /// Effect registry id, e.g. `denoise`, `deesser`, `compressor`, `limiter`.
    pub effect: String,
    /// Parameter overrides by id, e.g. `{"reduction": 12.0}`.
    #[serde(default)]
    pub params: Option<std::collections::BTreeMap<String, f32>>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioChainParams {
    /// The whole chain in signal order: `[{"effect": "speech_enhance"}, {"effect": "denoise", "params": {"reduction": 12.0}}]`.
    pub chain: Vec<serde_json::Value>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AudioChainRemoveParams {
    /// Index from `audio_chain`.
    #[serde(default)]
    pub index: Option<usize>,
    /// Remove the first matching effect id instead.
    #[serde(default)]
    pub effect: Option<String>,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct AudioNormalizeParams {
    /// Integrated loudness target in LUFS (default -16).
    #[serde(default)]
    pub target_lufs: Option<f64>,
    /// True-peak ceiling in dBTP (default -1.5).
    #[serde(default)]
    pub true_peak: Option<f64>,
    /// Write a 16-bit WAV here as well.
    #[serde(default)]
    pub path: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct AudioRenderParams {
    /// Destination WAV path.
    pub path: String,
}

#[derive(Debug, Default, Deserialize, JsonSchema)]
pub struct ElementsParams {
    /// Only element ids starting with this prefix (e.g. `timeline.clip.`).
    #[serde(default)]
    pub prefix: Option<String>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct ClickParams {
    /// Element id from `ui_elements` (e.g. `timeline.clip.<id>`, `panel.timeline`).
    #[serde(default)]
    pub id: Option<String>,
    /// Or coordinates in points.
    #[serde(default)]
    pub x: Option<f64>,
    #[serde(default)]
    pub y: Option<f64>,
    /// left | right | middle
    #[serde(default)]
    pub button: Option<String>,
    /// 2 for a double-click.
    #[serde(default)]
    pub count: Option<u32>,
    /// {"shift":bool,"alt":bool,"command":bool,"ctrl":bool}
    #[serde(default)]
    pub modifiers: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct DragParams {
    /// Start: {"id":..} or {"x":..,"y":..}.
    pub from: Value,
    /// End point, same forms.
    pub to: Value,
    #[serde(default)]
    pub steps: Option<u32>,
    #[serde(default)]
    pub modifiers: Option<Value>,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct KeyParams {
    /// Key with optional modifiers, e.g. `Space`, `Cmd+K`, `Shift+Delete`, `L`.
    pub key: String,
}

#[derive(Debug, Deserialize, JsonSchema)]
pub struct TypeParams {
    pub text: String,
}

fn place_params(p: PlaceParams) -> Value {
    let mut params = json!({});
    if let Some(clip) = p.clip {
        params["clip"] = json!(clip);
    }
    if let Some(asset) = p.asset {
        params["asset"] = json!(asset);
    }
    if let Some(track) = p.track {
        params["track"] = json!(track);
    }
    if let Some(at) = p.at {
        params["at"] = json!({"seconds": at});
    }
    params
}

fn seconds_params(p: SecondsParams) -> Value {
    let mut params = json!({});
    if let Some(us) = p.us {
        params["us"] = json!(us);
    } else if let Some(seconds) = p.seconds {
        params["seconds"] = json!(seconds);
    }
    params
}

fn ok_json(v: &Value) -> CallToolResult {
    CallToolResult::success(vec![Content::text(
        serde_json::to_string_pretty(v).unwrap_or_default(),
    )])
}

fn fail(e: impl std::fmt::Display) -> CallToolResult {
    CallToolResult::error(vec![Content::text(e.to_string())])
}

fn png_result(png: &[u8], note: &str) -> CallToolResult {
    CallToolResult::success(vec![
        Content::image(
            base64::engine::general_purpose::STANDARD.encode(png),
            "image/png",
        ),
        Content::text(note.to_string()),
    ])
}

fn wrap(r: Result<Value, AutomationError>) -> Result<CallToolResult, McpError> {
    Ok(match r {
        Ok(v) => ok_json(&v),
        Err(e) => fail(e),
    })
}

impl SnapshortMcp {
    pub fn headless(session: Session) -> Self {
        Self {
            backend: Arc::new(Backend::Headless(Arc::new(Mutex::new(session)))),
            tool_router: Self::tool_router(),
        }
    }

    pub fn bridge(addr: &str) -> Result<Self, AutomationError> {
        Ok(Self {
            backend: Arc::new(Backend::Bridge(Arc::new(BridgeClient::new(addr)?))),
            tool_router: Self::tool_router(),
        })
    }

    pub async fn serve_stdio(self) -> Result<(), AutomationError> {
        self.serve_io(tokio::io::stdin(), tokio::io::stdout()).await
    }

    /// Serve MCP over any line stream: a line that isn't JSON gets a JSON-RPC
    /// parse error instead of being dropped, and one writer task owns the
    /// output so replies never interleave.
    pub async fn serve_io(
        self,
        input: impl tokio::io::AsyncRead + Unpin + Send + 'static,
        output: impl tokio::io::AsyncWrite + Unpin + Send + 'static,
    ) -> Result<(), AutomationError> {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        let (mut to_service, service_in) = tokio::io::duplex(1 << 20);
        let (service_out, from_service) = tokio::io::duplex(1 << 20);
        let (err_tx, mut err_rx) = tokio::sync::mpsc::unbounded_channel::<String>();
        let reader = tokio::spawn(async move {
            let mut lines = BufReader::new(input).lines();
            while let Ok(Some(line)) = lines.next_line().await {
                if line.trim().is_empty() {
                    continue;
                }
                match serde_json::from_str::<Value>(&line) {
                    Ok(_) => {
                        if to_service
                            .write_all(format!("{line}\n").as_bytes())
                            .await
                            .is_err()
                        {
                            break;
                        }
                    }
                    Err(e) => {
                        let reply = json!({"jsonrpc": "2.0", "id": Value::Null, "error": {"code": -32700, "message": format!("parse error: {e}")}});
                        let _ = err_tx.send(reply.to_string());
                    }
                }
            }
            let _ = to_service.shutdown().await;
        });
        let writer = tokio::spawn(async move {
            let mut output = output;
            let mut replies = BufReader::new(from_service).lines();
            loop {
                let line = tokio::select! {
                    l = replies.next_line() => match l { Ok(Some(l)) => l, _ => break },
                    Some(e) = err_rx.recv() => e,
                };
                if output
                    .write_all(format!("{line}\n").as_bytes())
                    .await
                    .is_err()
                    || output.flush().await.is_err()
                {
                    break;
                }
            }
        });
        let running = self
            .serve((service_in, service_out))
            .await
            .map_err(|e| AutomationError::Other(format!("MCP init: {e}")))?;
        let r = running
            .waiting()
            .await
            .map_err(|e| AutomationError::Other(e.to_string()));
        reader.abort();
        let _ = writer.await;
        r.map(|_| ())
    }

    /// Run an engine command on whichever backend.
    pub async fn run(&self, id: &str, params: Value) -> Result<Value, AutomationError> {
        match &*self.backend {
            Backend::Headless(s) => {
                let s = s.clone();
                let id = id.to_string();
                tokio::task::spawn_blocking(move || {
                    // A command that panicked earlier poisoned the lock; the session stays usable.
                    let mut g = s.lock().unwrap_or_else(PoisonError::into_inner);
                    let session = &mut *g;
                    let handle = tokio::runtime::Handle::current();
                    tokio::task::block_in_place(|| handle.block_on(session.execute(&id, params)))
                        .map_err(AutomationError::from)
                })
                .await
                .map_err(join_error)?
            }
            Backend::Bridge(b) => b.execute(id, params).await,
        }
    }

    fn bridge_client(&self) -> Option<Arc<BridgeClient>> {
        match &*self.backend {
            Backend::Bridge(b) => Some(b.clone()),
            Backend::Headless(_) => None,
        }
    }

    /// The in-process session, when this server runs headless. The bridge
    /// backend has no session to ask about command enablement.
    fn headless_session(&self) -> Option<Arc<Mutex<Session>>> {
        match &*self.backend {
            Backend::Headless(s) => Some(s.clone()),
            Backend::Bridge(_) => None,
        }
    }

    /// One registry entry from static data, for the bridge case where no
    /// session can be asked.
    fn static_command_entry(spec: &'static snapshort_engine::CommandSpec) -> Value {
        json!({
            "id": spec.id,
            "label": spec.label,
            "menu": spec.menu,
            "shortcut": spec.shortcut,
            "params": spec.params,
            "enabled": true,
            "disabledReason": null,
        })
    }

    /// The registry with live enablement, filtered; blocking (it takes the
    /// session lock), so run it from `spawn_blocking`.
    fn command_entries_blocking(&self, filter: &str, only: bool) -> Vec<Value> {
        let session = self.headless_session();
        let session = session
            .as_ref()
            .map(|session| session.lock().unwrap_or_else(PoisonError::into_inner));
        let mut items = Vec::new();
        for spec in snapshort_engine::command_specs() {
            let id = spec.id.to_ascii_lowercase();
            let label = spec.label.to_ascii_lowercase();
            if !filter.is_empty() && !id.contains(filter) && !label.contains(filter) {
                continue;
            }
            let entry = match session.as_deref() {
                Some(session) => snapshort_engine::commands::spec_json(spec, session),
                None => Self::static_command_entry(spec),
            };
            if only && entry["enabled"].as_bool() == Some(false) {
                continue;
            }
            items.push(entry);
        }
        items
    }

    /// One registry entry with live enablement; blocking, so run it from
    /// `spawn_blocking`.
    fn command_entry_blocking(&self, spec: &'static snapshort_engine::CommandSpec) -> Value {
        match self.headless_session() {
            Some(session) => {
                let session = session.lock().unwrap_or_else(PoisonError::into_inner);
                snapshort_engine::commands::spec_json(spec, &session)
            }
            None => Self::static_command_entry(spec),
        }
    }

    /// The error a live-app tool returns when the server runs headless.
    fn not_live(&self) -> CallToolResult {
        fail("this tool drives the live app: run `snapshort-cli mcp --bridge 127.0.0.1:<port>` with the app started as `snapshort --control <port>`")
    }

    async fn ui(&self, method: &str, params: Value) -> Result<CallToolResult, McpError> {
        match self.bridge_client() {
            Some(b) => wrap(b.call(method, params).await),
            None => Ok(self.not_live()),
        }
    }

    /// Decode an app screenshot into an MCP image block; fall back to the raw
    /// reply when it carries no `png`.
    fn screenshot_result(shot: Result<Value, AutomationError>, note: &str) -> CallToolResult {
        match shot {
            Ok(shot) => match shot.get("png").and_then(Value::as_str) {
                Some(png) => {
                    let bytes = base64::engine::general_purpose::STANDARD
                        .decode(png)
                        .unwrap_or_default();
                    png_result(&bytes, note)
                }
                None => ok_json(&shot),
            },
            Err(e) => fail(e),
        }
    }
}

#[tool_router]
impl SnapshortMcp {
    #[tool(
        title = "List commands",
        description = "List every command (id, label, menu path, shortcut, parameter doc, enabled now). Menus, shortcuts, panels and timeline gestures all map to these ids.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn command_list(
        &self,
        Parameters(p): Parameters<ListParams>,
    ) -> Result<CallToolResult, McpError> {
        let filter = p.filter.unwrap_or_default().to_ascii_lowercase();
        let only = p.enabled_only.unwrap_or(false);
        let this = self.clone();
        let items = match tokio::task::spawn_blocking(move || {
            this.command_entries_blocking(&filter, only)
        })
        .await
        {
            Ok(items) => items,
            Err(e) => return Ok(fail(format!("internal error: {e}"))),
        };
        wrap(Ok(json!({ "commands": items, "count": items.len() })))
    }

    #[tool(
        title = "Describe a command",
        description = "One command in full: id, label, menu path, shortcut, the `params` doc, and whether it can run right now.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn command_describe(
        &self,
        Parameters(p): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        let Some(spec) = snapshort_engine::find_command(&p.id) else {
            return Ok(fail(format!("unknown command `{}`", p.id)));
        };
        let this = self.clone();
        let entry = match tokio::task::spawn_blocking(move || this.command_entry_blocking(spec)).await
        {
            Ok(entry) => entry,
            Err(e) => return Ok(fail(format!("internal error: {e}"))),
        };
        wrap(Ok(entry))
    }

    #[tool(
        title = "The mixer",
        description = "The master mix: the master gain and each track's gain, in dB and linear. These are saved with the project, so an export and an audio render hear the same mix.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn mixer_inspect(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("mixer.inspect", json!({})).await)
    }

    #[tool(
        title = "Set a track gain",
        description = "Set one track's gain, either linear or in dB.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn mixer_set_track_gain(
        &self,
        Parameters(p): Parameters<MixerGainParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({"track": p.track});
        if let Some(gain) = p.gain {
            params["gain"] = json!(gain);
        }
        if let Some(db) = p.gain_db {
            params["gainDb"] = json!(db);
        }
        wrap(self.run("mixer.setTrackGain", params).await)
    }

    #[tool(
        title = "Set the master gain",
        description = "Set the master gain, either linear or in dB. The audio commands and the export read it unless they are given an explicit override.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn mixer_set_master_gain(
        &self,
        Parameters(p): Parameters<MixerMasterParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(gain) = p.gain {
            params["gain"] = json!(gain);
        }
        if let Some(db) = p.gain_db {
            params["gainDb"] = json!(db);
        }
        wrap(self.run("mixer.setMasterGain", params).await)
    }

    #[tool(
        title = "Join through edits",
        description = "Heal the cuts between adjacent pieces of the same source that continue each other in media time: what a razor leaves behind, and what dead-space removal produces. Chains of cuts rejoin into one clip holding all of their media, as one undo step.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn timeline_join_through_edits(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("timeline.joinThroughEdits", json!({})).await)
    }

    #[tool(
        title = "Voices",
        description = "The narration voices that ship with the app: id, name, language, gender and licence. The formant voices need no download and work everywhere; they sound synthetic.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn tts_voices(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("tts.voices", json!({})).await)
    }

    #[tool(
        title = "Preview a voice",
        description = "Speak `text` with a voice and report the audio length, sample rate and sample count. Nothing is placed on the timeline. Supports `[pause 1s]` markers; `pace` is 0.5-2.0, `semitones` shifts pitch.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn tts_preview(
        &self,
        Parameters(p): Parameters<NarrationParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({"text": p.text});
        if let Some(voice) = p.voice {
            params["voice"] = json!(voice);
        }
        if let Some(pace) = p.pace {
            params["pace"] = json!(pace);
        }
        if let Some(semitones) = p.semitones {
            params["semitones"] = json!(semitones);
        }
        wrap(self.run("tts.preview", params).await)
    }

    #[tool(
        title = "Add narration to the timeline",
        description = "Speak `text` with a voice, save it as a WAV in the project data, import it and place it on the timeline as an audio clip. Returns the audio length, the file path and the timeline report.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn tts_add_to_timeline(
        &self,
        Parameters(p): Parameters<NarrationParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({"text": p.text});
        if let Some(voice) = p.voice {
            params["voice"] = json!(voice);
        }
        if let Some(pace) = p.pace {
            params["pace"] = json!(pace);
        }
        if let Some(semitones) = p.semitones {
            params["semitones"] = json!(semitones);
        }
        wrap(self.run("tts.addToTimeline", params).await)
    }

    #[tool(
        title = "Transcript words",
        description = "The words of the current transcript in sequence time: text, start/end microseconds, speaker and confidence. Empty until `transcript_derive` or `transcript_import` has run.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn transcript_words(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("transcript.words", json!({})).await)
    }

    #[tool(
        title = "Derive speech runs from audio",
        description = "Detect speech in the timeline's mix from its own loudness envelope: runs of frames above a threshold, separated by at least `minSilenceMs` and lasting at least `minSpeechMs`. Each run becomes a word-like span, which is all dead-space removal needs — no recogniser required. Report only; nothing is cut.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn transcript_derive(
        &self,
        Parameters(p): Parameters<SpeechRunParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(db) = p.threshold_db {
            params["thresholdDb"] = json!(db);
        }
        if let Some(ms) = p.min_speech_ms {
            params["minSpeechMs"] = json!(ms);
        }
        if let Some(ms) = p.min_silence_ms {
            params["minSilenceMs"] = json!(ms);
        }
        wrap(self.run("transcript.derive", params).await)
    }

    #[tool(
        title = "Import a transcript",
        description = "Load words from a JSON file: an array of `{\"text\", \"start\", \"end\"}` objects, times in seconds. This is what a recogniser's output (Whisper and friends) feeds in, and it is what makes filler-word removal possible.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn transcript_import(
        &self,
        Parameters(p): Parameters<TranscriptImportParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("transcript.import", json!({"path": p.path})).await)
    }

    #[tool(
        title = "Find dead space",
        description = "The pauses between words worth removing: each keeps `keepUs` of silence on both sides of the cut, and only gaps of at least `minUs` count. Report only; nothing is cut.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn deadspace_find(
        &self,
        Parameters(p): Parameters<PauseParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(us) = p.min_us {
            params["minUs"] = json!(us);
        }
        if let Some(us) = p.keep_us {
            params["keepUs"] = json!(us);
        }
        wrap(self.run("deadspace.find", params).await)
    }

    #[tool(
        title = "Remove dead space",
        description = "Cut the pauses out of the timeline and close the holes, as one undo step. Needs a transcript first. The surviving material stays contiguous, so nothing goes out of sync.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn deadspace_remove(
        &self,
        Parameters(p): Parameters<PauseParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(us) = p.min_us {
            params["minUs"] = json!(us);
        }
        if let Some(us) = p.keep_us {
            params["keepUs"] = json!(us);
        }
        wrap(self.run("deadspace.remove", params).await)
    }

    #[tool(
        title = "Remove filler words",
        description = "Cut the filler words (\"um\", \"uh\", \"er\", or your own phrases such as \"you know\") out of the timeline, never cutting into the neighbouring words' frames. Needs a real transcript with word text.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn fillers_remove(
        &self,
        Parameters(p): Parameters<FillerParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(fillers) = p.fillers {
            params["fillers"] = json!(fillers);
        }
        wrap(self.run("fillers.remove", params).await)
    }

    #[tool(
        title = "Find gaps",
        description = "The empty stretches of a track followed by material, in sequence time.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn gaps_find(
        &self,
        Parameters(p): Parameters<AudioTrackParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(track) = p.track {
            params["track"] = json!(track);
        }
        wrap(self.run("gaps.find", params).await)
    }

    #[tool(
        title = "Find through edits",
        description = "The cuts between two adjacent pieces of the same source that continue each other in media time — what a razor leaves behind, and what dead-space removal produces. `timeline_join_through_edits` heals them.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn through_find(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("through.find", json!({})).await)
    }

    #[tool(
        title = "Audio effects",
        description = "Every audio effect the DSP engine can build: id, name, category and each parameter with its range, default and unit. Use the id with audio_chain_add.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_effects(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.effects", json!({})).await)
    }

    #[tool(
        title = "One audio effect",
        description = "The parameters of one audio effect by registry id (for example `denoise`, `deesser`, `speech_enhance`, `compressor`, `limiter`).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_effect(
        &self,
        Parameters(p): Parameters<AudioEffectParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.effect", json!({"effect": p.effect})).await)
    }

    #[tool(
        title = "The audio chain",
        description = "The master audio chain, in order: each effect with its parameter overrides resolved against the effect's defaults. A broken chain is reported, not hidden.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_chain(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.chain", json!({})).await)
    }

    #[tool(
        title = "Replace the audio chain",
        description = "Replace the master audio chain. Order is signal order, e.g. [speech_enhance, denoise, deesser, compressor]. Takes the whole chain at once.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_chain_set(
        &self,
        Parameters(p): Parameters<AudioChainParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.chainSet", json!({"chain": p.chain})).await)
    }

    #[tool(
        title = "Add to the audio chain",
        description = "Append one effect to the end of the master audio chain, optionally overriding parameters by id.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn audio_chain_add(
        &self,
        Parameters(p): Parameters<AudioEffectParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({"effect": p.effect});
        if let Some(values) = p.params {
            params["params"] = json!(values);
        }
        wrap(self.run("audio.chainAdd", params).await)
    }

    #[tool(
        title = "Remove from the audio chain",
        description = "Remove an effect from the master audio chain by its index, or by effect id (removes the first match).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_chain_remove(
        &self,
        Parameters(p): Parameters<AudioChainRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(index) = p.index {
            params["index"] = json!(index);
        }
        if let Some(effect) = p.effect {
            params["effect"] = json!(effect);
        }
        wrap(self.run("audio.chainRemove", params).await)
    }

    #[tool(
        title = "Clear the audio chain",
        description = "Remove every effect from the master audio chain.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_chain_clear(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.chainClear", json!({})).await)
    }

    #[tool(
        title = "Measure audio",
        description = "BS.1770 / EBU R128 measurements of the timeline's mix with the audio chain applied, and of the raw mix alongside: integrated loudness, loudness range, true peak and sample peak. No rendering.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_measure(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.measure", json!({})).await)
    }

    #[tool(
        title = "Normalize audio",
        description = "Two-pass loudness normalization of the timeline's mix: measure, derive the gain from the R128 offset, then limit to a true-peak ceiling. Reports the measured before and after. With `path`, also writes a 16-bit WAV. A lossy delivery codec adds about 1.5 dB of peak overshoot on top of the pre-encode ceiling, so leave headroom: about -2.5 dBTP for AAC, -1.5 for linear PCM.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_normalize(
        &self,
        Parameters(p): Parameters<AudioNormalizeParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({
            "targetLufs": p.target_lufs.unwrap_or(-16.0),
            "truePeak": p.true_peak.unwrap_or(-1.5),
        });
        if let Some(path) = p.path {
            params["path"] = json!(path);
        }
        wrap(self.run("audio.normalize", params).await)
    }

    #[tool(
        title = "Render audio",
        description = "Mix the timeline with the audio chain applied and write a 16-bit WAV to `path`. Reports the path, size, duration and the measurements of what was written.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn audio_render(
        &self,
        Parameters(p): Parameters<AudioRenderParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("audio.render", json!({"path": p.path})).await)
    }

    #[tool(
        title = "Run a command",
        description = "Run a command by id with JSON params, e.g. {\"id\":\"timeline.addAsset\",\"params\":{\"asset\":\"…\"}} or {\"id\":\"timeline.split\",\"params\":{\"at\":{\"seconds\":3.5}}}. Undoable edits land in History (one undo step each).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn command_run(
        &self,
        Parameters(p): Parameters<RunParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let params = p.params.unwrap_or_else(|| json!({}));
        if crate::long_job::is_long(&p.id, &params) {
            return Ok(self.run_long(&p.id, params, &context).await);
        }
        wrap(self.run(&p.id, params).await)
    }

    #[tool(
        title = "Run several commands",
        description = "Run several commands in order: {steps: [{id, params?}], stop_on_error?: true}. Returns {completed, failed, results: [{ok, result | error}]}; each edit is its own undo step. Use `timeline.batch` when the steps belong to a single undo step.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn command_batch(
        &self,
        Parameters(p): Parameters<BatchParams>,
    ) -> Result<CallToolResult, McpError> {
        let stop = p.stop_on_error.unwrap_or(true);
        let (mut completed, mut failed, mut results) = (0u32, 0u32, Vec::new());
        for step in p.steps {
            match self
                .run(&step.id, step.params.unwrap_or_else(|| json!({})))
                .await
            {
                Ok(v) => {
                    completed += 1;
                    results.push(json!({"ok": true, "result": v}));
                }
                Err(e) => {
                    failed += 1;
                    results.push(json!({"ok": false, "error": e.to_string()}));
                    if stop {
                        break;
                    }
                }
            }
        }
        let v = json!({"completed": completed, "failed": failed, "results": results});
        Ok(if failed > 0 {
            CallToolResult::error(vec![Content::text(v.to_string())])
        } else {
            ok_json(&v)
        })
    }

    #[tool(
        title = "Inspect the document",
        description = "Everything the editor holds, as JSON: project, timeline, assets, selection, playback, history, jobs and settings.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn doc_inspect(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("app.inspect", json!({})).await)
    }

    #[tool(
        title = "Inspect the project",
        description = "The open project: name, path, tracks, export profile.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn project_inspect(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("project.inspect", json!({})).await)
    }

    #[tool(
        title = "Inspect the timeline",
        description = "The timeline as JSON: tracks (id, name, kind, muted, locked) and clips (start/duration in microseconds and seconds, source range, speed, volume, opacity, effects, transitions, keyframes, masks).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn timeline_inspect(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("timeline.inspect", json!({})).await)
    }

    #[tool(
        title = "List assets",
        description = "Imported media: id, name, path, type, status (offline/error), proxy, and probed media info (codec, size, fps, duration).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn asset_list(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("asset.list", json!({})).await)
    }

    #[tool(
        title = "Import media",
        description = "Import media files by absolute path (MP4/MOV/MKV/WebM, WAV/MP3/FLAC/Ogg, PNG/JPEG/…). Analysis and proxy generation follow the asset policy.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn media_import(
        &self,
        Parameters(p): Parameters<TypeParams>,
    ) -> Result<CallToolResult, McpError> {
        let paths: Vec<&str> = p
            .text
            .split('\n')
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .collect();
        wrap(self.run("asset.import", json!({"paths": paths})).await)
    }

    #[tool(
        title = "Probe media",
        description = "Probe media files without importing them: container, duration, file size, video streams (codec, size, fps, colour) and audio streams.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn media_probe(
        &self,
        Parameters(p): Parameters<TypeParams>,
    ) -> Result<CallToolResult, McpError> {
        let engine = std::sync::Arc::new(snapshort_infra_media::MediaEngine);
        let mut out = Vec::new();
        for path in p.text.split('\n').map(str::trim).filter(|s| !s.is_empty()) {
            let info = tokio::task::spawn_blocking({
                let engine = engine.clone();
                let path = path.to_string();
                move || engine.probe(std::path::Path::new(&path))
            })
            .await
            .unwrap_or_else(|e| Err(snapshort_infra_media::MediaError::NotFound(e.to_string())));
            match info {
                Ok(info) => out.push(json!({
                    "path": path,
                    "ok": true,
                    "info": snapshort_engine::media_info_json(&info),
                })),
                Err(e) => out.push(json!({"path": path, "ok": false, "error": e.to_string()})),
            }
        }
        wrap(Ok(json!({ "media": out })))
    }

    #[tool(
        title = "Render a frame",
        description = "The timeline's frame at `seconds` (default: the playhead) as PNG, at most `max_side` pixels on its longest side.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn render_frame(
        &self,
        Parameters(p): Parameters<FrameParams>,
    ) -> Result<CallToolResult, McpError> {
        match &*self.backend {
            Backend::Headless(s) => {
                let s = s.clone();
                let seconds = p.seconds;
                let max = p.max_side.unwrap_or(960);
                let result = tokio::task::spawn_blocking(move || {
                    let mut g = s.lock().unwrap_or_else(PoisonError::into_inner);
                    let session = &mut *g;
                    let handle = tokio::runtime::Handle::current();
                    tokio::task::block_in_place(|| {
                        let at = seconds.map(miniter_domain::Timestamp::from_secs);
                        handle.block_on(session.render_frame_png(at, max))
                    })
                })
                .await
                .unwrap_or_else(|e| Err(EngineError::Other(e.to_string())));
                Ok(match result {
                    Ok(png) => png_result(&png, "rendered frame"),
                    Err(e) => fail(e),
                })
            }
            Backend::Bridge(b) => {
                let max_side = p.max_side.unwrap_or(960);
                let shot_params = json!({"maxSide": max_side});
                let shot = match p.seconds {
                    None => b.call("ui.screenshot", shot_params).await,
                    Some(seconds) => {
                        let before = match self.run("app.inspect", json!({})).await {
                            Ok(v) => v,
                            Err(e) => return Ok(fail(e)),
                        };
                        if let Err(e) = self
                            .run("playback.seek", json!({"seconds": seconds}))
                            .await
                        {
                            return Ok(fail(e));
                        }
                        let shot = b.call("ui.screenshot", shot_params).await;
                        if let Some(us) = before["playback"]["playhead"]["us"].as_i64() {
                            let _ = self.run("playback.seek", json!({"us": us})).await;
                        }
                        shot
                    }
                };
                Ok(Self::screenshot_result(shot, "rendered frame"))
            }
        }
    }

    #[tool(
        title = "Export the timeline",
        description = "Export the timeline: {output, format?, quality?, width?, height?, fps?, videoBitrate?, audioBitrate?, hardwareAccel?, masterVolume?, wait?}. With `wait: true` the call blocks until the render finishes and reports progress.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn export(
        &self,
        Parameters(p): Parameters<RunParams>,
        context: RequestContext<RoleServer>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = p.params.unwrap_or_else(|| json!({}));
        if let Some(obj) = params.as_object_mut() {
            obj.entry("wait").or_insert(json!(true));
        }
        if params.get("wait").and_then(Value::as_bool) == Some(false) {
            return wrap(self.run("render.export", params).await);
        }
        Ok(self.run_long("render.export", params, &context).await)
    }

    #[tool(
        title = "Background jobs",
        description = "Analyze and proxy jobs: id, kind, status, progress, error.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn jobs_list(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("jobs.list", json!({})).await)
    }

    #[tool(
        title = "Export FCPXML",
        description = "Write the timeline as Final Cut Pro XML (FCPXML 1.10), readable by Final Cut Pro, Premiere Pro and Resolve.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn export_fcpxml(
        &self,
        Parameters(p): Parameters<PathParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("export.fcpxml", json!({"path": p.path})).await)
    }

    #[tool(
        title = "Export EDL",
        description = "Write the timeline as a CMX3600 EDL, the classic cut list every NLE reads.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn export_edl(
        &self,
        Parameters(p): Parameters<PathParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("export.edl", json!({"path": p.path})).await)
    }

    #[tool(
        title = "Export captions",
        description = "Write the timeline's timed text clips as SubRip (SRT) cues.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn export_captions(
        &self,
        Parameters(p): Parameters<PathParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("export.srt", json!({"path": p.path})).await)
    }

    #[tool(
        title = "Import captions",
        description = "Read a SubRip (SRT) file and add its cues as timed text clips on a text track (optionally on `track`, offset by `start` seconds).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn import_captions(
        &self,
        Parameters(p): Parameters<CaptionParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({"path": p.path});
        if let Some(track) = p.track {
            params["track"] = json!(track);
        }
        if let Some(start) = p.start {
            params["start"] = json!({"seconds": start});
        }
        wrap(self.run("captions.import", params).await)
    }

    #[tool(
        title = "Add a marker",
        description = "Add a timeline marker at a time (default: the playhead), with an optional label, colour (ARGB hex) and duration. Markers are part of the saved project.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn marker_add(
        &self,
        Parameters(p): Parameters<MarkerParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(seconds) = p.seconds {
            params["seconds"] = json!(seconds);
        }
        if let Some(us) = p.us {
            params["us"] = json!(us);
        }
        if let Some(label) = p.label {
            params["label"] = json!(label);
        }
        if let Some(color) = p.color {
            params["color"] = json!(color);
        }
        if let Some(duration) = p.duration {
            params["duration"] = json!({"seconds": duration});
        }
        wrap(self.run("marker.add", params).await)
    }

    #[tool(
        title = "List markers",
        description = "Every timeline marker: index, time in microseconds and seconds, label, colour and duration.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn marker_list(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("marker.list", json!({})).await)
    }

    #[tool(
        title = "Remove a marker",
        description = "Remove a marker by `index`, or the one at a time (`at` seconds).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn marker_remove(
        &self,
        Parameters(p): Parameters<MarkerRemoveParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut params = json!({});
        if let Some(index) = p.index {
            params["index"] = json!(index);
        }
        if let Some(at) = p.at {
            params["at"] = json!({"seconds": at});
        }
        wrap(self.run("marker.remove", params).await)
    }

    #[tool(
        title = "Insert at the playhead",
        description = "Three-point insert: drop the clip or asset at `at` (default: the playhead), splitting what it lands in and shifting the rest right.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn timeline_insert(
        &self,
        Parameters(p): Parameters<PlaceParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("timeline.insert", place_params(p)).await)
    }

    #[tool(
        title = "Overwrite at the playhead",
        description = "Three-point overwrite: drop the clip or asset at `at` (default: the playhead), replacing what is there and leaving the rest in place.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn timeline_overwrite(
        &self,
        Parameters(p): Parameters<PlaceParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("timeline.overwrite", place_params(p)).await)
    }

    #[tool(
        title = "Inspect a frame's levels",
        description = "The composed frame at `seconds` (default: the playhead) as numbers: per-channel min/max, mean luma, and clipped blacks and whites in per mille.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn scopes_levels(
        &self,
        Parameters(p): Parameters<SecondsParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("scopes.levels", seconds_params(p)).await)
    }

    #[tool(
        title = "Inspect a frame's balance",
        description = "Mean red, green and blue of the composed frame, and whether the cast reads warm, cool or neutral.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn scopes_balance(
        &self,
        Parameters(p): Parameters<SecondsParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(self.run("scopes.balance", seconds_params(p)).await)
    }

    #[tool(
        title = "Inspect a frame's waveform",
        description = "The composed frame's luma waveform as text rows, `width` by `height` cells.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn scopes_waveform(
        &self,
        Parameters(p): Parameters<WaveformParams>,
    ) -> Result<CallToolResult, McpError> {
        let seconds = SecondsParams {
            seconds: p.seconds,
            us: p.us,
        };
        let mut params = seconds_params(seconds);
        if let Some(width) = p.width {
            params["width"] = json!(width);
        }
        if let Some(height) = p.height {
            params["height"] = json!(height);
        }
        wrap(self.run("scopes.waveform", params).await)
    }

    #[tool(
        title = "Title templates",
        description = "The named text styles that ship with the app (title, subtitle, lower-third, caption), as full TextStyle objects.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn text_templates(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("text.templates", json!({})).await)
    }

    #[tool(
        title = "Recovery versions",
        description = "The rolling autosave versions, newest first: index, saved-at time, project name and path.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn recovery_versions(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("project.recovery", json!({})).await)
    }

    #[tool(
        title = "Restore an autosave version",
        description = "Restore one of the rolling autosave versions (`index`, default: the newest), replacing the open project.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn restore_version(
        &self,
        Parameters(p): Parameters<VersionParams>,
    ) -> Result<CallToolResult, McpError> {
        wrap(
            self.run("project.restoreVersion", json!({"index": p.index}))
                .await,
        )
    }

    #[tool(
        title = "Live app: UI state",
        description = "Live app: UI state (open panels, playhead, selection, zoom, playback, export progress, status).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ui_inspect(&self) -> Result<CallToolResult, McpError> {
        self.ui("ui.inspect", json!({})).await
    }

    #[tool(
        title = "Live app: on-screen elements",
        description = "Live app: interactive elements with id, label and rect (points). Use the ids with ui_click / ui_drag.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ui_elements(
        &self,
        Parameters(p): Parameters<ElementsParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui(
            "ui.elements",
            json!({"prefix": p.prefix.unwrap_or_default()}),
        )
        .await
    }

    #[tool(
        title = "Live app: click",
        description = "Live app: click an element by id or at x,y (points): a clip selects it, a track/panel/transport activates it, and empty timeline space moves the playhead. `count=2` on a clip also clears the inspector; `button: \"right\"` with x,y opens the app's context menu for the clip or track there.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ui_click(
        &self,
        Parameters(p): Parameters<ClickParams>,
    ) -> Result<CallToolResult, McpError> {
        let mut v = json!({"button": p.button, "count": p.count, "modifiers": p.modifiers});
        if let Some(id) = p.id {
            v["id"] = json!(id);
        } else {
            v["x"] = json!(p.x);
            v["y"] = json!(p.y);
        }
        self.ui("ui.click", v).await
    }

    #[tool(
        title = "Live app: drag",
        description = "Live app: press-drag-release from one point/element to another (move clips, trim edges, scrub).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ui_drag(
        &self,
        Parameters(p): Parameters<DragParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui(
            "ui.drag",
            json!({"from": p.from, "to": p.to, "steps": p.steps, "modifiers": p.modifiers}),
        )
        .await
    }

    #[tool(
        title = "Live app: press a key",
        description = "Live app: press a key or shortcut. Space plays/pauses; Left/Right step; Home/End; Delete/Backspace deletes the selection; S splits; Cmd+Z, Cmd+Shift+Z, Cmd+S, Cmd+D, Cmd+E, Cmd+I; + and - zoom. Other keys answer with an error.",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ui_key(
        &self,
        Parameters(p): Parameters<KeyParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui("ui.key", json!({"key": p.key})).await
    }

    #[tool(
        title = "Live app: type text",
        description = "Live app: type text into the media search field.",
        annotations(
            read_only_hint = false,
            destructive_hint = false,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ui_type(
        &self,
        Parameters(p): Parameters<TypeParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui("ui.type", json!({"text": p.text})).await
    }

    #[tool(
        title = "Live app: invoke a menu item",
        description = "Live app: activate a menu item or command id as if it were clicked (e.g. `timeline.split`, `panel.timeline`).",
        annotations(
            read_only_hint = false,
            destructive_hint = true,
            idempotent_hint = false,
            open_world_hint = false
        )
    )]
    async fn ui_menu_invoke(
        &self,
        Parameters(p): Parameters<RunParams>,
    ) -> Result<CallToolResult, McpError> {
        self.ui("ui.menu", json!({"id": p.id})).await
    }

    #[tool(
        title = "Live app: screenshot",
        description = "Live app: a PNG of the Program monitor at the playhead (the composed frame).",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn ui_screenshot(&self) -> Result<CallToolResult, McpError> {
        let Some(bridge) = self.bridge_client() else {
            return Ok(self.not_live());
        };
        Ok(Self::screenshot_result(
            bridge.call("ui.screenshot", json!({})).await,
            "the Program monitor",
        ))
    }

    #[tool(
        title = "Editor settings",
        description = "Frame rate, monitor quality, proxy policy, hardware acceleration, supported formats and quality presets.",
        annotations(
            read_only_hint = true,
            destructive_hint = false,
            idempotent_hint = true,
            open_world_hint = false
        )
    )]
    async fn settings_inspect(&self) -> Result<CallToolResult, McpError> {
        wrap(self.run("app.settings", json!({})).await)
    }
}

#[tool_handler]
impl ServerHandler for SnapshortMcp {}
