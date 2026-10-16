//! Render commands: export plans, exports, background jobs.

use std::collections::HashMap;

use serde_json::Value;
use snapshort_infra_render::{OutputFormat, QualityPreset};
use snapshort_usecases::{RenderCommand, RenderOverrides};

use crate::commands::cmd;
use crate::params;
use crate::{EngineError, Session};

use super::{RunFuture, export_running, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "render.plan",
            "Build Render Plan",
            [],
            None::<&'static str>,
            "{}",
            has_project,
            run_plan
        ),
        cmd!(
            "render.export",
            "Export Timeline",
            ["File", "Export"],
            Some("Cmd+E"),
            "{\"output\": path, \"format\"?: mp4-h264|mp4-h265|webm-vp9|mov-prores|png-sequence|jpeg-sequence, \"quality\"?: draft|preview|standard|high|master, \"width\"?: int, \"height\"?: int, \"fps\"?: number, \"videoBitrate\"?: int, \"audioBitrate\"?: int, \"hardwareAccel\"?: bool, \"masterVolume\"?: number, \"trackVolumes\"?: {track-id: number}, \"wait\"?: bool}",
            has_project,
            run_export
        ),
        cmd!(
            "render.cancel",
            "Cancel Export",
            [],
            None::<&'static str>,
            "{}",
            export_running,
            run_cancel
        ),
        query!("jobs.list", "Background Jobs", "{}", run_jobs),
        cmd!(
            "jobs.cancel",
            "Cancel Job",
            [],
            None::<&'static str>,
            "{\"job\": id}",
            has_project,
            run_cancel_job
        ),
    ]
}

fn format_of(name: &str) -> OutputFormat {
    match name.to_ascii_lowercase().as_str() {
        "mp4-h265" | "h265" | "hevc" => OutputFormat::Mp4H265,
        "webm-vp9" | "webm" | "vp9" => OutputFormat::WebmVp9,
        "mov-prores" | "mov" | "prores" => OutputFormat::MovProRes,
        "png-sequence" | "png" => OutputFormat::PngSequence,
        "jpeg-sequence" | "jpg-sequence" | "jpg" => OutputFormat::JpegSequence,
        _ => OutputFormat::Mp4H264,
    }
}

/// Format from the output file's extension, so `out.mov` just works.
fn format_for_output(path: &std::path::Path) -> OutputFormat {
    match path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase()
        .as_str()
    {
        "mov" => OutputFormat::MovProRes,
        "webm" => OutputFormat::WebmVp9,
        "png" => OutputFormat::PngSequence,
        "jpg" | "jpeg" => OutputFormat::JpegSequence,
        _ => OutputFormat::Mp4H264,
    }
}

fn quality_of(name: &str) -> QualityPreset {
    match name.to_ascii_lowercase().as_str() {
        "draft" => QualityPreset::Draft,
        "preview" => QualityPreset::Preview,
        "high" => QualityPreset::High,
        "master" => QualityPreset::Master,
        _ => QualityPreset::Standard,
    }
}

fn run_plan<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Render(RenderCommand::PreparePlan))
            .await
            .map_err(EngineError::from)
    })
}

fn run_export<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let output = params::require_file(p, "output", "render.export")?;
        let format = match params::str_p(p, "format") {
            Some(name) => format_of(name),
            None => format_for_output(&output),
        };
        let quality = quality_of(params::str_p(p, "quality").unwrap_or("standard"));
        let use_hardware_accel = params::bool_p(p, "hardwareAccel").unwrap_or(true);
        let master_volume = params::f64_p(p, "masterVolume").unwrap_or(1.0) as f32;
        let overrides = RenderOverrides {
            resolution: params::u32_p(p, "width")
                .filter(|width| *width > 0)
                .zip(params::u32_p(p, "height").filter(|height| *height > 0)),
            fps: params::f64_p(p, "fps").filter(|fps| *fps > 0.0),
            video_bitrate_kbps: params::u32_p(p, "videoBitrate").filter(|kbps| *kbps > 0),
            audio_bitrate_kbps: params::u32_p(p, "audioBitrate").filter(|kbps| *kbps > 0),
        };
        let mut track_volumes: HashMap<miniter_domain::TrackId, f32> = HashMap::new();
        if let Some(map) = p.get("trackVolumes").and_then(Value::as_object) {
            for (key, value) in map {
                if let (Ok(id), Some(v)) = (uuid::Uuid::parse_str(key), value.as_f64()) {
                    track_volumes.insert(miniter_domain::TrackId(id), v as f32);
                }
            }
        }

        s.dispatch(crate::SessionCommand::Render(RenderCommand::Export {
            output_path: output,
            format,
            quality,
            use_hardware_accel,
            track_volumes,
            master_volume,
            overrides,
        }))
        .await?;

        if params::bool_p(p, "wait").unwrap_or(false) {
            return s.await_render(600).await;
        }
        Ok(serde_json::json!({"ok": true, "started": true}))
    })
}

fn run_cancel<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        s.dispatch(crate::SessionCommand::Render(RenderCommand::CancelExport))
            .await
            .map_err(EngineError::from)
    })
}

fn run_cancel_job<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let job = params::require_uuid(p, "job", "jobs.cancel")?;
        s.services().jobs.cancel(job).await?;
        Ok(crate::inspect::jobs_json(s.services()).await)
    })
}

fn run_jobs<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::jobs_json(s.services()).await) })
}
