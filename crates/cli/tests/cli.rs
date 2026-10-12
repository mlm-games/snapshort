//! CLI end-to-end: the shell surface over the engine.

use std::io::Write as _;
use std::process::{Command, Stdio};

fn cli() -> Command {
    let mut cmd = Command::new(env!("CARGO_BIN_EXE_snapshort-cli"));
    cmd.current_dir(env!("CARGO_MANIFEST_DIR"));
    cmd
}

fn data_dir(name: &str) -> std::path::PathBuf {
    let dir = std::env::temp_dir().join(format!(
        "snapshort-cli-{}-{}-{name}",
        std::process::id(),
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_nanos()
    ));
    let _ = std::fs::remove_dir_all(&dir);
    dir
}

#[test]
fn exec_runs_a_command_and_prints_json() {
    let dir = data_dir("exec");
    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "exec",
            "project.new",
            "name=CLI",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["name"], "CLI");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn exec_takes_one_json_object_as_params() {
    let dir = data_dir("exec-json");
    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "exec",
            "timeline.addText",
            r#"{"text":"Hi","start":{"seconds":2},"duration":{"seconds":4}}"#,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["clipCount"], 1);
    assert_eq!(value["tracks"][2]["clips"][0]["start"]["seconds"], 2.0);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_failing_command_exits_non_zero() {
    let dir = data_dir("fail");
    let out = cli()
        .args(["--data-dir", dir.to_str().unwrap(), "exec", "nope.nope"])
        .output()
        .unwrap();
    assert!(!out.status.success());
    assert!(
        String::from_utf8_lossy(&out.stderr).contains("unknown command"),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn run_executes_a_jsonl_script() {
    let dir = data_dir("run");
    let script = dir.join("script.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    let mut file = std::fs::File::create(&script).unwrap();
    writeln!(file, "# a comment is skipped").unwrap();
    writeln!(
        file,
        r#"{{"id":"timeline.addText","params":{{"text":"one","start":{{"seconds":0}}}}}}"#
    )
    .unwrap();
    writeln!(
        file,
        r#"{{"id":"timeline.addText","params":{{"text":"two","start":{{"seconds":5}}}}}}"#
    )
    .unwrap();
    drop(file);

    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "run",
            script.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["completed"], 2);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn a_script_stops_at_the_first_failure() {
    let dir = data_dir("run-fail");
    let script = dir.join("script.jsonl");
    std::fs::create_dir_all(&dir).unwrap();
    std::fs::write(
        &script,
        "{\"id\":\"timeline.addText\",\"params\":{\"text\":\"one\"}}\n{\"id\":\"nope.nope\"}\n{\"id\":\"timeline.addText\",\"params\":{\"text\":\"three\"}}\n",
    )
    .unwrap();

    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "run",
            script.to_str().unwrap(),
        ])
        .output()
        .unwrap();
    // The report is printed, and the exit status still says it failed.
    assert!(!out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let results = value["results"].as_array().unwrap();
    assert_eq!(results.len(), 2, "the script stopped at the failure");
    assert_eq!(results[0]["ok"], true);
    assert_eq!(results[1]["ok"], false);
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn commands_lists_the_registry_and_describe_explains_one() {
    let dir = data_dir("commands");
    let out = cli()
        .args(["--data-dir", dir.to_str().unwrap(), "commands", "timeline"])
        .output()
        .unwrap();
    assert!(out.status.success());
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    let ids: Vec<(String, String)> = value["commands"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| {
            (
                c["id"].as_str().unwrap().to_string(),
                c["label"].as_str().unwrap().to_string(),
            )
        })
        .collect();
    assert!(ids.iter().any(|(id, _)| id == "timeline.inspect"));
    assert!(
        ids.iter()
            .all(|(id, label)| id.to_ascii_lowercase().contains("timeline")
                || label.to_lowercase().contains("timeline")),
        "the filter only keeps timeline commands: {ids:?}"
    );

    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "describe",
            "timeline.addText",
        ])
        .output()
        .unwrap();
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["id"], "timeline.addText");
    assert!(value["params"].as_str().unwrap().contains("text"));
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn edits_survive_a_save_and_reopen() {
    let dir = data_dir("persist");
    let project = dir.join("cut.snap");
    std::fs::create_dir_all(&dir).unwrap();

    // Edit, then save to a project file.
    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "--project",
            project.to_str().unwrap(),
            "--save",
            "exec",
            "timeline.addText",
            r#"{"text":"kept","start":{"seconds":1}}"#,
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    assert!(project.exists(), "the project file was written");

    // A fresh session opening that file sees the edit.
    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "--project",
            project.to_str().unwrap(),
            "--compact",
            "inspect",
            "timeline",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let value: serde_json::Value = serde_json::from_slice(&out.stdout).unwrap();
    assert_eq!(value["clipCount"], 1);
    assert_eq!(value["tracks"][2]["clips"][0]["kind"]["text"], "kept");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn render_writes_a_png() {
    let dir = data_dir("render");
    let out_png = dir.join("frame.png");
    std::fs::create_dir_all(&dir).unwrap();
    let out = cli()
        .args([
            "--data-dir",
            dir.to_str().unwrap(),
            "render",
            "--out",
            out_png.to_str().unwrap(),
            "--seconds",
            "1.0",
            "--scale",
            "160",
        ])
        .output()
        .unwrap();
    assert!(
        out.status.success(),
        "{}",
        String::from_utf8_lossy(&out.stderr)
    );
    let bytes = std::fs::read(&out_png).unwrap();
    assert_eq!(&bytes[1..4], b"PNG", "expected a PNG file");
    let _ = std::fs::remove_dir_all(&dir);
}

#[test]
fn help_prints_the_reference() {
    let out = cli().arg("--help").output().unwrap();
    let text = String::from_utf8_lossy(&out.stdout);
    assert!(text.contains("exec <id>"));
    assert!(text.contains("mcp"));
}

#[test]
fn mcp_stdio_is_fed_by_stdin() {
    let dir = data_dir("mcp");
    let mut child = cli()
        .args(["--data-dir", dir.to_str().unwrap(), "mcp"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .unwrap();
    let stdin = child.stdin.as_mut().unwrap();
    writeln!(
        stdin,
        "{}",
        serde_json::json!({
            "jsonrpc": "2.0",
            "id": 1,
            "method": "initialize",
            "params": {
                "protocolVersion": "2025-03-26",
                "capabilities": {},
                "clientInfo": {"name": "cli-test", "version": "1"}
            }
        })
    )
    .unwrap();
    stdin.flush().unwrap();

    let mut line = String::new();
    use std::io::BufRead as _;
    let mut reader = std::io::BufReader::new(child.stdout.as_mut().unwrap());
    reader.read_line(&mut line).unwrap();
    let reply: serde_json::Value = serde_json::from_str(&line).unwrap();
    assert_eq!(reply["id"], 1);
    assert!(reply["result"]["serverInfo"].is_object());

    child.kill().ok();
    let _ = std::fs::remove_dir_all(&dir);
}
