use std::time::Duration;

use gpui::{
    App, AppContext, Entity, IntoElement, ParentElement, SharedString, Styled, Window, div,
    prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable, Selectable, Sizable,
    button::{Button, ButtonGroup},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};

use crate::library_sources::{ConnectState, LibrarySources, describe_error};
use crate::localization::tr;
use crate::remote_settings::{added, server_list, set_connecting, torrent_remote};
use crate::servers::ServerKind;
use crate::services::Services;
use crate::settings_store::{SettingsStore, TorrentSource, TorrentUpload, notify_save_error};
use crate::theme_colors::Colors;

const RESOLVE_TIMEOUT: Duration = Duration::from_secs(120);
const IDLE_UNLOAD: Duration = Duration::from_secs(15 * 60);
const WORK_LIMIT_BYTES: u64 = 4 * 1024 * 1024 * 1024;

pub fn torrent_host(store: &SettingsStore) -> std::sync::Arc<crate::servers::torrent::TorrentHost> {
    let base = dirs::cache_dir()
        .unwrap_or_else(std::env::temp_dir)
        .join("pawse");
    let state = dirs::data_dir()
        .unwrap_or_else(|| base.clone())
        .join("pawse")
        .join("torrents");
    let config = torrent::Config {
        work_dir: base.join("torrent"),
        state_dir: state,
        upload: store.torrent_upload().engine(),
        idle_unload: IDLE_UNLOAD,
        work_limit_bytes: WORK_LIMIT_BYTES,
        network: torrent::Network::Public,
    };
    crate::servers::torrent::TorrentHost::new(config)
}

pub fn parse_input(text: &str) -> Option<torrent::Input> {
    let text = text.trim();
    let is_magnet = text
        .get(..8)
        .is_some_and(|scheme| scheme.eq_ignore_ascii_case("magnet:?"));
    if is_magnet && text.len() > 8 {
        return Some(torrent::Input::Magnet(text.to_string()));
    }
    let bare = text.len() == 40 && text.bytes().all(|b| b.is_ascii_hexdigit())
        || text.len() == 32 && text.bytes().all(|b| b.is_ascii_alphanumeric());
    bare.then(|| torrent::Input::Magnet(format!("magnet:?xt=urn:btih:{text}")))
}

fn set_state(sources: &Entity<LibrarySources>, state: ConnectState, cx: &mut App) {
    set_connecting(sources, ServerKind::Torrent, state, cx);
}

fn fail(sources: &Entity<LibrarySources>, error: SharedString, cx: &mut App) {
    set_state(
        sources,
        ConnectState {
            connecting: false,
            error: Some(error),
        },
        cx,
    );
}

fn add(
    sources: Entity<LibrarySources>,
    input: torrent::Input,
    magnet: Option<Entity<InputState>>,
    window: &mut Window,
    cx: &mut App,
) {
    set_state(
        &sources,
        ConnectState {
            connecting: true,
            error: None,
        },
        cx,
    );
    let handle = window.window_handle();
    let host = cx.global::<Services>().torrents.clone();
    cx.spawn(async move |cx| {
        let resolved = cx
            .background_spawn(async move {
                let _state = host.state_lock();
                host.engine()
                    .ok_or_else(|| torrent::Error::Other("torrents are unavailable".into()))?
                    .resolve(input, RESOLVE_TIMEOUT)
            })
            .await;
        cx.update(|cx| {
            let meta = match resolved {
                Ok(meta) => meta,
                Err(e) => {
                    let error = describe_error(&crate::servers::torrent::error(e));
                    fail(&sources, error, cx);
                    return;
                }
            };
            set_state(&sources, ConnectState::default(), cx);
            let source = TorrentSource {
                info_hash: meta.info_hash,
                name: meta.name,
            };
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .add_torrent_source(source.clone())
            {
                notify_save_error(cx, e);
            }
            let host = cx.global::<Services>().torrents.clone();
            added(torrent_remote(&source, &host), cx);
            if let Some(magnet) = magnet {
                let _ = handle.update(cx, |_, window, cx| {
                    magnet.update(cx, |state, cx| state.set_value("", window, cx));
                });
            }
        });
    })
    .detach();
}

fn add_magnet(
    sources: Entity<LibrarySources>,
    magnet: Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    match parse_input(&magnet.read(cx).value()) {
        Some(input) => add(sources, input, Some(magnet), window, cx),
        None => fail(&sources, tr().torrent_invalid.clone(), cx),
    }
}

fn add_file(sources: Entity<LibrarySources>, window: &mut Window, cx: &mut App) {
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let Some(picked) = rfd::AsyncFileDialog::new()
            .add_filter("BitTorrent", &["torrent"])
            .pick_file()
            .await
        else {
            return;
        };
        let bytes = picked.read().await;
        let _ = handle.update(cx, |_, window, cx| {
            add(sources, torrent::Input::File(bytes), None, window, cx);
        });
    })
    .detach();
}

fn apply_upload(upload: TorrentUpload, cx: &mut App) {
    if let Err(e) = cx.global_mut::<SettingsStore>().set_torrent_upload(upload) {
        notify_save_error(cx, e);
    }
    cx.global::<Services>().torrents.set_upload(upload.engine());
    cx.refresh_windows();
}

pub fn torrent_group(sources: Entity<LibrarySources>, magnet: Entity<InputState>) -> SettingGroup {
    let upload_labels: [SharedString; 3] = [
        tr().torrent_upload_while_active.clone(),
        tr().torrent_upload_limited.clone(),
        tr().torrent_upload_off.clone(),
    ];
    SettingGroup::new()
        .title(tr().torrents.clone())
        .item(
            SettingItem::unlabeled(SettingField::render(move |_window, cx: &mut App| {
                let list = server_list(sources.read(cx), ServerKind::Torrent, cx);
                let state = sources.read(cx).connect_state(ServerKind::Torrent);
                let for_magnet = sources.clone();
                let for_file = sources.clone();
                let input = magnet.clone();
                let form = v_flex()
                    .gap_2()
                    .child(
                        h_flex()
                            .flex_wrap()
                            .gap_2()
                            .child(
                                div()
                                    .flex_1()
                                    .min_w(px(240.))
                                    .max_w(px(380.))
                                    .child(Input::new(&magnet).small()),
                            )
                            .child(
                                Button::new("torrent-add")
                                    .small()
                                    .label(tr().torrent_add.clone())
                                    .loading(state.connecting)
                                    .disabled(state.connecting)
                                    .on_click(move |_, window, cx| {
                                        add_magnet(for_magnet.clone(), input.clone(), window, cx);
                                    }),
                            )
                            .child(
                                Button::new("torrent-file")
                                    .small()
                                    .label(tr().torrent_choose_file.clone())
                                    .disabled(state.connecting)
                                    .on_click(move |_, window, cx| {
                                        add_file(for_file.clone(), window, cx);
                                    }),
                            ),
                    )
                    .when_some(state.error, |form, error| {
                        form.child(
                            div()
                                .min_w(px(0.))
                                .max_w_full()
                                .text_xs()
                                .text_color(Colors::danger(cx))
                                .child(error),
                        )
                    });
                v_flex().gap_3().w_full().children(list).child(form)
            }))
            .description(tr().torrents_desc.clone()),
        )
        .item(
            SettingItem::new(
                tr().torrent_upload.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    let current = cx.global::<SettingsStore>().torrent_upload();
                    TorrentUpload::ALL
                        .iter()
                        .zip(&upload_labels)
                        .enumerate()
                        .fold(
                            ButtonGroup::new("torrent-upload").small(),
                            |group, (ix, (&upload, label))| {
                                group.child(
                                    Button::new(("torrent-upload", ix))
                                        .label(label.clone())
                                        .selected(current == upload),
                                )
                            },
                        )
                        .on_click(|clicks: &Vec<usize>, _, cx| {
                            if let Some(&upload) =
                                clicks.first().and_then(|&ix| TorrentUpload::ALL.get(ix))
                            {
                                apply_upload(upload, cx);
                            }
                        })
                        .into_any_element()
                }),
            )
            .description(tr().torrent_upload_desc.clone()),
        )
}

pub fn magnet_input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(tr().torrent_magnet.clone()))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn magnets_and_bare_hashes_are_accepted_and_anything_else_is_not() {
        assert_eq!(
            parse_input("  magnet:?xt=urn:btih:ABC&dn=x "),
            Some(torrent::Input::Magnet(
                "magnet:?xt=urn:btih:ABC&dn=x".into()
            ))
        );
        let hex = "c00e22aab66c00c9cc85a0c98ee202b78b39dbcc";
        assert_eq!(
            parse_input(hex),
            Some(torrent::Input::Magnet(format!("magnet:?xt=urn:btih:{hex}")))
        );
        assert_eq!(parse_input("https://example.com/a.torrent"), None);
        assert_eq!(parse_input(""), None);
        assert_eq!(parse_input("magnet:?"), None);
        assert_eq!(parse_input("日本語の曲名"), None);
        assert_eq!(parse_input("mag日本"), None);
    }
}
