use gpui::App;
use interprocess::local_socket::{GenericNamespaced, ListenerOptions, Name, prelude::*};

const SOCKET_NAME: &str = "dev.pawse.app.sock";
const MAX_CONSECUTIVE_ACCEPT_ERRORS: u32 = 16;

pub enum Acquire {
    First(Option<LocalSocketListener>),
    Duplicate,
}

fn socket_name() -> std::io::Result<Name<'static>> {
    SOCKET_NAME.to_ns_name::<GenericNamespaced>()
}

pub fn acquire() -> Acquire {
    let Ok(name) = socket_name() else {
        log::warn!("single-instance: invalid socket name");
        return Acquire::First(None);
    };

    if LocalSocketStream::connect(name.clone()).is_ok() {
        return Acquire::Duplicate;
    }

    match ListenerOptions::new().name(name.clone()).create_sync() {
        Ok(listener) => Acquire::First(Some(listener)),
        Err(err) => {
            if LocalSocketStream::connect(name).is_ok() {
                Acquire::Duplicate
            } else {
                log::warn!("single-instance: failed to bind listener: {err}");
                Acquire::First(None)
            }
        }
    }
}

pub fn install(cx: &mut App, listener: Option<LocalSocketListener>) {
    let Some(listener) = listener else {
        return;
    };

    let (tx, rx) = flume::unbounded::<()>();

    if let Err(err) = std::thread::Builder::new()
        .name("single-instance".into())
        .spawn(move || {
            let mut consecutive_errors = 0u32;
            loop {
                match listener.accept() {
                    Ok(_) => {
                        consecutive_errors = 0;
                        if tx.send(()).is_err() {
                            break;
                        }
                    }
                    Err(err) => {
                        log::warn!("single-instance: accept failed: {err}");
                        consecutive_errors += 1;
                        if consecutive_errors >= MAX_CONSECUTIVE_ACCEPT_ERRORS {
                            log::warn!(
                                "single-instance: stopping listener after repeated failures"
                            );
                            break;
                        }
                    }
                }
            }
        })
    {
        log::warn!("single-instance: failed to spawn listener thread: {err}");
        return;
    }

    cx.spawn(async move |cx| {
        while rx.recv_async().await.is_ok() {
            let _ = cx.update(raise_to_front);
        }
    })
    .detach();
}

fn raise_to_front(cx: &mut App) {
    cx.activate(true);
    if let Some(handle) = cx.windows().into_iter().next() {
        let _ = handle.update(cx, |_, window, _| window.activate_window());
    } else {
        crate::open_initial_window(cx, false);
    }
}
