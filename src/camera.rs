//! Grab webcam frames and decode the first Monero QR found.
//!
//! GPUI has no live camera widget, so this opens the default camera, reads a
//! short burst of frames, and returns the first payload `rqrr` can decode.

use nokhwa::pixel_format::RgbFormat;
use nokhwa::utils::{CameraIndex, RequestedFormat, RequestedFormatType};
use nokhwa::Camera;

use crate::qr;

const MAX_FRAMES: u32 = 40;

pub fn scan_qr() -> Result<String, String> {
    let requested =
        RequestedFormat::new::<RgbFormat>(RequestedFormatType::AbsoluteHighestFrameRate);
    let mut camera = Camera::new(CameraIndex::Index(0), requested)
        .map_err(|err| format!("Camera unavailable: {err}"))?;
    if let Err(err) = camera.open_stream() {
        return Err(format!("Could not start camera: {err}"));
    }
    let mut last_err = "No QR code found. Hold a Monero payment QR in front of the camera.".to_string();
    for _ in 0..MAX_FRAMES {
        match camera.frame() {
            Ok(frame) => match frame.decode_image::<RgbFormat>() {
                Ok(rgb) => {
                    let (width, height) = rgb.dimensions();
                    if let Some(payload) = qr::decode_rgb8(width, height, rgb.as_raw()) {
                        let _ = camera.stop_stream();
                        return Ok(payload);
                    }
                }
                Err(err) => last_err = format!("Camera frame decode failed: {err}"),
            },
            Err(err) => last_err = format!("Camera frame failed: {err}"),
        }
    }
    let _ = camera.stop_stream();
    Err(last_err)
}
