use gpui::{
    App, AppContext, Axis, Entity, ParentElement, SharedString, Styled, Window, div,
    prelude::FluentBuilder, px, svg,
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

use crate::library_sources::{LibrarySources, ServerStatus, describe_error};
use crate::localization::tr;
use crate::remote_sync::RemoteServer;
use crate::services::Services;
use crate::settings_store::{SettingsStore, SubsonicServer, notify_save_error};
use crate::theme_colors::Colors;

#[derive(Clone)]
pub struct SubsonicInputs {
    pub url: Entity<InputState>,
    pub username: Entity<InputState>,
    pub password: Entity<InputState>,
}

impl SubsonicInputs {
    pub fn new(window: &mut Window, cx: &mut App) -> Self {
        Self {
            url: cx.new(|cx| InputState::new(window, cx).placeholder(tr().subsonic_url.clone())),
            username: cx
                .new(|cx| InputState::new(window, cx).placeholder(tr().subsonic_username.clone())),
            password: cx.new(|cx| {
                InputState::new(window, cx)
                    .masked(true)
                    .placeholder(tr().subsonic_password.clone())
            }),
        }
    }
}

fn to_remote(server: &SubsonicServer) -> RemoteServer {
    RemoteServer {
        uri: server.source_uri(),
        name: server.normalized_url(),
        config: server.config(),
    }
}

pub fn remote_servers(cx: &App) -> Vec<RemoteServer> {
    cx.global::<SettingsStore>()
        .subsonic_servers()
        .iter()
        .map(to_remote)
        .collect()
}

pub fn apply_remote_sources(cx: &mut App) {
    let servers = remote_servers(cx);
    let services = cx.global::<Services>();
    let configs = services.library.reconcile_remote(&servers);
    services.remote_media.set_configs(configs);
}

pub fn sync_all(cx: &mut App) {
    apply_remote_sources(cx);
    let servers = remote_servers(cx);
    cx.global::<Services>().library.sync_remote(servers);
}

fn connect(
    sources: Entity<LibrarySources>,
    inputs: SubsonicInputs,
    window: &mut Window,
    cx: &mut App,
) {
    let url = inputs.url.read(cx).value().trim().to_string();
    let username = inputs.username.read(cx).value().trim().to_string();
    let password = inputs.password.read(cx).value().to_string();
    if url.is_empty() || username.is_empty() {
        sources.update(cx, |state, cx| {
            state.connect_error = Some(tr().subsonic_fill_fields.clone());
            cx.notify();
        });
        return;
    }
    let url = if url.contains("://") {
        url
    } else {
        format!("http://{url}")
    };
    let server = SubsonicServer {
        url,
        username,
        password,
    };
    sources.update(cx, |state, cx| {
        state.connecting = true;
        state.connect_error = None;
        cx.notify();
    });
    let handle = window.window_handle();
    cx.spawn(async move |cx| {
        let config = server.config();
        let result = cx
            .background_spawn(async move { subsonic::Client::new(&config).ping() })
            .await;
        cx.update(|cx| {
            sources.update(cx, |state, cx| {
                state.connecting = false;
                state.connect_error = result
                    .as_ref()
                    .err()
                    .map(|e| describe_error(&crate::remote_sync::RemoteError::from(e.clone())));
                cx.notify();
            });
            if result.is_err() {
                return;
            }
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .add_subsonic_server(server.clone())
            {
                notify_save_error(cx, e);
            }
            apply_remote_sources(cx);
            cx.global::<Services>()
                .library
                .sync_remote(vec![to_remote(&server)]);
            let _ = handle.update(cx, |_, window, cx| {
                for input in [&inputs.url, &inputs.username, &inputs.password] {
                    input.update(cx, |state, cx| state.set_value("", window, cx));
                }
            });
        });
    })
    .detach();
}

fn confirm_remove_server(uri: String, window: &mut Window, cx: &mut App) {
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let uri = uri.clone();
        dialog
            .overlay_closable(false)
            .close_button(false)
            .title(tr().remove_server_confirm_title.clone())
            .child(div().child(tr().remove_server_confirm_message.clone()))
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
                if let Err(e) = cx
                    .global_mut::<SettingsStore>()
                    .remove_subsonic_server(&uri)
                {
                    notify_save_error(cx, e);
                }
                apply_remote_sources(cx);
                cx.global::<Services>()
                    .library
                    .refresh_after_source_change();
                true
            })
    });
}

pub fn subsonic_group(sources: Entity<LibrarySources>, inputs: SubsonicInputs) -> SettingGroup {
    SettingGroup::new()
        .title(SharedString::from("Subsonic"))
        .item(
            SettingItem::new(
                tr().subsonic_servers.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    let state = sources.read(cx);
                    let muted_fg = Colors::muted_foreground(cx);
                    let mut list = v_flex().gap_2().w_full();

                    if state.remote().is_empty() {
                        list = list.child(
                            div()
                                .px_3()
                                .py_2()
                                .text_sm()
                                .text_color(muted_fg)
                                .child(tr().no_servers_added.clone()),
                        );
                    }
                    for (ix, row) in state.remote().iter().enumerate() {
                        let status_color = match row.status {
                            ServerStatus::Online => Colors::primary(cx),
                            ServerStatus::Offline => Colors::danger(cx),
                            ServerStatus::Syncing => muted_fg,
                        };
                        let syncing = row.status == ServerStatus::Syncing;
                        let for_sync = to_remote(&row.server);
                        let for_stars = to_remote(&row.server);
                        let uri = row.server.source_uri();
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
                                                .child(
                                                    div()
                                                        .size(px(8.))
                                                        .rounded_full()
                                                        .bg(status_color),
                                                )
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
                                            Button::new(("subsonic-sync", ix))
                                                .label(tr().subsonic_sync.clone())
                                                .disabled(syncing)
                                                .on_click(move |_, _, cx| {
                                                    apply_remote_sources(cx);
                                                    cx.global::<Services>()
                                                        .library
                                                        .sync_remote(vec![for_sync.clone()]);
                                                }),
                                        )
                                        .child(
                                            Button::new(("subsonic-stars", ix))
                                                .label(tr().subsonic_import_stars.clone())
                                                .disabled(syncing)
                                                .on_click(move |_, _, cx| {
                                                    cx.global::<Services>()
                                                        .library
                                                        .import_remote_stars(for_stars.clone());
                                                }),
                                        )
                                        .child(
                                            Button::new(("subsonic-remove", ix))
                                                .label(tr().remove.clone())
                                                .on_click(move |_, window, cx| {
                                                    confirm_remove_server(uri.clone(), window, cx);
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

                    let connecting = state.connecting;
                    let connect_error = state.connect_error.clone();
                    let sources_for_connect = sources.clone();
                    let inputs_for_connect = inputs.clone();
                    let form = v_flex()
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
                                    Button::new("subsonic-connect")
                                        .label(tr().subsonic_connect.clone())
                                        .loading(connecting)
                                        .disabled(connecting)
                                        .on_click(move |_, window, cx| {
                                            connect(
                                                sources_for_connect.clone(),
                                                inputs_for_connect.clone(),
                                                window,
                                                cx,
                                            );
                                        }),
                                ),
                        )
                        .when_some(connect_error, |form, error| {
                            form.child(
                                div()
                                    .min_w(px(0.))
                                    .max_w_full()
                                    .text_xs()
                                    .text_color(Colors::danger(cx))
                                    .child(error),
                            )
                        });

                    v_flex().gap_3().w_full().child(list).child(form)
                }),
            )
            .layout(Axis::Vertical)
            .description(tr().subsonic_servers_desc.clone()),
        )
}
