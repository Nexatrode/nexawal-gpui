//! Optional, short send-result cues. Audio is best-effort and never blocks wallet work.

use std::process::{Command, Stdio};

#[derive(Clone, Copy)]
enum Cue {
    Success,
    Failure,
}

pub fn success(enabled: bool) {
    play(enabled, Cue::Success);
}

pub fn failure(enabled: bool) {
    play(enabled, Cue::Failure);
}

fn play(enabled: bool, cue: Cue) {
    if !enabled {
        return;
    }
    // Audio backends may stall while waking a device. Keep that work off the UI/send
    // completion path; inability to play a cue must never change send state.
    let _ = std::thread::Builder::new()
        .name("nexawal-send-sound".into())
        .spawn(move || play_native(cue));
}

#[cfg(target_os = "macos")]
fn play_native(cue: Cue) {
    let sound = match cue {
        Cue::Success => "/System/Library/Sounds/Glass.aiff",
        Cue::Failure => "/System/Library/Sounds/Basso.aiff",
    };
    let _ = Command::new("/usr/bin/afplay")
        .args(["-v", "0.22", sound])
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status();
}

#[cfg(target_os = "windows")]
fn play_native(cue: Cue) {
    const MB_ICONHAND: u32 = 0x0000_0010;
    const MB_OK: u32 = 0x0000_0000;

    #[link(name = "user32")]
    unsafe extern "system" {
        fn MessageBeep(kind: u32) -> i32;
    }

    let kind = match cue {
        Cue::Success => MB_OK,
        Cue::Failure => MB_ICONHAND,
    };
    // Uses the user's Windows sound scheme; the native call is intentionally
    // best-effort and runs on the detached audio thread above.
    unsafe {
        let _ = MessageBeep(kind);
    }
}

#[cfg(target_os = "linux")]
fn play_native(cue: Cue) {
    let (event, file) = match cue {
        Cue::Success => (
            "complete",
            "/usr/share/sounds/freedesktop/stereo/complete.oga",
        ),
        Cue::Failure => (
            "dialog-error",
            "/usr/share/sounds/freedesktop/stereo/dialog-error.oga",
        ),
    };

    // Prefer the desktop sound theme so the user gets their configured cue.
    if run_command("/usr/bin/canberra-gtk-play", &["--id", event]) {
        return;
    }

    // Minimal fallback for PipeWire/PulseAudio desktops that ship the common
    // freedesktop sound theme but not libcanberra's command-line helper.
    for player in ["/usr/bin/pw-play", "/usr/bin/paplay"] {
        if run_command(player, &[file]) {
            return;
        }
    }
}

#[cfg(target_os = "linux")]
fn run_command(program: &str, args: &[&str]) -> bool {
    Command::new(program)
        .args(args)
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .status()
        .is_ok_and(|status| status.success())
}

#[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
fn play_native(_cue: Cue) {}

#[cfg(test)]
mod tests {
    #[test]
    fn disabled_cues_are_safe_no_ops() {
        super::success(false);
        super::failure(false);
    }
}
