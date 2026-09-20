use diagnostics::{Notice, Severity};
use gpui::{App, AsyncApp, SharedString};
use gpui_component::WindowExt;
use gpui_component::notification::Notification;

pub fn spawn_notice_forwarder(cx: &mut App, notices: flume::Receiver<Notice>) {
    cx.spawn(async move |cx: &mut AsyncApp| {
        while let Ok(notice) = notices.recv_async().await {
            cx.update(|cx| push_notice(cx, notice));
        }
    })
    .detach();
}

fn push_notice(cx: &mut App, notice: Notice) {
    let Some(handle) = cx.active_window() else {
        return;
    };
    let _ = handle.update(cx, |_, window, cx| {
        let notification = match notice.severity {
            Severity::Error => Notification::error(notice.message),
            Severity::Warning => Notification::warning(notice.message),
        };
        window.push_notification(notification.title(notice.title), cx);
    });
}

pub fn push_background_error(
    cx: &mut App,
    title: impl Into<SharedString>,
    message: impl Into<SharedString>,
) -> bool {
    let Some(handle) = cx.active_window().or_else(|| cx.windows().first().copied()) else {
        return false;
    };
    let (title, message) = (title.into(), message.into());
    handle
        .update(cx, |_, window, cx| {
            window.push_notification(
                Notification::error(message).title(title).autohide(false),
                cx,
            );
        })
        .is_ok()
}
