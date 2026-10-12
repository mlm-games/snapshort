//! The mixer: track gains and the master gain, part of the saved project so an
//! export and an audio render hear the same mix.

use serde_json::{Value, json};
use std::collections::HashMap;

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("mixer.inspect", "Inspect the Mixer", "{}", run_inspect),
        cmd!(
            "mixer.setTrackGain",
            "Set Track Gain",
            [],
            None::<&'static str>,
            "{\"track\": id, \"gain\"?: number, \"gainDb\"?: number}",
            has_project,
            run_set_track
        ),
        cmd!(
            "mixer.setMasterGain",
            "Set Master Gain",
            [],
            None::<&'static str>,
            "{\"gain\"?: number, \"gainDb\"?: number}",
            has_project,
            run_set_master
        ),
    ]
}

fn run_inspect<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let mixer = s.services().project.mixer().await;
        Ok(json!({
            "masterGain": mixer.master_gain,
            "masterGainDb": 20.0 * (mixer.master_gain as f64).log10(),
            "trackGains": mixer.track_gains,
        }))
    })
}

fn gain_of(p: &Value, cmd: &str) -> Result<f32, EngineError> {
    if let Some(db) = params::f64_p(p, "gainDb") {
        return Ok((10f64.powf(db / 20.0)) as f32);
    }
    params::f64_p(p, "gain")
        .map(|g| g as f32)
        .ok_or_else(|| EngineError::bad(cmd, "give `gain` or `gainDb`"))
}

fn run_set_track<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let track = params::require_str(p, "track", "mixer.setTrackGain")?.to_string();
        uuid::Uuid::parse_str(&track).map_err(|_| {
            EngineError::bad("mixer.setTrackGain", format!("no such track `{track}`"))
        })?;
        let gain = gain_of(p, "mixer.setTrackGain")?;
        s.services().project.set_gain(Some(track), gain).await;
        run_inspect(s, p).await
    })
}

fn run_set_master<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let gain = gain_of(p, "mixer.setMasterGain")?;
        s.services().project.set_gain(None, gain).await;
        run_inspect(s, p).await
    })
}

/// The mix the engine commands use: the saved gains, with any per-command
/// override from the params layered on top.
pub(crate) fn mix_of(
    p: &Value,
    mixer: &snapshort_usecases::MixerState,
) -> (HashMap<miniter_domain::TrackId, f32>, f32) {
    let mut volumes: HashMap<miniter_domain::TrackId, f32> = HashMap::new();
    for (key, gain) in &mixer.track_gains {
        if let Ok(id) = uuid::Uuid::parse_str(key) {
            volumes.insert(miniter_domain::TrackId(id), *gain);
        }
    }
    if let Some(map) = p.get("trackVolumes").and_then(Value::as_object) {
        for (key, value) in map {
            if let (Ok(id), Some(v)) = (uuid::Uuid::parse_str(key), value.as_f64()) {
                volumes.insert(miniter_domain::TrackId(id), v as f32);
            }
        }
    }
    let master = params::f64_p(p, "masterVolume")
        .map(|v| v as f32)
        .unwrap_or(mixer.master_gain);
    (volumes, master)
}
