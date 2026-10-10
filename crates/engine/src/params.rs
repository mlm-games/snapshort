//! JSON parameter readers shared by every command handler.
//!
//! Ids arrive as UUID strings (what agents and the CLI send), times as
//! microseconds or seconds — never as floats inside edit math.

use miniter_domain::{ClipId, MediaDuration, Timestamp, TrackId};
use serde_json::{Value, json};
use uuid::Uuid;

use crate::EngineError;

pub fn str_p<'a>(p: &'a Value, k: &str) -> Option<&'a str> {
    p.get(k).and_then(Value::as_str)
}

pub fn bool_p(p: &Value, k: &str) -> Option<bool> {
    p.get(k).and_then(Value::as_bool)
}

pub fn f64_p(p: &Value, k: &str) -> Option<f64> {
    p.get(k).and_then(Value::as_f64)
}

pub fn u32_p(p: &Value, k: &str) -> Option<u32> {
    number(p, k).and_then(|n| {
        if n.fract() == 0.0 && (0.0..=f64::from(u32::MAX)).contains(&n) {
            Some(n as u32)
        } else {
            None
        }
    })
}

pub fn usize_p(p: &Value, k: &str) -> Option<usize> {
    number(p, k).and_then(|n| {
        if n.fract() == 0.0 && n >= 0.0 {
            Some(n as usize)
        } else {
            None
        }
    })
}

/// A JSON number, accepting integer-valued floats (`1920.0`).
fn number(p: &Value, k: &str) -> Option<f64> {
    p.get(k)
        .and_then(|v| v.as_f64().or_else(|| v.as_i64().map(|i| i as f64)))
}

pub fn require_str<'a>(p: &'a Value, k: &str, cmd: &str) -> Result<&'a str, EngineError> {
    str_p(p, k).ok_or_else(|| EngineError::bad(cmd, format!("`{k}` is required")))
}

pub fn require_usize(p: &Value, k: &str, cmd: &str) -> Result<usize, EngineError> {
    usize_p(p, k)
        .ok_or_else(|| EngineError::bad(cmd, format!("`{k}` must be a non-negative integer")))
}

pub fn require_f64(p: &Value, k: &str, cmd: &str) -> Result<f64, EngineError> {
    f64_p(p, k).ok_or_else(|| EngineError::bad(cmd, format!("`{k}` is required")))
}

pub fn require_bool(p: &Value, k: &str, cmd: &str) -> Result<bool, EngineError> {
    bool_p(p, k).ok_or_else(|| EngineError::bad(cmd, format!("`{k}` is required")))
}

pub fn uuid_p(p: &Value, k: &str) -> Option<Uuid> {
    str_p(p, k).and_then(|s| Uuid::parse_str(s).ok())
}

pub fn require_uuid(p: &Value, k: &str, cmd: &str) -> Result<Uuid, EngineError> {
    uuid_p(p, k).ok_or_else(|| EngineError::bad(cmd, format!("`{k}` must be a UUID")))
}

pub fn clip_p(p: &Value, k: &str) -> Option<ClipId> {
    uuid_p(p, k).map(ClipId)
}

pub fn require_clip(p: &Value, k: &str, cmd: &str) -> Result<ClipId, EngineError> {
    require_uuid(p, k, cmd).map(ClipId)
}

pub fn track_p(p: &Value, k: &str) -> Option<TrackId> {
    uuid_p(p, k).map(TrackId)
}

pub fn require_track(p: &Value, k: &str, cmd: &str) -> Result<TrackId, EngineError> {
    require_uuid(p, k, cmd).map(TrackId)
}

/// Timeline time from `seconds`, `us`, `ms` or `frame` (with the session's
/// frame rate). Microseconds win: they are the model's own unit.
pub fn time_p(p: &Value, fps: i64) -> Option<Timestamp> {
    if let Some(us) = p.get("us").and_then(Value::as_i64) {
        return Some(Timestamp::from_micros(us));
    }
    if let Some(ms) = p.get("ms").and_then(Value::as_f64) {
        return Some(Timestamp::from_millis(ms));
    }
    if let Some(sec) = p.get("seconds").and_then(Value::as_f64) {
        return Some(Timestamp::from_secs(sec));
    }
    let frame = p.get("frame").and_then(Value::as_i64)?;
    let fps = fps.max(1);
    let mut step = snapshort_usecases::FrameStepper::new(fps);
    let mut total = 0i64;
    for _ in 0..frame.max(0) {
        total += step.next_step_us();
    }
    Some(Timestamp::from_micros(total))
}

/// Time under a named key: `{"start": {"seconds": 1}}`, `{"start": 1.5}`
/// (seconds) or `{"start": 1500000}` (microseconds above a second).
pub fn time_at(p: &Value, key: &str, fps: i64) -> Option<Timestamp> {
    let value = p.get(key)?;
    match value {
        Value::Object(_) => time_p(value, fps),
        Value::Number(n) => {
            let n = n.as_f64().unwrap_or(0.0);
            if n.abs() < 1e6 {
                Some(Timestamp::from_secs(n))
            } else {
                Some(Timestamp::from_micros(n as i64))
            }
        }
        _ => None,
    }
}

pub fn duration_at(p: &Value, key: &str, fps: i64) -> Option<MediaDuration> {
    time_at(p, key, fps).map(|t| MediaDuration::from_micros(t.as_micros()))
}

pub fn require_time(p: &Value, fps: i64, cmd: &str) -> Result<Timestamp, EngineError> {
    time_p(p, fps).ok_or_else(|| EngineError::bad(cmd, "give `seconds`, `us`, `ms` or `frame`"))
}

pub fn duration_p(p: &Value, fps: i64) -> Option<MediaDuration> {
    time_p(p, fps).map(|t| MediaDuration::from_micros(t.as_micros()))
}

pub fn require_duration(p: &Value, fps: i64, cmd: &str) -> Result<MediaDuration, EngineError> {
    require_time(p, fps, cmd).map(|t| MediaDuration::from_micros(t.as_micros()))
}

/// A typed object, e.g. `{"speed":0.5}` for a filter. `null` means "none".
pub fn value_p(p: &Value, k: &str) -> Option<Value> {
    p.get(k).filter(|v| !v.is_null()).cloned()
}

pub fn require_object<T: serde::de::DeserializeOwned>(
    p: &Value,
    k: &str,
    cmd: &str,
) -> Result<T, EngineError> {
    let v = value_p(p, k).ok_or_else(|| EngineError::bad(cmd, format!("`{k}` is required")))?;
    serde_json::from_value(v).map_err(|e| EngineError::bad(cmd, format!("`{k}`: {e}")))
}

pub fn optional_object<T: serde::de::DeserializeOwned>(
    p: &Value,
    k: &str,
    cmd: &str,
) -> Result<Option<T>, EngineError> {
    match value_p(p, k) {
        Some(v) => serde_json::from_value(v)
            .map(Some)
            .map_err(|e| EngineError::bad(cmd, format!("`{k}`: {e}"))),
        None => Ok(None),
    }
}

/// Paths from `paths` (a list) or a single `path`.
pub fn paths_p(p: &Value) -> Vec<std::path::PathBuf> {
    let mut out = Vec::new();
    match p.get("paths") {
        Some(Value::Array(items)) => {
            for item in items {
                if let Some(s) = item.as_str() {
                    out.push(std::path::PathBuf::from(s));
                }
            }
        }
        _ => {
            if let Some(s) = str_p(p, "path") {
                out.push(std::path::PathBuf::from(s));
            }
        }
    }
    out
}

pub fn require_file(p: &Value, key: &str, cmd: &str) -> Result<std::path::PathBuf, EngineError> {
    if let Some(path) = str_p(p, key) {
        return Ok(std::path::PathBuf::from(path));
    }
    let mut paths = paths_p(p);
    if paths.is_empty() {
        return Err(EngineError::bad(cmd, format!("`{key}` is required")));
    }
    Ok(paths.remove(0))
}

pub fn require_paths(p: &Value, cmd: &str) -> Result<Vec<std::path::PathBuf>, EngineError> {
    let paths = paths_p(p);
    if paths.is_empty() {
        return Err(EngineError::bad(cmd, "`paths` is required"));
    }
    Ok(paths)
}

/// Ids from `clips`/`clip`, `tracks`/`track`, `items`/`item` — whichever the
/// command documents. Empty when the caller named none.
pub fn ids_p(p: &Value, many: &str, one: &str) -> Vec<Uuid> {
    let mut out = Vec::new();
    if let Some(Value::Array(items)) = p.get(many) {
        out.extend(
            items
                .iter()
                .filter_map(|v| v.as_str())
                .filter_map(|s| Uuid::parse_str(s).ok()),
        );
    }
    if out.is_empty()
        && let Some(id) = uuid_p(p, one)
    {
        out.push(id);
    }
    out
}

/// `clips`/`clip` as clip ids.
pub fn clip_ids_p(p: &Value) -> Vec<ClipId> {
    ids_p(p, "clips", "clip").into_iter().map(ClipId).collect()
}

pub fn track_ids_p(p: &Value) -> Vec<TrackId> {
    ids_p(p, "tracks", "track")
        .into_iter()
        .map(TrackId)
        .collect()
}

pub fn ok() -> Value {
    json!({ "ok": true })
}
