use objc2::rc::Retained;
use objc2_app_kit::{NSFloatingWindowLevel, NSNormalWindowLevel, NSView, NSWindow};
use objc2_core_foundation::CGSize;
use raw_window_handle::AppKitWindowHandle;

use crate::{Error, SizeLimits};

pub(crate) fn set(handle: AppKitWindowHandle, on: bool) -> Result<(), Error> {
    ns_window(handle)?.setLevel(if on {
        NSFloatingWindowLevel
    } else {
        NSNormalWindowLevel
    });
    Ok(())
}

pub(crate) fn set_size_limits(handle: AppKitWindowHandle, limits: SizeLimits) -> Result<(), Error> {
    let window = ns_window(handle)?;
    let size = |(w, h): (f32, f32)| CGSize::new(f64::from(w), f64::from(h));
    window.setContentMinSize(size(limits.min));
    window.setContentMaxSize(size(limits.max));
    Ok(())
}

fn ns_window(handle: AppKitWindowHandle) -> Result<Retained<NSWindow>, Error> {
    let view: &NSView = unsafe { handle.ns_view.cast::<NSView>().as_ref() };
    view.window()
        .ok_or_else(|| Error::Platform("the view has no window".into()))
}
