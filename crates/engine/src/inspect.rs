//! JSON views of the open document: project, timeline, clips, assets, jobs.
//!
//! Ids serialize as strings and every time carries microseconds plus seconds,
//! so an agent never has to guess a unit.

use miniter_domain::{ClipKind, Timeline, Timestamp, TrackKind};
use serde_json::{Value, json};
use snapshort_usecases::{Asset, AssetId};

use crate::session::{Services, Session};

fn ts(t: Timestamp) -> Value {
    json!({ "us": t.as_micros(), "seconds": t.as_secs_f64() })
}

fn id(uuid: uuid::Uuid) -> String {
    uuid.to_string()
}

fn track_kind(kind: TrackKind) -> &'static str {
    match kind {
        TrackKind::Video => "video",
        TrackKind::Audio => "audio",
        TrackKind::Text => "text",
        TrackKind::Subtitle => "subtitle",
        _ => "unknown",
    }
}

fn clip_kind(kind: &ClipKind) -> Value {
    match kind {
        ClipKind::Video(v) => json!({
            "type": "video",
            "sourcePath": v.source_path,
            "width": v.width,
            "height": v.height,
            "fps": v.fps,
            "filters": v.filters.len(),
            "audioFilters": v.audio_filters.len(),
            "masks": v.masks.len(),
        }),
        ClipKind::Audio(a) => json!({
            "type": "audio",
            "sourcePath": a.source_path,
            "sampleRate": a.sample_rate,
            "channels": a.channels,
            "filters": a.filters.len(),
        }),
        ClipKind::Text(t) => json!({
            "type": "text",
            "text": t.text,
            "style": t.style,
        }),
        ClipKind::Subtitle(s) => json!({
            "type": "subtitle",
            "sourcePath": s.source_path,
            "fontPath": s.font_path,
        }),
        _ => json!({ "type": "unknown" }),
    }
}

fn clip_json(clip: &miniter_domain::Clip) -> Value {
    json!({
        "id": id(clip.id.0),
        "start": ts(clip.timeline_start),
        "duration": ts(Timestamp::from_micros(clip.timeline_duration.as_micros())),
        "end": ts(clip.timeline_end()),
        "sourceStart": ts(Timestamp::from_micros(clip.source_start.as_micros())),
        "sourceEnd": ts(Timestamp::from_micros(clip.source_end.as_micros())),
        "speed": clip.speed,
        "volume": clip.volume,
        "opacity": clip.opacity,
        "muted": clip.muted,
        "blendMode": clip.blend_mode,
        "transitionIn": clip.transition_in,
        "transitionOut": clip.transition_out,
        "keyframes": clip.keyframes,
        "kind": clip_kind(&clip.kind),
    })
}

pub fn timeline_json_from(timeline: &Timeline) -> Value {
    let tracks: Vec<Value> = timeline
        .tracks
        .iter()
        .map(|t| {
            json!({
                "id": id(t.id.0),
                "name": t.name,
                "kind": track_kind(t.kind),
                "muted": t.muted,
                "locked": t.locked,
                "clips": t.clips.iter().map(clip_json).collect::<Vec<_>>(),
            })
        })
        .collect();
    json!({
        "tracks": tracks,
        "duration": ts(timeline.duration_end()),
        "clipCount": timeline.tracks.iter().map(|t| t.clips.len()).sum::<usize>(),
    })
}

pub async fn timeline_json(session: &Session) -> Value {
    match session.services().project.current_timeline().await {
        Some(timeline) => timeline_json_from(&timeline),
        None => json!({ "tracks": [], "duration": ts(Timestamp::ZERO), "clipCount": 0 }),
    }
}

pub async fn project_json(session: &Session) -> Value {
    let services = session.services();
    let project = services.project.current_project().await;
    let (project_value, name, path) = match project {
        Some(project) => {
            let name = project.meta.name.clone();
            let value = serde_json::to_value(&project).unwrap_or(Value::Null);
            (value, name, services.project.current_path().await)
        }
        None => (Value::Null, String::new(), None),
    };
    json!({
        "ok": true,
        "name": name,
        "path": path.map(|p| p.display().to_string()),
        "project": project_value,
    })
}

pub async fn assets_json(session: &Session) -> Value {
    let mut assets = session.services().project.list_assets().await;
    assets.sort_by_key(|a| a.imported_at);
    let items: Vec<Value> = assets.iter().map(asset_json).collect();
    json!({ "assets": items, "count": items.len() })
}

pub fn asset_json(asset: &Asset) -> Value {
    json!({
        "id": id(asset.id.0),
        "name": asset.name,
        "path": asset.path.display().to_string(),
        "effectivePath": asset.effective_path().display().to_string(),
        "type": match asset.asset_type {
            snapshort_usecases::AssetType::Video => "video",
            snapshort_usecases::AssetType::Audio => "audio",
            snapshort_usecases::AssetType::Image => "image",
            snapshort_usecases::AssetType::Sequence => "sequence",
        },
        "status": format!("{:?}", asset.status),
        "usable": asset.status.is_usable(),
        "offline": matches!(asset.status, snapshort_usecases::AssetStatus::Offline),
        "error": asset.status.error_message(),
        "proxy": asset.proxy.as_ref().map(|p| json!({
            "path": p.path.display().to_string(),
            "width": p.width,
            "height": p.height,
            "codec": p.codec,
        })),
        "mediaInfo": asset.media_info.as_ref().map(media_info_json),
        "tags": asset.tags,
        "rating": asset.rating,
        "notes": asset.notes,
        "markers": asset.markers.iter().map(|m| json!({
            "frame": m.frame,
            "label": m.label,
            "color": m.color,
        })).collect::<Vec<_>>(),
    })
}

pub fn media_info_json(info: &snapshort_infra_media::MediaInfo) -> Value {
    json!({
        "container": info.container,
        "durationMs": info.duration_ms,
        "fileSize": info.file_size,
        "video": info.primary_video().map(|v| json!({
            "codec": v.codec_name,
            "profile": v.codec_profile,
            "bitDepth": v.bit_depth,
            "chroma": v.chroma_subsampling,
            "width": v.width,
            "height": v.height,
            "fps": v.fps,
            "frames": v.duration_frames,
            "pixelFormat": v.pixel_format,
            "colorSpace": v.color_space,
            "hdr": v.hdr,
        })),
        "audio": info.primary_audio().map(|a| json!({
            "codec": a.codec_name,
            "sampleRate": a.sample_rate,
            "channels": a.channels,
            "durationSamples": a.duration_samples,
        })),
    })
}

pub async fn jobs_json(services: &Services) -> Value {
    let jobs = services.jobs.list().await;
    json!({
        "jobs": jobs.iter().map(|j| json!({
            "id": j.id.to_string(),
            "kind": j.kind,
            "status": j.status.as_str(),
            "progress": j.progress,
            "error": j.error,
            "createdAt": j.created_at,
            "updatedAt": j.updated_at,
        })).collect::<Vec<_>>(),
    })
}

pub async fn playback_json(session: &Session) -> Value {
    let services = session.services();
    json!({
        "state": format!("{:?}", services.playback.state().await),
        "playhead": ts(services.project.current_playhead().await),
        "fps": session.fps(),
        "preferProxy": services.preview.prefer_proxy(),
    })
}

pub async fn selection_json(session: &Session) -> Value {
    let selection = session.selection();
    json!({
        "clips": selection.clips.iter().map(|c| id(c.0)).collect::<Vec<_>>(),
        "tracks": selection.tracks.iter().map(|t| id(t.0)).collect::<Vec<_>>(),
        "range": selection.time_range.map(|r| json!({
            "start": ts(r.start),
            "end": ts(r.end),
        })),
    })
}

pub async fn undo_json(session: &Session) -> Value {
    let guard = session.services().project.editor().await;
    let guard = guard.read().await;
    match guard.as_ref() {
        Some(editor) => json!({
            "canUndo": editor.history.can_undo(),
            "canRedo": editor.history.can_redo(),
            "undoLabel": editor.history.undo_label(),
            "redoLabel": editor.history.redo_label(),
            "undoDepth": editor.history.undo_depth(),
            "redoDepth": editor.history.redo_depth(),
        }),
        None => json!({
            "canUndo": false,
            "canRedo": false,
            "undoDepth": 0,
            "redoDepth": 0,
        }),
    }
}

/// The whole document in one call: what `doc_inspect` and the CLI's
/// `inspect` return.
pub async fn document_json(session: &Session) -> Value {
    json!({
        "project": project_json(session).await,
        "timeline": timeline_json(session).await,
        "assets": assets_json(session).await,
        "selection": selection_json(session).await,
        "playback": playback_json(session).await,
        "history": undo_json(session).await,
        "jobs": jobs_json(session.services()).await,
        "settings": settings_json(session).await,
    })
}

pub async fn settings_json(session: &Session) -> Value {
    let services = session.services();
    let policy = services.assets.proxy_policy().await;
    json!({
        "fps": session.fps(),
        "preferProxy": services.preview.prefer_proxy(),
        "autoProxy": policy.auto_generate,
        "proxyMinWidth": policy.min_width,
        "hardwareAccel": services.render.is_hardware_accel_available(),
        "formats": ["mp4-h264", "mp4-h265", "webm-vp9", "mov-prores", "png-sequence", "jpeg-sequence"],
        "qualities": ["draft", "preview", "standard", "high", "master"],
    })
}

pub fn asset_id_from(value: &str) -> Option<AssetId> {
    uuid::Uuid::parse_str(value).ok().map(AssetId)
}
