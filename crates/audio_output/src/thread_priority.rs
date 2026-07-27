pub use imp::RenderThreadPriority;

#[cfg(target_os = "windows")]
mod imp {
    use audio_thread_priority::{
        RtPriorityHandle, demote_current_thread_from_real_time, promote_current_thread_to_real_time,
    };

    pub struct RenderThreadPriority(Option<RtPriorityHandle>);

    impl RenderThreadPriority {
        pub fn acquire(buffer_frames: u32, sample_rate: u32) -> Self {
            match promote_current_thread_to_real_time(buffer_frames, sample_rate) {
                Ok(handle) => Self(Some(handle)),
                Err(e) => {
                    log::warn!("audio output: render thread stays at normal priority: {e}");
                    Self(None)
                }
            }
        }
    }

    impl Drop for RenderThreadPriority {
        fn drop(&mut self) {
            if let Some(handle) = self.0.take()
                && let Err(e) = demote_current_thread_from_real_time(handle)
            {
                log::debug!("audio output: could not restore render thread priority: {e}");
            }
        }
    }
}

#[cfg(not(target_os = "windows"))]
mod imp {
    pub struct RenderThreadPriority;

    impl RenderThreadPriority {
        pub fn acquire(_buffer_frames: u32, _sample_rate: u32) -> Self {
            Self
        }
    }
}

#[cfg(target_os = "macos")]
pub fn boost_decoder_thread() {
    let rc = unsafe {
        libc::pthread_set_qos_class_self_np(libc::qos_class_t::QOS_CLASS_USER_INITIATED, 0)
    };
    if rc != 0 {
        log::debug!("audio: decoder thread QoS unchanged (rc {rc})");
    }
}

#[cfg(target_os = "linux")]
pub fn boost_decoder_thread() {
    const DECODER_NICE: i32 = -5;
    let rc = unsafe { libc::setpriority(libc::PRIO_PROCESS, 0, DECODER_NICE) };
    if rc != 0 {
        log::debug!("audio: decoder thread niceness unchanged (needs RLIMIT_NICE)");
    }
}

#[cfg(target_os = "windows")]
pub fn boost_decoder_thread() {
    use windows::Win32::System::Threading::{
        GetCurrentThread, SetThreadPriority, THREAD_PRIORITY_ABOVE_NORMAL,
    };

    if let Err(e) = unsafe { SetThreadPriority(GetCurrentThread(), THREAD_PRIORITY_ABOVE_NORMAL) } {
        log::debug!("audio: decoder thread priority unchanged ({e})");
    }
}

#[cfg(not(any(target_os = "macos", target_os = "linux", target_os = "windows")))]
pub fn boost_decoder_thread() {}

#[cfg(all(test, target_os = "macos"))]
mod tests {
    #[test]
    fn boost_puts_the_decoder_thread_in_user_initiated_qos() {
        std::thread::spawn(|| {
            super::boost_decoder_thread();

            let mut class = libc::qos_class_t::QOS_CLASS_UNSPECIFIED;
            let mut relative = 0;
            let rc = unsafe {
                libc::pthread_get_qos_class_np(libc::pthread_self(), &mut class, &mut relative)
            };

            assert_eq!(rc, 0, "pthread_get_qos_class_np failed");
            assert_eq!(
                class as u32,
                libc::qos_class_t::QOS_CLASS_USER_INITIATED as u32,
                "decoder thread did not land in the user-initiated QoS class"
            );
        })
        .join()
        .unwrap();
    }
}
