//! Narration: text to speech on a track, and the voice catalogue.
//!
//! The formant voices ship with the app and need no download, so a script becomes
//! audio with nothing to install; they sound synthetic, which is what the voice
//! descriptions say rather than hide.

use serde_json::{Value, json};
use snapshort_tts as tts;

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Result, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("tts.voices", "Voices", "{}", run_voices),
        query!(
            "tts.script",
            "Parse a Narration Script",
            "{\"text\": string, \"pace\"?: number, \"semitones\"?: number}",
            run_script
        ),
        cmd!(
            "tts.preview",
            "Preview a Voice",
            [],
            None::<&'static str>,
            "{\"text\": string, \"voice\"?: string, \"pace\"?: number, \"semitones\"?: number, \"seconds\"?: number}",
            has_project,
            run_preview
        ),
        cmd!(
            "tts.addToTimeline",
            "Add Narration",
            [],
            None::<&'static str>,
            "{\"text\": string, \"voice\"?: string, \"pace\"?: number, \"semitones\"?: number, \"track\"?: id, \"start\"?: time}",
            has_project,
            run_add
        ),
    ]
}

fn voice_of(p: &Value) -> std::result::Result<Box<dyn tts::Voice>, EngineError> {
    let id = params::str_p(p, "voice").unwrap_or("basic-female");
    tts::voice(id).map_err(|why| EngineError::bad("tts", why.to_string()))
}

fn params_of(p: &Value) -> std::result::Result<tts::Params, EngineError> {
    Ok(tts::Params {
        semitones: params::f64_p(p, "semitones").unwrap_or(0.0),
        pace: params::f64_p(p, "pace").unwrap_or(1.0),
    })
}

fn run_voices<'a>(_s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let voices: Vec<Value> = tts::voices()
            .into_iter()
            .map(|v| {
                json!({
                    "id": v.id,
                    "name": v.name,
                    "language": v.language,
                    "gender": format!("{:?}", v.gender),
                    "engine": v.engine,
                    "description": v.description,
                    "license": v.license,
                    "installed": v.installed,
                })
            })
            .collect();
        Ok(json!({"count": voices.len(), "voices": voices}))
    })
}

fn run_script<'a>(_s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let text = params::require_str(p, "text", "tts.script")?;
        let params = params_of(p)?;
        let segments = tts::script::parse(text);
        let voice = voice_of(p)?;
        let audio = voice
            .synthesize(text, &params)
            .map_err(|why| EngineError::bad("tts.script", why.to_string()))?;
        Ok(json!({
            "segments": segments.len(),
            "seconds": audio.seconds(),
            "sampleRate": audio.sample_rate,
        }))
    })
}

fn run_preview<'a>(_s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let text = params::require_str(p, "text", "tts.preview")?;
        let params = params_of(p)?;
        let voice = voice_of(p)?;
        let audio = voice
            .synthesize(text, &params)
            .map_err(|why| EngineError::bad("tts.preview", why.to_string()))?;
        Ok(json!({
            "seconds": audio.seconds(),
            "sampleRate": audio.sample_rate,
            "samples": audio.samples.len(),
        }))
    })
}

fn run_add<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let text = params::require_str(p, "text", "tts.addToTimeline")?;
        let params = params_of(p)?;
        let voice = voice_of(p)?;
        let audio = voice
            .synthesize(text, &params)
            .map_err(|why| EngineError::bad("tts.addToTimeline", why.to_string()))?;

        let dir = s.data_dir().join("narrations");
        std::fs::create_dir_all(&dir)
            .map_err(|why| EngineError::bad("tts.addToTimeline", why.to_string()))?;
        let path = dir.join(format!(
            "narration-{}.wav",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_millis()
        ));
        write_wav(&path, &audio)
            .map_err(|why| EngineError::bad("tts.addToTimeline", why.to_string()))?;

        let imported = s
            .execute(
                "asset.import",
                json!({"paths": [path.display().to_string()]}),
            )
            .await?;
        let asset_id = imported["assets"][0]["id"]
            .as_str()
            .ok_or_else(|| EngineError::bad("tts.addToTimeline", "the narration was not imported"))?
            .to_string();
        let report = s
            .execute(
                "timeline.addAsset",
                json!({
                    "asset": asset_id,
                    "track": params::str_p(p, "track"),
                    "start": params::value_p(p, "start"),
                    "duration": {"us": (audio.seconds() * 1e6) as i64},
                }),
            )
            .await?;
        let mut result = json!({
            "seconds": audio.seconds(),
            "path": path.display().to_string(),
            "sampleRate": audio.sample_rate,
        });
        if let Some(object) = result.as_object_mut() {
            object.insert("timeline".into(), report);
        }
        Ok(result)
    })
}

fn write_wav(path: &std::path::Path, audio: &tts::Audio) -> std::io::Result<()> {
    let mut bytes = Vec::new();
    for sample in &audio.samples {
        let clamped = sample.clamp(-1.0, 1.0);
        let value = (clamped * 32_767.0) as i16;
        bytes.extend_from_slice(&value.to_le_bytes());
    }
    let rate = audio.sample_rate;
    let mut wav = Vec::new();
    wav.extend_from_slice(b"RIFF");
    wav.extend_from_slice(&((36 + bytes.len()) as u32).to_le_bytes());
    wav.extend_from_slice(b"WAVE");
    wav.extend_from_slice(b"fmt ");
    wav.extend_from_slice(&16u32.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&1u16.to_le_bytes());
    wav.extend_from_slice(&rate.to_le_bytes());
    wav.extend_from_slice(&(rate * 2).to_le_bytes());
    wav.extend_from_slice(&2u16.to_le_bytes());
    wav.extend_from_slice(&16u16.to_le_bytes());
    wav.extend_from_slice(b"data");
    wav.extend_from_slice(&(bytes.len() as u32).to_le_bytes());
    wav.extend_from_slice(&bytes);
    std::fs::write(path, wav)
}
