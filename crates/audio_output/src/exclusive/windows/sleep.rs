// Prevents system sleep during playback via SetThreadExecutionState.
// Mirrors the macOS `SleepPreventer` (IOPMAssertion) contract.

use windows::Win32::System::Power::{
    ES_CONTINUOUS, ES_SYSTEM_REQUIRED, EXECUTION_STATE, SetThreadExecutionState,
};

pub(super) struct SleepPreventer {
    active: bool,
}

impl SleepPreventer {
    pub(super) fn new() -> Self {
        Self { active: false }
    }

    pub(super) fn prevent(&mut self) {
        if self.active {
            return;
        }
        // SetThreadExecutionState returns the previous state (0 on failure).
        let prev = unsafe { SetThreadExecutionState(ES_CONTINUOUS | ES_SYSTEM_REQUIRED) };
        if prev != EXECUTION_STATE(0) {
            self.active = true;
        }
    }

    pub(super) fn allow(&mut self) {
        if self.active {
            unsafe { SetThreadExecutionState(ES_CONTINUOUS) };
            self.active = false;
        }
    }
}

impl Drop for SleepPreventer {
    fn drop(&mut self) {
        self.allow();
    }
}
