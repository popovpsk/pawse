use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

pub(crate) struct DefaultDeviceWatcher {
    changed: Arc<AtomicBool>,
    _inner: platform::Inner,
}

impl DefaultDeviceWatcher {
    pub(crate) fn start() -> Option<Self> {
        if cfg!(not(any(target_os = "windows", target_os = "linux"))) {
            return None;
        }
        let changed = Arc::new(AtomicBool::new(false));
        match platform::Inner::start(changed.clone()) {
            Ok(inner) => Some(Self {
                changed,
                _inner: inner,
            }),
            Err(e) => {
                log::warn!("audio output: can't watch the system default device: {e}");
                None
            }
        }
    }

    pub(crate) fn take_changed(&self) -> bool {
        self.changed.swap(false, Ordering::AcqRel)
    }
}

#[cfg(target_os = "windows")]
mod platform {
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use windows::Win32::Foundation::PROPERTYKEY;
    use windows::Win32::Media::Audio::{
        DEVICE_STATE, EDataFlow, ERole, IMMDeviceEnumerator, IMMNotificationClient,
        IMMNotificationClient_Impl, MMDeviceEnumerator, eConsole, eRender,
    };
    use windows::Win32::System::Com::{
        CLSCTX_ALL, COINIT_APARTMENTTHREADED, CoCreateInstance, CoInitializeEx,
    };
    use windows::core::{PCWSTR, implement};

    #[implement(IMMNotificationClient)]
    struct Client {
        changed: Arc<AtomicBool>,
    }

    impl IMMNotificationClient_Impl for Client_Impl {
        fn OnDeviceStateChanged(&self, _: &PCWSTR, _: DEVICE_STATE) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceAdded(&self, _: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDeviceRemoved(&self, _: &PCWSTR) -> windows::core::Result<()> {
            Ok(())
        }

        fn OnDefaultDeviceChanged(
            &self,
            flow: EDataFlow,
            role: ERole,
            _: &PCWSTR,
        ) -> windows::core::Result<()> {
            if flow == eRender && role == eConsole {
                self.changed.store(true, Ordering::Release);
            }
            Ok(())
        }

        fn OnPropertyValueChanged(&self, _: &PCWSTR, _: &PROPERTYKEY) -> windows::core::Result<()> {
            Ok(())
        }
    }

    pub(super) struct Inner {
        enumerator: IMMDeviceEnumerator,
        client: IMMNotificationClient,
    }

    unsafe impl Send for Inner {}
    unsafe impl Sync for Inner {}

    impl Inner {
        pub(super) fn start(changed: Arc<AtomicBool>) -> Result<Self, String> {
            let _ = unsafe { CoInitializeEx(None, COINIT_APARTMENTTHREADED) };
            let enumerator: IMMDeviceEnumerator =
                unsafe { CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL) }
                    .map_err(|e| format!("CoCreateInstance(MMDeviceEnumerator): {e}"))?;
            let client: IMMNotificationClient = Client { changed }.into();
            unsafe { enumerator.RegisterEndpointNotificationCallback(&client) }
                .map_err(|e| format!("RegisterEndpointNotificationCallback: {e}"))?;
            Ok(Self { enumerator, client })
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            let _ = unsafe {
                self.enumerator
                    .UnregisterEndpointNotificationCallback(&self.client)
            };
        }
    }
}

#[cfg(target_os = "linux")]
mod platform {
    use std::io::{BufRead, BufReader};
    use std::process::{Child, Command, Stdio};
    use std::sync::Arc;
    use std::sync::atomic::{AtomicBool, Ordering};

    use parking_lot::Mutex;

    use crate::device::pulse_default_sink;

    pub(super) struct Inner {
        child: Mutex<Child>,
    }

    impl Inner {
        pub(super) fn start(changed: Arc<AtomicBool>) -> Result<Self, String> {
            let mut child = Command::new("pactl")
                .arg("subscribe")
                .stdin(Stdio::null())
                .stdout(Stdio::piped())
                .stderr(Stdio::null())
                .spawn()
                .map_err(|e| format!("pactl subscribe: {e}"))?;
            let stdout = child.stdout.take().ok_or("pactl subscribe: no stdout")?;
            let mut last = pulse_default_sink();
            std::thread::Builder::new()
                .name("audio-default-watch".into())
                .spawn(move || {
                    for line in BufReader::new(stdout).lines() {
                        let Ok(line) = line else { break };
                        if !line.contains("on server") {
                            continue;
                        }
                        let current = pulse_default_sink();
                        if current.is_some() && current != last {
                            last = current;
                            changed.store(true, Ordering::Release);
                        }
                    }
                })
                .map_err(|e| format!("spawn watcher thread: {e}"))?;
            Ok(Self {
                child: Mutex::new(child),
            })
        }
    }

    impl Drop for Inner {
        fn drop(&mut self) {
            let child = self.child.get_mut();
            let _ = child.kill();
            let _ = child.wait();
        }
    }
}

#[cfg(not(any(target_os = "windows", target_os = "linux")))]
mod platform {
    use std::sync::Arc;
    use std::sync::atomic::AtomicBool;

    pub(super) struct Inner;

    impl Inner {
        pub(super) fn start(_: Arc<AtomicBool>) -> Result<Self, String> {
            Err("not needed on this platform".into())
        }
    }
}
