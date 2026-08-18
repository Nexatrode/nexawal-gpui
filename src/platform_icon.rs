use std::sync::Arc;

const APP_ICON_PNG: &[u8] = include_bytes!("../assets/nexawal.png");

/// X11 accepts a per-window icon. Wayland associates the desktop icon by app ID.
#[cfg(any(target_os = "linux", target_os = "freebsd"))]
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    image::load_from_memory(APP_ICON_PNG)
        .ok()
        .map(|image| Arc::new(image.into_rgba8()))
}

#[cfg(not(any(target_os = "linux", target_os = "freebsd")))]
pub fn window_icon() -> Option<Arc<image::RgbaImage>> {
    None
}

/// A packaged `.app` gets this from `nexawal.icns`. Setting it at runtime also
/// gives `cargo run` and the raw executable the proper Dock/Cmd-Tab icon.
#[cfg(target_os = "macos")]
pub fn install_process_icon() {
    use objc2::{AnyThread, MainThreadMarker};
    use objc2_app_kit::{NSApplication, NSImage};
    use objc2_foundation::NSData;

    let Some(main_thread) = MainThreadMarker::new() else {
        return;
    };
    let data = unsafe {
        NSData::dataWithBytes_length(APP_ICON_PNG.as_ptr().cast(), APP_ICON_PNG.len())
    };
    let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
        return;
    };
    let application = NSApplication::sharedApplication(main_thread);
    unsafe { application.setApplicationIconImage(Some(&image)) };
}

#[cfg(not(target_os = "macos"))]
pub fn install_process_icon() {}
