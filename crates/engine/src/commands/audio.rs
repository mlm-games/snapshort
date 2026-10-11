//! The master audio chain: browse effects, build the chain, measure the mix
//! (R128) and render the processed audio to a WAV.
//!
//! The chain is project state: it saves with the project and is applied to the
//! mix exactly as an export's audio is. Normalization leaves the true-peak
//! ceiling alone: a lossy delivery codec (AAC) adds roughly 1.5 dB of overshoot
//! on top of it, so the ceiling is the caller's choice, not a constant.

use serde_json::{Value, json};
use snapshort_usecases::AudioEffectData;

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("audio.effects", "Audio Effects", "{}", run_effects),
        query!(
            "audio.effect",
            "Audio Effect Parameters",
            "{\"effect\": string}",
            run_effect
        ),
        query!("audio.chain", "Audio Chain", "{}", run_chain_view),
        cmd!(
            "audio.chainSet",
            "Set the Audio Chain",
            [],
            None::<&'static str>,
            "{\"chain\": [{effect: string, params?: object}]}",
            has_project,
            run_chain_set
        ),
        cmd!(
            "audio.chainAdd",
            "Add to the Audio Chain",
            [],
            None::<&'static str>,
            "{\"effect\": string, \"params\"?: object}",
            has_project,
            run_chain_add
        ),
        cmd!(
            "audio.chainRemove",
            "Remove from the Audio Chain",
            [],
            None::<&'static str>,
            "{\"index\"?: int, \"effect\"?: string}",
            has_project,
            run_chain_remove
        ),
        query!(
            "audio.chainClear",
            "Clear the Audio Chain",
            "{}",
            run_chain_clear
        ),
        cmd!(
            "audio.normalize",
            "Normalize Audio",
            [],
            None::<&'static str>,
            "{\"targetLufs\"?: number, \"truePeak\"?: number, \"path\"?: string}",
            has_project,
            run_normalize
        ),
        query!("audio.measure", "Measure Audio", "{}", run_measure),
        cmd!(
            "audio.render",
            "Render Audio",
            [],
            None::<&'static str>,
            "{\"path\": string}",
            has_project,
            run_render
        ),
    ]
}

fn run_effects<'a>(_s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        Ok(json!({
            "count": snapshort_infra_audio::AudioDspService::catalog().len(),
            "effects": snapshort_infra_audio::AudioDspService::catalog(),
        }))
    })
}

fn run_effect<'a>(_s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let effect = params::str_p(p, "effect")
            .ok_or_else(|| EngineError::bad("audio.effect", "missing `effect`".to_string()))?;
        snapshort_infra_audio::AudioDspService::effect(effect).map_err(EngineError::from)
    })
}

async fn chain_view(s: &Session) -> Value {
    let data = s.services().project.audio_chain().await;
    snapshort_infra_audio::AudioDspService::chain_report_data(&data)
}

fn run_chain_view<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(chain_view(s).await) })
}

fn run_chain_set<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let data: Vec<AudioEffectData> = params::require_object(p, "chain", "audio.chainSet")?;
        snapshort_infra_audio::AudioDspService::chain(&data)?;
        s.services().project.set_audio_chain(data).await;
        Ok(chain_view(s).await)
    })
}

fn run_chain_add<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let effect = params::require_str(p, "effect", "audio.chainAdd")?.to_string();
        let params_map: std::collections::BTreeMap<String, f32> =
            params::optional_object(p, "params", "audio.chainAdd")?.unwrap_or_default();

        let mut data = s.services().project.audio_chain().await;
        data.push(AudioEffectData {
            effect,
            params: params_map,
        });
        snapshort_infra_audio::AudioDspService::chain(&data)?;
        s.services().project.set_audio_chain(data).await;
        Ok(chain_view(s).await)
    })
}

fn run_chain_remove<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let mut data = s.services().project.audio_chain().await;
        let index = if let Some(index) = params::usize_p(p, "index") {
            index
        } else if let Some(effect) = params::str_p(p, "effect") {
            data.iter()
                .position(|item| item.effect == effect)
                .ok_or_else(|| {
                    EngineError::bad(
                        "audio.chainRemove",
                        format!("no `{effect}` in the audio chain"),
                    )
                })?
        } else {
            data.len().checked_sub(1).ok_or_else(|| {
                EngineError::bad("audio.chainRemove", "the audio chain is empty".to_string())
            })?
        };
        if index >= data.len() {
            return Err(EngineError::bad(
                "audio.chainRemove",
                format!("index {index} is outside the audio chain"),
            ));
        }
        data.remove(index);
        s.services().project.set_audio_chain(data).await;
        Ok(chain_view(s).await)
    })
}

fn run_chain_clear<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.services().project.clear_audio_chain().await;
        Ok(chain_view(s).await)
    })
}

/// The mix volumes, matching `render.export`'s `masterVolume` / `trackVolumes`.
pub(crate) async fn volumes_of(
    s: &Session,
    p: &Value,
) -> (std::collections::HashMap<miniter_domain::TrackId, f32>, f32) {
    super::mixer::mix_of(p, &s.services().project.mixer().await)
}

fn run_normalize<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = params::str_p(p, "path").map(std::path::PathBuf::from);
        let Some(timeline) = s.services().project.current_timeline().await else {
            return Err(EngineError::bad(
                "audio.normalize",
                "there is no project open".to_string(),
            ));
        };
        let data = s.services().project.audio_chain().await;
        let chain = snapshort_infra_audio::AudioDspService::chain(&data)?;
        let (volumes, master) = volumes_of(s, p).await;
        let target = params::f64_p(p, "targetLufs").unwrap_or(-16.0);
        let ceiling = params::f64_p(p, "truePeak").unwrap_or(-1.5);
        let report = match &path {
            Some(path) => s
                .services()
                .audio
                .render_normalized(&timeline, &volumes, master, &chain, target, ceiling, path)
                .map_err(EngineError::from)?,
            None => s
                .services()
                .audio
                .normalize(&timeline, &volumes, master, &chain, target, ceiling)
                .map_err(EngineError::from)?,
        };
        Ok(report)
    })
}

fn run_measure<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let Some(timeline) = s.services().project.current_timeline().await else {
            return Err(EngineError::bad(
                "audio.measure",
                "there is no project open".to_string(),
            ));
        };
        let data = s.services().project.audio_chain().await;
        let chain = snapshort_infra_audio::AudioDspService::chain(&data)?;
        let (volumes, master) = volumes_of(s, p).await;
        s.services()
            .audio
            .measure(&timeline, &volumes, master, &chain)
            .map_err(EngineError::from)
    })
}

fn run_render<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let path = params::require_file(p, "path", "audio.render")?;
        let Some(timeline) = s.services().project.current_timeline().await else {
            return Err(EngineError::bad(
                "audio.render",
                "there is no project open".to_string(),
            ));
        };
        let data = s.services().project.audio_chain().await;
        let chain = snapshort_infra_audio::AudioDspService::chain(&data)?;
        let (volumes, master) = volumes_of(s, p).await;
        s.services()
            .audio
            .render_wav(&timeline, &volumes, master, &chain, &path)
            .map_err(EngineError::from)
    })
}
