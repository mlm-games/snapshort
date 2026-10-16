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
            "{\"output\": path, \"format\"?: mp4|mkv|mov|av1-mp4|av1-mkv|av1-webm|av1-ivf|opus|flac, \"quality\"?: draft|preview|standard|high|master, \"width\"?: int, \"height\"?: int, \"fps\"?: number, \"videoBitrate\"?: int, \"audioBitrate\"?: int, \"hardwareAccel\"?: bool, \"masterVolume\"?: number, \"trackVolumes\"?: {track-id: number}, \"wait\"?: bool}",
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

/// A format name the exporter cannot write must fail loudly: falling back to
/// MP4/H.264 silently exports the wrong codec for a stale preset or a typo.
fn format_of(name: &str) -> crate::Result<OutputFormat> {
    let format = match name.to_ascii_lowercase().as_str() {
        "h264-mp4" | "mp4" => OutputFormat::Mp4H264,
        "h264-mkv" | "mkv" => OutputFormat::H264Mkv,
        "h264-mov" | "mov" => OutputFormat::MovH264,
        "av1" | "av1-mp4" => OutputFormat::Av1Mp4,
        "av1-mkv" => OutputFormat::Av1Mkv,
        "av1-webm" | "webm" => OutputFormat::Av1WebM,
        "av1-ivf" | "ivf" => OutputFormat::Av1Ivf,
        "opus" | "ogg" => OutputFormat::Opus,
        "flac" => OutputFormat::Flac,
        other => {
            return Err(EngineError::bad(
                "render.export",
                format!(
                    "unknown format `{other}`; expected one of {}",
                    crate::export_format_names().join(", ")
                ),
            ));
        }
    };
    Ok(format)
}

/// Format from the output file's extension, so `out.mov` just works. An
/// extension the exporter cannot write is an error rather than a guess.
fn format_for_output(path: &std::path::Path) -> crate::Result<OutputFormat> {
    let extension = path
        .extension()
        .and_then(|e| e.to_str())
        .unwrap_or_default()
        .to_ascii_lowercase();
    let format = match extension.as_str() {
        "mp4" | "m4v" => OutputFormat::Mp4H264,
        "mkv" => OutputFormat::H264Mkv,
        "mov" => OutputFormat::MovH264,
        "webm" => OutputFormat::Av1WebM,
        "ivf" => OutputFormat::Av1Ivf,
        "opus" => OutputFormat::Opus,
        "flac" => OutputFormat::Flac,
        other => {
            return Err(EngineError::bad(
                "render.export",
                format!(
                    "cannot tell the format from `.{other}`; pass \"format\": one of {}",
                    crate::export_format_names().join(", ")
                ),
            ));
        }
    };
    Ok(format)
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
            Some(name) => format_of(name)?,
            None => format_for_output(&output)?,
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

#[cfg(test)]
mod format_name_tests {
    use super::*;

    #[test]
    fn format_names_map_to_their_formats() {
        for (name, want) in [
            ("mp4", OutputFormat::Mp4H264),
            ("mkv", OutputFormat::H264Mkv),
            ("mov", OutputFormat::MovH264),
            ("av1-mp4", OutputFormat::Av1Mp4),
            ("av1-mkv", OutputFormat::Av1Mkv),
            ("av1-webm", OutputFormat::Av1WebM),
            ("av1-ivf", OutputFormat::Av1Ivf),
            ("opus", OutputFormat::Opus),
            ("flac", OutputFormat::Flac),
        ] {
            assert_eq!(format_of(name).unwrap(), want, "{name}");
            assert_eq!(
                format_of(&name.to_ascii_uppercase()).unwrap(),
                want,
                "{name} must be case-insensitive",
            );
        }
        assert!(format_of("nonsense").is_err(), "unknown names must not guess");
        assert!(format_of("webm-vp9").is_err(), "removed formats must not guess");
    }

    #[test]
    fn extension_selects_the_format() {
        for (file, want) in [
            ("out.mp4", OutputFormat::Mp4H264),
            ("out.mkv", OutputFormat::H264Mkv),
            ("out.mov", OutputFormat::MovH264),
            ("out.webm", OutputFormat::Av1WebM),
            ("out.ivf", OutputFormat::Av1Ivf),
            ("out.opus", OutputFormat::Opus),
            ("out.flac", OutputFormat::Flac),
        ] {
            assert_eq!(
                format_for_output(std::path::Path::new(file)).unwrap(),
                want,
                "{file}",
            );
        }
        assert!(format_for_output(std::path::Path::new("out.wav")).is_err());
    }
}
