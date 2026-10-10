//! Shared helpers for the DSP oracle tests: float WAV writing, unique temp paths
//! and ffmpeg discovery (a test skips itself when ffmpeg is not installed).

use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicUsize, Ordering};

pub fn fixtures_dir(sub: &str) -> PathBuf {
    let mut d = std::env::temp_dir().join("snapshort-audio-dsp-fixtures");
    for part in sub.split('/').filter(|p| !p.is_empty()) {
        d.push(part);
    }
    std::fs::create_dir_all(&d).unwrap_or_else(|e| panic!("create {}: {e}", d.display()));
    d
}

pub fn temp_path(out: &Path) -> PathBuf {
    static N: AtomicUsize = AtomicUsize::new(0);
    let tid = format!("{:?}", std::thread::current().id())
        .replace(|c: char| !c.is_ascii_alphanumeric(), "");
    let n = N.fetch_add(1, Ordering::Relaxed);
    let stem = out
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_default();
    let name = match out.extension() {
        Some(ext) => format!(
            "{stem}.tmp-{}-{tid}-{n}.{}",
            std::process::id(),
            ext.to_string_lossy()
        ),
        None => format!("{stem}.tmp-{}-{tid}-{n}", std::process::id()),
    };
    std::env::temp_dir().join(name)
}

pub mod wav {
    use std::path::Path;

    pub fn write_f32(path: &Path, channels: &[&[f32]], sample_rate: u32) -> std::io::Result<()> {
        std::fs::write(path, super::encode_f32(channels, sample_rate))
    }
}

/// Encode planar f32 channels (equal lengths) as a 32-bit float WAV file.
pub fn encode_f32(channels: &[&[f32]], sample_rate: u32) -> Vec<u8> {
    let nch = channels.len().max(1) as u16;
    let frames = channels.first().map_or(0, |c| c.len());
    assert!(
        channels.iter().all(|c| c.len() == frames),
        "channels must have equal lengths"
    );
    let data_len = (frames * nch as usize * 4) as u32;
    let mut v = Vec::with_capacity(44 + data_len as usize);
    v.extend_from_slice(b"RIFF");
    v.extend_from_slice(&(36 + data_len).to_le_bytes());
    v.extend_from_slice(b"WAVEfmt ");
    v.extend_from_slice(&16u32.to_le_bytes());
    v.extend_from_slice(&3u16.to_le_bytes());
    v.extend_from_slice(&nch.to_le_bytes());
    v.extend_from_slice(&sample_rate.to_le_bytes());
    v.extend_from_slice(&(sample_rate * nch as u32 * 4).to_le_bytes());
    v.extend_from_slice(&(nch * 4).to_le_bytes());
    v.extend_from_slice(&32u16.to_le_bytes());
    v.extend_from_slice(b"data");
    v.extend_from_slice(&data_len.to_le_bytes());
    for i in 0..frames {
        for c in channels {
            v.extend_from_slice(&c[i].to_le_bytes());
        }
    }
    v
}

fn on_path(name: &str) -> Option<PathBuf> {
    let paths = std::env::var_os("PATH")?;
    for dir in std::env::split_paths(&paths) {
        let candidate = dir.join(name);
        let executable = candidate
            .is_file()
            .then(|| {
                use std::os::unix::fs::PermissionsExt;
                std::fs::metadata(&candidate)
                    .map(|m| m.permissions().mode() & 0o111 != 0)
                    .unwrap_or(false)
            })
            .unwrap_or(false);
        if executable {
            return Some(candidate);
        }
    }
    None
}

pub fn ffmpeg_or_skip(context: &str) -> Option<PathBuf> {
    let found = on_path("ffmpeg");
    if found.is_none() {
        eprintln!("skipping {context}: ffmpeg not found on PATH");
    }
    found
}
