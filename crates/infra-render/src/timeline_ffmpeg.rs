use crate::{RenderError, RenderResult, RenderSettings, OutputFormat, QualityPreset};
use miniter_domain::{ClipKind, Timeline, Timestamp, TrackKind};
use std::process::Command;

#[derive(Clone)]
struct PreparedClip {
    source_path: String,
    track_kind: TrackKind,
    output_start_us: i64,
    output_duration_us: i64,
    source_start_us: i64,
    source_duration_us: i64,
    speed: f64,
    reversed: bool,
    volume: f32,
    opacity: f32,
    position: (f32, f32),
    is_image: bool,
    has_audio: bool,
}

#[derive(Clone)]
struct PreviewClip {
    source_path: String,
    source_seek_seconds: f64,
    speed: f64,
    reversed: bool,
    opacity: f32,
    position: (f32, f32),
    is_image: bool,
}

pub(crate) fn export_timeline(
    timeline: &Timeline,
    settings: RenderSettings,
) -> Result<RenderResult, RenderError> {
    ensure_ffmpeg_available()?;

    if !matches!(settings.format, OutputFormat::Mp4H264) {
        return Err(RenderError::CodecNotAvailable("Only MP4 H.264 export is currently supported".into()));
    }

    let total_duration_us = timeline_duration_us(timeline).max(1);
    let total_seconds = total_duration_us as f64 / 1_000_000.0;

    let prepared = prepare_clips(timeline);
    let video_clips: Vec<_> = prepared.iter().filter(|c| c.track_kind == TrackKind::Video).cloned().collect();
    let audio_clips: Vec<_> = prepared.iter().filter(|c| c.has_audio).cloned().collect();

    if let Some(parent) = settings.output_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RenderError::IoError(e.to_string()))?;
    }

    let mut cmd = ffmpeg_command();
    cmd.arg("-f").arg("lavfi")
        .arg("-i").arg(format!(
            "color=c=black:s={}x{}:r={:.6}:d={:.6}",
            settings.resolution.0, settings.resolution.1, settings.fps, total_seconds
        ))
        .arg("-f").arg("lavfi")
        .arg("-i").arg(format!(
            "anullsrc=channel_layout=stereo:sample_rate=48000:d={:.6}",
            total_seconds
        ));

    let mut next_input_index = 2usize;
    let mut video_input_indices = Vec::new();
    let mut audio_input_indices = Vec::new();

    for clip in &video_clips {
        append_clip_input(&mut cmd, clip);
        video_input_indices.push(next_input_index);
        next_input_index += 1;
    }

    for clip in &audio_clips {
        append_audio_input(&mut cmd, clip);
        audio_input_indices.push(next_input_index);
        next_input_index += 1;
    }

    let filter = build_export_filter(
        &video_clips, &video_input_indices,
        &audio_clips, &audio_input_indices,
        &settings,
    );

    let (preset, crf) = quality_profile(settings.quality);
    let output = cmd
        .arg("-filter_complex").arg(filter)
        .arg("-map").arg("[vout]")
        .arg("-map").arg("[aout]")
        .arg("-r").arg(format!("{:.6}", settings.fps))
        .arg("-c:v").arg("libx264")
        .arg("-preset").arg(preset)
        .arg("-crf").arg(crf)
        .arg("-pix_fmt").arg("yuv420p")
        .arg("-movflags").arg("+faststart")
        .arg("-c:a").arg("aac")
        .arg("-b:a").arg(format!("{}k", settings.audio_bitrate.max(192)))
        .arg(&settings.output_path)
        .output()
        .map_err(|e| RenderError::IoError(e.to_string()))?;

    if !output.status.success() {
        return Err(RenderError::EncodingError(
            String::from_utf8_lossy(&output.stderr).trim().to_string(),
        ));
    }

    let file_size = std::fs::metadata(&settings.output_path)
        .map(|m| m.len()).unwrap_or(0);

    Ok(RenderResult {
        output_path: settings.output_path,
        render_time_seconds: 0.0,
        file_size_bytes: file_size,
    })
}

pub(crate) fn render_preview_frame(
    timeline: &Timeline,
    frame: Timestamp,
) -> Result<Vec<u8>, RenderError> {
    ensure_ffmpeg_available()?;

    let active = prepare_preview_clips(timeline, frame);

    let mut cmd = ffmpeg_command();
    cmd.arg("-f").arg("lavfi").arg("-i").arg(format!(
        "color=c=black:s={}x{}:r=30:d=1",
        1920, 1080,
    ));

    for clip in &active {
        if clip.is_image {
            cmd.arg("-loop").arg("1").arg("-i").arg(&clip.source_path);
        } else {
            cmd.arg("-ss").arg(format!("{:.6}", clip.source_seek_seconds.max(0.0)))
                .arg("-i").arg(&clip.source_path);
        }
    }

    let filter = build_preview_filter(&active);
    let output = cmd
        .arg("-filter_complex").arg(filter)
        .arg("-map").arg("[vout]")
        .arg("-frames:v").arg("1")
        .arg("-f").arg("image2pipe")
        .arg("-vcodec").arg("png")
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

fn prepare_clips(timeline: &Timeline) -> Vec<PreparedClip> {
    let mut prepared = Vec::new();

    for track in &timeline.tracks {
        if track.locked || track.muted {
            continue;
        }
        for clip in &track.clips {
            if clip.muted {
                continue;
            }

            let source_path = match &clip.kind {
                ClipKind::Video(v) => v.source_path.clone(),
                ClipKind::Audio(a) => a.source_path.clone(),
                _ => continue,
            };

            let has_audio = match &clip.kind {
                ClipKind::Video(_) => true,
                ClipKind::Audio(_) => true,
                _ => false,
            };

            prepared.push(PreparedClip {
                source_path,
                track_kind: track.kind,
                output_start_us: clip.timeline_start.as_micros(),
                output_duration_us: clip.timeline_duration.as_micros(),
                source_start_us: clip.source_start.as_micros(),
                source_duration_us: clip.source_duration().as_micros(),
                speed: clip.speed.clamp(0.1, 10.0),
                reversed: false,
                volume: clip.volume.clamp(0.0, 2.0),
                opacity: clip.opacity.clamp(0.0, 1.0),
                position: (0.0, 0.0),
                is_image: false,
                has_audio,
            });
        }
    }

    prepared.sort_by_key(|c| c.output_start_us);
    prepared
}

fn prepare_preview_clips(timeline: &Timeline, frame: Timestamp) -> Vec<PreviewClip> {
    let mut clips = Vec::new();

    for track in &timeline.tracks {
        if track.kind != TrackKind::Video || track.muted {
            continue;
        }
        if let Some(clip) = track.clip_at(frame) {
            if clip.muted {
                continue;
            }

            let seek_seconds = if let ClipKind::Video(v) = &clip.kind {
                let local_offset = frame - clip.timeline_start;
                let source_seek_us = clip.source_start.as_micros()
                    + (local_offset.as_micros() as f64 * clip.speed) as i64;
                source_seek_us as f64 / 1_000_000.0
            } else {
                continue;
            };

            let source_path = match &clip.kind {
                ClipKind::Video(v) => v.source_path.clone(),
                _ => continue,
            };

            clips.push(PreviewClip {
                source_path,
                source_seek_seconds: seek_seconds,
                speed: clip.speed.clamp(0.1, 10.0),
                reversed: false,
                opacity: clip.opacity.clamp(0.0, 1.0),
                position: (0.0, 0.0),
                is_image: false,
            });
        }
    }

    clips
}

fn build_preview_filter(clips: &[PreviewClip]) -> String {
    let mut parts = vec!["[0:v]format=rgba[canvas0]".to_string()];
    let mut current = "canvas0".to_string();

    for (idx, clip) in clips.iter().enumerate() {
        let processed = format!("preview{idx}");
        let speed = clip.speed as f32;
        let opacity = clip.opacity;
        let mut filter = "format=rgba".to_string();
        if (speed - 1.0).abs() > f32::EPSILON {
            filter += &format!(",setpts=(PTS-STARTPTS)/{speed:.6}");
        }
        if (opacity - 1.0).abs() > f32::EPSILON {
            filter += &format!(",colorchannelmixer=aa={opacity:.6}");
        }
        parts.push(format!("[{}:v]{}[{}]", idx + 1, filter, processed));

        let next = format!("canvas{}", idx + 1);
        parts.push(format!(
            "[{current}][{processed}]overlay=x='(W-w)/2+{:.3}':y='(H-h)/2+{:.3}':eof_action=pass:format=auto[{next}]",
            clip.position.0, clip.position.1,
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
) -> String {
    let mut parts = vec![
        "[0:v]format=rgba[canvas0]".to_string(),
        "[1:a]anull[audbase]".to_string(),
    ];
    let mut current = "canvas0".to_string();

    for (idx, (clip, input_idx)) in video_clips.iter().zip(video_input_indices.iter()).enumerate() {
        let processed = format!("vclip{idx}");
        let speed = clip.speed as f32;
        let start_seconds = clip.output_start_us as f64 / 1_000_000.0;

        let mut filter = "format=rgba".to_string();
        filter += &format!(",setpts=(PTS-STARTPTS)/{:.6}+{:.6}/TB", speed.max(0.1), start_seconds);
        filter += &format!(",scale={}:{}:force_original_aspect_ratio=decrease:flags=lanczos",
            settings.resolution.0, settings.resolution.1);

        if (clip.opacity - 1.0).abs() > f32::EPSILON {
            filter += &format!(",colorchannelmixer=aa={:.6}", clip.opacity);
        }

        parts.push(format!("[{}:v]{}[{}]", input_idx, filter, processed));

        let next = format!("canvas{}", idx + 1);
        parts.push(format!(
            "[{current}][{processed}]overlay=x='(W-w)/2+{:.3}':y='(H-h)/2+{:.3}':eof_action=pass:format=auto[{next}]",
            clip.position.0, clip.position.1,
        ));
        current = next;
    }

    let mut audio_labels = vec!["[audbase]".to_string()];
    for (idx, (clip, input_idx)) in audio_clips.iter().zip(audio_input_indices.iter()).enumerate() {
        let processed = format!("aclip{idx}");
        let start_seconds = clip.output_start_us as f64 / 1_000_000.0;

        let mut filter = "asetpts=PTS-STARTPTS".to_string();
        filter += ",aresample=48000";

        let speed = clip.speed as f32;
        if (speed - 1.0).abs() > f32::EPSILON {
            filter += &format!(",atempo={:.6}", speed.max(0.5).min(2.0));
        }
        if (clip.volume - 1.0).abs() > f32::EPSILON {
            filter += &format!(",volume={:.6}", clip.volume.max(0.0));
        }

        let delay_ms = (start_seconds * 1000.0).round().max(0.0) as i64;
        filter += &format!(",adelay={delay_ms}|{delay_ms}");

        parts.push(format!("[{}:a]{}[{}]", input_idx, filter, processed));
        audio_labels.push(format!("[{processed}]"));
    }

    parts.push(format!(
        "{}amix=inputs={}:duration=longest:normalize=0[aout]",
        audio_labels.join(""), audio_labels.len()
    ));
    parts.push(format!("[{current}]format=yuv420p[vout]"));
    parts.join(";")
}

fn append_clip_input(cmd: &mut Command, clip: &PreparedClip) {
    let source_duration = clip.source_duration_us as f64 / 1_000_000.0;
    let source_start = clip.source_start_us as f64 / 1_000_000.0;

    if clip.is_image {
        cmd.arg("-loop").arg("1")
            .arg("-t").arg(format!("{:.6}", source_duration))
            .arg("-i").arg(&clip.source_path);
    } else {
        cmd.arg("-ss").arg(format!("{source_start:.6}"))
            .arg("-t").arg(format!("{source_duration:.6}"))
            .arg("-i").arg(&clip.source_path);
    }
}

fn append_audio_input(cmd: &mut Command, clip: &PreparedClip) {
    let source_duration = clip.source_duration_us as f64 / 1_000_000.0;
    let source_start = clip.source_start_us as f64 / 1_000_000.0;

    cmd.arg("-ss").arg(format!("{source_start:.6}"))
        .arg("-t").arg(format!("{source_duration:.6}"))
        .arg("-i").arg(&clip.source_path);
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
    cmd.arg("-y").arg("-hide_banner").arg("-loglevel").arg("error");
    cmd
}

fn ensure_ffmpeg_available() -> Result<(), RenderError> {
    let output = Command::new("ffmpeg").arg("-version")
        .output().map_err(|_| RenderError::CodecNotAvailable("ffmpeg not found in PATH".into()))?;
    if output.status.success() { Ok(()) }
    else { Err(RenderError::CodecNotAvailable("ffmpeg not found in PATH".into())) }
}

fn timeline_duration_us(timeline: &Timeline) -> i64 {
    timeline.duration_end().as_micros().max(0)
}
