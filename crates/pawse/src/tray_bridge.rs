use std::sync::Arc;

use gpui::{App, Global};
use system_tray::{EventSink, IconImage, Tray, TrayConfig, TrayError, TrayEvent, TrayLabels};

use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::SettingsStore;

const ICON_PNGS: [&[u8]; 2] = [
    include_bytes!("../../ui_resources/assets/app-icon/32x32.png"),
    include_bytes!("../../ui_resources/assets/app-icon/128x128.png"),
];

struct TrayHost {
    tray: Option<Tray>,
    wanted: bool,
    generation: u64,
    sink: EventSink,
}

impl Global for TrayHost {}

pub fn setup(cx: &mut App) {
    let (tx, rx) = flume::unbounded::<TrayEvent>();
    cx.set_global(TrayHost {
        tray: None,
        wanted: false,
        generation: 0,
        sink: Arc::new(move |event| {
            let _ = tx.send(event);
        }),
    });
    cx.spawn(async move |cx| {
        while let Ok(event) = rx.recv_async().await {
            cx.update(|cx| match event {
                TrayEvent::Show => crate::raise_main_window(cx),
                TrayEvent::Quit => cx.quit(),
            });
        }
    })
    .detach();

    cx.observe_global::<SettingsStore>(|cx| {
        let wanted = cx.global::<SettingsStore>().close_to_tray();
        if wanted != cx.global::<TrayHost>().wanted {
            apply(wanted, cx);
        }
    })
    .detach();
    let lang_event_bus = cx.global::<Services>().lang_event_bus.clone();
    cx.subscribe(&lang_event_bus, |_, _: &LangChanged, cx| relabel(cx))
        .detach();

    let wanted = cx.global::<SettingsStore>().close_to_tray();
    apply(wanted, cx);
}

pub fn keeps_app(cx: &App) -> bool {
    shown(cx).is_some()
}

fn shown(cx: &App) -> Option<&Tray> {
    cx.try_global::<TrayHost>()
        .and_then(|host| host.tray.as_ref())
        .filter(|tray| tray.is_shown())
}

pub fn park(cx: &mut App) -> bool {
    let Some(tray) = shown(cx) else {
        return false;
    };
    #[cfg(target_os = "windows")]
    {
        let hwnd = tray.window_handle();
        crate::media_bridge::bind_window(Some(hwnd), cx);
    }
    #[cfg(not(target_os = "windows"))]
    let _ = tray;
    true
}

pub fn shutdown(cx: &mut App) {
    if cx.has_global::<TrayHost>() {
        let host = cx.global_mut::<TrayHost>();
        host.generation += 1;
        host.tray = None;
    }
}

fn apply(wanted: bool, cx: &mut App) {
    let host = cx.global_mut::<TrayHost>();
    host.wanted = wanted;
    host.generation += 1;
    if !wanted {
        host.tray = None;
        return;
    }
    let generation = host.generation;
    let sink = host.sink.clone();
    #[cfg(target_os = "linux")]
    {
        let task = cx
            .background_executor()
            .spawn(async move { Tray::new(config(), sink) });
        cx.spawn(async move |cx| {
            let result = task.await;
            cx.update(|cx| install(generation, result, cx));
        })
        .detach();
    }
    #[cfg(target_os = "windows")]
    install(generation, Tray::new(config(), sink), cx);
}

fn install(generation: u64, result: Result<Tray, TrayError>, cx: &mut App) {
    let host = cx.global_mut::<TrayHost>();
    if host.generation != generation {
        return;
    }
    match result {
        Ok(tray) => host.tray = Some(tray),
        Err(err) => log::warn!("{err}; closing the window quits"),
    }
}

fn relabel(cx: &mut App) {
    let host = cx.global::<TrayHost>();
    if let Some(tray) = &host.tray {
        tray.set_labels(&labels());
    } else if host.wanted {
        apply(true, cx);
    }
}

fn config() -> TrayConfig {
    TrayConfig {
        id: "pawse".into(),
        title: "Pawse".into(),
        labels: labels(),
        icons: ICON_PNGS.into_iter().filter_map(decode_icon).collect(),
    }
}

fn labels() -> TrayLabels {
    TrayLabels {
        show: ui_resources::i18n::tray_strings().show.to_string(),
        quit: tr().quit_pawse.to_string(),
    }
}

fn decode_icon(png: &[u8]) -> Option<IconImage> {
    let image = image::load_from_memory_with_format(png, image::ImageFormat::Png)
        .inspect_err(|err| log::warn!("tray: icon decode failed: {err}"))
        .ok()?
        .into_rgba8();
    Some(IconImage {
        width: image.width(),
        height: image.height(),
        rgba: image.into_raw(),
    })
}
