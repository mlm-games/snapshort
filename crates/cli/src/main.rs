//! `snapshort-cli`: drive Snapshort from the shell — a headless session over
//! the engine command surface, or the running app with `--bridge`.
//!
//! `snapshort-cli help` prints the full reference.

#![cfg_attr(
    not(test),
    deny(clippy::unwrap_used, clippy::expect_used, clippy::panic)
)]

use anyhow::{Context, Result, anyhow, bail};
use base64::Engine as _;
use clap::{Parser, Subcommand};
use serde_json::{Value, json};
use snapshort_automation::BridgeClient;
use snapshort_engine::{Session, SessionCommand};
use snapshort_usecases::ProjectCommand;
use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::Arc;

#[derive(Parser)]
#[command(name = "snapshort-cli", version, about = "Snapshort Video Editor CLI")]
struct Cli {
    #[command(subcommand)]
    command: Commands,

    /// Open this project first (headless).
    #[arg(long, global = true)]
    project: Option<PathBuf>,

    /// Data directory for projects, jobs and autosaves.
    #[arg(long, global = true)]
    data_dir: Option<PathBuf>,

    /// Send commands to the running app (`snapshort --control <port>`).
    #[arg(long, global = true)]
    bridge: Option<String>,

    /// Save the project back to `--project` when done.
    #[arg(long, global = true)]
    save: bool,

    /// One-line JSON output instead of pretty-printed.
    #[arg(long, global = true)]
    compact: bool,
}

#[derive(Subcommand)]
enum Commands {
    /// Run one command: `exec <id> [key=value ...]` or `exec <id> '<json>'`.
    Exec {
        /// Command id, e.g. `timeline.addAsset`.
        id: String,
        /// `key=value` pairs (values parse as JSON when they can), or one JSON object.
        args: Vec<String>,
    },
    /// Run commands from a JSONL script, one {"id","params"} per line (`-` for stdin).
    Run { script: String },
    /// List commands (id, label, menu, shortcut, params).
    #[command(name = "commands", alias = "list")]
    List {
        /// Only ids or labels containing this text.
        filter: Option<String>,
        /// Only commands that can run right now.
        #[arg(long)]
        enabled: bool,
    },
    /// Describe one command as JSON.
    Describe { id: String },
    /// Inspect the document: all, project, timeline, assets, selection, playback, history, jobs, settings.
    Inspect {
        #[arg(default_value = "all")]
        what: String,
    },
    /// Import media files.
    Import { files: Vec<PathBuf> },
    /// Probe media files.
    Probe { files: Vec<PathBuf> },
    /// Render one frame to a PNG.
    Render {
        /// Output PNG path.
        #[arg(long)]
        out: PathBuf,
        /// Timeline time in seconds (default: the playhead).
        #[arg(long)]
        seconds: Option<f64>,
        /// Longest side of the PNG in pixels (default 960).
        #[arg(long)]
        scale: Option<u32>,
    },
    /// Export the timeline to a file.
    Export {
        /// Output file path.
        output: PathBuf,
        /// Format: mp4-h264 | mp4-h265 | webm-vp9 | mov-prores | png-sequence | jpeg-sequence.
        #[arg(long)]
        format: Option<String>,
        /// Quality: draft | preview | standard | high | master.
        #[arg(long, default_value = "standard")]
        quality: String,
        /// Wait for the render to finish.
        #[arg(long)]
        wait: bool,
    },
    /// Open a project file.
    Open { path: PathBuf },
    /// Save the project (needs an open file, or give a path for Save As).
    Save { path: Option<PathBuf> },
    /// Start the MCP server on stdio (headless, or `--bridge` to the live app).
    Mcp,
    /// Version and capabilities.
    Info,
}

const HELP: &str = "\
snapshort-cli: drive Snapshort from the shell (headless engine, or the live app with --bridge)

USAGE
  snapshort-cli [OPTIONS] <SUBCOMMAND> [ARGS]

SUBCOMMANDS
  exec <id> [key=value ...]     run one command; prints its result as JSON
  exec <id> '<json params>'     same, params as one JSON object
  run <script.jsonl | ->        run commands, one {\"id\",\"params\"} per line (# comments ok)
  commands [filter] [--enabled]  list commands (id, label, menu, shortcut, params)
  describe <id>                 one command as JSON
  inspect [what]                all | project | timeline | assets | selection | playback | history | jobs | settings
  import <file>...              import media into the project
  probe <media>...              media info as JSON (no import)
  render --out f.png [--seconds S] [--scale 960]
                                render one timeline frame to PNG
  export <out> [--format f] [--quality q] [--wait]
                                export the timeline; --wait blocks until it finishes
                                formats: mp4 mkv mov av1-mp4 av1-mkv av1-webm av1-ivf opus flac
                                (default: from <out>'s extension, else mp4)
  open <project.snap>           open a project file
  save [path]                   save (Save As when a path is given)
  mcp                           MCP server on stdio (headless, or --bridge to the live app)
  info                          version and capabilities

OPTIONS
  --project <p.snap>            open this project first (headless)
  --data-dir <dir>              data directory for projects, jobs and autosaves
  --bridge <127.0.0.1:PORT>     send commands to the running app (`snapshort --control PORT`)
  --save                        save the project back to --project when done
  --compact                     one-line JSON output

VALUES
  key=value parses value as JSON when it can (3.5, true, [1,2], {\"a\":1}), else as a string:
  `exec timeline.razor seconds=3.5`, `exec asset.import paths=[\"/tmp/a.mp4\"]`.
  Times are given as {\"seconds\":…}, {\"us\":…} (microseconds) or {\"frame\":…}.

EXIT STATUS
  0 success - 1 a command failed - 2 usage error
";

fn main() -> Result<()> {
    let cli = Cli::parse();
    if matches!(cli.command, Commands::Mcp) {
        return run_mcp(&cli);
    }
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("async runtime")?;
    runtime.block_on(run(&cli))
}

async fn run(cli: &Cli) -> Result<()> {
    let outcome: Result<Value> = match &cli.command {
        Commands::Info => {
            print_info();
            return Ok(());
        }
        Commands::Probe { files } => probe(files),
        Commands::Mcp => unreachable!("mcp is handled before the runtime starts"),
        // Every other subcommand needs the document, so one session serves
        // them all: the command runs, then `--save` writes the same state.
        other => {
            let (backend, session) = local_session(cli).await?;
            let result = dispatch(other, &backend).await;
            if cli.save
                && let Some(session) = &session
                && let Ok(saved) = save_via(session, cli.project.as_deref()).await
            {
                print_saved(&saved, cli.compact);
            }
            result
        }
    };
    match outcome {
        Ok(value) => {
            print_value(&value, cli.compact);
            // A script that reported failures prints its report, then fails.
            if value.get("failed").and_then(Value::as_u64).unwrap_or(0) > 0 {
                std::process::exit(1);
            }
            Ok(())
        }
        Err(e) => {
            eprintln!("snapshort-cli: {e}");
            std::process::exit(1);
        }
    }
}

/// Where commands run: an in-process session, or the app's control channel.
enum Backend {
    Local(Arc<Session>),
    Bridge(BridgeClient),
}

impl Backend {
    async fn run(&self, id: &str, params: Value) -> Result<Value> {
        match self {
            Backend::Local(session) => Ok(session.execute(id, params).await?),
            Backend::Bridge(bridge) => Ok(bridge.execute(id, params).await?),
        }
    }
}

fn headless(cli: &Cli) -> Result<Session> {
    let data_dir = snapshort_cli::data_dir(cli.data_dir.clone());
    Ok(Session::new(data_dir))
}

async fn local_session(cli: &Cli) -> Result<(Backend, Option<Arc<Session>>)> {
    if let Some(addr) = &cli.bridge {
        return Ok((Backend::Bridge(BridgeClient::new(addr)?), None));
    }
    let session = Arc::new(headless(cli)?);
    let path = cli.project.clone();
    session.start().await?;
    match &path {
        Some(path) if path.exists() => {
            session
                .dispatch(SessionCommand::Project(ProjectCommand::Open {
                    path: path.clone(),
                }))
                .await?;
        }
        Some(_) => {
            // A path that does not exist yet is Save As: start fresh, `--save`
            // writes it at the end.
        }
        None => {
            // A shadow copy means the last session died before saving: restore
            // it rather than booting an empty project on top of that work.
            if session.services().project.check_autosave().await.is_some() {
                session
                    .dispatch(SessionCommand::Project(ProjectCommand::RestoreAutosave))
                    .await?;
            }
        }
    }
    Ok((Backend::Local(session.clone()), Some(session)))
}

/// Every subcommand that needs the document, on one shared backend.
async fn dispatch(cmd: &Commands, backend: &Backend) -> Result<Value> {
    match cmd {
        Commands::Exec { id, args } => {
            let params = params_of(args, "exec")?;
            backend.run(id, params).await
        }
        Commands::Run { script } => run_script(script, backend).await,
        Commands::List { filter, enabled } => commands(filter.as_deref(), *enabled, backend).await,
        Commands::Describe { id } => describe(id, backend),
        Commands::Inspect { what } => inspect(what, backend).await,
        Commands::Import { files } => {
            let paths: Vec<String> = files.iter().map(|p| p.display().to_string()).collect();
            backend.run("asset.import", json!({"paths": paths})).await
        }
        Commands::Render {
            out,
            seconds,
            scale,
        } => render(out, *seconds, *scale, backend).await,
        Commands::Export {
            output,
            format,
            quality,
            wait,
        } => export(output, format.as_deref(), quality, *wait, backend).await,
        Commands::Open { path } => Ok(match backend {
            Backend::Bridge(bridge) => {
                bridge
                    .execute("project.open", json!({"path": path.display().to_string()}))
                    .await?
            }
            Backend::Local(session) => {
                session
                    .dispatch(SessionCommand::Project(ProjectCommand::Open {
                        path: path.to_path_buf(),
                    }))
                    .await?
            }
        }),
        Commands::Save { path } => Ok(match backend {
            Backend::Bridge(bridge) => {
                let params = match path {
                    Some(path) => json!({"path": path.display().to_string()}),
                    None => json!({}),
                };
                let id = if path.is_some() {
                    "project.saveAs"
                } else {
                    "project.save"
                };
                bridge.execute(id, params).await?
            }
            Backend::Local(session) => save_via(session, path.as_deref()).await?,
        }),
        Commands::Info | Commands::Probe { .. } | Commands::Mcp => {
            Err(anyhow!("handled before the session starts"))
        }
    }
}

/// Save the session's project (Save As when a path is given).
async fn save_via(session: &Session, path: Option<&Path>) -> Result<Value> {
    let command = match path {
        Some(path) => ProjectCommand::SaveAs {
            path: path.to_path_buf(),
            markers: Vec::new(),
        },
        None => ProjectCommand::Save {
            markers: Vec::new(),
        },
    };
    Ok(session.dispatch(SessionCommand::Project(command)).await?)
}

fn print_saved(value: &Value, compact: bool) {
    if compact {
        return;
    }
    if let Some(path) = value["path"].as_str() {
        println!("saved: {path}");
    }
}

fn parse_value(raw: &str) -> Value {
    serde_json::from_str(raw).unwrap_or_else(|_| Value::String(raw.to_string()))
}

/// `key=value` pairs (or one JSON object) into a params object.
fn params_of(args: &[String], cmd: &str) -> Result<Value> {
    if args.len() == 1
        && let Ok(value) = serde_json::from_str::<Value>(&args[0])
        && value.is_object()
    {
        return Ok(value);
    }
    let mut map = serde_json::Map::new();
    for arg in args {
        let Some((key, value)) = arg.split_once('=') else {
            bail!("usage: {cmd} <id> [key=value ...] (or one JSON object)");
        };
        map.insert(key.to_string(), parse_value(value));
    }
    Ok(Value::Object(map))
}

async fn run_script(script: &str, backend: &Backend) -> Result<Value> {
    let text = if script == "-" {
        use std::io::Read as _;
        let mut buf = String::new();
        std::io::stdin().read_to_string(&mut buf)?;
        buf
    } else {
        std::fs::read_to_string(script).with_context(|| format!("reading {script}"))?
    };
    let mut results = Vec::new();
    let mut completed = 0u32;
    let mut failed = 0u32;
    for (index, line) in text.lines().enumerate() {
        let line = line.trim();
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let step: Value = serde_json::from_str(line)
            .with_context(|| format!("line {}: not a JSON object", index + 1))?;
        let id = step
            .get("id")
            .and_then(Value::as_str)
            .with_context(|| format!("line {}: missing `id`", index + 1))?
            .to_string();
        let params = step.get("params").cloned().unwrap_or_else(|| json!({}));
        match backend.run(&id, params).await {
            Ok(value) => {
                completed += 1;
                results.push(json!({"ok": true, "id": id, "result": value}));
            }
            Err(e) => {
                failed += 1;
                results.push(json!({"ok": false, "id": id, "error": e.to_string()}));
                break;
            }
        }
    }
    if failed > 0 {
        return Ok(json!({
            "completed": completed,
            "failed": failed,
            "results": results,
        }));
    }
    Ok(json!({"completed": completed, "failed": failed, "results": results}))
}

async fn commands(filter: Option<&str>, enabled_only: bool, backend: &Backend) -> Result<Value> {
    let session = match backend {
        Backend::Local(session) => Some(session),
        Backend::Bridge(_) => None,
    };
    let filter = filter.unwrap_or_default().to_ascii_lowercase();
    let mut items = Vec::new();
    for spec in snapshort_engine::command_specs() {
        let id = spec.id.to_ascii_lowercase();
        let label = spec.label.to_ascii_lowercase();
        if !filter.is_empty() && !id.contains(&filter) && !label.contains(&filter) {
            continue;
        }
        let entry = match session {
            Some(session) => snapshort_engine::commands::spec_json(spec, session),
            None => json!({
                "id": spec.id,
                "label": spec.label,
                "menu": spec.menu,
                "shortcut": spec.shortcut,
                "params": spec.params,
                "enabled": true,
            }),
        };
        if enabled_only && entry["enabled"].as_bool() == Some(false) {
            continue;
        }
        items.push(entry);
    }
    Ok(json!({"commands": items, "count": items.len()}))
}

fn describe(id: &str, backend: &Backend) -> Result<Value> {
    let Some(spec) = snapshort_engine::find_command(id) else {
        bail!("unknown command `{id}`");
    };
    match backend {
        Backend::Local(session) => Ok(snapshort_engine::commands::spec_json(spec, session)),
        Backend::Bridge(_) => Ok(json!({
            "id": spec.id,
            "label": spec.label,
            "menu": spec.menu,
            "shortcut": spec.shortcut,
            "params": spec.params,
            "enabled": true,
        })),
    }
}

async fn inspect(what: &str, backend: &Backend) -> Result<Value> {
    let id = match what {
        "all" | "doc" => "app.inspect",
        "project" => "project.inspect",
        "timeline" => "timeline.inspect",
        "assets" => "asset.list",
        "selection" => "selection.inspect",
        "playback" => "playback.inspect",
        "history" => "app.history",
        "jobs" => "jobs.list",
        "settings" => "app.settings",
        other => bail!("unknown inspect target `{other}`"),
    };
    backend.run(id, json!({})).await
}

fn probe(files: &[PathBuf]) -> Result<Value> {
    let mut out = Vec::new();
    for file in files {
        match snapshort_cli::probe_media(file) {
            Ok(info) => {
                out.push(json!({"path": file.display().to_string(), "ok": true, "info": info}))
            }
            Err(e) => {
                out.push(json!({"path": file.display().to_string(), "ok": false, "error": e}))
            }
        }
    }
    if files.len() == 1 {
        let first = &out[0];
        if first["ok"].as_bool() == Some(true) {
            snapshort_cli::print_probe(&files[0], &first["info"]);
        }
    }
    Ok(json!({"media": out}))
}

async fn render(
    out: &Path,
    seconds: Option<f64>,
    scale: Option<u32>,
    backend: &Backend,
) -> Result<Value> {
    let max_side = scale.unwrap_or(960);
    match backend {
        Backend::Local(session) => {
            let png = session
                .render_frame_png(seconds.map(miniter_domain::Timestamp::from_secs), max_side)
                .await?;
            std::fs::write(out, &png).with_context(|| format!("writing {}", out.display()))?;
            Ok(json!({"path": out.display().to_string(), "bytes": png.len(), "maxSide": max_side}))
        }
        Backend::Bridge(bridge) => {
            let shot_params = json!({"maxSide": max_side});
            let shot = if let Some(seconds) = seconds {
                let before = bridge.execute("app.inspect", json!({})).await?;
                if let Err(e) = bridge
                    .execute("playback.seek", json!({"seconds": seconds}))
                    .await
                {
                    return Err(e.into());
                }
                let shot = bridge.call("ui.screenshot", shot_params).await;
                if let Some(us) = before["playback"]["playhead"]["us"].as_i64() {
                    let _ = bridge.execute("playback.seek", json!({"us": us})).await;
                }
                shot?
            } else {
                bridge.call("ui.screenshot", shot_params).await?
            };
            let png = shot
                .get("png")
                .and_then(Value::as_str)
                .context("the app returned no image")?;
            let bytes = base64::engine::general_purpose::STANDARD
                .decode(png)
                .map_err(|e| anyhow!("bad image: {e}"))?;
            std::fs::write(out, &bytes).with_context(|| format!("writing {}", out.display()))?;
            Ok(json!({"path": out.display().to_string(), "bytes": bytes.len()}))
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn export(
    output: &Path,
    format: Option<&str>,
    quality: &str,
    wait: bool,
    backend: &Backend,
) -> Result<Value> {
    let mut params = json!({
        "output": output.display().to_string(),
        "quality": quality,
        "wait": wait,
    });
    if let Some(format) = format
        && let Some(obj) = params.as_object_mut()
    {
        obj.insert("format".into(), json!(format));
    }
    backend.run("render.export", params).await
}

fn run_mcp(cli: &Cli) -> Result<()> {
    let runtime = tokio::runtime::Builder::new_multi_thread()
        .worker_threads(2)
        .enable_all()
        .build()
        .context("async runtime")?;
    runtime.block_on(async move {
        let outcome: Result<snapshort_automation::SnapshortMcp> = async {
            if let Some(addr) = &cli.bridge {
                return Ok(snapshort_automation::SnapshortMcp::bridge(addr)?);
            }
            let session = headless(cli)?;
            session.start().await?;
            if let Some(path) = &cli.project {
                session
                    .dispatch(SessionCommand::Project(ProjectCommand::Open {
                        path: path.clone(),
                    }))
                    .await?;
            } else if session.services().project.check_autosave().await.is_some() {
                session
                    .dispatch(SessionCommand::Project(ProjectCommand::RestoreAutosave))
                    .await?;
            }
            Ok(snapshort_automation::SnapshortMcp::headless(session))
        }
        .await;
        outcome?.serve_stdio().await.map_err(Into::into)
    })
}

fn print_value(value: &Value, compact: bool) {
    if compact {
        println!("{}", serde_json::to_string(value).unwrap_or_default());
    } else {
        println!(
            "{}",
            serde_json::to_string_pretty(value).unwrap_or_default()
        );
    }
    let _ = std::io::stdout().flush();
}

fn print_info() {
    println!("Snapshort Video Editor CLI");
    println!("Version: {}", env!("CARGO_PKG_VERSION"));
    println!();
    println!("Commands: {}", snapshort_engine::command_specs().len());
    println!("Formats: mp4-h264, mp4-h265, webm-vp9, mov-prores, png-sequence, jpeg-sequence");
    println!("MCP: run `snapshort-cli mcp` (headless) or `--bridge 127.0.0.1:<port>` (live app)");
}

pub fn help_text() -> &'static str {
    HELP
}
