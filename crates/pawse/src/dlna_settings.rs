use gpui::{
    App, AppContext, Entity, IntoElement, ParentElement, Styled, Window, div,
    prelude::FluentBuilder, px, svg,
};
use gpui_component::{
    Disableable, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputState},
    v_flex,
};
use ui_components::settings::{SettingField, SettingGroup, SettingItem};

use crate::library_sources::{
    ConnectState, DlnaDiscovery, FoundDevice, LibrarySources, describe_error,
};
use crate::localization::tr;
use crate::remote_settings::{ICON_SIZE, added, dlna_remote, server_list, set_connecting};
use crate::servers::{RemoteConfig, ServerKind, describe_dlna};
use crate::settings_store::{DlnaServer, SettingsStore, notify_save_error};
use crate::theme_colors::Colors;

enum Target {
    Found(dlna::Device),
    Address(String),
}

pub fn address_input(window: &mut Window, cx: &mut App) -> Entity<InputState> {
    cx.new(|cx| InputState::new(window, cx).placeholder(tr().dlna_address.clone()))
}

fn set_state(sources: &Entity<LibrarySources>, state: ConnectState, cx: &mut App) {
    set_connecting(sources, ServerKind::Dlna, state, cx);
}

fn set_discovery(sources: &Entity<LibrarySources>, discovery: DlnaDiscovery, cx: &mut App) {
    sources.update(cx, |sources, cx| {
        sources.set_dlna_discovery(discovery);
        cx.notify();
    });
}

fn discover(sources: Entity<LibrarySources>, cx: &mut App) {
    let mut discovery = sources.read(cx).dlna_discovery().clone();
    if discovery.searching {
        return;
    }
    discovery.searching = true;
    set_discovery(&sources, discovery, cx);
    cx.spawn(async move |cx| {
        let devices = cx
            .background_spawn(async { dlna::discover(dlna::DISCOVER_TIMEOUT) })
            .await;
        cx.update(|cx| {
            let discovery = DlnaDiscovery {
                searching: false,
                searched: true,
                found: devices.into_iter().map(FoundDevice::new).collect(),
            };
            set_discovery(&sources, discovery, cx);
        });
    })
    .detach();
}

fn add(
    sources: Entity<LibrarySources>,
    target: Target,
    address: Option<Entity<InputState>>,
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
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move {
                let device = match target {
                    Target::Found(device) => device,
                    Target::Address(address) => describe_dlna(&address)?,
                };
                let server = DlnaServer {
                    udn: device.udn,
                    location: device.location,
                    name: device.name,
                };
                RemoteConfig::Dlna(server.config()).client().ping()?;
                Ok(server)
            })
            .await;
        cx.update(|cx| {
            let server = match result {
                Ok(server) => server,
                Err(error) => {
                    set_state(
                        &sources,
                        ConnectState {
                            connecting: false,
                            error: Some(describe_error(&error)),
                        },
                        cx,
                    );
                    return;
                }
            };
            set_state(&sources, ConnectState::default(), cx);
            if let Err(e) = cx
                .global_mut::<SettingsStore>()
                .add_dlna_server(server.clone())
            {
                notify_save_error(cx, e);
            }
            added(dlna_remote(&server), cx);
            if let Some(address) = address {
                let _ = handle.update(cx, |_, window, cx| {
                    address.update(cx, |state, cx| state.set_value("", window, cx));
                });
            }
        });
    })
    .detach();
}

fn add_address(
    sources: Entity<LibrarySources>,
    address: Entity<InputState>,
    window: &mut Window,
    cx: &mut App,
) {
    let text = address.read(cx).value().trim().to_string();
    if text.is_empty() {
        set_state(
            &sources,
            ConnectState {
                connecting: false,
                error: Some(tr().server_fill_fields.clone()),
            },
            cx,
        );
        return;
    }
    add(sources, Target::Address(text), Some(address), window, cx);
}

fn found_row(
    ix: usize,
    found: &FoundDevice,
    sources: &Entity<LibrarySources>,
    connecting: bool,
    cx: &App,
) -> impl IntoElement {
    let muted_fg = Colors::muted_foreground(cx);
    let sources = sources.clone();
    let device = found.device.clone();
    h_flex()
        .gap_2()
        .items_center()
        .px_3()
        .py_2()
        .rounded(px(6.))
        .border_1()
        .border_color(Colors::border(cx))
        .child(
            svg()
                .flex_shrink_0()
                .path("icons/devices.svg")
                .size(px(ICON_SIZE))
                .text_color(muted_fg),
        )
        .child(
            v_flex()
                .flex_1()
                .min_w(px(0.))
                .child(
                    div()
                        .text_sm()
                        .text_color(Colors::foreground(cx))
                        .child(found.name.clone()),
                )
                .child(
                    div()
                        .text_xs()
                        .text_color(muted_fg)
                        .child(found.detail.clone()),
                ),
        )
        .child(
            Button::new(("dlna-add", ix))
                .small()
                .label(tr().dlna_add.clone())
                .disabled(connecting)
                .on_click(move |_, window, cx| {
                    add(
                        sources.clone(),
                        Target::Found(device.clone()),
                        None,
                        window,
                        cx,
                    );
                }),
        )
}

pub fn dlna_group(sources: Entity<LibrarySources>, address: Entity<InputState>) -> SettingGroup {
    SettingGroup::new().title(ServerKind::Dlna.title()).item(
        SettingItem::unlabeled(SettingField::render(move |_window, cx: &mut App| {
            let state = sources.read(cx);
            let list = server_list(state, ServerKind::Dlna, cx);
            let connect = state.connect_state(ServerKind::Dlna);
            let discovery = state.dlna_discovery();
            let searching = discovery.searching;
            let nothing_found = discovery.searched && !searching && discovery.found.is_empty();
            let found: Vec<_> = state
                .dlna_unadded()
                .enumerate()
                .map(|(ix, found)| found_row(ix, found, &sources, connect.connecting, cx))
                .collect();
            let for_discover = sources.clone();
            let for_address = sources.clone();
            let input = address.clone();
            let form = v_flex()
                .gap_2()
                .child(
                    h_flex().child(
                        Button::new("dlna-discover")
                            .small()
                            .label(tr().dlna_discover.clone())
                            .loading(searching)
                            .disabled(searching)
                            .on_click(move |_, _, cx| discover(for_discover.clone(), cx)),
                    ),
                )
                .children(found)
                .when(nothing_found, |form| {
                    form.child(
                        div()
                            .min_w(px(0.))
                            .max_w_full()
                            .text_xs()
                            .text_color(Colors::muted_foreground(cx))
                            .child(tr().dlna_nothing_found.clone()),
                    )
                })
                .child(
                    h_flex()
                        .flex_wrap()
                        .gap_2()
                        .child(
                            div()
                                .flex_1()
                                .min_w(px(240.))
                                .max_w(px(380.))
                                .child(Input::new(&address).small()),
                        )
                        .child(
                            Button::new("dlna-connect")
                                .small()
                                .label(tr().server_connect.clone())
                                .loading(connect.connecting)
                                .disabled(connect.connecting)
                                .on_click(move |_, window, cx| {
                                    add_address(for_address.clone(), input.clone(), window, cx);
                                }),
                        ),
                )
                .when_some(connect.error, |form, error| {
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
        .description(tr().dlna_servers_desc.clone()),
    )
}
