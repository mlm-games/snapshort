use serde::{Deserialize, Serialize};
use std::io::Read;
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

        let output = std::process::Command::new("ffprobe")
            .arg("-v")
            .arg("error")
            .arg("-print_format")
            .arg("json")
            .arg("-show_format")
            .arg("-show_streams")
            .arg(path)
            .output()
            .map_err(|e| MediaError::ExternalTool {
                tool: "ffprobe",
                message: e.to_string(),
            })?;

        if !output.status.success() {
            let stderr = String::from_utf8_lossy(&output.stderr).to_string();
            return Err(MediaError::ExternalTool {
                tool: "ffprobe",
                message: stderr,
            });
        }

        let raw: ProbeOutput = serde_json::from_slice(&output.stdout)?;
        let file_size = std::fs::metadata(path).map(|m| m.len())?;
        let duration_ms =
            parse_duration_ms(raw.format.as_ref().and_then(|f| f.duration.as_deref()));

        let container = raw
            .format
            .as_ref()
            .and_then(|f| f.format_name.as_deref())
            .and_then(|n| n.split(',').next())
            .map(str::to_string)
            .or_else(|| {
                path.extension()
                    .and_then(|e| e.to_str())
                    .map(|s| s.to_lowercase())
            })
            .unwrap_or_else(|| "unknown".to_string());

        let mut video_streams = Vec::new();
        let mut audio_streams = Vec::new();

        for stream in raw.streams {
            match stream.codec_type.as_deref() {
                Some("video") => {
                    let width = stream.width.unwrap_or(1920).max(1);
                    let height = stream.height.unwrap_or(1080).max(1);
                    let fps = parse_fps(stream.r_frame_rate.as_deref())
                        .or_else(|| parse_fps(stream.avg_frame_rate.as_deref()))
                        .unwrap_or(30.0);
                    let duration_frames = stream
                        .nb_frames
                        .as_deref()
                        .and_then(|n| n.parse::<i64>().ok())
                        .filter(|v| *v > 0)
                        .or_else(|| {
                            stream
                                .duration
                                .as_deref()
                                .and_then(|d| d.parse::<f64>().ok())
                                .map(|secs| (secs * fps).round() as i64)
                        })
                        .unwrap_or_else(|| {
                            ((duration_ms as f64 / 1000.0) * fps).round() as i64
                        })
                        .max(0);

                    video_streams.push(VideoStream {
                        codec_name: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
                        codec_profile: stream.profile.unwrap_or_else(|| "unknown".to_string()),
                        bit_depth: stream
                            .bits_per_raw_sample
                            .and_then(|s| s.parse::<u8>().ok()),
                        chroma_subsampling: stream
                            .pix_fmt
                            .as_ref()
                            .map(|pix| pix_to_chroma(pix)),
                        width,
                        height,
                        fps,
                        duration_frames,
                        pixel_format: stream.pix_fmt.unwrap_or_else(|| "unknown".to_string()),
                        color_space: stream.color_space.unwrap_or_else(|| "unknown".to_string()),
                        hdr: matches!(
                            stream.color_transfer.as_deref(),
                            Some("smpte2084") | Some("arib-std-b67")
                        ),
                    });
                }
                Some("audio") => {
                    let sample_rate = stream
                        .sample_rate
                        .as_deref()
                        .and_then(|s| s.parse::<u32>().ok())
                        .unwrap_or(48_000);
                    let channels = stream.channels.unwrap_or(2).max(1) as u16;
                    let bit_depth = stream
                        .bits_per_raw_sample
                        .as_deref()
                        .and_then(|s| s.parse::<u8>().ok());
                    let duration_samples = stream
                        .duration
                        .as_deref()
                        .and_then(|d| d.parse::<f64>().ok())
                        .map(|secs| (secs * sample_rate as f64).round() as u64)
                        .unwrap_or_else(|| {
                            ((duration_ms as f64 / 1000.0) * sample_rate as f64) as u64
                        });

                    audio_streams.push(AudioStream {
                        codec_name: stream.codec_name.unwrap_or_else(|| "unknown".to_string()),
                        codec_profile: stream.profile.unwrap_or_else(|| "unknown".to_string()),
                        bit_depth,
                        channels,
                        sample_rate,
                        duration_samples,
                    });
                }
                _ => {}
            }
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
        asset_id: uuid::Uuid,
        input_path: &Path,
        out_dir: &Path,
    ) -> Result<ProxyInfo, MediaError> {
        if !input_path.exists() {
            return Err(MediaError::NotFound(input_path.display().to_string()));
        }

        std::fs::create_dir_all(out_dir)?;
        let out_path = out_dir.join(format!("{}_proxy.mp4", asset_id));

        let output = std::process::Command::new("ffmpeg")
            .arg("-y")
            .arg("-i")
            .arg(input_path)
            .arg("-vf")
            .arg("scale='if(gt(iw,ih),min(1280,iw),-2)':'if(gt(ih,iw),min(720,ih),-2)':flags=lanczos")
            .arg("-c:v")
            .arg("libx264")
            .arg("-preset")
            .arg("veryfast")
            .arg("-crf")
            .arg("28")
            .arg("-pix_fmt")
            .arg("yuv420p")
            .arg("-movflags")
            .arg("+faststart")
            .arg("-c:a")
            .arg("aac")
            .arg("-b:a")
            .arg("128k")
            .arg(&out_path)
            .output()
            .map_err(|e| MediaError::ExternalTool {
                tool: "ffmpeg",
                message: e.to_string(),
            })?;

        if !output.status.success() {
            return Err(MediaError::ExternalTool {
                tool: "ffmpeg",
                message: String::from_utf8_lossy(&output.stderr).to_string(),
            });
        }

        let info = self.probe(&out_path)?;
        let (width, height) = info
            .primary_video()
            .map(|v| (v.width, v.height))
            .unwrap_or((1280, 720));
        let fps = info.primary_video().map(|v| v.fps).unwrap_or(30.0);

        Ok(ProxyInfo {
            path: out_path,
            codec: "h264".to_string(),
            bitrate_kbps: 2_000,
            fps,
            width,
            height,
            created_at: chrono::Utc::now(),
        })
    }

    pub fn extract_waveform(&self, path: &Path) -> Result<Vec<f32>, MediaError> {
        let mut child = std::process::Command::new("ffmpeg")
            .arg("-v")
            .arg("quiet")
            .arg("-i")
            .arg(path)
            .arg("-vn")
            .arg("-ac")
            .arg("1")
            .arg("-ar")
            .arg("8000")
            .arg("-f")
            .arg("f32le")
            .arg("pipe:1")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .map_err(|e| MediaError::ExternalTool {
                tool: "ffmpeg",
                message: e.to_string(),
            })?;

        let mut child_stdout = child.stdout.take().unwrap();
        let mut raw = Vec::new();
        child_stdout
            .read_to_end(&mut raw)
            .map_err(|e| MediaError::ExternalTool {
                tool: "ffmpeg",
                message: e.to_string(),
            })?;

        let status = child.wait().map_err(|e| MediaError::ExternalTool {
            tool: "ffmpeg",
            message: e.to_string(),
        })?;

        if !status.success() {
            let mut stderr = Vec::new();
            child
                .stderr
                .take()
                .unwrap()
                .read_to_end(&mut stderr)
                .ok();
            return Err(MediaError::ExternalTool {
                tool: "ffmpeg",
                message: String::from_utf8_lossy(&stderr).to_string(),
            });
        }

        let samples: Vec<f32> = raw
            .chunks_exact(4)
            .map(|chunk| f32::from_le_bytes([chunk[0], chunk[1], chunk[2], chunk[3]]))
            .collect();

        let window_size = 8000 / 20;
        if window_size == 0 || samples.is_empty() {
            return Ok(Vec::new());
        }

        let waveform: Vec<f32> = samples
            .chunks(window_size)
            .map(|chunk| chunk.iter().copied().map(f32::abs).fold(0.0f32, f32::max))
            .collect();

        Ok(waveform)
    }
}

fn parse_duration_ms(duration: Option<&str>) -> u64 {
    duration
        .and_then(|s| s.parse::<f64>().ok())
        .map(|secs| (secs * 1000.0).round().max(0.0) as u64)
        .unwrap_or(0)
}

fn parse_fps(v: Option<&str>) -> Option<f64> {
    let raw = v?.trim();
    if raw.is_empty() || raw == "0/0" {
        return None;
    }
    if let Some((num_s, den_s)) = raw.split_once('/') {
        let num: f64 = num_s.parse().ok()?;
        let den: f64 = den_s.parse().ok()?;
        if num <= 0.0 || den <= 0.0 {
            return None;
        }
        return Some(num / den);
    }
    let fps = raw.parse::<f64>().ok()?;
    if fps <= 0.0 {
        None
    } else {
        Some(fps)
    }
}

fn pix_to_chroma(pix_fmt: &str) -> String {
    let lower = pix_fmt.to_lowercase();
    if lower.contains("420") {
        "4:2:0".to_string()
    } else if lower.contains("422") {
        "4:2:2".to_string()
    } else if lower.contains("444") {
        "4:4:4".to_string()
    } else {
        "unknown".to_string()
    }
}

#[derive(Debug, Deserialize)]
struct ProbeOutput {
    #[serde(default)]
    streams: Vec<ProbeStream>,
    format: Option<ProbeFormat>,
}

#[derive(Debug, Deserialize)]
struct ProbeFormat {
    duration: Option<String>,
    format_name: Option<String>,
}

#[derive(Debug, Deserialize)]
struct ProbeStream {
    codec_type: Option<String>,
    codec_name: Option<String>,
    profile: Option<String>,
    width: Option<u32>,
    height: Option<u32>,
    pix_fmt: Option<String>,
    color_space: Option<String>,
    color_transfer: Option<String>,
    r_frame_rate: Option<String>,
    avg_frame_rate: Option<String>,
    nb_frames: Option<String>,
    duration: Option<String>,
    sample_rate: Option<String>,
    channels: Option<u32>,
    bits_per_raw_sample: Option<String>,
}
