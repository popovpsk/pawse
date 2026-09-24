use gpui::{
    AnyElement, App, AppContext, Axis, Entity, IntoElement, ParentElement, SharedString, Styled,
    Window, div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{
    Disableable, Sizable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};

use crate::library_sources::{ConnectState, LibrarySources, ServerStatus};
use crate::localization::tr;
use crate::servers::{RemoteConfig, RemoteServer, ServerKind};
use crate::services::Services;
use crate::settings_store::{JellyfinServer, SettingsStore, SubsonicServer, notify_save_error};
use crate::theme_colors::Colors;

const OFFLINE_RETRY: std::time::Duration = std::time::Duration::from_secs(60);

#[derive(Clone)]
pub struct ServerInputs {
    pub url: Entity<InputState>,
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
}

impl ServerInputs {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            url: cx.new(|cx| InputState::new(window, cx).placeholder(tr().server_url.clone())),
            username: cx
                .new(|cx| InputState::new(window, cx).placeholder(tr().server_username.clone())),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr().server_password.clone())
            }),
        }
    }

    pub fn read(&self, cx: &App) -> Option<(String, String, String)> {
        let url = self.url.read(cx).value().trim().to_string();
        let username = self.username.read(cx).value().trim().to_string();
        let password = self.password.read(cx).value().to_string();
        if url.is_empty() || username.is_empty() {
            return None;
        }
        let url = if url.contains("://") {
            url
        } else {
            format!("http://{url}")
        };
        Some((url, username, password))
    }

    pub fn clear(&self, window: &mut Window, cx: &mut App) {
        for input in [&self.url, &self.username, &self.password] {
            input.update(cx, |state, cx| state.set_value("", window, cx));
        }
    }
}

pub type Connect = fn(Entity<LibrarySources>, ServerInputs, &mut Window, &mut App);

pub fn subsonic_remote(server: &SubsonicServer) -> RemoteServer {
    RemoteServer {
        uri: server.source_uri(),
        name: server.normalized_url(),
        config: RemoteConfig::Subsonic(server.config()),
    }
}

pub fn jellyfin_remote(server: &JellyfinServer) -> RemoteServer {
    RemoteServer {
        uri: server.source_uri(),
        name: server.normalized_url(),
        config: RemoteConfig::Jellyfin(server.config()),
    }
}

pub fn configured_servers(
    subsonic: &[SubsonicServer],
    jellyfin: &[JellyfinServer],
) -> Vec<RemoteServer> {
    subsonic
        .iter()
        .map(subsonic_remote)
        .chain(jellyfin.iter().map(jellyfin_remote))
        .collect()
}

pub fn has_servers(store: &SettingsStore) -> bool {
    !store.subsonic_servers().is_empty() || !store.jellyfin_servers().is_empty()
}

pub fn remote_servers(cx: &App) -> Vec<RemoteServer> {
    let store = cx.global::<SettingsStore>();
    configured_servers(store.subsonic_servers(), store.jellyfin_servers())
}

pub fn apply_remote_sources(cx: &mut App) {
    let servers = remote_servers(cx);
    let services = cx.global::<Services>();
    let configs = services.library.reconcile_remote(&servers);
    services.remote_media.set_servers(configs);
}

pub fn sync_all(cx: &mut App) {
    apply_remote_sources(cx);
    let servers = remote_servers(cx);
    cx.global::<Services>().library.sync_remote(servers);
}

pub fn sync_offline(cx: &mut App) {
    let servers = remote_servers(cx);
    cx.global::<Services>().library.sync_offline_remote(servers);
}

pub fn watch_offline_servers(cx: &mut App) {
    cx.spawn(async move |cx| {
        loop {
            cx.background_executor().timer(OFFLINE_RETRY).await;
            cx.update(sync_offline);
        }
    })
    .detach();
}

pub fn set_connecting(
    sources: &Entity<LibrarySources>,
    kind: ServerKind,
    state: ConnectState,
    cx: &mut App,
) {
    sources.update(cx, |sources, cx| {
        sources.set_connect_state(kind, state);
        cx.notify();
    });
}

pub fn added(server: RemoteServer, cx: &mut App) {
    apply_remote_sources(cx);
    cx.global::<Services>().library.sync_remote(vec![server]);
}

fn remove_server(server: &RemoteServer, cx: &mut App) {
    let removed = cx
        .global_mut::<SettingsStore>()
        .remove_server(server.kind(), &server.uri);
    if let Err(e) = removed {
        notify_save_error(cx, e);
    }
    apply_remote_sources(cx);
    cx.global::<Services>()
        .library
        .refresh_after_source_change();
}

fn confirm_remove_server(server: RemoteServer, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let server = server.clone();
        dialog
            .overlay_closable(false)
            .close_button(false)
            .title(tr().remove_server_confirm_title.clone())
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cancel")
                            .label(tr().cancel.clone())
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    )
                    .child(
                        Button::new("ok")
                            .label(tr().remove.clone())
                            .primary()
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                    ),
            )
            .on_ok(move |_, _, cx| {
                remove_server(&server, cx);
                true
            })
    });
}

struct Ids {
    sync: &'static str,
    stars: &'static str,
    remove: &'static str,
    connect: &'static str,
}

fn ids(kind: ServerKind) -> Ids {
    match kind {
        ServerKind::Jellyfin => Ids {
            sync: "jellyfin-sync",
            stars: "jellyfin-stars",
            remove: "jellyfin-remove",
            connect: "jellyfin-connect",
        },
        ServerKind::Subsonic => Ids {
            sync: "subsonic-sync",
            stars: "subsonic-stars",
            remove: "subsonic-remove",
            connect: "subsonic-connect",
        },
    }
}

fn server_list(state: &LibrarySources, kind: ServerKind, cx: &App) -> AnyElement {
    let ids = ids(kind);
    let muted_fg = Colors::muted_foreground(cx);
    let mut list = v_flex().gap_2().w_full();
    let mut empty = true;
    for (ix, row) in state.remote(kind).enumerate() {
        empty = false;
        let status_color = match row.status {
            ServerStatus::Online => Colors::primary(cx),
            ServerStatus::Offline => Colors::danger(cx),
            ServerStatus::Syncing => muted_fg,
        };
        let syncing = row.status == ServerStatus::Syncing;
        let for_sync = row.server.clone();
        let for_stars = row.server.clone();
        let for_remove = row.server.clone();
        list = list.child(
            v_flex()
                .gap_1()
                .px_3()
                .py_2()
                .rounded(px(6.))
                .bg(Colors::muted(cx))
                .child(
                    h_flex()
                        .gap_2()
                        .items_center()
                        .child(
                            svg()
                                .path("icons/devices.svg")
                                .size(px(16.))
                                .text_color(muted_fg),
                        )
                        .child(
                            div()
                                .flex_1()
                                .text_sm()
                                .truncate()
                                .text_color(Colors::foreground(cx))
                                .child(row.title.clone()),
                        )
                        .child(
                            h_flex()
                                .flex_shrink_0()
                                .gap_1p5()
                                .items_center()
                                .child(div().size(px(8.)).rounded_full().bg(status_color))
                                .child(
                                    div()
                                        .text_sm()
                                        .text_color(status_color)
                                        .child(row.status_label.clone()),
                                ),
                        )
                        .child(
                            div()
                                .flex_shrink_0()
                                .text_sm()
                                .text_color(muted_fg)
                                .child(row.count_label.clone()),
                        )
                        .child(
                            Button::new((ids.sync, ix))
                                .small()
                                .label(tr().server_sync.clone())
                                .disabled(syncing)
                                .on_click(move |_, _, cx| {
                                    apply_remote_sources(cx);
                                    cx.global::<Services>()
                                        .library
                                        .sync_remote(vec![for_sync.clone()]);
                                }),
                        )
                        .child(
                            Button::new((ids.stars, ix))
                                .small()
                                .label(tr().server_import_favorites.clone())
                                .disabled(syncing)
                                .on_click(move |_, _, cx| {
                                    cx.global::<Services>()
                                        .library
                                        .import_remote_stars(for_stars.clone());
                                }),
                        )
                        .child(
                            Button::new((ids.remove, ix))
                                .small()
                                .label(tr().remove.clone())
                                .on_click(move |_, window, cx| {
                                    confirm_remove_server(for_remove.clone(), window, cx);
                                }),
                        ),
                )
                .children(row.message.clone().map(|message| {
                    div()
                        .min_w(px(0.))
                        .max_w_full()
                        .text_xs()
                        .text_color(muted_fg)
                        .child(message)
                })),
        );
    }
    if empty {
        list = list.child(
            div()
                .px_3()
                .py_2()
                .text_sm()
                .text_color(muted_fg)
                .child(tr().no_servers_added.clone()),
        );
    }
    list.into_any_element()
}

fn server_form(
    sources: &Entity<LibrarySources>,
    kind: ServerKind,
    inputs: &ServerInputs,
    connect: Connect,
    cx: &App,
) -> AnyElement {
    let state = sources.read(cx).connect_state(kind);
    let sources = sources.clone();
    let for_connect = inputs.clone();
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .child(div().w(px(260.)).child(Input::new(&inputs.url).small()))
                .child(
                    div()
                        .w(px(150.))
                        .child(Input::new(&inputs.username).small()),
                )
                .child(
                    div()
                        .w(px(150.))
                        .child(Input::new(&inputs.password).small().mask_toggle()),
                )
                .child(
                    Button::new(ids(kind).connect)
                        .small()
                        .label(tr().server_connect.clone())
                        .loading(state.connecting)
                        .disabled(state.connecting)
                        .on_click(move |_, window, cx| {
                            connect(sources.clone(), for_connect.clone(), window, cx);
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
        })
        .into_any_element()
}

pub fn server_group(
    kind: ServerKind,
    description: SharedString,
    sources: Entity<LibrarySources>,
    inputs: ServerInputs,
    connect: Connect,
) -> SettingGroup {
    SettingGroup::new()
        .title(SharedString::from(kind.title()))
        .item(
            SettingItem::new(
                tr().server_list.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    let list = server_list(sources.read(cx), kind, cx);
                    let form = server_form(&sources, kind, &inputs, connect, cx);
                    v_flex().gap_3().w_full().child(list).child(form)
                }),
            )
            .layout(Axis::Vertical)
            .description(description),
        )
}
