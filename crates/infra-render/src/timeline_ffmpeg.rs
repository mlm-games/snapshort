use crate::{
    OutputFormat, QualityPreset, RenderEffects, RenderError, RenderResult, RenderSettings,
};
use snapshort_domain::prelude::*;
use std::path::Path;
use std::process::Command;
use std::time::Instant;

#[derive(Clone)]
struct PreparedClip {
    asset_type: AssetType,
    path: std::path::PathBuf,
    track: TrackRef,
    output_start_frames: i64,
    output_duration_frames: i64,
    source_start_frame: i64,
    source_duration_frames: i64,
    source_fps: Fps,
    effects: RenderEffects,
    has_audio: bool,
}

#[derive(Clone)]
struct PreviewClip {
    path: std::path::PathBuf,
    track: TrackRef,
    asset_type: AssetType,
    source_seek_seconds: f64,
    effects: RenderEffects,
}

pub(crate) fn export_timeline(
    timeline: &Timeline,
    assets: &[Asset],
    settings: RenderSettings,
) -> Result<RenderResult, RenderError> {
    ensure_ffmpeg_available()?;

    if !matches!(settings.format, OutputFormat::Mp4H264) {
        return Err(RenderError::CodecNotAvailable(
            "Only MP4 H.264 export is currently supported".into(),
        ));
    }

    let render_range = settings
        .frame_range
        .unwrap_or_else(|| FrameRange::new_unchecked(0, timeline.duration().0.max(1)));
    let total_frames = render_range.duration().max(1);
    let total_seconds = seconds_from_frames(total_frames, timeline.settings.fps);

    let prepared = prepare_clips(timeline, assets, render_range);
    let video_clips: Vec<_> = prepared
        .iter()
        .filter(|clip| clip_supports_video(clip))
        .cloned()
        .collect();
    let audio_clips: Vec<_> = prepared
        .iter()
        .filter(|clip| clip.has_audio)
        .cloned()
        .collect();

    if let Some(parent) = settings.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
    }

    let mut cmd = ffmpeg_command();
    cmd.arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(format!(
            "color=c=black:s={}x{}:r={:.6}:d={:.6}",
            settings.resolution.0, settings.resolution.1, settings.fps, total_seconds
        ))
        .arg("-f")
        .arg("lavfi")
        .arg("-i")
        .arg(format!(
            "anullsrc=channel_layout={}:sample_rate={}:d={:.6}",
            audio_channel_layout(timeline.settings.audio_channels),
            timeline.settings.sample_rate,
            total_seconds
        ));

    let mut next_input_index = 2usize;
    let mut video_input_indices = Vec::new();
    let mut audio_input_indices = Vec::new();

    for clip in &video_clips {
        append_clip_input(&mut cmd, clip, false);
        video_input_indices.push(next_input_index);
        next_input_index += 1;
    }

    for clip in &audio_clips {
        append_clip_input(&mut cmd, clip, true);
        audio_input_indices.push(next_input_index);
        next_input_index += 1;
    }

    let filter = build_export_filter(
        &video_clips,
        &video_input_indices,
        &audio_clips,
        &audio_input_indices,
        &settings,
        timeline,
    );

    let (preset, crf) = quality_profile(settings.quality);
    let mut exec = cmd;
    exec.arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[vout]")
        .arg("-map")
        .arg("[aout]")
        .arg("-r")
        .arg(format!("{:.6}", settings.fps))
        .arg("-c:v")
        .arg("libx264")
        .arg("-preset")
        .arg(preset)
        .arg("-crf")
        .arg(crf)
        .arg("-pix_fmt")
        .arg("yuv420p")
        .arg("-movflags")
        .arg("+faststart")
        .arg("-c:a")
        .arg("aac")
        .arg("-b:a")
        .arg(format!("{}k", settings.audio_bitrate.max(192)))
        .arg(&settings.output_path);

    let started = Instant::now();
    let output = exec
        .output()
        .map_err(|e| RenderError::IoError(e.to_string()))?;
    if !output.status.success() {
        return Err(RenderError::EncodingError(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let file_size = std::fs::metadata(&settings.output_path)
        .map(|m| m.len())
        .unwrap_or(0);

    Ok(RenderResult {
        output_path: settings.output_path,
        render_time_seconds: started.elapsed().as_secs_f64(),
        file_size_bytes: file_size,
    })
}

pub(crate) fn render_preview_frame(
    timeline: &Timeline,
    assets: &[Asset],
    frame: Frame,
) -> Result<Vec<u8>, RenderError> {
    ensure_ffmpeg_available()?;

    let active = prepare_preview_clips(timeline, assets, frame);
    let resolution = timeline.settings.resolution;

    let mut cmd = ffmpeg_command();
    cmd.arg("-f").arg("lavfi").arg("-i").arg(format!(
        "color=c=black:s={}x{}:r={:.6}:d=1",
        resolution.width,
        resolution.height,
        timeline.settings.fps.as_f64()
    ));

    for clip in &active {
        match clip.asset_type {
            AssetType::Image => {
                cmd.arg("-loop").arg("1").arg("-i").arg(&clip.path);
            }
            AssetType::Video | AssetType::Sequence => {
                cmd.arg("-ss")
                    .arg(format!("{:.6}", clip.source_seek_seconds.max(0.0)))
                    .arg("-i")
                    .arg(&clip.path);
            }
            AssetType::Audio => {}
        }
    }

    let filter = build_preview_filter(&active, resolution);
    let output = cmd
        .arg("-filter_complex")
        .arg(filter)
        .arg("-map")
        .arg("[vout]")
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("image2pipe")
        .arg("-vcodec")
        .arg("png")
        .arg("-")
        .output()
        .map_err(|e| RenderError::IoError(e.to_string()))?;

    if !output.status.success() {
        return Err(RenderError::EncodingError(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    Ok(output.stdout)
}

fn prepare_clips(
    timeline: &Timeline,
    assets: &[Asset],
    render_range: FrameRange,
) -> Vec<PreparedClip> {
    let mut prepared = Vec::new();
    for clip in timeline.clips.iter().filter(|clip| clip.enabled) {
        let Some(asset_id) = clip.asset_id else {
            continue;
        };
        let Some(asset) = assets.iter().find(|asset| asset.id == asset_id) else {
            continue;
        };
        if !asset.status.is_usable() {
            continue;
        }
        if let Some(item) = prepare_clip(clip, asset, timeline.settings.fps, render_range) {
            prepared.push(item);
        }
    }

    prepared.sort_by_key(|clip| match clip.track.track_type {
        TrackType::Video => (0u8, clip.track.index, clip.output_start_frames),
        TrackType::Audio => (1u8, clip.track.index, clip.output_start_frames),
    });
    prepared
}

fn prepare_clip(
    clip: &Clip,
    asset: &Asset,
    timeline_fps: Fps,
    render_range: FrameRange,
) -> Option<PreparedClip> {
    let clip_range = clip.timeline_range();
    if !clip_range.overlaps(&render_range) && clip_range.start != render_range.start {
        return None;
    }

    let visible_start = clip.timeline_start.0.max(render_range.start.0);
    let visible_end = clip.timeline_end().0.min(render_range.end.0);
    if visible_end <= visible_start {
        return None;
    }

    let speed = clip.effects.speed.clamp(0.1, 10.0) as f64;
    let timeline_offset = visible_start - clip.timeline_start.0;
    let visible_timeline_frames = visible_end - visible_start;
    let source_shift_frames = ((timeline_offset as f64) * speed).round() as i64;
    let mut visible_source_frames = ((visible_timeline_frames as f64) * speed).round() as i64;
    if visible_source_frames <= 0 {
        visible_source_frames = 1;
    }

    let source_start_frame = if clip.effects.reverse {
        clip.source_range.end.0 - source_shift_frames - visible_source_frames
    } else {
        clip.source_range.start.0 + source_shift_frames
    }
    .clamp(
        clip.source_range.start.0,
        clip.source_range.end.0.saturating_sub(1),
    );

    let max_source = clip.source_range.end.0.saturating_sub(source_start_frame);
    let source_duration_frames = visible_source_frames.min(max_source).max(1);

    Some(PreparedClip {
        asset_type: asset.asset_type,
        path: asset.effective_path().clone(),
        track: clip.track,
        output_start_frames: visible_start - render_range.start.0,
        output_duration_frames: visible_timeline_frames.max(1),
        source_start_frame,
        source_duration_frames,
        source_fps: asset
            .media_info
            .as_ref()
            .and_then(|info| info.fps())
            .unwrap_or(timeline_fps),
        effects: RenderEffects::from(&clip.effects),
        has_audio: asset
            .media_info
            .as_ref()
            .and_then(|info| info.primary_audio())
            .is_some(),
    })
}

fn prepare_preview_clips(timeline: &Timeline, assets: &[Asset], frame: Frame) -> Vec<PreviewClip> {
    let mut clips = Vec::new();
    for clip in timeline
        .clips_at_frame(frame)
        .filter(|clip| clip.enabled && clip.track.track_type == TrackType::Video)
    {
        let Some(asset_id) = clip.asset_id else {
            continue;
        };
        let Some(asset) = assets.iter().find(|asset| asset.id == asset_id) else {
            continue;
        };
        if !asset.status.is_usable() {
            continue;
        }
        if matches!(asset.asset_type, AssetType::Audio) {
            continue;
        }

        let source_fps = asset
            .media_info
            .as_ref()
            .and_then(|info| info.fps())
            .unwrap_or(timeline.settings.fps);
        let offset_frames = (frame.0 - clip.timeline_start.0).max(0);
        let source_offset =
            ((offset_frames as f64) * clip.effects.speed.clamp(0.1, 10.0) as f64).round() as i64;
        let source_frame = if clip.effects.reverse {
            clip.source_range.end.0 - 1 - source_offset
        } else {
            clip.source_range.start.0 + source_offset
        }
        .clamp(
            clip.source_range.start.0,
            clip.source_range.end.0.saturating_sub(1),
        );

        clips.push(PreviewClip {
            path: asset.effective_path().clone(),
            track: clip.track,
            asset_type: asset.asset_type,
            source_seek_seconds: seconds_from_frames(source_frame, source_fps),
            effects: RenderEffects::from(&clip.effects),
        });
    }

    clips.sort_by_key(|clip| clip.track.index);
    clips
}

fn build_preview_filter(clips: &[PreviewClip], resolution: Resolution) -> String {
    let mut parts = vec!["[0:v]format=rgba[canvas0]".to_string()];
    let mut current = "canvas0".to_string();

    for (idx, clip) in clips.iter().enumerate() {
        let processed = format!("preview{idx}");
        parts.push(format!(
            "[{}:v]{}[{}]",
            idx + 1,
            video_filter_chain(&clip.effects, (resolution.width, resolution.height), None),
            processed
        ));
        let next = format!("canvas{}", idx + 1);
        parts.push(format!(
            "[{current}][{processed}]overlay=x='(W-w)/2+{:.3}':y='(H-h)/2+{:.3}':eof_action=pass:format=auto[{next}]",
            clip.effects.transform.position.0,
            clip.effects.transform.position.1,
        ));
        current = next;
    }

    parts.push(format!("[{current}]format=rgba[vout]"));
    parts.join(";")
}

fn build_export_filter(
    video_clips: &[PreparedClip],
    video_input_indices: &[usize],
    audio_clips: &[PreparedClip],
    audio_input_indices: &[usize],
    settings: &RenderSettings,
    timeline: &Timeline,
) -> String {
    let mut parts = vec![
        "[0:v]format=rgba[canvas0]".to_string(),
        "[1:a]anull[audbase]".to_string(),
    ];
    let mut current = "canvas0".to_string();

    for (idx, (clip, input_idx)) in video_clips
        .iter()
        .zip(video_input_indices.iter())
        .enumerate()
    {
        let processed = format!("vclip{idx}");
        parts.push(format!(
            "[{}:v]{}[{}]",
            input_idx,
            video_filter_chain(
                &clip.effects,
                settings.resolution,
                Some(seconds_from_frames(
                    clip.output_start_frames,
                    timeline.settings.fps
                )),
            ),
            processed
        ));
        let next = format!("canvas{}", idx + 1);
        parts.push(format!(
            "[{current}][{processed}]overlay=x='(W-w)/2+{:.3}':y='(H-h)/2+{:.3}':eof_action=pass:format=auto[{next}]",
            clip.effects.transform.position.0,
            clip.effects.transform.position.1,
        ));
        current = next;
    }

    let mut audio_labels = vec!["[audbase]".to_string()];
    for (idx, (clip, input_idx)) in audio_clips
        .iter()
        .zip(audio_input_indices.iter())
        .enumerate()
    {
        let processed = format!("aclip{idx}");
        parts.push(format!(
            "[{}:a]{}[{}]",
            input_idx,
            audio_filter_chain(
                &clip.effects,
                seconds_from_frames(clip.output_start_frames, timeline.settings.fps),
                timeline.settings.sample_rate,
                timeline.settings.audio_channels,
            ),
            processed
        ));
        audio_labels.push(format!("[{processed}]"));
    }

    parts.push(format!(
        "{}amix=inputs={}:duration=longest:normalize=0[aout]",
        audio_labels.join(""),
        audio_labels.len()
    ));
    parts.push(format!("[{current}]format=yuv420p[vout]"));
    parts.join(";")
}

fn clip_supports_video(clip: &PreparedClip) -> bool {
    matches!(
        clip.asset_type,
        AssetType::Video | AssetType::Image | AssetType::Sequence
    )
}

fn append_clip_input(cmd: &mut Command, clip: &PreparedClip, audio_only: bool) {
    match clip.asset_type {
        AssetType::Image => {
            cmd.arg("-loop")
                .arg("1")
                .arg("-t")
                .arg(format!(
                    "{:.6}",
                    seconds_from_frames(clip.output_duration_frames, Fps::new(24, 1))
                ))
                .arg("-i")
                .arg(&clip.path);
        }
        AssetType::Video | AssetType::Audio | AssetType::Sequence => {
            let fps = if audio_only {
                Fps::new(48_000, 1)
            } else {
                clip.source_fps
            };
            cmd.arg("-ss")
                .arg(format!(
                    "{:.6}",
                    seconds_from_frames(clip.source_start_frame, clip.source_fps)
                ))
                .arg("-t")
                .arg(format!(
                    "{:.6}",
                    seconds_from_frames(clip.source_duration_frames, fps)
                ))
                .arg("-i")
                .arg(&clip.path);
        }
    }
}

fn video_filter_chain(
    effects: &RenderEffects,
    resolution: (u32, u32),
    start_seconds: Option<f64>,
) -> String {
    let mut parts = vec!["format=rgba".to_string()];

    if let Some(start_seconds) = start_seconds {
        parts.push(format!(
            "setpts=(PTS-STARTPTS)/{:.6}+{:.6}/TB",
            effects.speed.max(0.1),
            start_seconds
        ));
    } else {
        parts.push("setpts=PTS-STARTPTS".to_string());
    }

    if effects.reverse {
        parts.push("reverse".to_string());
    }

    parts.push(format!(
        "scale={}:{}:force_original_aspect_ratio=decrease:flags=lanczos",
        resolution.0, resolution.1
    ));

    if (effects.transform.scale.0 - 1.0).abs() > f32::EPSILON
        || (effects.transform.scale.1 - 1.0).abs() > f32::EPSILON
    {
        parts.push(format!(
            "scale='max(2,trunc(iw*{:.6}/2)*2)':'max(2,trunc(ih*{:.6}/2)*2)'",
            effects.transform.scale.0.max(0.1),
            effects.transform.scale.1.max(0.1)
        ));
    }

    if effects.transform.flip_horizontal {
        parts.push("hflip".to_string());
    }
    if effects.transform.flip_vertical {
        parts.push("vflip".to_string());
    }

    if effects.transform.rotation_deg.abs() > f32::EPSILON {
        parts.push(format!(
            "rotate={:.6}*PI/180:c=none:ow=rotw(iw):oh=roth(ih)",
            effects.transform.rotation_deg
        ));
    }

    let contrast = (1.0 + effects.color.contrast).clamp(0.0, 2.0);
    let saturation = (1.0 + effects.color.saturation).clamp(0.0, 2.0);
    if effects.color.brightness.abs() > f32::EPSILON
        || (contrast - 1.0).abs() > f32::EPSILON
        || (saturation - 1.0).abs() > f32::EPSILON
    {
        parts.push(format!(
            "eq=brightness={:.6}:contrast={:.6}:saturation={:.6}",
            effects.color.brightness, contrast, saturation
        ));
    }

    if (effects.color.opacity - 1.0).abs() > f32::EPSILON {
        parts.push(format!(
            "colorchannelmixer=aa={:.6}",
            effects.color.opacity.clamp(0.0, 1.0)
        ));
    }

    parts.join(",")
}

fn audio_filter_chain(
    effects: &RenderEffects,
    start_seconds: f64,
    sample_rate: u32,
    channels: u8,
) -> String {
    let mut parts = vec![
        "asetpts=PTS-STARTPTS".to_string(),
        format!("aresample={sample_rate}"),
    ];

    if effects.reverse {
        parts.push("areverse".to_string());
    }

    parts.extend(atempo_filters(effects.speed.max(0.1)));

    if (effects.volume - 1.0).abs() > f32::EPSILON {
        parts.push(format!("volume={:.6}", effects.volume.max(0.0)));
    }

    let delay_ms = (start_seconds * 1000.0).round().max(0.0) as i64;
    let delay = std::iter::repeat_n(delay_ms.to_string(), usize::from(channels.max(1)))
        .collect::<Vec<_>>()
        .join("|");
    parts.push(format!("adelay={delay}"));
    parts.join(",")
}

fn atempo_filters(speed: f32) -> Vec<String> {
    let mut filters = Vec::new();
    let mut remaining = speed.max(0.1) as f64;

    while remaining > 2.0 {
        filters.push("atempo=2.0".to_string());
        remaining /= 2.0;
    }
    while remaining < 0.5 {
        filters.push("atempo=0.5".to_string());
        remaining /= 0.5;
    }
    filters.push(format!("atempo={remaining:.6}"));
    filters
}

fn quality_profile(quality: QualityPreset) -> (&'static str, &'static str) {
    match quality {
        QualityPreset::Draft => ("veryfast", "30"),
        QualityPreset::Preview => ("faster", "27"),
        QualityPreset::Standard => ("medium", "23"),
        QualityPreset::High => ("slow", "20"),
        QualityPreset::Master => ("slow", "17"),
    }
}

fn ffmpeg_command() -> Command {
    let mut cmd = Command::new("ffmpeg");
    cmd.arg("-y")
        .arg("-hide_banner")
        .arg("-loglevel")
        .arg("error");
    cmd
}

fn ensure_ffmpeg_available() -> Result<(), RenderError> {
    let output = Command::new("ffmpeg")
        .arg("-version")
        .output()
        .map_err(|_| RenderError::CodecNotAvailable("ffmpeg not found in PATH".into()))?;
    if output.status.success() {
        Ok(())
    } else {
        Err(RenderError::CodecNotAvailable(
            "ffmpeg not found in PATH".into(),
        ))
    }
}

fn seconds_from_frames(frames: i64, fps: Fps) -> f64 {
    frames.max(0) as f64 / fps.as_f64().max(0.000_001)
}

fn audio_channel_layout(channels: u8) -> &'static str {
    if channels <= 1 {
        "mono"
    } else {
        "stereo"
    }
}

#[allow(dead_code)]
fn is_relative_to(base: &Path, path: &Path) -> bool {
    path.strip_prefix(base).is_ok()
}
