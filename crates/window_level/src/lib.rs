use raw_window_handle::{HasWindowHandle, RawWindowHandle};

#[cfg(target_os = "macos")]
mod macos;
#[cfg(target_os = "windows")]
mod win32;
#[cfg(target_os = "linux")]
mod x11;

#[derive(Debug, thiserror::Error)]
pub enum Error {
    #[error("this window system has no always-on-top")]
    Unsupported,
    #[error("no native window handle: {0}")]
    Handle(#[from] raw_window_handle::HandleError),
    #[error("{0}")]
    Platform(String),
}

pub fn supported(window: &impl HasWindowHandle) -> bool {
    window
        .window_handle()
        .is_ok_and(|handle| supported_handle(handle.as_raw()))
}

fn supported_handle(raw: RawWindowHandle) -> bool {
    match raw {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(_) => true,
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(_) => true,
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(_) | RawWindowHandle::Xlib(_) => true,
        _ => false,
    }
}

#[derive(Clone, Copy, Debug)]
pub struct SizeLimits {
    pub min: (f32, f32),
    pub max: (f32, f32),
}

pub fn set_size_limits(
    window: &impl HasWindowHandle,
    limits: SizeLimits,
    scale: f32,
) -> Result<(), Error> {
    match window.window_handle()?.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => macos::set_size_limits(handle, limits),
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(handle) => win32::set_max_size(handle, limits.max),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(handle) => x11::set_size_limits(handle.window.get(), limits, scale),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(handle) => x11::set_size_limits(handle.window as u32, limits, scale),
        _ => {
            let _ = (limits, scale);
            Err(Error::Unsupported)
        }
    }
}

pub fn set_always_on_top(window: &impl HasWindowHandle, on: bool) -> Result<(), Error> {
    match window.window_handle()?.as_raw() {
        #[cfg(target_os = "macos")]
        RawWindowHandle::AppKit(handle) => macos::set(handle, on),
        #[cfg(target_os = "windows")]
        RawWindowHandle::Win32(handle) => win32::set(handle, on),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xcb(handle) => x11::set(handle.window.get(), on),
        #[cfg(target_os = "linux")]
        RawWindowHandle::Xlib(handle) => x11::set(handle.window as u32, on),
        _ => Err(Error::Unsupported),
    }
}
