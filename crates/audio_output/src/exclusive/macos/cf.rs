// Minimal CoreFoundation bindings — just enough to create/release CFStringRefs
// for CoreAudio property qualifier data and assertion names.

use std::ffi::c_void;

pub(super) type CFStringRef = *const c_void;

pub(super) const K_CF_STRING_ENCODING_UTF8: u32 = 0x08000100;

#[link(name = "CoreFoundation", kind = "framework")]
unsafe extern "C" {
    pub(super) fn CFStringCreateWithBytes(
        alloc: *const c_void,
        bytes: *const u8,
        num_bytes: isize,
        encoding: u32,
        is_external_representation: u8,
    ) -> CFStringRef;

    pub(super) fn CFStringGetLength(cf: CFStringRef) -> isize;

    pub(super) fn CFStringGetCString(
        cf: CFStringRef,
        buffer: *mut u8,
        buffer_size: isize,
        encoding: u32,
    ) -> u8;

    pub(super) fn CFRelease(cf: *const c_void);
}

/// Creates a CFString from a Rust string slice. Returns null on failure.
/// Caller is responsible for `CFRelease`.
pub(super) fn cfstring_from_str(s: &str) -> CFStringRef {
    unsafe {
        CFStringCreateWithBytes(
            std::ptr::null(),
            s.as_ptr(),
            s.len() as isize,
            K_CF_STRING_ENCODING_UTF8,
            0,
        )
    }
}

/// Best-effort conversion of a CFString to a Rust `String`. Returns `None`
/// on null input or any extraction failure.
pub(super) fn cfstring_to_string(cf: CFStringRef) -> Option<String> {
    if cf.is_null() {
        return None;
    }
    // Worst-case: 4 bytes per UTF-16 code unit (length is in UTF-16 units),
    // plus NUL terminator. CFStringGetMaximumSizeForEncoding would be more
    // exact but we don't bind it; this overhead is negligible for device UIDs.
    let len = unsafe { CFStringGetLength(cf) };
    if len < 0 {
        return None;
    }
    let buf_size = (len as usize).saturating_mul(4).saturating_add(1);
    let mut buf = vec![0u8; buf_size];
    let ok = unsafe {
        CFStringGetCString(
            cf,
            buf.as_mut_ptr(),
            buf_size as isize,
            K_CF_STRING_ENCODING_UTF8,
        )
    };
    if ok == 0 {
        return None;
    }
    // Trim at first NUL.
    let nul = buf.iter().position(|&b| b == 0).unwrap_or(buf.len());
    buf.truncate(nul);
    String::from_utf8(buf).ok()
}
