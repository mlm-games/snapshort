use image::ImageEncoder;
use miniter_domain::{ease_in_out, SubtitleMode, Timestamp};
use miniter_render_plan::render_graph::{plan_frame, RenderNode};
use miniter_render_plan::transition_blend::{opacity_pair, slide_offset};

use crate::RenderError;

pub fn render_preview_frame(
    timeline: &miniter_domain::Timeline,
    timestamp: Timestamp,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let plan = plan_frame(timeline, timestamp, width, height, SubtitleMode::Soft);
    let rgba = render_plan_to_rgba(&plan, width, height)?;

    let mut png_bytes = Vec::new();
    let encoder = image::codecs::png::PngEncoder::new(&mut png_bytes);
    encoder
        .write_image(&rgba, width, height, image::ExtendedColorType::Rgba8)
        .map_err(|e| RenderError::EncodingError(e.to_string()))?;

    Ok(png_bytes)
}

fn render_plan_to_rgba(plan: &miniter_render_plan::render_graph::RenderPlan, w: u32, h: u32) -> Result<Vec<u8>, RenderError> {
    let ws = w as usize;
    let hs = h as usize;
    let mut canvas = vec![0u8; ws * hs * 4];
    render_node(&plan.root, ws, hs, &mut canvas, w, h)?;
    Ok(canvas)
}

fn render_node(
    node: &RenderNode,
    ws: usize,
    hs: usize,
    canvas: &mut [u8],
    full_w: u32,
    full_h: u32,
) -> Result<(), RenderError> {
    match node {
        RenderNode::VideoFrame {
            source_path,
            source_pts,
            opacity,
            ..
        } => {
            let frame = extract_frame_ffmpeg(source_path, source_pts.0, full_w, full_h)?;
            blend_over(canvas, &frame, *opacity);
            Ok(())
        }
        RenderNode::TransitionBlend {
            bottom,
            top,
            kind,
            progress,
        } => {
            let mut bottom_buf = vec![0u8; ws * hs * 4];
            render_node(bottom, ws, hs, &mut bottom_buf, full_w, full_h)?;
            let mut top_buf = vec![0u8; ws * hs * 4];
            render_node(top, ws, hs, &mut top_buf, full_w, full_h)?;

            let eased = ease_in_out(*progress);
            blend_transition(&mut bottom_buf, &top_buf, ws, hs, *kind, eased);
            canvas.copy_from_slice(&bottom_buf);
            Ok(())
        }
        RenderNode::Stack(layers) => {
            for layer in layers {
                render_node(layer, ws, hs, canvas, full_w, full_h)?;
            }
            Ok(())
        }
        _ => Ok(()),
    }
}

fn blend_transition(dst: &mut [u8], src: &[u8], w: usize, h: usize, kind: miniter_domain::TransitionKind, progress: f32) {
    match kind {
        miniter_domain::TransitionKind::CrossFade | miniter_domain::TransitionKind::Dissolve => {
            let (bottom_a, top_a) = opacity_pair(kind, progress);
            for y in 0..h {
                for x in 0..w {
                    let i = (y * w + x) * 4;
                    dst[i] = (dst[i] as f32 * bottom_a + src[i] as f32 * top_a).round().clamp(0.0, 255.0) as u8;
                    dst[i + 1] = (dst[i + 1] as f32 * bottom_a + src[i + 1] as f32 * top_a).round().clamp(0.0, 255.0) as u8;
                    dst[i + 2] = (dst[i + 2] as f32 * bottom_a + src[i + 2] as f32 * top_a).round().clamp(0.0, 255.0) as u8;
                    dst[i + 3] = 255;
                }
            }
        }
        miniter_domain::TransitionKind::SlideLeft | miniter_domain::TransitionKind::SlideRight => {
            let dx = (slide_offset(kind, progress) * w as f32).round() as i32;
            let mut temp = dst.to_vec();
            for y in 0..h {
                for x in 0..w {
                    let dst_i = (y * w + x) * 4;
                    let src_x = x as i32 - dx;
                    if src_x >= 0 && src_x < w as i32 {
                        let src_i = (y * w + src_x as usize) * 4;
                        temp[dst_i..dst_i + 3].copy_from_slice(&src[src_i..src_i + 3]);
                    }
                }
            }
            dst.copy_from_slice(&temp);
        }
        _ => {}
    }
}

fn blend_over(dst: &mut [u8], src: &[u8], src_alpha: f32) {
    let sa = src_alpha.clamp(0.0, 1.0);
    for chunk in dst.chunks_exact_mut(4).zip(src.chunks_exact(4)) {
        let (d, s) = chunk;
        if sa <= 0.0 {
            continue;
        }
        if sa >= 1.0 {
            d.copy_from_slice(&s[..4]);
            continue;
        }
        let inv = 1.0 - sa;
        d[0] = (d[0] as f32 * inv + s[0] as f32 * sa).round().clamp(0.0, 255.0) as u8;
        d[1] = (d[1] as f32 * inv + s[1] as f32 * sa).round().clamp(0.0, 255.0) as u8;
        d[2] = (d[2] as f32 * inv + s[2] as f32 * sa).round().clamp(0.0, 255.0) as u8;
        d[3] = 255;
    }
}

fn extract_frame_ffmpeg(
    source_path: &str,
    source_time_us: i64,
    width: u32,
    height: u32,
) -> Result<Vec<u8>, RenderError> {
    let seek_sec = source_time_us as f64 / 1_000_000.0;

    let output = std::process::Command::new("ffmpeg")
        .arg("-ss")
        .arg(format!("{seek_sec:.6}"))
        .arg("-i")
        .arg(source_path)
        .arg("-vf")
        .arg(format!(
            "scale='min({},{})':'min({},{}):force_original_aspect_ratio=decrease',pad={}:{}:(iw)/2:(ih)/2:color=black",
            width, height, width, height, width, height
        ))
        .arg("-frames:v")
        .arg("1")
        .arg("-f")
        .arg("rawvideo")
        .arg("-pix_fmt")
        .arg("rgba")
        .arg("-")
        .output()
        .map_err(|e| RenderError::IoError(e.to_string()))?;

    if !output.status.success() {
        let stderr = String::from_utf8_lossy(&output.stderr).trim().to_string();
        return Err(RenderError::EncodingError(stderr));
    }

    let expected = (width * height * 4) as usize;
    if output.stdout.len() < expected {
        return Err(RenderError::EncodingError(format!(
            "Expected {} bytes from ffmpeg, got {}",
            expected,
            output.stdout.len()
        )));
    }

    Ok(output.stdout)
}
