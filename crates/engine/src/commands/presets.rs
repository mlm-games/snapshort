//! The commands that read and write export presets.

use serde_json::{Value, json};

use crate::commands::cmd;
use crate::params;
use crate::presets::{
    Preset, all_presets_in, builtin_presets, find_preset, load_user_presets_in, save_user_presets,
};
use crate::{EngineError, Session};

use super::{RunFuture, has_project, query};

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        query!("preset.list", "Export Presets", "{}", run_list),
        cmd!(
            "preset.save",
            "Save Export Preset",
            [],
            None::<&'static str>,
            "{\"name\": string, \"format\"?: string, \"quality\"?: string, \"width\"?: int, \"height\"?: int, \"fps\"?: number, \"videoBitrate\"?: int, \"audioBitrate\"?: int, \"hardwareAccel\"?: bool}",
            has_project,
            run_save
        ),
        cmd!(
            "preset.delete",
            "Delete Export Preset",
            [],
            None::<&'static str>,
            "{\"name\": string}",
            has_project,
            run_delete
        ),
        cmd!(
            "export.preset",
            "Export with a Preset",
            ["File", "Export"],
            None::<&'static str>,
            "{\"name\": string, \"output\": path, \"wait\"?: bool}",
            has_project,
            run_export_preset
        ),
    ]
}

fn preset_json(preset: &Preset) -> Value {
    json!({
        "name": preset.name,
        "format": preset.format,
        "quality": preset.quality,
        "width": preset.width,
        "height": preset.height,
        "fps": preset.fps,
        "videoBitrate": preset.video_bitrate,
        "audioBitrate": preset.audio_bitrate,
        "hardwareAccel": preset.hardware_accel,
    })
}

fn run_list<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let presets = all_presets_in(s.data_dir());
        let items: Vec<Value> = presets.iter().map(preset_json).collect();
        Ok(json!({"presets": items, "count": items.len()}))
    })
}

fn run_save<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let name = params::require_str(p, "name", "preset.save")?.to_string();
        if name.trim().is_empty() {
            return Err(EngineError::bad("preset.save", "`name` must not be empty"));
        }
        // A preset is edited, not duplicated: saving the same name replaces it.
        let mut presets: Vec<Preset> = load_user_presets_in(s.data_dir())
            .into_iter()
            .filter(|preset| preset.name != name)
            .collect();
        let template = find_preset(s.data_dir(), &name);
        let (format, quality, width, height, fps) = match template {
            Some(template) => (
                params::str_p(p, "format")
                    .unwrap_or(&template.format)
                    .to_string(),
                params::str_p(p, "quality")
                    .unwrap_or(&template.quality)
                    .to_string(),
                params::u32_p(p, "width").unwrap_or(template.width),
                params::u32_p(p, "height").unwrap_or(template.height),
                params::f64_p(p, "fps").unwrap_or(template.fps),
            ),
            None => (
                params::str_p(p, "format").unwrap_or("mp4").to_string(),
                params::str_p(p, "quality")
                    .unwrap_or("standard")
                    .to_string(),
                params::u32_p(p, "width").unwrap_or(1920),
                params::u32_p(p, "height").unwrap_or(1080),
                params::f64_p(p, "fps").unwrap_or(30.0),
            ),
        };
        presets.insert(
            0,
            Preset {
                name: name.clone(),
                format,
                quality,
                width: width.max(16),
                height: height.max(16),
                fps: if fps > 0.0 { fps } else { 30.0 },
                video_bitrate: params::u32_p(p, "videoBitrate").unwrap_or(0),
                audio_bitrate: params::u32_p(p, "audioBitrate").unwrap_or(0),
                hardware_accel: params::bool_p(p, "hardwareAccel").unwrap_or(true),
            },
        );
        save_user_presets(s.data_dir(), &presets).map_err(|e| EngineError::Other(e.to_string()))?;
        let saved = presets
            .iter()
            .find(|preset| preset.name == name)
            .cloned()
            .ok_or_else(|| EngineError::Other("the preset did not save".into()))?;
        Ok(json!({"ok": true, "preset": preset_json(&saved)}))
    })
}

fn run_delete<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let name = params::require_str(p, "name", "preset.delete")?;
        let before = load_user_presets_in(s.data_dir());
        let after: Vec<Preset> = before
            .iter()
            .filter(|preset| preset.name != name)
            .cloned()
            .collect();
        if after.len() == before.len() {
            // Deleting a built-in is a no-op that says so.
            if builtin_presets().iter().any(|preset| preset.name == name) {
                return Err(EngineError::bad(
                    "preset.delete",
                    "that is a built-in preset",
                ));
            }
            return Err(EngineError::bad("preset.delete", "no such preset"));
        }
        save_user_presets(s.data_dir(), &after).map_err(|e| EngineError::Other(e.to_string()))?;
        Ok(json!({"ok": true, "deleted": name}))
    })
}

fn run_export_preset<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let name = params::require_str(p, "name", "export.preset")?;
        let Some(preset) = find_preset(s.data_dir(), name) else {
            return Err(EngineError::bad(
                "export.preset",
                format!("no preset `{name}`"),
            ));
        };
        let output = params::require_file(p, "output", "export.preset")?;
        // The preset replaces the settings the timeline would otherwise pick.
        s.set_timeline_fps_from_preset(preset.fps).await;
        let mut params = json!({
            "output": output.display().to_string(),
            "format": preset.format,
            "quality": preset.quality,
            "width": preset.width,
            "height": preset.height,
            "hardwareAccel": preset.hardware_accel,
            "wait": params::bool_p(p, "wait").unwrap_or(false),
        });
        if preset.fps > 0.0 {
            params["fps"] = json!(preset.fps);
        }
        if preset.video_bitrate > 0 {
            params["videoBitrate"] = json!(preset.video_bitrate);
        }
        if preset.audio_bitrate > 0 {
            params["audioBitrate"] = json!(preset.audio_bitrate);
        }
        s.execute("render.export", params).await
    })
}
