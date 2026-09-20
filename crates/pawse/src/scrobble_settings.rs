use gpui::{
    AnyElement, App, AppContext, Axis, Entity, IntoElement, ParentElement, SharedString, Styled,
    div, prelude::FluentBuilder, px,
};
use gpui_component::{
    Disableable, Sizable,
    button::Button,
    h_flex,
    input::{Input, InputState},
    switch::Switch,
    v_flex,
};
use scrobble::{SessionError, TargetId};
use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage};

use crate::localization::tr;
use crate::settings_store::{ScrobbleSettings, ServiceState, SettingsStore, notify_save_error};
use crate::theme_colors::Colors;

#[derive(Default)]
pub enum AuthPhase {
    #[default]
    Idle,
    Awaiting(String),
}

#[derive(Default)]
pub struct ServiceUi {
    pub phase: AuthPhase,
    pub busy: bool,
    pub error: Option<SharedString>,
}

#[derive(Default)]
pub struct ScrobbleUiState {
    pub lastfm: ServiceUi,
    pub librefm: ServiceUi,
    pub listenbrainz: ServiceUi,
    pub csv: ServiceUi,
    pub import_busy: bool,
    pub import_result: Option<SharedString>,
}

impl ScrobbleUiState {
    pub fn new() -> Self {
        Self::default()
    }
}

#[derive(Clone, Copy, PartialEq, Eq)]
enum WebAuthService {
    Lastfm,
    Librefm,
}

impl WebAuthService {
    fn title(self) -> SharedString {
        match self {
            WebAuthService::Lastfm => SharedString::from("Last.fm"),
            WebAuthService::Librefm => SharedString::from("Libre.fm"),
        }
    }

    fn target(self) -> TargetId {
        match self {
            WebAuthService::Lastfm => TargetId::Lastfm,
            WebAuthService::Librefm => TargetId::Librefm,
        }
    }

    fn element_ids(self) -> [&'static str; 5] {
        match self {
            WebAuthService::Lastfm => [
                "scrobble-toggle-lastfm",
                "scrobble-sign-in-lastfm",
                "scrobble-confirm-lastfm",
                "scrobble-sign-out-lastfm",
                "scrobble-restart-lastfm",
            ],
            WebAuthService::Librefm => [
                "scrobble-toggle-librefm",
                "scrobble-sign-in-librefm",
                "scrobble-confirm-librefm",
                "scrobble-sign-out-librefm",
                "scrobble-restart-librefm",
            ],
        }
    }

    fn available(self) -> bool {
        match self {
            WebAuthService::Lastfm => scrobble::is_available(),
            WebAuthService::Librefm => true,
        }
    }

    fn client(self) -> Option<scrobble::AudioscrobblerClient> {
        match self {
            WebAuthService::Lastfm => scrobble::lastfm_client(),
            WebAuthService::Librefm => Some(scrobble::librefm_client()),
        }
    }

    fn state(self, settings: &ScrobbleSettings) -> &ServiceState {
        match self {
            WebAuthService::Lastfm => &settings.lastfm,
            WebAuthService::Librefm => &settings.librefm,
        }
    }

    fn state_mut(self, settings: &mut ScrobbleSettings) -> &mut ServiceState {
        match self {
            WebAuthService::Lastfm => &mut settings.lastfm,
            WebAuthService::Librefm => &mut settings.librefm,
        }
    }

    fn ui(self, ui: &ScrobbleUiState) -> &ServiceUi {
        match self {
            WebAuthService::Lastfm => &ui.lastfm,
            WebAuthService::Librefm => &ui.librefm,
        }
    }

    fn ui_mut(self, ui: &mut ScrobbleUiState) -> &mut ServiceUi {
        match self {
            WebAuthService::Lastfm => &mut ui.lastfm,
            WebAuthService::Librefm => &mut ui.librefm,
        }
    }
}

const FIELD_MAX_W: f32 = 360.;

#[derive(Clone)]
pub struct ScrobbleInputs {
    pub token: Entity<InputState>,
    pub api_root: Entity<InputState>,
}

pub fn scrobble_page(
    scrobble_ui: Entity<ScrobbleUiState>,
    scrobble_inputs: ScrobbleInputs,
) -> SettingPage {
    let mut page = SettingPage::new(tr().settings_scrobbling.clone());

    page = page.group(web_auth_group(WebAuthService::Lastfm, scrobble_ui.clone()));

    let ui = scrobble_ui.clone();
    page = page.group(
        SettingGroup::new()
            .title(SharedString::from("ListenBrainz"))
            .item(enabled_item(
                "scrobble-toggle-listenbrainz",
                |s| !s.listenbrainz.token.is_empty(),
                |s| s.listenbrainz.enabled,
                |s, value| s.listenbrainz.enabled = value,
            ))
            .item(
                SettingItem::new(
                    tr().scrobble_account.clone(),
                    SettingField::render(move |_window, cx: &mut App| {
                        listenbrainz_field(ui.clone(), scrobble_inputs.clone(), cx)
                    }),
                )
                .layout(Axis::Vertical),
            ),
    );

    page = page.group(web_auth_group(WebAuthService::Librefm, scrobble_ui.clone()));

    let csv_ui = scrobble_ui.clone();
    page = page.group(
        SettingGroup::new()
            .title(tr().scrobble_csv.clone())
            .item(enabled_item(
                "scrobble-toggle-csv",
                |s| !s.csv_log.path.is_empty(),
                |s| s.csv_log.enabled,
                |s, value| s.csv_log.enabled = value,
            ))
            .item(
                SettingItem::new(
                    tr().scrobble_file.clone(),
                    SettingField::render(move |_window, cx: &mut App| {
                        csv_log_field(csv_ui.clone(), cx)
                    }),
                )
                .layout(Axis::Vertical)
                .description(tr().scrobble_csv_desc.clone()),
            ),
    );

    page.group(
        SettingGroup::new()
            .title(tr().settings_general.clone())
            .item(
                SettingItem::new(
                    tr().scrobble_first_artist.clone(),
                    SettingField::render(|_window, cx: &mut App| {
                        let enabled = cx.global::<SettingsStore>().scrobble().first_artist_only;
                        h_flex().items_center().justify_end().child(
                            Switch::new("scrobble-first-artist-toggle")
                                .checked(enabled)
                                .on_click(|new_val, _, cx| {
                                    let value = *new_val;
                                    update_scrobble(cx, move |s| s.first_artist_only = value);
                                }),
                        )
                    }),
                )
                .description(tr().scrobble_first_artist_desc.clone()),
            )
            .item(SettingItem::new(
                tr().scrobble_queued.clone(),
                SettingField::render(|_window, cx: &mut App| {
                    let pending = cx
                        .try_global::<crate::scrobble_bridge::ScrobbleService>()
                        .map(|service| service.status().read(cx).pending)
                        .unwrap_or(0);
                    h_flex().items_center().justify_end().child(
                        div()
                            .text_sm()
                            .text_color(Colors::muted_foreground(cx))
                            .child(SharedString::from(pending.to_string())),
                    )
                }),
            )),
    )
}

fn web_auth_group(service: WebAuthService, scrobble_ui: Entity<ScrobbleUiState>) -> SettingGroup {
    let ui = scrobble_ui.clone();
    let mut group = SettingGroup::new()
        .title(service.title())
        .item(enabled_item(
            service.element_ids()[0],
            move |s| service.state(s).session.is_some(),
            move |s| service.state(s).enabled,
            move |s, value| service.state_mut(s).enabled = value,
        ))
        .item(
            SettingItem::new(
                tr().scrobble_account.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    web_auth_field(service, ui.clone(), cx)
                }),
            )
            .layout(Axis::Vertical),
        );
    if service == WebAuthService::Lastfm {
        group = group.item(
            SettingItem::new(
                tr().scrobble_import_loves.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    import_loves_field(scrobble_ui.clone(), cx)
                }),
            )
            .layout(Axis::Vertical),
        );
    }
    group
}

fn enabled_item(
    id: &'static str,
    available: impl Fn(&ScrobbleSettings) -> bool + 'static,
    read: impl Fn(&ScrobbleSettings) -> bool + 'static,
    write: impl Fn(&mut ScrobbleSettings, bool) + Copy + 'static,
) -> SettingItem {
    SettingItem::new(
        tr().scrobble_enabled.clone(),
        SettingField::render(move |_window, cx: &mut App| {
            let settings = cx.global::<SettingsStore>().scrobble();
            let ready = available(settings);
            let checked = ready && read(settings);
            h_flex().items_center().justify_end().child(
                Switch::new(id).checked(checked).disabled(!ready).on_click(
                    move |new_val, _, cx| {
                        let value = *new_val;
                        update_scrobble(cx, move |s| write(s, value));
                    },
                ),
            )
        }),
    )
}

fn update_scrobble(cx: &mut App, edit: impl FnOnce(&mut ScrobbleSettings)) {
    if let Err(e) = cx.global_mut::<SettingsStore>().update_scrobble(edit) {
        notify_save_error(cx, e);
    }
    crate::scrobble_bridge::apply_settings(cx);
}

fn auth_expired(cx: &App, target: TargetId) -> bool {
    cx.try_global::<crate::scrobble_bridge::ScrobbleService>()
        .is_some_and(|service| service.status().read(cx).auth_failed.contains(&target))
}

fn service_row(
    identity: AnyElement,
    controls: Vec<AnyElement>,
    error: Option<SharedString>,
    cx: &App,
) -> AnyElement {
    v_flex()
        .gap_1()
        .child(
            h_flex()
                .w_full()
                .items_center()
                .justify_between()
                .gap_2()
                .child(identity)
                .child(h_flex().items_center().gap_2().children(controls)),
        )
        .when_some(error, |this, error| {
            this.child(
                div()
                    .w_full()
                    .text_sm()
                    .text_color(Colors::danger(cx))
                    .child(error),
            )
        })
        .into_any_element()
}

fn identity_line(name: Option<SharedString>, status: SharedString, cx: &App) -> AnyElement {
    match name {
        Some(name) => h_flex()
            .flex_1()
            .min_w(px(0.))
            .items_baseline()
            .gap_1p5()
            .child(
                div()
                    .flex_shrink()
                    .min_w(px(0.))
                    .overflow_hidden()
                    .text_ellipsis()
                    .text_sm()
                    .text_color(Colors::foreground(cx))
                    .child(name),
            )
            .child(
                div()
                    .flex_shrink_0()
                    .text_sm()
                    .text_color(Colors::muted_foreground(cx))
                    .child(SharedString::from(format!("· {status}"))),
            )
            .into_any_element(),
        None => div()
            .flex_1()
            .min_w(px(0.))
            .max_w_full()
            .text_sm()
            .text_color(Colors::muted_foreground(cx))
            .child(status)
            .into_any_element(),
    }
}

fn web_auth_field(
    service: WebAuthService,
    state: Entity<ScrobbleUiState>,
    cx: &mut App,
) -> AnyElement {
    if !service.available() {
        return div()
            .text_sm()
            .text_color(Colors::muted_foreground(cx))
            .child(tr().lastfm_unavailable.clone())
            .into_any_element();
    }

    let settings = cx.global::<SettingsStore>().scrobble().clone();
    let stored = service.state(&settings);
    let session = stored.session.clone();
    let expired = auth_expired(cx, service.target());

    let (busy, awaiting, error) = {
        let ui = service.ui(state.read(cx));
        (
            ui.busy,
            matches!(ui.phase, AuthPhase::Awaiting(_)),
            ui.error.clone(),
        )
    };

    let status = if session.is_some() && expired {
        tr().scrobble_auth_expired.clone()
    } else if session.is_some() {
        tr().lastfm_status_connected.clone()
    } else if awaiting {
        tr().lastfm_status_awaiting.clone()
    } else {
        tr().lastfm_status_disconnected.clone()
    };

    let [_, sign_in_id, confirm_id, sign_out_id, restart_id] = service.element_ids();

    let controls: Vec<AnyElement> = if session.is_some() {
        let state = state.clone();
        vec![
            Button::new(sign_out_id)
                .small()
                .label(tr().lastfm_sign_out.clone())
                .disabled(busy)
                .on_click(move |_, _, cx| sign_out_web(cx, service, state.clone()))
                .into_any_element(),
        ]
    } else if awaiting {
        let restart_state = state.clone();
        let confirm_state = state.clone();
        vec![
            Button::new(restart_id)
                .small()
                .label(tr().scrobble_auth_restart.clone())
                .disabled(busy)
                .on_click(move |_, _, cx| start_web_sign_in(cx, service, restart_state.clone()))
                .into_any_element(),
            Button::new(confirm_id)
                .small()
                .label(tr().lastfm_confirm.clone())
                .loading(busy)
                .disabled(busy)
                .on_click(move |_, _, cx| {
                    let AuthPhase::Awaiting(token) = &service.ui(confirm_state.read(cx)).phase
                    else {
                        return;
                    };
                    let token = token.clone();
                    confirm_web_sign_in(cx, service, confirm_state.clone(), token);
                })
                .into_any_element(),
        ]
    } else {
        let state = state.clone();
        vec![
            Button::new(sign_in_id)
                .small()
                .label(tr().lastfm_sign_in.clone())
                .loading(busy)
                .disabled(busy)
                .on_click(move |_, _, cx| start_web_sign_in(cx, service, state.clone()))
                .into_any_element(),
        ]
    };

    let name = session.map(|s| SharedString::from(s.name));
    service_row(identity_line(name, status, cx), controls, error, cx)
}

fn start_web_sign_in(cx: &mut App, service: WebAuthService, state: Entity<ScrobbleUiState>) {
    state.update(cx, |s, cx| {
        let ui = service.ui_mut(s);
        ui.busy = true;
        ui.error = None;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move {
                let client = service
                    .client()
                    .ok_or_else(|| anyhow::anyhow!("service is not configured"))?;
                let token = client.get_token()?;
                let url = client.auth_url(&token);
                anyhow::Ok((token, url))
            })
            .await;
        cx.update(|cx| match result {
            Ok((token, url)) => {
                cx.open_url(&url);
                state.update(cx, |s, cx| {
                    let ui = service.ui_mut(s);
                    ui.busy = false;
                    ui.phase = AuthPhase::Awaiting(token);
                    cx.notify();
                });
            }
            Err(e) => state.update(cx, |s, cx| {
                let ui = service.ui_mut(s);
                ui.busy = false;
                ui.phase = AuthPhase::Idle;
                ui.error = Some(SharedString::from(format!("{e:#}")));
                cx.notify();
            }),
        })
        .ok();
    })
    .detach();
}

fn confirm_web_sign_in(
    cx: &mut App,
    service: WebAuthService,
    state: Entity<ScrobbleUiState>,
    token: String,
) {
    state.update(cx, |s, cx| {
        let ui = service.ui_mut(s);
        ui.busy = true;
        ui.error = None;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let result = cx
            .background_spawn(async move {
                let client = service
                    .client()
                    .ok_or_else(|| anyhow::anyhow!("service is not configured"))?;
                client.get_session(&token)
            })
            .await;
        cx.update(|cx| match result {
            Ok(session) => {
                update_scrobble(cx, move |s| {
                    let stored = service.state_mut(s);
                    stored.session = Some(session);
                    stored.enabled = true;
                });
                state.update(cx, |s, cx| {
                    let ui = service.ui_mut(s);
                    ui.busy = false;
                    ui.phase = AuthPhase::Idle;
                    ui.error = None;
                    cx.notify();
                });
            }
            Err(e) => state.update(cx, |s, cx| {
                let ui = service.ui_mut(s);
                ui.busy = false;
                ui.error = Some(match e {
                    SessionError::NotAuthorized => tr().scrobble_auth_pending.clone(),
                    SessionError::TokenExpired => tr().scrobble_auth_link_expired.clone(),
                    other => SharedString::from(format!("{other:#}")),
                });
                cx.notify();
            }),
        })
        .ok();
    })
    .detach();
}

fn sign_out_web(cx: &mut App, service: WebAuthService, state: Entity<ScrobbleUiState>) {
    update_scrobble(cx, move |s| service.state_mut(s).session = None);
    state.update(cx, |s, cx| {
        let ui = service.ui_mut(s);
        ui.phase = AuthPhase::Idle;
        ui.error = None;
        cx.notify();
    });
}

fn listenbrainz_field(
    state: Entity<ScrobbleUiState>,
    inputs: ScrobbleInputs,
    cx: &mut App,
) -> AnyElement {
    let settings = cx.global::<SettingsStore>().scrobble().clone();
    let connected = !settings.listenbrainz.token.is_empty();
    let expired = auth_expired(cx, TargetId::ListenBrainz);
    let (busy, error) = {
        let ui = &state.read(cx).listenbrainz;
        (ui.busy, ui.error.clone())
    };

    let status = if connected && expired {
        tr().scrobble_auth_expired.clone()
    } else if connected {
        tr().lastfm_status_connected.clone()
    } else {
        tr().lastfm_status_disconnected.clone()
    };

    if connected {
        let name =
            Some(SharedString::from(settings.listenbrainz.user.clone())).filter(|n| !n.is_empty());
        let button = Button::new("scrobble-sign-out-listenbrainz")
            .small()
            .label(tr().lastfm_sign_out.clone())
            .disabled(busy)
            .on_click(move |_, _, cx| {
                update_scrobble(cx, |s| {
                    s.listenbrainz.token.clear();
                    s.listenbrainz.user.clear();
                })
            })
            .into_any_element();
        return service_row(identity_line(name, status, cx), vec![button], error, cx);
    }

    let token_empty = inputs.token.read(cx).value().trim().is_empty();
    let connect = Button::new("scrobble-connect-listenbrainz")
        .small()
        .label(tr().scrobble_connect.clone())
        .loading(busy)
        .disabled(busy || token_empty)
        .on_click({
            let state = state.clone();
            let inputs = inputs.clone();
            move |_, _, cx| connect_listenbrainz_from_inputs(cx, state.clone(), &inputs)
        });

    v_flex()
        .gap_3()
        .child(service_row(
            identity_line(None, status, cx),
            Vec::new(),
            error,
            cx,
        ))
        .child(
            h_flex()
                .w_full()
                .items_end()
                .justify_between()
                .gap_2()
                .child(
                    v_flex()
                        .w_full()
                        .max_w(px(FIELD_MAX_W))
                        .gap_3()
                        .child(labeled_field(
                            tr().scrobble_server.clone(),
                            Input::new(&inputs.api_root).small().disabled(busy),
                            cx,
                        ))
                        .child(labeled_field(
                            tr().scrobble_token.clone(),
                            Input::new(&inputs.token)
                                .small()
                                .disabled(busy)
                                .mask_toggle(),
                            cx,
                        )),
                )
                .child(connect),
        )
        .into_any_element()
}

fn labeled_field(label: SharedString, input: impl IntoElement, cx: &App) -> AnyElement {
    v_flex()
        .w_full()
        .gap_1()
        .child(
            div()
                .text_sm()
                .text_color(Colors::muted_foreground(cx))
                .child(label),
        )
        .child(input)
        .into_any_element()
}

pub fn connect_listenbrainz_from_inputs(
    cx: &mut App,
    state: Entity<ScrobbleUiState>,
    inputs: &ScrobbleInputs,
) {
    if state.read(cx).listenbrainz.busy {
        return;
    }
    let token = inputs.token.read(cx).value().trim().to_string();
    if token.is_empty() {
        return;
    }
    let api_root = inputs.api_root.read(cx).value().trim().to_string();
    connect_listenbrainz(cx, state, token, api_root);
}

fn connect_listenbrainz(
    cx: &mut App,
    state: Entity<ScrobbleUiState>,
    token: String,
    api_root: String,
) {
    let root = if api_root.is_empty() {
        scrobble::LISTENBRAINZ_ROOT.to_string()
    } else {
        api_root
    };
    state.update(cx, |s, cx| {
        s.listenbrainz.busy = true;
        s.listenbrainz.error = None;
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let probe = token.clone();
        let probe_root = root.clone();
        let result = cx
            .background_spawn(async move {
                scrobble::ListenBrainzClient::new(probe_root, probe).validate()
            })
            .await;
        cx.update(|cx| match result {
            Ok(user) => {
                update_scrobble(cx, move |s| {
                    s.listenbrainz.token = token;
                    s.listenbrainz.user = user;
                    s.listenbrainz.api_root = root;
                    s.listenbrainz.enabled = true;
                });
                state.update(cx, |s, cx| {
                    s.listenbrainz.busy = false;
                    s.listenbrainz.error = None;
                    cx.notify();
                });
            }
            Err(e) => state.update(cx, |s, cx| {
                s.listenbrainz.busy = false;
                s.listenbrainz.error = Some(SharedString::from(format!("{e:#}")));
                cx.notify();
            }),
        })
        .ok();
    })
    .detach();
}

fn csv_log_field(state: Entity<ScrobbleUiState>, cx: &mut App) -> AnyElement {
    let settings = cx.global::<SettingsStore>().scrobble().clone();
    let has_path = !settings.csv_log.path.is_empty();
    let status = if has_path {
        SharedString::from(settings.csv_log.path.clone())
    } else {
        tr().lastfm_status_disconnected.clone()
    };
    let error = state.read(cx).csv.error.clone();

    let open = Button::new("scrobble-csv-open")
        .small()
        .label(tr().scrobble_file_open.clone())
        .on_click({
            let state = state.clone();
            move |_, _, cx| open_csv_path(cx, state.clone())
        })
        .into_any_element();

    let create = Button::new("scrobble-csv-new")
        .small()
        .label(tr().scrobble_file_new.clone())
        .on_click(move |_, _, cx| create_csv_path(cx, state.clone()))
        .into_any_element();

    service_row(
        identity_line(None, status, cx),
        vec![open, create],
        error,
        cx,
    )
}

fn create_csv_path(cx: &mut App, state: Entity<ScrobbleUiState>) {
    clear_csv_error(cx, &state);
    cx.spawn(async move |cx| {
        let picked = rfd::AsyncFileDialog::new()
            .set_file_name("pawse-scrobbles.csv")
            .add_filter("CSV", &["csv"])
            .save_file()
            .await;
        let Some(handle) = picked else {
            return;
        };
        let path = handle.path().to_path_buf();
        cx.update(|cx| store_csv_path(cx, path)).ok();
    })
    .detach();
}

fn open_csv_path(cx: &mut App, state: Entity<ScrobbleUiState>) {
    clear_csv_error(cx, &state);
    cx.spawn(async move |cx| {
        let picked = rfd::AsyncFileDialog::new()
            .add_filter("CSV", &["csv"])
            .pick_file()
            .await;
        let Some(handle) = picked else {
            return;
        };
        let path = handle.path().to_path_buf();
        let probe = path.clone();
        let usable = cx
            .background_spawn(async move { scrobble::is_pawse_log(&probe) })
            .await;
        cx.update(|cx| {
            if usable {
                store_csv_path(cx, path);
            } else {
                state.update(cx, |s, cx| {
                    s.csv.error = Some(tr().scrobble_not_a_log.clone());
                    cx.notify();
                });
            }
        })
        .ok();
    })
    .detach();
}

fn store_csv_path(cx: &mut App, path: std::path::PathBuf) {
    let path = path.to_string_lossy().to_string();
    update_scrobble(cx, move |s| {
        s.csv_log.path = path;
        s.csv_log.enabled = true;
    });
}

fn clear_csv_error(cx: &mut App, state: &Entity<ScrobbleUiState>) {
    state.update(cx, |s, cx| {
        if s.csv.error.take().is_some() {
            cx.notify();
        }
    });
}

fn import_loves_field(state: Entity<ScrobbleUiState>, cx: &mut App) -> AnyElement {
    let connected = cx
        .global::<SettingsStore>()
        .scrobble()
        .lastfm
        .session
        .is_some();
    let (busy, result) = {
        let ui = state.read(cx);
        (ui.import_busy, ui.import_result.clone())
    };
    let status = if busy {
        tr().scrobble_importing.clone()
    } else {
        result.unwrap_or_else(|| tr().scrobble_import_loves_desc.clone())
    };

    let button = Button::new("scrobble-import-loves")
        .small()
        .label(tr().scrobble_import_run.clone())
        .loading(busy)
        .disabled(!connected || busy)
        .on_click(move |_, _, cx| crate::scrobble_import::start(cx, state.clone()))
        .into_any_element();

    service_row(identity_line(None, status, cx), vec![button], None, cx)
}
