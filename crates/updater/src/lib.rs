use anyhow::Result;
use gpui::{
    App, AppContext as _, AsyncApp, Context, Entity, Global, SharedString, Task, WeakEntity,
    actions,
};
use gpui_component::WindowExt as _;
use gpui_component::notification::{Notification, NotificationType};
use semver::Version;
use std::sync::Arc;
use std::time::Duration;

mod github;
mod install;
mod version;

use install::Staged;

const POLL_INTERVAL: Duration = Duration::from_secs(6 * 60 * 60);

actions!(updater, [CheckForUpdates]);

struct UpdateToast;

struct StatusToast;

enum Status {
    Idle,
    Checking,
    Downloading,
    Ready { version: Version },
}

pub struct AutoUpdater {
    current_version: Version,
    status: Status,
    staged: Option<Arc<Staged>>,
    poll_task: Option<Task<()>>,
    in_flight: bool,
    enabled: bool,
    apply_on_quit: bool,
}

struct GlobalUpdater(Entity<AutoUpdater>);

impl Global for GlobalUpdater {}

pub fn init(cx: &mut App, current_version: &str, enabled: bool) {
    if !is_supported() {
        return;
    }
    let version = match version::parse(current_version) {
        Ok(version) => version,
        Err(error) => {
            log::error!("updater: invalid current version {current_version:?}: {error:#}");
            return;
        }
    };

    let updater = cx.new(|_| AutoUpdater {
        current_version: version,
        status: Status::Idle,
        staged: None,
        poll_task: None,
        in_flight: false,
        enabled,
        apply_on_quit: false,
    });
    cx.set_global(GlobalUpdater(updater.clone()));

    if enabled {
        updater.update(cx, |this, cx| this.start_polling(cx));
    }

    let on_quit = updater.clone();
    cx.on_app_quit(move |cx| {
        let this = on_quit.read(cx);
        let relaunch = this.apply_on_quit;
        let staged = (this.apply_on_quit || this.enabled)
            .then(|| this.staged.clone())
            .flatten();
        async move {
            if let Some(staged) = staged {
                staged.finalize_on_quit(relaunch);
            }
        }
    })
    .detach();
}

pub fn check_now(cx: &mut App) {
    if let Some(updater) = global(cx) {
        updater.update(cx, |this, cx| this.poll(true, cx));
    }
}

pub fn set_enabled(cx: &mut App, enabled: bool) {
    if let Some(updater) = global(cx) {
        updater.update(cx, |this, cx| this.set_enabled(enabled, cx));
    }
}

pub fn apply_and_restart(cx: &mut App) {
    let Some(updater) = global(cx) else {
        return;
    };
    if updater.read(cx).staged.is_none() {
        return;
    }
    #[cfg(target_os = "macos")]
    cx.restart();
    #[cfg(target_os = "windows")]
    {
        updater.update(cx, |this, _| this.apply_on_quit = true);
        cx.quit();
    }
    #[cfg(target_os = "linux")]
    {
        if let Some(path) = install::appimage_path() {
            cx.set_restart_path(path);
        }
        cx.restart();
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    let _ = updater;
}

pub fn handle(cx: &App) -> Option<Entity<AutoUpdater>> {
    global(cx)
}

pub fn is_supported() -> bool {
    #[cfg(any(target_os = "macos", target_os = "windows"))]
    {
        true
    }
    #[cfg(target_os = "linux")]
    {
        std::env::var_os("APPIMAGE").is_some()
    }
    #[cfg(not(any(target_os = "macos", target_os = "windows", target_os = "linux")))]
    {
        false
    }
}

fn global(cx: &App) -> Option<Entity<AutoUpdater>> {
    cx.try_global::<GlobalUpdater>()
        .map(|global| global.0.clone())
}

impl AutoUpdater {
    pub fn has_staged_update(&self) -> bool {
        self.staged.is_some()
    }

    fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        self.enabled = enabled;
        if enabled {
            if self.poll_task.is_none() {
                self.start_polling(cx);
            }
        } else {
            self.poll_task = None;
        }
    }

    fn start_polling(&mut self, cx: &mut Context<Self>) {
        let task = cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            loop {
                if this.update(cx, |this, cx| this.poll(false, cx)).is_err() {
                    break;
                }
                cx.background_executor().timer(POLL_INTERVAL).await;
            }
        });
        self.poll_task = Some(task);
    }

    fn poll(&mut self, manual: bool, cx: &mut Context<Self>) {
        if self.in_flight {
            return;
        }
        if let Status::Ready { version } = &self.status {
            if manual {
                let version = version.clone();
                toast_ready(cx, &version);
            }
            return;
        }

        self.in_flight = true;
        self.status = Status::Checking;
        cx.notify();
        let current = self.current_version.clone();

        cx.spawn(async move |this: WeakEntity<Self>, cx: &mut AsyncApp| {
            let outcome = check_and_stage(current, this.clone(), cx).await;
            this.update(cx, |this, cx| {
                this.in_flight = false;
                cx.notify();
            })
            .ok();
            match outcome {
                Ok(Some((version, staged))) => {
                    let toast_version = version.clone();
                    this.update(cx, move |this, cx| {
                        this.staged = Some(Arc::new(staged));
                        this.status = Status::Ready { version };
                        cx.notify();
                    })
                    .ok();
                    cx.update(|cx| toast_ready(cx, &toast_version)).ok();
                }
                Ok(None) => {
                    this.update(cx, |this, cx| {
                        this.status = Status::Idle;
                        cx.notify();
                    })
                    .ok();
                    if manual {
                        cx.update(|cx| {
                            toast(
                                cx,
                                ui_resources::i18n::strings().up_to_date.clone(),
                                NotificationType::Info,
                            )
                        })
                        .ok();
                    }
                }
                Err(error) => {
                    this.update(cx, |this, cx| {
                        this.status = Status::Idle;
                        cx.notify();
                    })
                    .ok();
                    log::error!("updater: {error:#}");
                    if manual {
                        cx.update(|cx| {
                            toast(
                                cx,
                                ui_resources::i18n::strings()
                                    .update_check_failed(&error.to_string()),
                                NotificationType::Error,
                            )
                        })
                        .ok();
                    }
                }
            }
        })
        .detach();
    }
}

async fn check_and_stage(
    current: Version,
    this: WeakEntity<AutoUpdater>,
    cx: &mut AsyncApp,
) -> Result<Option<(Version, Staged)>> {
    let found = cx
        .background_executor()
        .spawn(async { github::fetch_latest() })
        .await?;

    if !version::is_newer(&current, &found.version) {
        return Ok(None);
    }

    this.update(cx, |this, cx| {
        this.status = Status::Downloading;
        cx.notify();
    })
    .ok();

    let app_bundle = cx.update(|cx| cx.app_path().ok())?;
    let url = found.url.clone();
    let staged = cx
        .background_executor()
        .spawn(async move { install::download_and_stage(&url, app_bundle) })
        .await?;

    Ok(Some((found.version, staged)))
}

fn toast_ready(cx: &mut App, version: &Version) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    let message =
        SharedString::from(ui_resources::i18n::strings().update_ready(&version.to_string()));
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(
            Notification::new()
                .id::<UpdateToast>()
                .message(message)
                .with_type(NotificationType::Success)
                .autohide(false)
                .on_click(|_, _, _| {}),
            cx,
        );
    });
}

fn toast(cx: &mut App, message: impl Into<SharedString>, kind: NotificationType) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    let message = message.into();
    let _ = handle.update(cx, |_, window, cx| {
        window.push_notification(
            Notification::new()
                .id::<StatusToast>()
                .message(message)
                .with_type(kind),
            cx,
        );
    });
}
