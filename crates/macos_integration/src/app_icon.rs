use std::ffi::c_void;

use objc2::AnyThread;
use objc2::msg_send;
use objc2::runtime::{AnyClass, AnyObject};
use objc2_app_kit::NSImage;
use objc2_foundation::{MainThreadMarker, NSData};

/// Sets the application's Dock icon from the embedded `pawse.png` asset.
///
/// This only has an effect when running outside of a proper `.app` bundle
/// (e.g. `cargo run`). When the app is bundled, macOS reads the icon from
/// `Info.plist` / `AppIcon.icns` instead.
pub fn set_application_icon() {
    let _mtm = match MainThreadMarker::new() {
        Some(mtm) => mtm,
        None => {
            eprintln!("macos_integration: set_application_icon must be called on the main thread");
            return;
        }
    };

    unsafe {
        let bytes = include_bytes!("../../../assets/icons/play.svg");
        let data = NSData::dataWithBytes_length(bytes.as_ptr().cast::<c_void>(), bytes.len());
        let Some(image) = NSImage::initWithData(NSImage::alloc(), &data) else {
            eprintln!("macos_integration: failed to decode pawse.png for app icon");
            return;
        };

        // Use raw msg_send to avoid an ivar-mismatch crash between objc2-app-kit's
        // NSApplication and the older `objc` crate used by gpui.
        let app_class = AnyClass::get(c"NSApplication").expect("NSApplication class not found");
        let app: *mut AnyObject = msg_send![app_class, sharedApplication];
        let _: () = msg_send![app, setApplicationIconImage: &*image];
    }
}
