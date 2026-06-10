use image::ImageEncoder;
use miniter_domain::Timestamp;
use std::path::Path;

use crate::RenderError;

pub fn render_frame_rgba(
    timeline: &miniter_domain::Timeline,
    timestamp: Timestamp,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    miniter_media_native::export::render_single_frame(timeline, timestamp, width, height)
        .map_err(|e| RenderError::EncodingError(e.to_string()))
}

pub fn render_preview_frame(
    timeline: &miniter_domain::Timeline,
    timestamp: Timestamp,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let rgba = render_frame_rgba(timeline, timestamp, width, height)?;
    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| RenderError::EncodingError(e.to_string()))?;
    Ok(png_bytes)
}

pub fn render_thumbnail(
    source_path: &str,
    time_us: i64,
) -> Result<Vec<u8>, RenderError> {
    let frame = miniter_media_native::thumbnailer::extract_thumbnail(
        Path::new(source_path),
        time_us,
    )
    .map_err(|e| RenderError::EncodingError(e.to_string()))?;

    let (thumb_w, thumb_h) = (160u32, 90u32);
    let scaled = scale_rgba_nearest(
        &frame.data,
        frame.width,
        frame.height,
        thumb_w,
        thumb_h,
    );

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(&scaled, thumb_w, thumb_h, image::ExtendedColorType::Rgba8)
        .map_err(|e| RenderError::EncodingError(e.to_string()))?;
    Ok(png_bytes)
}

fn scale_rgba_nearest(
    src: &[u8],
    src_w: u32,
    src_h: u32,
    dst_w: u32,
    dst_h: u32,
) -> Vec<u8> {
    let mut dst = vec![0u8; (dst_w * dst_h * 4) as usize];
    for y in 0..dst_h {
        for x in 0..dst_w {
            let sx = (x * src_w / dst_w).min(src_w - 1);
            let sy = (y * src_h / dst_h).min(src_h - 1);
            let si = ((sy * src_w + sx) * 4) as usize;
            let di = ((y * dst_w + x) * 4) as usize;
            dst[di..di + 4].copy_from_slice(&src[si..si + 4]);
        }
    }
    dst
}
