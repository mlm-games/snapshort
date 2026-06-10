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
        if !path.exists() {
            return Err(MediaError::NotFound(path.display().to_string()));
        }

        let file_size = std::fs::metadata(path).map(|m| m.len())?;

        let container = path
            .extension()
            .and_then(|e| e.to_str())
            .map(|s| s.to_lowercase())
            .unwrap_or_else(|| "unknown".to_string());

        let minfo =
            miniter_media_native::probe::probe_media(path).map_err(|e| MediaError::ExternalTool {
                tool: "miniter-probe",
                message: e.to_string(),
            })?;

        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();

        let duration_ms = minfo.duration_us.map(|u| (u as f64 / 1000.0) as u64).unwrap_or(0);

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

        if !info.audio_streams.is_empty() {
            if let Ok(waveform) = self.extract_waveform(path) {
                info.waveform = Some(waveform);
            }
        }

        Ok(info)
    }

    pub fn create_proxy(
        &self,
        _asset_id: uuid::Uuid,
        _input_path: &Path,
        _out_dir: &Path,
    ) -> Result<ProxyInfo, MediaError> {
        Err(MediaError::ExternalTool {
            tool: "create_proxy",
            message: "proxy creation not available without ffmpeg".into(),
        })
    }

    pub fn extract_waveform(&self, path: &Path) -> Result<Vec<f32>, MediaError> {
        let decoded = miniter_audio::decode::decode_audio_f32(path)
            .map_err(|e| MediaError::ExternalTool {
                tool: "decode_audio_f32",
                message: e.to_string(),
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


