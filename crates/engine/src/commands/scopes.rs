//! Scope commands: the frame's pixels, as numbers an agent can reason about.

use serde_json::{Value, json};

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Result, Session};

use super::{RunFuture, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!(
            "scopes.histogram",
            "Histogram",
            "{\"seconds\"?: number, \"us\"?: int, \"channels\"?: \"rgb\"|\"luma\"}",
            run_histogram
        ),
        query!(
            "scopes.levels",
            "Levels",
            "{\"seconds\"?: number, \"us\"?: int}",
            run_levels
        ),
        query!(
            "scopes.balance",
            "Colour Balance",
            "{\"seconds\"?: number, \"us\"?: int}",
            run_balance
        ),
        query!(
            "scopes.waveform",
            "Waveform",
            "{\"seconds\"?: number, \"us\"?: int, \"width\"?: int, \"height\"?: int}",
            run_waveform
        ),
    ]
}

/// The composed frame at a time, as RGBA8, downscaled for analysis.
async fn frame_rgba(s: &Session, p: &Value) -> Result<(Vec<u8>, u32, u32)> {
    let fps = s.fps();
    // The playhead, unless the caller gave a time.
    let at = match params::time_at(p, "at", fps).or_else(|| params::time_p(p, fps)) {
        Some(at) => at,
        None => s.services().project.current_playhead().await,
    };
    let png = s.render_frame_png(Some(at), 0).await?;
    let image = image::load_from_memory(&png).map_err(|e| EngineError::Other(e.to_string()))?;
    let rgba = image.to_rgba8();
    let (width, height) = (rgba.width(), rgba.height());
    Ok((rgba.into_raw(), width, height))
}

fn run_histogram<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let (rgba, width, height) = frame_rgba(s, p).await?;
        let histogram = snapshort_scopes::Histogram::of(&rgba);
        let luma_only = params::str_p(p, "channels") == Some("luma");
        // Arrays wider than 32 have no Serialize, so hand out vectors.
        let payload = if luma_only {
            json!({ "luma": histogram.luma.to_vec() })
        } else {
            json!({
                "r": histogram.r.to_vec(),
                "g": histogram.g.to_vec(),
                "b": histogram.b.to_vec(),
                "luma": histogram.luma.to_vec(),
            })
        };
        Ok(json!({
            "width": width,
            "height": height,
            "pixels": histogram.total(),
            "peak": histogram.peak(),
            "bins": payload,
        }))
    })
}

fn run_levels<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let (rgba, width, height) = frame_rgba(s, p).await?;
        let levels = snapshort_scopes::Levels::of(&rgba);
        Ok(json!({
            "width": width,
            "height": height,
            "r": [levels.r_min, levels.r_max],
            "g": [levels.g_min, levels.g_max],
            "b": [levels.b_min, levels.b_max],
            "lumaMean": levels.luma_mean,
            "clippedBlacksPermille": levels.clipped_blacks_permille,
            "clippedWhitesPermille": levels.clipped_whites_permille,
            "fullRange": levels.is_full_range(),
        }))
    })
}

fn run_balance<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let (rgba, width, height) = frame_rgba(s, p).await?;
        let balance = snapshort_scopes::Balance::of(&rgba);
        Ok(json!({
            "width": width,
            "height": height,
            "meanR": balance.mean_r,
            "meanG": balance.mean_g,
            "meanB": balance.mean_b,
            "cast": balance.cast(),
        }))
    })
}

fn run_waveform<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let out_width = params::u32_p(p, "width").unwrap_or(72).clamp(8, 240) as usize;
        let out_height = params::u32_p(p, "height").unwrap_or(24).clamp(4, 120) as usize;
        let (rgba, width, height) = frame_rgba(s, p).await?;
        let waveform = snapshort_scopes::Waveform::of(
            &rgba,
            width as usize,
            height as usize,
            out_width,
            out_height,
        );
        Ok(json!({
            "width": out_width,
            "height": out_height,
            "rows": waveform.as_rows(),
        }))
    })
}
