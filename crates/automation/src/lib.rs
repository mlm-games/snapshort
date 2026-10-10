//! Automation: the MCP server, the bridge to a running app, and the control
//! channel that app serves.
//!
//! * **Headless** MCP drives an in-process [`snapshort_engine::Session`]: every
//!   command, inspection and frame render, with no window.
//! * **Bridge** MCP forwards to a running app over a JSON-lines control
//!   channel (`snapshort --control <port>`), so agents also drive the live UI.
//! * The control channel server half lives here too; the app only supplies a
//!   handler that runs commands and UI actions on its own thread.

pub mod bridge;
pub mod control;
pub mod long_job;
pub mod server;

pub use bridge::BridgeClient;
pub use control::{ControlFuture, ControlHandler, ControlServer};
pub use server::SnapshortMcp;

#[derive(Debug, thiserror::Error)]
pub enum AutomationError {
    #[error("{0}")]
    BadRequest(String),
    #[error("engine: {0}")]
    Engine(#[from] snapshort_engine::EngineError),
    #[error("bridge: {0}")]
    Bridge(String),
    #[error("app: {0}")]
    App(String),
    #[error("{0}")]
    Other(String),
}

/// Encode RGBA8 as PNG, downscaled so the longest side is at most `max_side`
/// (0 = no limit).
pub fn png_rgba(w: u32, h: u32, rgba: Vec<u8>, max_side: u32) -> Result<Vec<u8>, AutomationError> {
    let mut img = image::RgbaImage::from_raw(w, h, rgba)
        .ok_or_else(|| AutomationError::Other("bad image".into()))?;
    if max_side > 0 && w.max(h) > max_side {
        let s = max_side as f32 / w.max(h) as f32;
        img = image::imageops::resize(
            &img,
            ((w as f32 * s) as u32).max(1),
            ((h as f32 * s) as u32).max(1),
            image::imageops::FilterType::Triangle,
        );
    }
    let mut out = std::io::Cursor::new(Vec::new());
    image::DynamicImage::ImageRgba8(img)
        .write_to(&mut out, image::ImageFormat::Png)
        .map_err(|e| AutomationError::Other(e.to_string()))?;
    Ok(out.into_inner())
}
