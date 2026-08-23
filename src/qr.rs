//! QR for a Monero payment URI, rendered as a GPUI image.

use std::sync::Arc;

use gpui::RenderImage;
use image::{ImageBuffer, Rgba};
use qrcode::{Color, EcLevel, QrCode};
use smallvec::SmallVec;

const CLASSIC_FOREGROUND: Rgba<u8> = Rgba([0, 0, 0, 255]);
const CLASSIC_BACKGROUND: Rgba<u8> = Rgba([255, 255, 255, 255]);
const TECHNO_FOREGROUND: Rgba<u8> = Rgba([0x39, 0xFF, 0x14, 255]);
const TECHNO_BACKGROUND: Rgba<u8> = Rgba([0, 0, 0, 255]);

pub fn render_image(payload: &str, techno: bool) -> Option<Arc<RenderImage>> {
    let buf = render_rgba(payload, techno)?;
    let frame = image::Frame::new(buf);
    Some(Arc::new(RenderImage::new(SmallVec::from_elem(frame, 1))))
}

fn render_rgba(payload: &str, techno: bool) -> Option<image::RgbaImage> {
    let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M).ok()?;
    let colors = code.to_colors();
    let width = code.width() as u32;
    if width == 0 {
        return None;
    }
    let quiet = 2u32;
    let modules = width + quiet * 2;
    let module_px = (280 / modules).max(4);
    let size = modules * module_px;
    let (foreground, background) = if techno {
        (TECHNO_FOREGROUND, TECHNO_BACKGROUND)
    } else {
        (CLASSIC_FOREGROUND, CLASSIC_BACKGROUND)
    };
    let mut buf = ImageBuffer::from_pixel(size, size, background);
    for y in 0..width {
        for x in 0..width {
            if colors[(y * width + x) as usize] != Color::Dark {
                continue;
            }
            let px0 = (quiet + x) * module_px;
            let py0 = (quiet + y) * module_px;
            for dy in 0..module_px {
                for dx in 0..module_px {
                    buf.put_pixel(px0 + dx, py0 + dy, foreground);
                }
            }
        }
    }
    Some(buf)
}

pub fn decode_bytes(bytes: &[u8]) -> Option<String> {
    decode_luma(image::load_from_memory(bytes).ok()?.to_luma8())
}

pub fn decode_path(path: &std::path::Path) -> Option<String> {
    decode_bytes(&std::fs::read(path).ok()?)
}

pub fn decode_rgb8(width: u32, height: u32, rgb: &[u8]) -> Option<String> {
    if width == 0 || height == 0 {
        return None;
    }
    let expected = (width as usize)
        .checked_mul(height as usize)?
        .checked_mul(3)?;
    if rgb.len() < expected {
        return None;
    }
    let mut luma = Vec::with_capacity(width as usize * height as usize);
    for chunk in rgb[..expected].chunks_exact(3) {
        let y = (u16::from(chunk[0]) + u16::from(chunk[1]) + u16::from(chunk[2])) / 3;
        luma.push(y as u8);
    }
    let gray = image::GrayImage::from_raw(width, height, luma)?;
    decode_luma(gray)
}

pub fn decode_luma(gray: image::GrayImage) -> Option<String> {
    let mut prepared = rqrr::PreparedImage::prepare(gray);
    let grids = prepared.detect_grids();
    let grid = grids.first()?;
    grid.decode().ok().map(|(_, payload)| payload)
}

#[cfg(test)]
mod tests {
    use super::*;
    use image::Luma;

    #[test]
    fn decode_rendered_qr() {
        let payload = "monero:4A1ZVG4SrnpMgcnr3EDTHz2rzprHZm9pq7sZKj3dNL6L5j9CFcVd3RK3yZFJm3XWW3W2YtCAUxb7yBJEvsnicvriSZVA5nq";
        let code = QrCode::with_error_correction_level(payload.as_bytes(), EcLevel::M).unwrap();
        let width = code.width() as u32;
        let colors = code.to_colors();
        let quiet = 4u32;
        let scale = 4u32;
        let size = (width + quiet * 2) * scale;
        let mut gray = image::GrayImage::from_pixel(size, size, Luma([255]));
        for y in 0..width {
            for x in 0..width {
                if colors[(y * width + x) as usize] != Color::Dark {
                    continue;
                }
                for dy in 0..scale {
                    for dx in 0..scale {
                        gray.put_pixel(
                            (quiet + x) * scale + dx,
                            (quiet + y) * scale + dy,
                            Luma([0]),
                        );
                    }
                }
            }
        }
        assert_eq!(decode_luma(gray).as_deref(), Some(payload));
    }

    #[test]
    fn techno_qr_uses_neon_green_on_black_and_decodes() {
        let payload = "monero:4A1ZVG4SrnpMgcnr3EDTHz2rzprHZm9pq7sZKj3dNL6L5j9CFcVd3RK3yZFJm3XWW3W2YtCAUxb7yBJEvsnicvriSZVA5nq";
        let rgba = render_rgba(payload, true).expect("techno QR");
        assert!(rgba.pixels().any(|pixel| *pixel == TECHNO_FOREGROUND));
        assert!(rgba.pixels().any(|pixel| *pixel == TECHNO_BACKGROUND));

        // rqrr expects dark modules on a light background. Inverting the
        // luminance models the polarity normalization used by QR scanners.
        let mut gray = image::DynamicImage::ImageRgba8(rgba).to_luma8();
        for pixel in gray.pixels_mut() {
            pixel.0[0] = 255 - pixel.0[0];
        }
        assert_eq!(decode_luma(gray).as_deref(), Some(payload));
    }
}
