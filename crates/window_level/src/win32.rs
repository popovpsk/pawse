use ::windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use ::windows::Win32::UI::HiDpi::GetDpiForWindow;
use ::windows::Win32::UI::Shell::{DefSubclassProc, RemoveWindowSubclass, SetWindowSubclass};
use ::windows::Win32::UI::WindowsAndMessaging::{
    GetClientRect, GetWindowRect, HWND_NOTOPMOST, HWND_TOPMOST, MINMAXINFO, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowPos, WM_GETMINMAXINFO, WM_NCDESTROY,
};
use raw_window_handle::Win32WindowHandle;

use crate::Error;

pub(crate) fn set(handle: Win32WindowHandle, on: bool) -> Result<(), Error> {
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
    let after = if on { HWND_TOPMOST } else { HWND_NOTOPMOST };
    unsafe {
        SetWindowPos(
            hwnd,
            Some(after),
            0,
            0,
            0,
            0,
            SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
        )
    }
    .map_err(|e| Error::Platform(e.to_string()))
}

const MAX_SIZE_SUBCLASS: usize = 0x5057_4d58;

pub(crate) fn set_max_size(
    handle: Win32WindowHandle,
    (width, height): (f32, f32),
) -> Result<(), Error> {
    let hwnd = HWND(handle.hwnd.get() as *mut core::ffi::c_void);
    let packed = (width.max(0.) as usize) << 16 | (height.max(0.) as usize & 0xffff);
    let ok = unsafe { SetWindowSubclass(hwnd, Some(max_size_proc), MAX_SIZE_SUBCLASS, packed) };
    if ok.as_bool() {
        Ok(())
    } else {
        Err(Error::Platform("SetWindowSubclass failed".into()))
    }
}

unsafe extern "system" fn max_size_proc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    id: usize,
    packed: usize,
) -> LRESULT {
    let result = unsafe { DefSubclassProc(hwnd, msg, wparam, lparam) };
    match msg {
        WM_GETMINMAXINFO => {
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.;
            let (mut window, mut client) = (RECT::default(), RECT::default());
            let measured = unsafe { GetWindowRect(hwnd, &mut window) }.is_ok()
                && unsafe { GetClientRect(hwnd, &mut client) }.is_ok();
            let (frame_w, frame_h) = if measured {
                (
                    (window.right - window.left) - (client.right - client.left),
                    (window.bottom - window.top) - (client.bottom - client.top),
                )
            } else {
                (0, 0)
            };
            let width = ((packed >> 16) & 0xffff) as f32;
            let height = (packed & 0xffff) as f32;
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMaxTrackSize.x = (width * scale) as i32 + frame_w;
            info.ptMaxTrackSize.y = (height * scale) as i32 + frame_h;
        }
        WM_NCDESTROY => {
            let _ = unsafe { RemoveWindowSubclass(hwnd, Some(max_size_proc), id) };
        }
        _ => {}
    }
    result
}
