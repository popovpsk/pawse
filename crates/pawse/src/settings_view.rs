use std::path::PathBuf;

use gpui::prelude::FluentBuilder as _;
use gpui::{App, Axis, ParentElement, SharedString, Styled, div, px};
use gpui_component::{
    ActiveTheme as _, Icon, IconName,
    button::{Button, ButtonVariants},
    h_flex,
    setting::{SettingField, SettingGroup, SettingItem, SettingPage, Settings},
    theme::ThemeRegistry,
    v_flex,
};

use crate::services::Services;
use crate::settings_store::{SettingsStore, ThemeChoice, notify_save_error};

/// Build the list of `SettingPage`s for the Settings widget.
///
/// Built once and cached on `MainView` (render runs at ~120fps; rebuilding
/// these closures each frame is wasted work). `SettingPage` is `Clone`, so the
/// cache is cloned cheaply into a fresh `Settings::new(...).pages(...)` shell
/// on each render.
///
/// Pass `cx` so the theme dropdown can enumerate all registered themes at build time.
pub fn build_settings_pages(cx: &App) -> Vec<SettingPage> {
    vec![
        SettingPage::new("Appearance").group(appearance_group(cx)),
        SettingPage::new("Library").group(library_group()),
    ]
}

/// Wrap pre-built pages into the `Settings` element for inline rendering.
pub fn settings_widget(pages: Vec<SettingPage>) -> Settings {
    Settings::new("pawse-settings").pages(pages)
}

/// Spawn an OS folder picker on a background thread, then save the chosen
/// folder to `SettingsStore` and trigger a library rescan. Shared between the
/// Settings view and the app menu's "Rescan" action.
pub fn pick_folder_and_rescan(cx: &mut App) {
    let (tx, rx) = flume::bounded::<PathBuf>(1);
    std::thread::spawn(move || {
        if let Some(path) = rfd::FileDialog::new().pick_folder() {
            let _ = tx.send(path);
        }
    });
    cx.spawn(async move |cx| {
        if let Ok(path) = rx.recv_async().await {
            cx.update(|cx| {
                let save_result = cx
                    .global_mut::<SettingsStore>()
                    .set_music_folder(path.clone());
                if let Err(e) = save_result {
                    notify_save_error(cx, e);
                }
                cx.global::<Services>().library.clear_and_rescan(path);
            })
            .ok();
        }
    })
    .detach();
}

fn appearance_group(cx: &App) -> SettingGroup {
    let mut theme_options: Vec<(SharedString, SharedString)> =
        vec![("system".into(), "System".into())];
    for cfg in ThemeRegistry::global(cx).sorted_themes() {
        let name: SharedString = cfg.name.clone();
        theme_options.push((name.clone(), name));
    }

    SettingGroup::new().item(
        SettingItem::new(
            "Theme",
            SettingField::dropdown(
                theme_options,
                |cx: &App| cx.global::<SettingsStore>().theme().as_key().into(),
                |val: SharedString, cx: &mut App| {
                    let choice = ThemeChoice::from_key(val.as_ref());
                    let save_result = cx.global_mut::<SettingsStore>().set_theme(choice.clone());
                    if let Err(e) = save_result {
                        notify_save_error(cx, e);
                    }
                    match choice {
                        ThemeChoice::System => {
                            gpui_component::theme::Theme::sync_system_appearance(None, cx)
                        }
                        ThemeChoice::Named(ref name) => {
                            crate::settings_store::apply_named_theme(name, cx)
                        }
                    }
                },
            ),
        )
        .description("Color scheme for the application"),
    )
}

fn library_group() -> SettingGroup {
    SettingGroup::new().item(
        SettingItem::new(
            "Music folder",
            SettingField::render(|_opts, _window, cx: &mut App| {
                let folder = cx.global::<SettingsStore>().music_folder().cloned();
                let folder_text: SharedString = folder
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned().into())
                    .unwrap_or_else(|| "Not set".into());

                v_flex()
                    .gap_3()
                    .w_full()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .px_3()
                            .py_2()
                            .rounded(px(6.))
                            .bg(cx.theme().muted)
                            .child(
                                Icon::new(IconName::Folder)
                                    .text_color(cx.theme().muted_foreground),
                            )
                            .child(
                                div()
                                    .flex_1()
                                    .text_sm()
                                    .truncate()
                                    .text_color(cx.theme().foreground)
                                    .child(folder_text),
                            ),
                    )
                    .child(
                        h_flex()
                            .gap_2()
                            .child(
                                Button::new("choose-folder")
                                    .label("Choose…")
                                    .on_click(|_, _, cx| pick_folder_and_rescan(cx)),
                            )
                            .when_some(folder, |row, path| {
                                let path_for_finder = path.clone();
                                row.child(
                                    Button::new("rescan")
                                        .ghost()
                                        .label("Rescan")
                                        .on_click(move |_, _, cx| {
                                            cx.global::<Services>()
                                                .library
                                                .clear_and_rescan(path.clone());
                                        }),
                                )
                                .child(
                                    Button::new("show-in-finder")
                                        .ghost()
                                        .label("Show in Finder")
                                        .on_click(move |_, _, _| {
                                            let _ = std::process::Command::new("open")
                                                .arg(&path_for_finder)
                                                .spawn();
                                        }),
                                )
                            }),
                    )
            }),
        )
        .layout(Axis::Vertical)
        .description("Folder scanned for music files"),
    )
}
