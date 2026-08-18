//! Device owner authentication. macOS uses Touch ID / password; other platforms are unavailable.

#[cfg(target_os = "macos")]
mod macos {
    use std::sync::mpsc;

    use block2::RcBlock;
    use objc2::rc::Retained;
    use objc2::runtime::Bool;
    use objc2_foundation::{NSError, NSString};
    use objc2_local_authentication::{LAContext, LAPolicy};

    const POLICY: LAPolicy = LAPolicy::DeviceOwnerAuthentication;

    pub fn is_available() -> bool {
        unsafe {
            let context = LAContext::new();
            context.canEvaluatePolicy_error(POLICY).is_ok()
        }
    }

    pub fn authenticate(reason: &str) -> Result<(), String> {
        unsafe {
            let context = LAContext::new();
            if let Err(err) = context.canEvaluatePolicy_error(POLICY) {
                return Err(nserror_message(&err));
            }
            let reason_ns = NSString::from_str(reason);
            let (tx, rx) = mpsc::channel::<Result<(), String>>();
            let block = RcBlock::new(move |success: Bool, err: *mut NSError| {
                let result = if success.as_bool() {
                    Ok(())
                } else if err.is_null() {
                    Err("Authentication failed.".into())
                } else if let Some(retained) = Retained::retain(err) {
                    Err(nserror_message(&retained))
                } else {
                    Err("Authentication failed.".into())
                };
                let _ = tx.send(result);
            });
            context.evaluatePolicy_localizedReason_reply(POLICY, &reason_ns, &block);
            rx.recv()
                .unwrap_or_else(|_| Err("Authentication was interrupted.".into()))
        }
    }

    fn nserror_message(err: &NSError) -> String {
        let desc = err.localizedDescription();
        let text = desc.to_string();
        if text.is_empty() {
            "Authentication failed.".into()
        } else {
            text
        }
    }
}

#[cfg(target_os = "macos")]
pub use macos::{authenticate, is_available};

#[cfg(not(target_os = "macos"))]
pub fn is_available() -> bool {
    false
}

#[cfg(not(target_os = "macos"))]
pub fn authenticate(_reason: &str) -> Result<(), String> {
    Err("Device authentication is not available on this platform.".into())
}
