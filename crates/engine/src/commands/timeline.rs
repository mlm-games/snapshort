//! Timeline commands: tracks, clips, trims, effects, transitions, text.

use miniter_domain::{
    BlendMode, Clip, ClipId, ClipKind, Keyframe, MaskEffect, MediaDuration, TextOverlay, TextStyle,
    Timestamp, TrackId, TrackKind, Transition, VideoClip, filter::VideoEffect,
};
use serde_json::{Value, json};
use snapshort_usecases::{Asset, AssetType};

use crate::commands::cmd;
use crate::params::{self, require_str, str_p};
use crate::{EngineError, Result, Session};

use super::{RunFuture, apply_batch, apply_edit, has_project, query, target_clips};
use crate::commands::transcript::segments_of;

pub fn specs() -> Vec<crate::commands::CommandSpec> {
    vec![
        cmd!(
            "timeline.addTrack",
            "Add Track",
            ["Timeline", "Add Track"],
            None::<&'static str>,
            "{\"kind\": \"video|audio|text|subtitle\", \"name\"?: string}",
            has_project,
            run_add_track
        ),
        cmd!(
            "timeline.removeTrack",
            "Delete Track",
            [],
            None::<&'static str>,
            "{\"track\": id}",
            has_project,
            run_remove_track
        ),
        cmd!(
            "timeline.renameTrack",
            "Rename Track",
            [],
            None::<&'static str>,
            "{\"track\": id, \"name\": string}",
            has_project,
            run_rename_track
        ),
        cmd!(
            "timeline.setTrackMuted",
            "Mute Track",
            [],
            None::<&'static str>,
            "{\"track\": id, \"muted\": bool}",
            has_project,
            run_set_track_muted
        ),
        cmd!(
            "timeline.setTrackLocked",
            "Lock Track",
            [],
            None::<&'static str>,
            "{\"track\": id, \"locked\": bool}",
            has_project,
            run_set_track_locked
        ),
        cmd!(
            "timeline.addAsset",
            "Add Clip to Timeline",
            [],
            None::<&'static str>,
            "{\"asset\": id, \"track\"?: id, \"start\"?: time, \"sourceIn\"?: time, \"duration\"?: time, \"speed\"?: number, \"volume\"?: number, \"opacity\"?: number}",
            has_project,
            run_add_asset
        ),
        cmd!(
            "timeline.addSubtitles",
            "Add Subtitles",
            ["Timeline", "Add Subtitles"],
            None::<&'static str>,
            "{\"path\": string, \"track\"?: id, \"font\"?: string}",
            has_project,
            run_add_subtitles
        ),
        cmd!(
            "timeline.addText",
            "Add Text Clip",
            ["Timeline", "Add Text"],
            None::<&'static str>,
            "{\"text\": string, \"track\"?: id, \"start\"?: time, \"duration\"?: time, \"style\"?: TextStyle}",
            has_project,
            run_add_text
        ),
        cmd!(
            "timeline.removeClip",
            "Delete Clip",
            ["Timeline", "Delete"],
            Some("Delete"),
            "{\"clip\"?: id, \"clips\"?: [id]}",
            has_project,
            run_remove_clip
        ),
        cmd!(
            "timeline.moveClip",
            "Move Clip",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"track\"?: id, \"start\": time}",
            has_project,
            run_move_clip
        ),
        cmd!(
            "timeline.duplicateClip",
            "Duplicate Clip",
            [],
            Some("Cmd+D"),
            "{\"clip\": id, \"track\"?: id, \"start\"?: time}",
            has_project,
            run_duplicate_clip
        ),
        cmd!(
            "timeline.trimIn",
            "Trim Clip In",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"start\"?: time, \"sourceIn\"?: time}",
            has_project,
            run_trim_in
        ),
        cmd!(
            "timeline.trimOut",
            "Trim Clip Out",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"duration\"?: time, \"end\"?: time}",
            has_project,
            run_trim_out
        ),
        cmd!(
            "timeline.split",
            "Split Clip at Playhead",
            ["Timeline", "Split"],
            Some("Cmd+K"),
            "{\"clip\"?: id, \"clips\"?: [id], \"at\"?: time}",
            has_project,
            run_split
        ),
        cmd!(
            "timeline.setClipSpeed",
            "Clip Speed",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"speed\": number}",
            has_project,
            run_set_speed
        ),
        cmd!(
            "timeline.setClipVolume",
            "Clip Volume",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"volume\": number}",
            has_project,
            run_set_volume
        ),
        cmd!(
            "timeline.setClipOpacity",
            "Clip Opacity",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"opacity\": number}",
            has_project,
            run_set_opacity
        ),
        cmd!(
            "timeline.setClipMuted",
            "Mute Clip",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"muted\": bool}",
            has_project,
            run_set_muted
        ),
        cmd!(
            "timeline.setClipBlendMode",
            "Clip Blend Mode",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"blendMode\": BlendMode}",
            has_project,
            run_set_blend_mode
        ),
        cmd!(
            "timeline.addEffect",
            "Add Video Effect",
            ["Timeline", "Add Effect"],
            None::<&'static str>,
            "{\"clip\": id, \"filter\": VideoFilter}",
            has_project,
            run_add_effect
        ),
        cmd!(
            "timeline.updateEffect",
            "Edit Video Effect",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int, \"filter\": VideoFilter}",
            has_project,
            run_update_effect
        ),
        cmd!(
            "timeline.removeEffect",
            "Remove Video Effect",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int}",
            has_project,
            run_remove_effect
        ),
        cmd!(
            "timeline.setEffectEnabled",
            "Toggle Video Effect",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int, \"enabled\": bool}",
            has_project,
            run_set_effect_enabled
        ),
        cmd!(
            "timeline.moveEffect",
            "Reorder Video Effects",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"from\": int, \"to\": int}",
            has_project,
            run_move_effect
        ),
        cmd!(
            "timeline.addAudioFilter",
            "Add Audio Effect",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"filter\": AudioFilter}",
            has_project,
            run_add_audio_filter
        ),
        cmd!(
            "timeline.removeAudioFilter",
            "Remove Audio Effect",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int}",
            has_project,
            run_remove_audio_filter
        ),
        cmd!(
            "timeline.setTransitionIn",
            "Transition In",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"transition\"?: {\"kind\":string,\"duration\":time}}",
            has_project,
            run_set_transition_in
        ),
        cmd!(
            "timeline.setTransitionOut",
            "Transition Out",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"transition\"?: {\"kind\":string,\"duration\":time}}",
            has_project,
            run_set_transition_out
        ),
        cmd!(
            "timeline.setText",
            "Edit Text",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"text\": string}",
            has_project,
            run_set_text
        ),
        cmd!(
            "timeline.setTextStyle",
            "Text Style",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"style\": TextStyle}",
            has_project,
            run_set_text_style
        ),
        cmd!(
            "timeline.setSubtitleFont",
            "Subtitle Font",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"font\"?: string}",
            has_project,
            run_set_subtitle_font
        ),
        cmd!(
            "timeline.addKeyframe",
            "Add Keyframe",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"keyframe\": Keyframe}",
            has_project,
            run_add_keyframe
        ),
        cmd!(
            "timeline.updateKeyframe",
            "Edit Keyframe",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int, \"keyframe\": Keyframe}",
            has_project,
            run_update_keyframe
        ),
        cmd!(
            "timeline.removeKeyframe",
            "Remove Keyframe",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int}",
            has_project,
            run_remove_keyframe
        ),
        cmd!(
            "timeline.addMask",
            "Add Mask",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"mask\": MaskEffect}",
            has_project,
            run_add_mask
        ),
        cmd!(
            "timeline.updateMask",
            "Edit Mask",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int, \"mask\": MaskEffect}",
            has_project,
            run_update_mask
        ),
        cmd!(
            "timeline.removeMask",
            "Remove Mask",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int}",
            has_project,
            run_remove_mask
        ),
        cmd!(
            "timeline.setMaskEnabled",
            "Toggle Mask",
            [],
            None::<&'static str>,
            "{\"clip\": id, \"index\": int, \"enabled\": bool}",
            has_project,
            run_set_mask_enabled
        ),
        cmd!(
            "timeline.rippleDelete",
            "Ripple Delete",
            ["Timeline", "Ripple Delete"],
            Some("Shift+Delete"),
            "{\"track\"?: id, \"start\"?: time, \"end\"?: time, \"clips\"?: [id]}",
            has_project,
            run_ripple_delete
        ),
        cmd!(
            "timeline.lift",
            "Lift",
            ["Timeline", "Lift"],
            Some(";"),
            "{\"track\"?: id, \"start\"?: time, \"end\"?: time, \"clips\"?: [id]}",
            has_project,
            run_lift
        ),
        cmd!(
            "timeline.closeGap",
            "Close Gap",
            ["Timeline", "Close Gap"],
            None::<&'static str>,
            "{\"track\"?: id, \"at\"?: time}",
            has_project,
            run_close_gap
        ),
        cmd!(
            "timeline.joinThroughEdits",
            "Join Through Edits",
            ["Timeline", "Join Through Edits"],
            None::<&str>,
            "{\"track\"?: id, \"at\"?: time}",
            has_project,
            run_join_through
        ),
        cmd!(
            "timeline.razor",
            "Razor at Playhead",
            ["Timeline", "Razor"],
            Some("Cmd+K"),
            "{\"all\"?: bool, \"at\"?: time}",
            has_project,
            run_razor
        ),
        cmd!(
            "timeline.insert",
            "Insert at Playhead",
            ["Timeline", "Insert"],
            Some(","),
            "{\"asset\"?: id, \"clip\"?: id, \"track\"?: id, \"at\"?: time}",
            has_project,
            run_insert
        ),
        cmd!(
            "timeline.overwrite",
            "Overwrite at Playhead",
            ["Timeline", "Overwrite"],
            Some("."),
            "{\"asset\"?: id, \"clip\"?: id, \"track\"?: id, \"at\"?: time}",
            has_project,
            run_overwrite
        ),
        query!("timeline.inspect", "Timeline", "{}", run_inspect),
    ]
}

fn track_kind(v: Option<&str>) -> TrackKind {
    match v.unwrap_or("video").to_ascii_lowercase().as_str() {
        "audio" => TrackKind::Audio,
        "text" => TrackKind::Text,
        "subtitle" => TrackKind::Subtitle,
        _ => TrackKind::Video,
    }
}

/// The track a clip of `kind` can live on: the named track when it matches,
/// otherwise the first unlocked track of that kind, otherwise a new one.
async fn ensure_track(s: &Session, kind: TrackKind, named: Option<TrackId>) -> Result<TrackId> {
    let timeline = s
        .services()
        .project
        .current_timeline()
        .await
        .ok_or(EngineError::NoProject)?;
    if let Some(id) = named {
        match timeline.track(id) {
            Some(t) if t.kind == kind && !t.locked => return Ok(id),
            Some(t) if t.kind == kind => {
                return Err(EngineError::Other(format!("track `{id:?}` is locked")));
            }
            _ => {
                return Err(EngineError::Other(format!(
                    "track `{id:?}` is not a {kind:?} track"
                )));
            }
        }
    }
    if let Some(t) = timeline.tracks.iter().find(|t| t.kind == kind && !t.locked) {
        return Ok(t.id);
    }
    let name = match kind {
        TrackKind::Video => "Video",
        TrackKind::Audio => "Audio",
        TrackKind::Text => "Text",
        TrackKind::Subtitle => "Subtitles",
        _ => "Track",
    };
    let name = format!(
        "{name} {}",
        timeline.tracks.iter().filter(|t| t.kind == kind).count() + 1
    );
    create_track(s, kind, Some(name)).await
}

async fn create_track(s: &Session, kind: TrackKind, name: Option<String>) -> Result<TrackId> {
    let name = name.unwrap_or_else(|| format!("{kind:?}"));
    // Mint the id here: miniter builds the track under it, so the caller never
    // has to find the new track again by name — which could match an existing
    // track that happens to share the name.
    let track_id = TrackId::new();
    apply_edit(
        s,
        miniter_usecases::EditCommand::AddTrack {
            kind,
            name,
            track_id: Some(track_id),
        },
        "Add Track",
    )
    .await?;
    Ok(track_id)
}

fn run_add_asset<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = build_asset_clip(s, p).await?;
        apply_edit(s, edit.command, edit.label).await?;
        s.note_timeline_fps().await;
        Ok(crate::inspect::timeline_json(s).await)
    })
}

fn run_add_text<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = build_text_clip(s, p).await?;
        apply_edit(s, edit.command, edit.label).await
    })
}

fn run_add_subtitles<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = build_subtitle_clip(s, p).await?;
        apply_edit(s, edit.command, edit.label).await
    })
}

/// One edit as a command plus its undo label.
pub(crate) struct EditSpec {
    pub(crate) command: miniter_usecases::EditCommand,
    pub(crate) label: &'static str,
}

impl EditSpec {
    fn new(command: miniter_usecases::EditCommand, label: &'static str) -> Self {
        Self { command, label }
    }
}

fn clip_of(p: &Value, cmd: &str) -> Result<ClipId> {
    params::require_clip(p, "clip", cmd)
}

fn index_of(p: &Value, cmd: &str) -> Result<usize> {
    params::require_usize(p, "index", cmd)
}

/// Every edit command as an [`EditSpec`], so `timeline.batch` and the direct
/// handlers share one mapping. Commands that resolve tracks, create clips or
/// talk to assets are not plain edits and return `None`.
pub(crate) async fn edit_of(s: &Session, id: &str, p: &Value) -> Result<Option<EditSpec>> {
    let cmd = id;
    let spec = match id {
        "timeline.removeTrack" => EditSpec::new(
            miniter_usecases::EditCommand::RemoveTrack {
                track_id: params::require_track(p, "track", cmd)?,
            },
            "Delete Track",
        ),
        "timeline.renameTrack" => EditSpec::new(
            miniter_usecases::EditCommand::RenameTrack {
                track_id: params::require_track(p, "track", cmd)?,
                new_name: require_str(p, "name", cmd)?.to_string(),
            },
            "Rename Track",
        ),
        "timeline.setTrackMuted" => EditSpec::new(
            miniter_usecases::EditCommand::SetTrackMuted {
                track_id: params::require_track(p, "track", cmd)?,
                muted: params::require_bool(p, "muted", cmd)?,
            },
            "Mute Track",
        ),
        "timeline.setTrackLocked" => EditSpec::new(
            miniter_usecases::EditCommand::SetTrackLocked {
                track_id: params::require_track(p, "track", cmd)?,
                locked: params::require_bool(p, "locked", cmd)?,
            },
            "Lock Track",
        ),
        "timeline.removeClip" => {
            return Ok(Some(EditSpec::new(
                miniter_usecases::EditCommand::Batch {
                    label: "Delete Clip".into(),
                    commands: target_clips(s, p, cmd)
                        .await?
                        .into_iter()
                        .map(|clip_id| miniter_usecases::EditCommand::RemoveClip { clip_id })
                        .collect(),
                },
                "Delete Clip",
            )));
        }
        "timeline.moveClip" => {
            let clip_id = clip_of(p, cmd)?;
            let fps = s.fps();
            let timeline = s
                .services()
                .project
                .current_timeline()
                .await
                .ok_or(EngineError::NoProject)?;
            let new_track_id = match params::track_p(p, "track") {
                Some(t) => t,
                None => timeline
                    .track_of_clip(clip_id)
                    .map(|t| t.id)
                    .ok_or_else(|| EngineError::bad(cmd, "clip not found"))?,
            };
            let fallback = timeline
                .track_of_clip(clip_id)
                .and_then(|t| t.clip_by_id(clip_id))
                .map(|c| c.timeline_start)
                .unwrap_or(Timestamp::ZERO);
            let new_start = params::time_at(p, "start", fps)
                .or_else(|| params::time_p(p, fps))
                .unwrap_or(fallback);
            EditSpec::new(
                miniter_usecases::EditCommand::MoveClip {
                    clip_id,
                    new_track_id,
                    new_start,
                },
                "Move Clip",
            )
        }
        "timeline.duplicateClip" => {
            let clip_id = clip_of(p, cmd)?;
            let fps = s.fps();
            let timeline = s
                .services()
                .project
                .current_timeline()
                .await
                .ok_or(EngineError::NoProject)?;
            let (track_id, clip) = timeline
                .tracks
                .iter()
                .find_map(|t| t.clip_by_id(clip_id).map(|c| (t.id, c.clone())))
                .ok_or_else(|| EngineError::bad(cmd, "clip not found"))?;
            let target_start = params::time_at(p, "start", fps)
                .or_else(|| params::time_p(p, fps))
                .unwrap_or(clip.timeline_end());
            EditSpec::new(
                miniter_usecases::EditCommand::DuplicateClip {
                    source_clip_id: clip_id,
                    new_clip_id: ClipId::new(),
                    target_track_id: params::track_p(p, "track").unwrap_or(track_id),
                    target_start,
                },
                "Duplicate Clip",
            )
        }
        "timeline.trimIn" => {
            let clip_id = clip_of(p, cmd)?;
            let fps = s.fps();
            let timeline = s
                .services()
                .project
                .current_timeline()
                .await
                .ok_or(EngineError::NoProject)?;
            let clip = timeline
                .tracks
                .iter()
                .find_map(|t| t.clip_by_id(clip_id))
                .ok_or_else(|| EngineError::bad(cmd, "clip not found"))?
                .clone();
            let new_start = params::time_at(p, "start", fps)
                .or_else(|| params::time_p(p, fps))
                .unwrap_or(clip.timeline_start + MediaDuration::from_secs(1.0));
            let delta = (new_start - clip.timeline_start).as_micros();
            let new_source_start = MediaDuration::from_micros(
                (clip.source_start.as_micros() + (delta as f64 / clip.speed) as i64).max(0),
            );
            EditSpec::new(
                miniter_usecases::EditCommand::TrimClipStart {
                    clip_id,
                    new_start,
                    new_source_start,
                },
                "Trim In",
            )
        }
        "timeline.trimOut" => {
            let clip_id = clip_of(p, cmd)?;
            let fps = s.fps();
            let timeline = s
                .services()
                .project
                .current_timeline()
                .await
                .ok_or(EngineError::NoProject)?;
            let clip = timeline
                .tracks
                .iter()
                .find_map(|t| t.clip_by_id(clip_id))
                .ok_or_else(|| EngineError::bad(cmd, "clip not found"))?
                .clone();
            let duration = match (
                params::duration_at(p, "duration", fps),
                params::time_at(p, "end", fps),
            ) {
                (Some(d), _) => d,
                (None, Some(end)) => (end - clip.timeline_start).max(MediaDuration::from_micros(1)),
                (None, None) => {
                    return Err(EngineError::bad(cmd, "give `duration` or `end`"));
                }
            };
            EditSpec::new(
                miniter_usecases::EditCommand::TrimClipEnd {
                    clip_id,
                    new_duration: duration,
                },
                "Trim Out",
            )
        }
        "timeline.split" => {
            let clips = target_clips(s, p, cmd).await?;
            let fps = s.fps();
            let playhead = s.services().project.current_playhead().await;
            let at = params::time_at(p, "at", fps)
                .or_else(|| params::time_p(p, fps))
                .unwrap_or(playhead);
            return Ok(Some(EditSpec::new(
                miniter_usecases::EditCommand::Batch {
                    label: "Split Clip".into(),
                    commands: clips
                        .into_iter()
                        .map(|clip_id| miniter_usecases::EditCommand::SplitClip {
                            clip_id,
                            at,
                            new_clip_id: ClipId::new(),
                        })
                        .collect(),
                },
                "Split Clip",
            )));
        }
        "timeline.setClipSpeed" => EditSpec::new(
            miniter_usecases::EditCommand::SetClipSpeed {
                clip_id: clip_of(p, cmd)?,
                speed: params::require_f64(p, "speed", cmd)?,
            },
            "Clip Speed",
        ),
        "timeline.setClipVolume" => EditSpec::new(
            miniter_usecases::EditCommand::SetClipVolume {
                clip_id: clip_of(p, cmd)?,
                volume: params::require_f64(p, "volume", cmd)? as f32,
            },
            "Clip Volume",
        ),
        "timeline.setClipOpacity" => EditSpec::new(
            miniter_usecases::EditCommand::SetClipOpacity {
                clip_id: clip_of(p, cmd)?,
                opacity: params::require_f64(p, "opacity", cmd)? as f32,
            },
            "Clip Opacity",
        ),
        "timeline.setClipMuted" => EditSpec::new(
            miniter_usecases::EditCommand::SetClipMuted {
                clip_id: clip_of(p, cmd)?,
                muted: params::require_bool(p, "muted", cmd)?,
            },
            "Mute Clip",
        ),
        "timeline.setClipBlendMode" => EditSpec::new(
            miniter_usecases::EditCommand::SetClipBlendMode {
                clip_id: clip_of(p, cmd)?,
                blend_mode: params::require_object::<BlendMode>(p, "blendMode", cmd)?,
            },
            "Clip Blend Mode",
        ),
        "timeline.addEffect" => EditSpec::new(
            miniter_usecases::EditCommand::AddVideoFilter {
                clip_id: clip_of(p, cmd)?,
                filter: params::require_object::<VideoEffect>(p, "filter", cmd)?,
                index: None,
            },
            "Add Effect",
        ),
        "timeline.updateEffect" => EditSpec::new(
            miniter_usecases::EditCommand::UpdateVideoFilter {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
                filter: params::require_object::<VideoEffect>(p, "filter", cmd)?,
            },
            "Edit Effect",
        ),
        "timeline.removeEffect" => EditSpec::new(
            miniter_usecases::EditCommand::RemoveVideoFilter {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
            },
            "Remove Effect",
        ),
        "timeline.setEffectEnabled" => EditSpec::new(
            miniter_usecases::EditCommand::SetVideoFilterEnabled {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
                enabled: params::require_bool(p, "enabled", cmd)?,
            },
            "Toggle Effect",
        ),
        "timeline.moveEffect" => EditSpec::new(
            miniter_usecases::EditCommand::MoveVideoFilter {
                clip_id: clip_of(p, cmd)?,
                from: params::require_usize(p, "from", cmd)?,
                to: params::require_usize(p, "to", cmd)?,
            },
            "Reorder Effects",
        ),
        "timeline.addAudioFilter" => EditSpec::new(
            miniter_usecases::EditCommand::AddAudioFilter {
                clip_id: clip_of(p, cmd)?,
                filter: params::require_object::<miniter_domain::AudioFilter>(p, "filter", cmd)?,
                index: None,
            },
            "Add Audio Effect",
        ),
        "timeline.removeAudioFilter" => EditSpec::new(
            miniter_usecases::EditCommand::RemoveAudioFilter {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
            },
            "Remove Audio Effect",
        ),
        "timeline.setTransitionIn" => EditSpec::new(
            miniter_usecases::EditCommand::SetTransitionIn {
                clip_id: clip_of(p, cmd)?,
                transition: transition_p(p, cmd)?,
            },
            "Transition In",
        ),
        "timeline.setTransitionOut" => EditSpec::new(
            miniter_usecases::EditCommand::SetTransitionOut {
                clip_id: clip_of(p, cmd)?,
                transition: transition_p(p, cmd)?,
            },
            "Transition Out",
        ),
        "timeline.setText" => EditSpec::new(
            miniter_usecases::EditCommand::UpdateTextContent {
                clip_id: clip_of(p, cmd)?,
                text: require_str(p, "text", cmd)?.to_string(),
            },
            "Edit Text",
        ),
        "timeline.setTextStyle" => EditSpec::new(
            miniter_usecases::EditCommand::UpdateTextStyle {
                clip_id: clip_of(p, cmd)?,
                style: params::require_object::<TextStyle>(p, "style", cmd)?,
            },
            "Text Style",
        ),
        "timeline.setSubtitleFont" => EditSpec::new(
            miniter_usecases::EditCommand::SetSubtitleFont {
                clip_id: clip_of(p, cmd)?,
                font_path: str_p(p, "font").map(str::to_string),
            },
            "Subtitle Font",
        ),
        "timeline.addKeyframe" => EditSpec::new(
            miniter_usecases::EditCommand::AddKeyframe {
                clip_id: clip_of(p, cmd)?,
                keyframe: params::require_object::<Keyframe>(p, "keyframe", cmd)?,
            },
            "Add Keyframe",
        ),
        "timeline.updateKeyframe" => EditSpec::new(
            miniter_usecases::EditCommand::UpdateKeyframe {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
                keyframe: params::require_object::<Keyframe>(p, "keyframe", cmd)?,
            },
            "Edit Keyframe",
        ),
        "timeline.removeKeyframe" => EditSpec::new(
            miniter_usecases::EditCommand::RemoveKeyframe {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
            },
            "Remove Keyframe",
        ),
        "timeline.addMask" => EditSpec::new(
            miniter_usecases::EditCommand::AddMask {
                clip_id: clip_of(p, cmd)?,
                mask: params::require_object::<MaskEffect>(p, "mask", cmd)?,
                index: None,
            },
            "Add Mask",
        ),
        "timeline.updateMask" => EditSpec::new(
            miniter_usecases::EditCommand::UpdateMask {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
                mask: params::require_object::<MaskEffect>(p, "mask", cmd)?,
            },
            "Edit Mask",
        ),
        "timeline.removeMask" => EditSpec::new(
            miniter_usecases::EditCommand::RemoveMask {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
            },
            "Remove Mask",
        ),
        "timeline.setMaskEnabled" => EditSpec::new(
            miniter_usecases::EditCommand::SetMaskEnabled {
                clip_id: clip_of(p, cmd)?,
                index: index_of(p, cmd)?,
                enabled: params::require_bool(p, "enabled", cmd)?,
            },
            "Toggle Mask",
        ),
        "timeline.addTrack" => {
            let kind = track_kind(str_p(p, "kind"));
            let name = str_p(p, "name")
                .map(str::to_string)
                .unwrap_or_else(|| format!("{kind:?}"));
            EditSpec::new(
                miniter_usecases::EditCommand::AddTrack {
                    kind,
                    name,
                    track_id: None,
                },
                "Add Track",
            )
        }
        "timeline.addText" => return build_text_clip(s, p).await.map(Some),
        "timeline.addSubtitles" => return build_subtitle_clip(s, p).await.map(Some),
        "timeline.addAsset" => return build_asset_clip(s, p).await.map(Some),
        _ => return Ok(None),
    };
    Ok(Some(spec))
}

/// A subtitle track clip backed by an SRT file: the renderer burns it in, and
/// MKV export can carry it as a soft track.
async fn build_subtitle_clip(s: &Session, p: &Value) -> Result<EditSpec> {
    let cmd = "timeline.addSubtitles";
    let path = params::require_file(p, "path", cmd)?;
    let body = std::fs::read_to_string(&path)
        .map_err(|e| EngineError::bad(cmd, format!("cannot read {}: {e}", path.display())))?;
    let cues = snapshort_interchange::parse_srt(&body);
    if cues.is_empty() {
        return Err(EngineError::bad(cmd, "no cues found in that file"));
    }
    let last = cues.last().map(|c| c.end_us).unwrap_or(0);
    let track_id = ensure_track(s, TrackKind::Subtitle, params::track_p(p, "track")).await?;
    // The render path burns srt and ass only: any other caption format is
    // written out as an ASS sidecar beside the source, so every format burns.
    let source_path = if is_burnable(&path) {
        path.display().to_string()
    } else {
        snapshort_ass_sidecar(&path, &body)?
    };
    Ok(EditSpec::new(
        miniter_usecases::EditCommand::AddClip {
            track_id,
            clip: new_clip(
                Timestamp::ZERO,
                MediaDuration::from_micros(last.max(1_000)),
                ClipKind::Subtitle(miniter_domain::SubtitleClip {
                    source_path,
                    font_path: str_p(p, "font").map(str::to_string),
                }),
            ),
        },
        "Add Subtitles",
    ))
}

/// The render path's burn step reads srt and ass; everything else goes through
/// an ASS sidecar.
fn is_burnable(path: &std::path::Path) -> bool {
    matches!(
        path.extension()
            .and_then(|e| e.to_str())
            .map(str::to_ascii_lowercase)
            .as_deref(),
        Some("srt" | "ass" | "ssa")
    )
}

/// Write a caption file as ASS next to its source, and return that path.
fn snapshort_ass_sidecar(path: &std::path::Path, body: &str) -> Result<String> {
    let bytes = body.as_bytes();
    let format = snapshort_captions::detect(bytes, path.extension().and_then(|e| e.to_str()))
        .or_else(|| {
            path.extension()
                .and_then(|e| e.to_str())
                .and_then(snapshort_captions::Format::from_name)
        })
        .ok_or_else(|| {
            EngineError::bad(
                "timeline.addSubtitles",
                format!("cannot tell the caption format of {}", path.display()),
            )
        })?;
    let document = snapshort_captions::parse(bytes, format)
        .map_err(|why| EngineError::bad("timeline.addSubtitles", why.to_string()))?;
    let script =
        snapshort_captions::ass::write(&document, &snapshort_captions::ass::Style::default());
    let sidecar = path.with_extension("snapshort.ass");
    std::fs::write(&sidecar, script)
        .map_err(|e| EngineError::bad("timeline.addSubtitles", e.to_string()))?;
    Ok(sidecar.display().to_string())
}

/// A text or subtitle clip, resolved onto a track.
async fn build_text_clip(s: &Session, p: &Value) -> Result<EditSpec> {
    let cmd = "timeline.addText";
    let text = require_str(p, "text", cmd)?.to_string();
    let fps = s.fps();
    let start = params::time_at(p, "start", fps)
        .or_else(|| params::time_p(p, fps))
        .unwrap_or(Timestamp::ZERO);
    let duration =
        params::duration_at(p, "duration", fps).unwrap_or_else(|| MediaDuration::from_secs(5.0));
    let style = match params::str_p(p, "template") {
        Some(name) => crate::commands::text::find_template(name)
            .map(|t| t.style.clone())
            .ok_or_else(|| EngineError::bad(cmd, format!("no template `{name}`")))?,
        None => params::optional_object::<TextStyle>(p, "style", cmd)?.unwrap_or_default(),
    };
    let kind = if str_p(p, "kind") == Some("subtitle") {
        TrackKind::Subtitle
    } else {
        TrackKind::Text
    };
    let track_id = ensure_track(s, kind, params::track_p(p, "track")).await?;
    let clip_kind = match kind {
        TrackKind::Subtitle => ClipKind::Subtitle(miniter_domain::SubtitleClip {
            source_path: text,
            font_path: str_p(p, "font").map(str::to_string),
        }),
        _ => ClipKind::Text(TextOverlay { text, style }),
    };
    Ok(EditSpec::new(
        miniter_usecases::EditCommand::AddClip {
            track_id,
            clip: new_clip(start, duration, clip_kind),
        },
        "Add Text",
    ))
}

/// A media asset turned into a clip on a matching track.
async fn build_asset_clip(s: &Session, p: &Value) -> Result<EditSpec> {
    let cmd = "timeline.addAsset";
    let asset_id = params::require_uuid(p, "asset", cmd)?;
    let asset = s
        .services()
        .project
        .list_assets()
        .await
        .into_iter()
        .find(|a| a.id.0 == asset_id)
        .ok_or_else(|| EngineError::bad(cmd, "no such asset"))?;
    let fps = s.fps();
    let start = params::time_at(p, "start", fps)
        .or_else(|| params::time_p(p, fps))
        .unwrap_or(Timestamp::ZERO);
    let source_in = params::duration_at(p, "sourceIn", fps).unwrap_or(MediaDuration::ZERO);
    let duration = params::duration_at(p, "duration", fps);
    let speed = params::f64_p(p, "speed").unwrap_or(1.0);
    let kind = track_kind_of_asset(&asset);
    let track_id = ensure_track(s, kind, params::track_p(p, "track")).await?;
    Ok(EditSpec::new(
        miniter_usecases::EditCommand::AddClip {
            track_id,
            clip: clip_from_asset(&asset, start, source_in, duration, speed)?,
        },
        "Add Clip",
    ))
}

fn new_clip(start: Timestamp, duration: MediaDuration, kind: ClipKind) -> Clip {
    Clip {
        id: ClipId::new(),
        timeline_start: start,
        timeline_duration: duration,
        source_start: MediaDuration::ZERO,
        source_end: duration,
        source_total_duration: duration,
        speed: 1.0,
        volume: 1.0,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        kind,
        keyframes: miniter_domain::KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    }
}

fn run_add_track<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.addTrack")
}

fn run_remove_track<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeTrack")
}

fn run_rename_track<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.renameTrack")
}

fn run_set_track_muted<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setTrackMuted")
}

fn run_set_track_locked<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setTrackLocked")
}

fn track_kind_of_asset(asset: &Asset) -> TrackKind {
    match asset.asset_type {
        AssetType::Audio => TrackKind::Audio,
        _ => TrackKind::Video,
    }
}

fn s_fps(asset: &Asset) -> f64 {
    asset
        .media_info
        .as_ref()
        .and_then(|i| i.primary_video())
        .map(|v| v.fps)
        .filter(|f| *f > 0.0)
        .unwrap_or(30.0)
}

fn asset_total_us(asset: &Asset) -> i64 {
    asset
        .media_info
        .as_ref()
        .map(|i| i.duration_ms as i64 * 1000)
        .unwrap_or(0)
}

/// The clip a media asset becomes on the timeline.
fn clip_from_asset(
    asset: &Asset,
    start: Timestamp,
    source_in: MediaDuration,
    duration: Option<MediaDuration>,
    speed: f64,
) -> Result<Clip> {
    let kind = match asset.asset_type {
        AssetType::Audio => ClipKind::Audio(miniter_domain::AudioClip {
            source_path: asset.path.display().to_string(),
            sample_rate: asset
                .media_info
                .as_ref()
                .and_then(|i| i.primary_audio())
                .map(|a| a.sample_rate)
                .unwrap_or(48_000),
            channels: asset
                .media_info
                .as_ref()
                .and_then(|i| i.primary_audio())
                .map(|a| a.channels)
                .unwrap_or(2),
            filters: Vec::new(),
        }),
        _ => {
            let (width, height) = asset
                .media_info
                .as_ref()
                .and_then(|i| i.primary_video())
                .map(|v| (v.width, v.height))
                .unwrap_or((1920, 1080));
            ClipKind::Video(VideoClip {
                source_path: asset.path.display().to_string(),
                width,
                height,
                fps: s_fps(asset),
                filters: Vec::new(),
                audio_filters: Vec::new(),
                masks: Vec::new(),
            })
        }
    };

    let total = asset_total_us(asset);
    if total <= 0 && duration.is_none() {
        return Err(EngineError::bad(
            "timeline.addAsset",
            format!(
                "asset `{}` has no measured duration yet; pass `duration`, or wait for its analysis to finish",
                asset.name
            ),
        ));
    }
    let speed = speed.max(0.01);
    // The timeline length and the source range are tied by the speed, the way
    // miniter's SetClipSpeed resolves them: timeline = source / speed, and a
    // given timeline length consumes length * speed of source.
    let source_from = source_in.as_micros();
    let (timeline_us, source_end_us) = match duration {
        Some(duration) => {
            let timeline_us = duration.as_micros().max(1);
            let consumed = miniter_domain::time::scale_us_round(timeline_us, speed);
            let end = if total > 0 {
                (source_from + consumed).min(total)
            } else {
                source_from + consumed
            };
            (timeline_us, end)
        }
        None => {
            let available = (total - source_from).max(1);
            let timeline_us = miniter_domain::time::unscale_us_round(available, speed).max(1);
            (timeline_us, source_from + available)
        }
    };
    let timeline_duration = MediaDuration::from_micros(timeline_us);
    let source_end = MediaDuration::from_micros(source_end_us);
    let mut clip = Clip {
        id: ClipId::new(),
        timeline_start: start,
        timeline_duration,
        source_start: source_in,
        source_end,
        source_total_duration: MediaDuration::from_micros(total.max(source_end.as_micros())),
        speed,
        volume: 1.0,
        opacity: 1.0,
        muted: false,
        transition_in: None,
        transition_out: None,
        kind,
        keyframes: miniter_domain::KeyframeCurve::default(),
        blend_mode: BlendMode::Normal,
    };
    clip.normalize_source_bounds_in_place();
    Ok(clip)
}

fn run_remove_clip<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeClip")
}

fn run_move_clip<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.moveClip")
}

fn run_duplicate_clip<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.duplicateClip")
}

fn run_trim_in<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.trimIn")
}

fn run_trim_out<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.trimOut")
}

fn run_split<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.split")
}

fn run_set_speed<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setClipSpeed")
}

fn run_set_volume<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setClipVolume")
}

fn run_set_opacity<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setClipOpacity")
}

fn run_set_muted<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setClipMuted")
}

fn run_set_blend_mode<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setClipBlendMode")
}

fn run_add_effect<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.addEffect")
}

fn run_update_effect<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.updateEffect")
}

fn run_remove_effect<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeEffect")
}

fn run_set_effect_enabled<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setEffectEnabled")
}

fn run_move_effect<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.moveEffect")
}

fn run_add_audio_filter<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.addAudioFilter")
}

fn run_remove_audio_filter<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeAudioFilter")
}

fn run_set_transition_in<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setTransitionIn")
}

fn run_set_transition_out<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setTransitionOut")
}

fn run_set_text<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setText")
}

fn run_set_text_style<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setTextStyle")
}

fn run_set_subtitle_font<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setSubtitleFont")
}

fn run_add_keyframe<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.addKeyframe")
}

fn run_update_keyframe<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.updateKeyframe")
}

fn run_remove_keyframe<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeKeyframe")
}

fn run_add_mask<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.addMask")
}

fn run_update_mask<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.updateMask")
}

fn run_remove_mask<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.removeMask")
}

fn run_set_mask_enabled<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    delegate(s, p, "timeline.setMaskEnabled")
}

/// A timeline range in the model's own microseconds.
pub(crate) struct Range {
    start: Timestamp,
    end: Timestamp,
}

impl Range {
    pub(crate) fn new(start: Timestamp, end: Timestamp) -> Self {
        Self {
            start: start.min(end),
            end: start.max(end),
        }
    }

    fn duration(&self) -> MediaDuration {
        MediaDuration::from_micros((self.end - self.start).as_micros().max(0))
    }

    /// How much of `[clip_start, clip_end)` the range covers.
    fn overlap(&self, clip_start: Timestamp, clip_end: Timestamp) -> MediaDuration {
        let from = clip_start.max(self.start);
        let to = clip_end.min(self.end);
        MediaDuration::from_micros((to - from).as_micros().max(0))
    }
}

/// The range a range command acts on: explicit `start`/`end`, or the extent of
/// the named (or selected) clips.
async fn range_of(s: &Session, p: &Value, cmd: &str) -> Result<Range> {
    let fps = s.fps();
    if let (Some(start), Some(end)) = (
        params::time_at(p, "start", fps),
        params::time_at(p, "end", fps),
    ) {
        return Ok(Range::new(start, end));
    }
    let named = target_clips(s, p, cmd).await?;
    let timeline = s
        .services()
        .project
        .current_timeline()
        .await
        .ok_or(EngineError::NoProject)?;
    if named.is_empty() {
        return Err(EngineError::bad(
            cmd,
            "give `start` and `end`, or name `clips`",
        ));
    }
    let mut extent: Option<Range> = None;
    for track in &timeline.tracks {
        for clip in &track.clips {
            if !named.contains(&clip.id) {
                continue;
            }
            extent = Some(match extent {
                Some(range) => Range::new(
                    range.start.min(clip.timeline_start),
                    range.end.max(clip.timeline_end()),
                ),
                None => Range::new(clip.timeline_start, clip.timeline_end()),
            });
        }
    }
    extent.ok_or_else(|| EngineError::bad(cmd, "those clips are not on the timeline"))
}

/// The tracks a range command touches: the named one, or all of them.
fn tracks_in(timeline: &miniter_domain::Timeline, track: Option<TrackId>) -> Vec<TrackId> {
    match track {
        Some(id) if timeline.track(id).is_some() => vec![id],
        Some(_) => Vec::new(),
        None => timeline.tracks.iter().map(|t| t.id).collect(),
    }
}

/// The edits removing `spans` (sorted ascending, non-overlapping) from the
/// tracks `track` names, or all of them. Shared by the single-range commands
/// and the multi-range dead-space pass, so both close holes identically.
///
/// Each clip is cut at every span boundary that falls inside it, so a later
/// span never has to reason about a clip an earlier one already trimmed. A
/// surviving piece's final position is its original start minus everything
/// removed before it, which is what closes the hole.
pub(crate) fn remove_spans_commands(
    timeline: &miniter_domain::Timeline,
    spans: &[Range],
    ripple: bool,
    track: Option<TrackId>,
    named: &[ClipId],
) -> Vec<miniter_usecases::EditCommand> {
    let only_named = !named.is_empty();
    let mut commands: Vec<miniter_usecases::EditCommand> = Vec::new();

    for track_id in tracks_in(timeline, track) {
        let Some(track) = timeline.track(track_id) else {
            continue;
        };
        for clip in &track.clips {
            let clip_start = clip.timeline_start.as_micros();
            let clip_end = clip.timeline_end().as_micros();
            let (clip_start, clip_end) = (clip_start.min(clip_end), clip_start.max(clip_end));
            let before: i64 = spans
                .iter()
                .filter(|range| range.end.as_micros() <= clip_start)
                .map(|range| (range.end - range.start).as_micros())
                .sum();

            if only_named && !named.contains(&clip.id) {
                // Never cut, but a ripple still pulls it left with everything else.
                if ripple && before > 0 {
                    commands.push(miniter_usecases::EditCommand::MoveClip {
                        clip_id: clip.id,
                        new_track_id: track_id,
                        new_start: miniter_domain::Timestamp::from_micros(
                            (clip_start - before).max(0),
                        ),
                    });
                }
                continue;
            }

            // Spans that cut inside this clip, clamped to it.
            let mut inside: Vec<(i64, i64)> = spans
                .iter()
                .map(|range| {
                    (
                        range.start.as_micros().max(clip_start),
                        range.end.as_micros().min(clip_end),
                    )
                })
                .filter(|(start, end)| end > start)
                .collect();
            inside.sort_by_key(|(start, _)| *start);
            if inside.is_empty() {
                // Nothing cuts this clip, but a ripple still closes the gap
                // left by spans that ended before it.
                if !ripple || before == 0 {
                    continue;
                }
                commands.push(miniter_usecases::EditCommand::MoveClip {
                    clip_id: clip.id,
                    new_track_id: track_id,
                    new_start: miniter_domain::Timestamp::from_micros((clip_start - before).max(0)),
                });
                continue;
            }

            // Every span boundary strictly inside the clip is a cut.
            let mut cuts: Vec<i64> = Vec::new();
            for (start, end) in &inside {
                if *start > clip_start {
                    cuts.push(*start);
                }
                if *end < clip_end {
                    cuts.push(*end);
                }
            }
            cuts.sort_unstable();
            cuts.dedup();

            let mut edges: Vec<i64> = vec![clip_start];
            edges.extend_from_slice(&cuts);
            edges.push(clip_end);

            // Piece 0 keeps the clip's id; each cut splits the piece before it.
            let mut ids: Vec<miniter_domain::ClipId> = vec![clip.id];
            ids.extend((0..cuts.len()).map(|_| miniter_domain::ClipId::new()));
            let pieces: Vec<(miniter_domain::ClipId, i64, i64)> = ids
                .iter()
                .zip(edges.windows(2))
                .map(|(id, window)| (*id, window[0], window[1]))
                .collect();
            for index in 1..pieces.len() {
                commands.push(miniter_usecases::EditCommand::SplitClip {
                    clip_id: pieces[index - 1].0,
                    at: miniter_domain::Timestamp::from_micros(pieces[index].1),
                    new_clip_id: pieces[index].0,
                });
            }

            let removed_before = |at: i64| -> i64 {
                spans
                    .iter()
                    .filter(|range| range.end.as_micros() <= at)
                    .map(|range| (range.end - range.start).as_micros())
                    .sum()
            };

            for (id, start, end) in &pieces {
                if inside
                    .iter()
                    .any(|(span_start, span_end)| *start >= *span_start && *end <= *span_end)
                {
                    commands.push(miniter_usecases::EditCommand::RemoveClip { clip_id: *id });
                    continue;
                }
                if !ripple {
                    continue;
                }
                let final_start = start - removed_before(*start);
                if final_start != *start {
                    commands.push(miniter_usecases::EditCommand::MoveClip {
                        clip_id: *id,
                        new_track_id: track_id,
                        new_start: miniter_domain::Timestamp::from_micros(final_start.max(0)),
                    });
                }
            }
        }
    }
    commands
}

/// Remove a range from every touched track. With `ripple` the hole is closed
/// and later material moves left; without it, the hole stays (lift).
///
/// The shift is per track and equals what that track actually lost, so a
/// ripple over a sparse track closes only its own gaps.
fn remove_range<'a>(s: &'a Session, p: &'a Value, cmd: &'a str, ripple: bool) -> RunFuture<'a> {
    Box::pin(async move {
        let range = range_of(s, p, cmd).await?;
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let named = target_clips(s, p, cmd).await.unwrap_or_default();
        let commands = remove_spans_commands(
            &timeline,
            &[range],
            ripple,
            params::track_p(p, "track"),
            &named,
        );
        let label = if ripple { "Ripple Delete" } else { "Lift" };
        apply_batch(s, commands, label).await
    })
}

fn run_ripple_delete<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    remove_range(s, p, "timeline.rippleDelete", true)
}

fn run_lift<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    remove_range(s, p, "timeline.lift", false)
}

/// Close the first hole at or after `at` on a track, pulling the rest left.
/// Heal the cuts between adjacent pieces of the same source that continue each
/// other in media time. Pieces chain (a split of a split), so consecutive
/// through edits are one run: the first piece absorbs the whole run, its
/// duration growing to cover every piece in it, which miniter's trim resolves
/// into the matching source range.
fn run_join_through<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let named = params::track_p(p, "track");
        let mut commands: Vec<miniter_usecases::EditCommand> = Vec::new();
        let mut joined = 0usize;

        for track in &timeline.tracks {
            if named.is_some_and(|id| id != track.id) {
                continue;
            }
            let edits = snapshort_edit::track_through_edits(&segments_of(&track.clips));
            for (first, last) in chain_runs(&edits) {
                // One native command per adjacent pair: each join absorbs into
                // the run's head, so later pairs reference the survivor.
                let head = track.clips[first].id;
                for index in first + 1..=last {
                    commands.push(miniter_usecases::EditCommand::JoinClips {
                        left_id: head,
                        right_id: track.clips[index].id,
                    });
                }
                joined += last - first;
            }
        }

        if joined == 0 {
            return Ok(json!({"joined": 0, "message": "no through edits to join"}));
        }
        apply_batch(s, commands, "Join Through Edits").await?;
        Ok(json!({"joined": joined}))
    })
}

/// Maximal runs of chained through edits, as (first piece, last piece) indices.
fn chain_runs(edits: &[snapshort_edit::ThroughEdit]) -> Vec<(usize, usize)> {
    let mut out: Vec<(usize, usize)> = Vec::new();
    for edit in edits {
        match out.last_mut() {
            Some((_, last)) if *last == edit.left => *last = edit.right,
            _ => out.push((edit.left, edit.right)),
        }
    }
    out
}

fn run_close_gap<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = s.fps();
        let playhead = s.services().project.current_playhead().await;
        let at = params::time_at(p, "at", fps)
            .or_else(|| params::time_p(p, fps))
            .unwrap_or(playhead);
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let named_track = params::track_p(p, "track");
        let tracks = tracks_in(&timeline, named_track);
        if tracks.is_empty() {
            return Err(EngineError::bad("timeline.closeGap", "no such track"));
        }

        let mut commands: Vec<miniter_usecases::EditCommand> = Vec::new();
        for track_id in tracks {
            let Some(track) = timeline.track(track_id) else {
                continue;
            };
            let mut expected: Option<Timestamp> = None;
            let mut gap: Option<i64> = None;
            for clip in &track.clips {
                match (expected, gap) {
                    (Some(previous_end), None) if clip.timeline_start > previous_end => {
                        // The first hole at or after `at`: everything from here
                        // on moves left by its size.
                        if previous_end >= at || clip.timeline_start >= at {
                            gap = Some((clip.timeline_start - previous_end).as_micros());
                        }
                        if let Some(shift) = gap {
                            commands.push(miniter_usecases::EditCommand::MoveClip {
                                clip_id: clip.id,
                                new_track_id: track_id,
                                new_start: Timestamp::from_micros(
                                    (clip.timeline_start.as_micros() - shift).max(0),
                                ),
                            });
                        }
                        expected = Some(clip.timeline_end());
                    }
                    _ => {
                        if let Some(shift) = gap {
                            commands.push(miniter_usecases::EditCommand::MoveClip {
                                clip_id: clip.id,
                                new_track_id: track_id,
                                new_start: Timestamp::from_micros(
                                    (clip.timeline_start.as_micros() - shift).max(0),
                                ),
                            });
                        }
                        expected = Some(clip.timeline_end());
                    }
                }
            }
        }

        if commands.is_empty() {
            return Err(EngineError::Other("there is no gap to close".into()));
        }
        apply_batch(s, commands, "Close Gap").await
    })
}

/// Split every clip under the playhead (or only the named/selected ones).
fn run_razor<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let fps = s.fps();
        let playhead = s.services().project.current_playhead().await;
        let at = params::time_at(p, "at", fps)
            .or_else(|| params::time_p(p, fps))
            .unwrap_or(playhead);
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let named = target_clips(s, p, "timeline.razor")
            .await
            .unwrap_or_default();
        let mut commands: Vec<miniter_usecases::EditCommand> = Vec::new();
        for track in &timeline.tracks {
            for clip in &track.clips {
                if !named.is_empty() && !named.contains(&clip.id) {
                    continue;
                }
                if clip.timeline_start < at && clip.timeline_end() > at {
                    commands.push(miniter_usecases::EditCommand::SplitClip {
                        clip_id: clip.id,
                        at,
                        new_clip_id: ClipId::new(),
                    });
                }
            }
        }
        if commands.is_empty() {
            return Err(EngineError::Other("no clip is under the playhead".into()));
        }
        apply_batch(s, commands, "Razor").await
    })
}

fn delegate<'a>(s: &'a Session, p: &'a Value, id: &'static str) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = edit_of(s, id, p)
            .await?
            .ok_or_else(|| EngineError::bad(id, "not an edit command"))?;
        apply_edit(s, edit.command, edit.label).await
    })
}

/// A transition in `{"kind":string,"duration":time}` form, or none at all.
fn transition_p(p: &Value, _cmd: &str) -> Result<Option<Transition>> {
    let Some(raw) = p.get("transition").filter(|v| !v.is_null()) else {
        return Ok(None);
    };
    let kind = match raw
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("dissolve")
        .to_ascii_lowercase()
        .as_str()
    {
        "crossfade" | "cross_fade" => miniter_domain::TransitionKind::CrossFade,
        "slideleft" | "slide_left" => miniter_domain::TransitionKind::SlideLeft,
        "slideright" | "slide_right" => miniter_domain::TransitionKind::SlideRight,
        _ => miniter_domain::TransitionKind::Dissolve,
    };
    let duration = transition_duration(raw.get("duration"));
    Ok(Some(Transition::new(kind, duration)))
}

/// A transition duration: `{"us":…}`, `{"ms":…}`, `{"seconds":…}` or a plain
/// number of seconds; one second when the caller gave none.
fn transition_duration(value: Option<&Value>) -> MediaDuration {
    let Some(value) = value else {
        return MediaDuration::from_secs(1.0);
    };
    if let Some(us) = value.get("us").and_then(Value::as_f64) {
        return MediaDuration::from_micros(us as i64);
    }
    if let Some(ms) = value.get("ms").and_then(Value::as_f64) {
        // MediaDuration has no from_millis; Timestamp's is the shared one.
        return MediaDuration::from_micros(Timestamp::from_millis(ms).as_micros());
    }
    if let Some(seconds) = value.get("seconds").and_then(Value::as_f64) {
        return MediaDuration::from_secs(seconds);
    }
    value
        .as_f64()
        .map(MediaDuration::from_secs)
        .unwrap_or_else(|| MediaDuration::from_secs(1.0))
}

/// The material a three-point edit places: a media asset, or a copy of an
/// existing clip.
async fn material_of(s: &Session, p: &Value, cmd: &str) -> Result<(Clip, TrackKind)> {
    if let Some(clip_id) = params::clip_p(p, "clip") {
        let timeline = s
            .services()
            .project
            .current_timeline()
            .await
            .ok_or(EngineError::NoProject)?;
        let (track_id, clip) = timeline
            .tracks
            .iter()
            .find_map(|t| t.clip_by_id(clip_id).map(|c| (t.id, c.clone())))
            .ok_or_else(|| EngineError::bad(cmd, "clip not found"))?;
        let kind = timeline
            .track(track_id)
            .map(|t| t.kind)
            .unwrap_or(TrackKind::Video);
        let mut copy = clip;
        copy.id = ClipId::new();
        return Ok((copy, kind));
    }
    let asset_id = params::require_uuid(p, "asset", cmd)?;
    let asset = s
        .services()
        .project
        .list_assets()
        .await
        .into_iter()
        .find(|a| a.id.0 == asset_id)
        .ok_or_else(|| EngineError::bad(cmd, "no such asset"))?;
    let clip = clip_from_asset(
        &asset,
        Timestamp::ZERO,
        MediaDuration::ZERO,
        None,
        params::f64_p(p, "speed").unwrap_or(1.0),
    )?;
    Ok((clip, track_kind_of_asset(&asset)))
}

/// Queue a clip move, remembering where it lands so moves can be ordered.
fn push_move(
    moves: &mut Vec<(i64, miniter_usecases::EditCommand)>,
    clip_id: ClipId,
    track_id: TrackId,
    from: Timestamp,
    by: MediaDuration,
) {
    let new_start = Timestamp::from_micros((from.as_micros() + by.as_micros()).max(0));
    moves.push((
        new_start.as_micros(),
        miniter_usecases::EditCommand::MoveClip {
            clip_id,
            new_track_id: track_id,
            new_start,
        },
    ));
}

/// Place material at `at` on a track. `ripple` pushes what follows out of the
/// way; without it, whatever was there is replaced.
async fn place_at(s: &Session, p: &Value, cmd: &str, ripple: bool) -> Result<EditSpec> {
    let (mut clip, kind) = material_of(s, p, cmd).await?;
    let fps = s.fps();
    let playhead = s.services().project.current_playhead().await;
    let at = params::time_at(p, "at", fps)
        .or_else(|| params::time_p(p, fps))
        .unwrap_or(playhead);
    let length = clip.timeline_duration;
    clip.timeline_start = at;

    let timeline = s
        .services()
        .project
        .current_timeline()
        .await
        .ok_or(EngineError::NoProject)?;
    let track_id = ensure_track(s, kind, params::track_p(p, "track")).await?;
    let end = at + length;
    let mut commands: Vec<miniter_usecases::EditCommand> = Vec::new();
    // Moves are applied last and right to left: a clip that slides into a slot
    // its former neighbour still occupies would otherwise be rejected.
    let mut moves: Vec<(i64, miniter_usecases::EditCommand)> = Vec::new();

    if let Some(track) = timeline.track(track_id) {
        for existing in &track.clips {
            let existing_end = existing.timeline_end();
            if existing_end <= at {
                // Entirely before the edit: untouched.
                continue;
            }
            if existing.timeline_start < at {
                // Cut in two by the edit point.
                if ripple {
                    // The head stays, the right half shifts aside.
                    let right = ClipId::new();
                    commands.push(miniter_usecases::EditCommand::SplitClip {
                        clip_id: existing.id,
                        at,
                        new_clip_id: right,
                    });
                    moves.push((
                        end.as_micros(),
                        miniter_usecases::EditCommand::MoveClip {
                            clip_id: right,
                            new_track_id: track_id,
                            new_start: end,
                        },
                    ));
                    continue;
                }
                // Overwrite: keep the head, and keep whatever of the tail sits
                // past the window with its source advanced past what was lost.
                if existing_end > end {
                    let tail = ClipId::new();
                    commands.push(miniter_usecases::EditCommand::SplitClip {
                        clip_id: existing.id,
                        at: end,
                        new_clip_id: tail,
                    });
                    commands.push(miniter_usecases::EditCommand::TrimClipEnd {
                        clip_id: existing.id,
                        new_duration: MediaDuration::from_micros(
                            (at - existing.timeline_start).as_micros(),
                        ),
                    });
                    commands.push(miniter_usecases::EditCommand::TrimClipStart {
                        clip_id: tail,
                        new_start: end,
                        new_source_start: MediaDuration::from_micros(
                            (existing.source_start.as_micros()
                                + ((end - existing.timeline_start).as_micros() as f64
                                    / existing.speed) as i64)
                                .max(0),
                        ),
                    });
                } else {
                    commands.push(miniter_usecases::EditCommand::TrimClipEnd {
                        clip_id: existing.id,
                        new_duration: MediaDuration::from_micros(
                            (at - existing.timeline_start).as_micros(),
                        ),
                    });
                }
                continue;
            }
            if existing.timeline_start >= end {
                // Entirely after the edit.
                if ripple {
                    push_move(
                        &mut moves,
                        existing.id,
                        track_id,
                        existing.timeline_start,
                        length,
                    );
                }
                continue;
            }
            // Starts inside the window.
            if ripple {
                push_move(
                    &mut moves,
                    existing.id,
                    track_id,
                    existing.timeline_start,
                    length,
                );
                continue;
            }
            if existing_end > end {
                commands.push(miniter_usecases::EditCommand::TrimClipStart {
                    clip_id: existing.id,
                    new_start: end,
                    new_source_start: MediaDuration::from_micros(
                        (existing.source_start.as_micros()
                            + ((end - existing.timeline_start).as_micros() as f64 / existing.speed)
                                as i64)
                            .max(0),
                    ),
                });
            } else {
                commands.push(miniter_usecases::EditCommand::RemoveClip {
                    clip_id: existing.id,
                });
            }
        }
    }

    // Right to left, so no move lands on a neighbour that has not moved yet.
    moves.sort_by(|a, b| b.0.cmp(&a.0));
    commands.extend(moves.into_iter().map(|(_, command)| command));
    commands.push(miniter_usecases::EditCommand::AddClip { track_id, clip });
    let label = if ripple { "Insert" } else { "Overwrite" };
    Ok(EditSpec::new(
        miniter_usecases::EditCommand::Batch {
            label: label.to_string(),
            commands,
        },
        label,
    ))
}

fn run_insert<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = place_at(s, p, "timeline.insert", true).await?;
        apply_edit(s, edit.command, edit.label).await
    })
}

fn run_overwrite<'a>(s: &'a Session, p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move {
        let edit = place_at(s, p, "timeline.overwrite", false).await?;
        apply_edit(s, edit.command, edit.label).await
    })
}

fn run_inspect<'a>(s: &'a Session, _p: &'a Value) -> RunFuture<'a> {
    Box::pin(async move { Ok(crate::inspect::timeline_json(s).await) })
}
