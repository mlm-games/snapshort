use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MediaInfo {
    pub container: String,
    pub duration_ms: u64,
    pub file_size: u64,
    pub video_streams: Vec<VideoStream>,
    pub audio_streams: Vec<AudioStream>,
    pub waveform: Option<Vec<f32>>,
}

impl MediaInfo {
    pub fn primary_video(&self) -> Option<&VideoStream> {
        self.video_streams.first()
    }

    pub fn primary_audio(&self) -> Option<&AudioStream> {
        self.audio_streams.first()
    }

    pub fn fps(&self) -> Option<f64> {
        self.primary_video().map(|v| v.fps)
    }

    pub fn resolution(&self) -> Option<(u32, u32)> {
        self.primary_video().map(|v| (v.width, v.height))
    }

    pub fn duration_frames(&self, fps: f64) -> i64 {
        if fps <= 0.0 {
            return 0;
        }
        ((self.duration_ms as f64 / 1000.0) * fps).round() as i64
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VideoStream {
    pub codec_name: String,
    pub codec_profile: String,
    pub bit_depth: Option<u8>,
    pub chroma_subsampling: Option<String>,
    pub width: u32,
    pub height: u32,
    pub fps: f64,
    pub duration_frames: i64,
    pub pixel_format: String,
    pub color_space: String,
    pub hdr: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AudioStream {
    pub codec_name: String,
    pub codec_profile: String,
    pub bit_depth: Option<u8>,
    pub channels: u16,
    pub sample_rate: u32,
    pub duration_samples: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProxyInfo {
    pub path: PathBuf,
    pub codec: String,
    pub bitrate_kbps: u32,
    pub fps: f64,
    pub width: u32,
    pub height: u32,
    pub created_at: chrono::DateTime<chrono::Utc>,
}

#[derive(Debug, thiserror::Error)]
pub enum MediaError {
    #[error("I/O error: {0}")]
    Io(#[from] std::io::Error),
    #[error("JSON parse error: {0}")]
    Json(#[from] serde_json::Error),
    #[error("External tool failed: {tool}: {message}")]
    ExternalTool { tool: &'static str, message: String },
    #[error("Media file not found: {0}")]
    NotFound(String),
}

#[derive(Debug, Clone, Default)]
pub struct MediaEngine;

impl MediaEngine {
    pub fn probe(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        self.probe_impl(path, true)
    }

    /// Metadata without the full audio decode. Used by `create_proxy`, which
    /// only needs container/video-stream info — decoding the whole track for
    /// a waveform it discards wastes time and spams a symphonia probe per job.
    pub fn probe_without_waveform(&self, path: &Path) -> Result<MediaInfo, MediaError> {
        self.probe_impl(path, false)
    }

    fn probe_impl(&self, path: &Path, with_waveform: bool) -> Result<MediaInfo, MediaError> {
        if !path.exists() {
            return Err(MediaError::NotFound(path.display().to_string()));
        }

        let file_size = std::fs::metadata(path).map(|m| m.len())?;

        let container = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let minfo = miniter_media_native::probe::probe_media(path).map_err(|e| {
            MediaError::ExternalTool {
                tool: "miniter-probe",
                message: e.to_string(),
            }
        })?;

        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();

        let duration_ms = minfo
            .duration_us
            .map(|u| (u as f64 / 1000.0) as u64)
            .unwrap_or(0);

        for vs in &minfo.video_streams {
            video_streams.push(VideoStream {
                codec_name: vs.codec.clone(),
                codec_profile: "unknown".to_string(),
                bit_depth: None,
                chroma_subsampling: None,
                width: vs.width,
                height: vs.height,
                fps: vs.frame_rate,
                duration_frames: if vs.frame_rate > 0.0 {
                    ((duration_ms as f64 / 1000.0) * vs.frame_rate).round() as i64
                } else {
                    0
                },
                pixel_format: "unknown".to_string(),
                color_space: "unknown".to_string(),
                hdr: false,
            });
        }

        for as_ in &minfo.audio_streams {
            audio_streams.push(AudioStream {
                codec_name: as_.codec.clone(),
                codec_profile: "unknown".to_string(),
                bit_depth: None,
                channels: as_.channels as u16,
                sample_rate: as_.sample_rate,
                duration_samples: (duration_ms as f64 / 1000.0 * as_.sample_rate as f64) as u64,
            });
        }

        let mut info = MediaInfo {
            container,
            duration_ms,
            file_size,
            video_streams,
            audio_streams,
            waveform: None,
        };

        if with_waveform && !info.audio_streams.is_empty() {
            if let Ok(waveform) = self.extract_waveform(path) {
                info.waveform = Some(waveform);
            }
        }

        Ok(info)
    }

    /// Build an edit-friendly H.264 proxy with the native Rust stack only
    /// (videoson decode → bilinear scale → less-avc encode → mp4 mux).
    /// No ffmpeg involved. Images and audio-only files are rejected with a
    /// plain message - they are already light enough to edit directly.
    ///
    /// `progress`, when given, receives 1..=99 as frames are written
    /// (invoked only when the percentage actually advances).
    pub fn create_proxy(
        &self,
        asset_id: uuid::Uuid,
        input_path: &Path,
        out_dir: &Path,
        progress: Option<std::sync::Arc<dyn Fn(u8) + Send + Sync>>,
    ) -> Result<ProxyInfo, MediaError> {
        use miniter_media_native::EncodedVideoOutput;
        use miniter_media_native::decoder::VideoDecodeSession;
        use miniter_media_native::encoder::VideoEncodeSession;
        use miniter_media_native::frame::RgbaFrame;
        use miniter_media_native::mux::{
            ContainerFormat, Mp4Muxer, VideoTrackCodecOut, extract_sps_pps,
        };

        fn tool_err(message: String) -> MediaError {
            MediaError::ExternalTool {
                tool: "create_proxy",
                message,
            }
        }

        if !input_path.exists() {
            return Err(MediaError::NotFound(input_path.display().to_string()));
        }

        let info = self
            .probe_without_waveform(input_path)
            .map_err(|e| tool_err(e.to_string()))?;
        let video = info
            .primary_video()
            .ok_or_else(|| tool_err("no video stream - only video files get proxies".into()))?;
        if video.width == 0 || video.height == 0 {
            return Err(tool_err("video stream has no dimensions".into()));
        }
        let fps = if video.fps > 0.0 { video.fps } else { 30.0 };
        let total_frames = ((info.duration_ms as f64 / 1000.0) * fps).round().max(1.0);

        let scale = (960.0 / video.width.max(video.height) as f64).min(1.0);
        let out_w = ((video.width as f64 * scale).round() as u32).max(2) & !1;
        let out_h = ((video.height as f64 * scale).round() as u32).max(2) & !1;

        let mut session =
            VideoDecodeSession::open(input_path, false).map_err(|e| tool_err(e.to_string()))?;
        let mut encoder = VideoEncodeSession::new(out_w, out_h, 2_000_000, fps as f32)
            .map_err(|e| tool_err(e.to_string()))?;

        std::fs::create_dir_all(out_dir)?;
        let out_path = out_dir.join(format!("{asset_id}_proxy.mp4"));
        let file = std::fs::File::create(&out_path)?;
        let mut writer: Option<std::io::BufWriter<std::fs::File>> =
            Some(std::io::BufWriter::new(file));

        let mut muxer: Option<Mp4Muxer<std::io::BufWriter<std::fs::File>>> = None;
        let mut frames_in = 0u32;
        let mut frames_out = 0u32;
        let mut skipped = 0u32;
        let mut stalled = 0u32;
        let mut last_pct = 0u8;

        loop {
            match session.next_frame().map_err(|e| tool_err(e.to_string()))? {
                Some(frame) => {
                    stalled = 0;
                    frames_in += 1;
                    let scaled = scale_rgba(&frame, out_w, out_h);
                    let proxy_frame = RgbaFrame {
                        width: out_w,
                        height: out_h,
                        data: scaled,
                        pts_us: frame.pts_us,
                        color_info: Default::default(),
                    };
                    match encoder
                        .encode_frame(&proxy_frame)
                        .map_err(|e| tool_err(e.to_string()))?
                    {
                        EncodedVideoOutput::Sample {
                            bytes, is_keyframe, ..
                        } => {
                            if bytes.is_empty() {
                                skipped += 1;
                                continue;
                            }
                            if muxer.is_none() {
                                let (sps, pps) = extract_sps_pps(&bytes).ok_or_else(|| {
                                    tool_err("encoder output missing H.264 config".into())
                                })?;
                                let w = writer.take().ok_or_else(|| {
                                    tool_err("proxy writer already consumed".into())
                                })?;
                                let m = Mp4Muxer::new(
                                    w,
                                    out_w,
                                    out_h,
                                    fps,
                                    &sps,
                                    &pps,
                                    ContainerFormat::Mp4,
                                    None,
                                    None,
                                    VideoTrackCodecOut::H264,
                                )
                                .map_err(|e| tool_err(e.to_string()))?;
                                muxer = Some(m);
                            }
                            if let Some(m) = muxer.as_mut() {
                                m.write_sample_at(
                                    proxy_frame.pts_us.max(0) as u64,
                                    &bytes,
                                    is_keyframe,
                                )
                                .map_err(|e| tool_err(e.to_string()))?;
                                frames_out += 1;
                                if let Some(report) = progress.as_ref() {
                                    let pct = ((frames_out as f64 * 100.0 / total_frames) as u8)
                                        .min(99)
                                        .max(1);
                                    if pct > last_pct {
                                        last_pct = pct;
                                        report(pct);
                                    }
                                }
                            }
                        }
                        EncodedVideoOutput::Skipped => skipped += 1,
                    }
                }
                None if session.is_eos() => break,
                None => {
                    stalled += 1;
                    if stalled > 10_000 {
                        return Err(tool_err("decoder stalled before end of stream".into()));
                    }
                }
            }
        }

        match muxer.as_mut() {
            Some(m) => m.finish().map_err(|e| tool_err(e.to_string()))?,
            None => {
                let _ = std::fs::remove_file(&out_path);
                return Err(tool_err(format!(
                    "no video frames could be encoded ({frames_in} decoded, {skipped} skipped)"
                )));
            }
        }

        if skipped > 0 {
            tracing::warn!("proxy for {asset_id}: {skipped} frame(s) skipped by the encoder");
        }

        Ok(ProxyInfo {
            path: out_path,
            codec: "h264".to_string(),
            bitrate_kbps: 2000,
            fps,
            width: out_w,
            height: out_h,
            created_at: chrono::Utc::now(),
        })
    }

    pub fn extract_waveform(&self, path: &Path) -> Result<Vec<f32>, MediaError> {
        let decoded = miniter_audio::decode::decode_audio_f32(path).map_err(|e| {
            MediaError::ExternalTool {
                tool: "decode_audio_f32",
                message: e.to_string(),
            }
        })?;

        let channels = decoded.channels.max(1) as usize;
        let frames = decoded.samples.len() / channels;

        let target_rate = 8000f64;
        let dec_rate = decoded.sample_rate as f64;

        let resample_ratio = dec_rate / target_rate;
        let window_size = (target_rate / 20.0).round() as usize;
        if window_size == 0 || frames == 0 {
            return Ok(Vec::new());
        }

        let mut envelope = Vec::new();
        let mut i = 0usize;
        while i < frames {
            let end = ((i as f64 + window_size as f64 * resample_ratio) as usize).min(frames);
            let mut max_val = 0.0f32;
            for f in i..end {
                let mut frame_peak = 0.0f32;
                for ch in 0..channels {
                    let s = decoded.samples[f * channels + ch].abs();
                    if s > frame_peak {
                        frame_peak = s;
                    }
                }
                if frame_peak > max_val {
                    max_val = frame_peak;
                }
            }
            envelope.push(max_val);
            i = end;
        }

        Ok(envelope)
    }
}

/// Bilinear downscale (or upscale) of packed RGBA bytes.
/// Same-size input is returned unchanged (cloned).
fn scale_rgba(frame: &miniter_media_native::frame::RgbaFrame, out_w: u32, out_h: u32) -> Vec<u8> {
    if frame.width == out_w && frame.height == out_h {
        return frame.data.clone();
    }
    let (sw, sh) = (frame.width as f32, frame.height as f32);
    let mut out = vec![0u8; out_w as usize * out_h as usize * 4];
    for y in 0..out_h {
        let sy = ((y as f32 + 0.5) * sh / out_h as f32 - 0.5).clamp(0.0, sh - 1.0);
        let y0 = sy.floor() as u32;
        let y1 = (y0 + 1).min(frame.height - 1);
        let fy = sy - y0 as f32;
        for x in 0..out_w {
            let sx = ((x as f32 + 0.5) * sw / out_w as f32 - 0.5).clamp(0.0, sw - 1.0);
            let x0 = sx.floor() as u32;
            let x1 = (x0 + 1).min(frame.width - 1);
            let fx = sx - x0 as f32;
            let di = ((y * out_w + x) * 4) as usize;
            for c in 0..4 {
                let at = |xx: u32, yy: u32| {
                    frame.data[((yy * frame.width + xx) * 4) as usize + c] as f32
                };
                let v = at(x0, y0) * (1.0 - fx) * (1.0 - fy)
                    + at(x1, y0) * fx * (1.0 - fy)
                    + at(x0, y1) * (1.0 - fx) * fy
                    + at(x1, y1) * fx * fy;
                out[di + c] = v.round().clamp(0.0, 255.0) as u8;
            }
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scale_rgba_same_size_clones() {
        let frame = miniter_media_native::frame::RgbaFrame {
            width: 2,
            height: 2,
            data: vec![
                10, 20, 30, 255, 40, 50, 60, 255, 70, 80, 90, 255, 1, 2, 3, 255,
            ],
            pts_us: 0,
            color_info: Default::default(),
        };
        assert_eq!(scale_rgba(&frame, 2, 2), frame.data);
    }

    #[test]
    fn scale_rgba_downsamples_solid_color() {
        let frame = miniter_media_native::frame::RgbaFrame {
            width: 4,
            height: 4,
            data: vec![200, 10, 10, 255].repeat(16),
            pts_us: 0,
            color_info: Default::default(),
        };
        let out = scale_rgba(&frame, 2, 2);
        assert_eq!(out.len(), 16);
        for px in out.chunks_exact(4) {
            assert_eq!(px, &[200, 10, 10, 255]);
        }
    }

    /// Full native round-trip: synthesize a tiny H.264 mp4 with the same
    /// encoder/muxer the proxy path uses, then run `create_proxy` on it.
    /// Proves decode → scale → encode → mux works with no ffmpeg.
    #[test]
    fn proxy_round_trip_without_ffmpeg() {
        use miniter_media_native::EncodedVideoOutput;
        use miniter_media_native::encoder::VideoEncodeSession;
        use miniter_media_native::frame::RgbaFrame;
        use miniter_media_native::mux::{
            ContainerFormat, Mp4Muxer, VideoTrackCodecOut, extract_sps_pps,
        };

        let dir = tempfile::tempdir().unwrap();
        let src_path = dir.path().join("src.mp4");
        let w = 64u32;
        let h = 64u32;
        let fps = 30.0;

        let mut encoder = VideoEncodeSession::new(w, h, 2_000_000, fps as f32).unwrap();
        let mut muxer: Option<Mp4Muxer<std::io::BufWriter<std::fs::File>>> = None;
        let mut writer: Option<std::io::BufWriter<std::fs::File>> = Some(std::io::BufWriter::new(
            std::fs::File::create(&src_path).unwrap(),
        ));
        for i in 0..8u32 {
            let shade = (i * 30) as u8;
            let frame = RgbaFrame {
                width: w,
                height: h,
                data: vec![shade, 100, 150, 255].repeat((w * h) as usize),
                pts_us: (i as i64) * 33_333,
                color_info: Default::default(),
            };
            match encoder.encode_frame(&frame).unwrap() {
                EncodedVideoOutput::Sample {
                    bytes, is_keyframe, ..
                } => {
                    if muxer.is_none() {
                        let (sps, pps) = extract_sps_pps(&bytes).unwrap();
                        muxer = Some(
                            Mp4Muxer::new(
                                writer.take().unwrap(),
                                w,
                                h,
                                fps,
                                &sps,
                                &pps,
                                ContainerFormat::Mp4,
                                None,
                                None,
                                VideoTrackCodecOut::H264,
                            )
                            .unwrap(),
                        );
                    }
                    muxer
                        .as_mut()
                        .unwrap()
                        .write_sample_at(frame.pts_us as u64, &bytes, is_keyframe)
                        .unwrap();
                }
                EncodedVideoOutput::Skipped => {}
            }
        }
        muxer.as_mut().unwrap().finish().unwrap();
        drop(muxer);
        drop(writer);

        let engine = MediaEngine;
        let out_dir = dir.path().join("proxies");
        let proxy = engine
            .create_proxy(uuid::Uuid::new_v4(), &src_path, &out_dir, None)
            .expect("native proxy generation must not need ffmpeg");

        assert_eq!(proxy.codec, "h264");
        assert_eq!((proxy.width, proxy.height), (64, 64));
        assert!(proxy.path.exists(), "proxy file must exist");
        assert!(
            std::fs::metadata(&proxy.path).unwrap().len() > 0,
            "proxy file must not be empty"
        );
        let info = engine.probe(&proxy.path).unwrap();
        assert!(info.primary_video().is_some());
    }

    #[test]
    fn proxy_rejects_non_video_with_plain_message() {
        let dir = tempfile::tempdir().unwrap();
        let txt = dir.path().join("note.txt");
        std::fs::write(&txt, b"hello").unwrap();
        let engine = MediaEngine;
        let err = engine
            .create_proxy(uuid::Uuid::new_v4(), &txt, dir.path(), None)
            .unwrap_err();
        let msg = err.to_string();
        assert!(
            msg.contains("only video files get proxies") || msg.contains("probe"),
            "unexpected message: {msg}"
        );
    }
}
