// IOPMAssertion bindings for preventing system sleep during playback.
// Replaces the fragile `caffeinate` subprocess approach.

use std::ptr::null;

use super::cf::{CFRelease, CFStringRef, cfstring_from_str};

type IOPMAssertionID = u32;
type IOReturn = i32;
type CFTimeInterval = f64;

#[link(name = "IOKit", kind = "framework")]
unsafe extern "C" {
    fn IOPMAssertionCreateWithDescription(
        assertion_type: CFStringRef,
        name: CFStringRef,
        details: CFStringRef,
        human_readable_reason: CFStringRef,
        localization_bundle_path: CFStringRef,
        timeout: CFTimeInterval,
        timeout_action: CFStringRef,
        assertion_id: *mut IOPMAssertionID,
    ) -> IOReturn;

    fn IOPMAssertionRelease(assertion_id: IOPMAssertionID) -> IOReturn;
}

const K_IORETURN_SUCCESS: i32 = 0;

pub(super) struct SleepPreventer {
    assertion_id: IOPMAssertionID,
    active: bool,
}

impl SleepPreventer {
    pub(super) fn new() -> Self {
        Self {
            assertion_id: 0,
            active: false,
        }
    }

    pub(super) fn prevent(&mut self) {
        if self.active {
            log::warn!(
                "coreaudio: SleepPreventer::prevent called while already active — assertion leak?"
            );
            return;
        }
        let assertion_type = cfstring_from_str("PreventUserIdleSystemSleep");
        let name = cfstring_from_str("Pawse playback");
        if assertion_type.is_null() || name.is_null() {
            if !assertion_type.is_null() {
                unsafe { CFRelease(assertion_type) };
            }
            if !name.is_null() {
                unsafe { CFRelease(name) };
            }
            log::warn!("coreaudio: failed to wrap sleep-assertion strings");
            return;
        }
        let mut assertion_id: IOPMAssertionID = 0;
        let ret = unsafe {
            IOPMAssertionCreateWithDescription(
                assertion_type,
                name,
                null(),
                null(),
                null(),
                0.0,
                null(),
                &mut assertion_id,
            )
        };
        unsafe {
            CFRelease(assertion_type);
            CFRelease(name);
        }
        if ret == K_IORETURN_SUCCESS {
            self.assertion_id = assertion_id;
            self.active = true;
        } else {
            log::warn!(
                "coreaudio: IOPMAssertionCreateWithDescription failed: {:#x}",
                ret
            );
        }
    }

    pub(super) fn allow(&mut self) {
        if self.active {
            let ret = unsafe { IOPMAssertionRelease(self.assertion_id) };
            if ret != K_IORETURN_SUCCESS {
                log::warn!("coreaudio: IOPMAssertionRelease failed: {:#x}", ret);
            }
            self.assertion_id = 0;
            self.active = false;
        }
    }
}

impl Drop for SleepPreventer {
    fn drop(&mut self) {
        self.allow();
    }
}
