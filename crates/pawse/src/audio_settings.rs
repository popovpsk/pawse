use audio_output::{BitPerfectStatus, OutputEvent, native_mode_available};
use gpui::prelude::FluentBuilder;
use gpui::{
    Anchor, AnyElement, App, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    Icon, IconName, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    popover::Popover,
    v_flex,
};

use crate::cast::CastState;
use crate::localization::{LangChanged, tr};
use crate::services::Services;
use crate::settings_store::{SettingsStore, ui_scale};
use crate::theme_colors::Colors;
use ui_resources::i18n::cast_strings;

pub struct AudioSettings {
    is_exclusive: bool,
    pending_notification: Option<String>,
    bit_perfect_tooltip: Option<(BitPerfectStatus, gpui::SharedString)>,
    _settings_store_subscription: gpui::Subscription,
    _cast_subscription: gpui::Subscription,
    _lang_subscription: gpui::Subscription,
    casting_to: Option<(&'static str, gpui::SharedString)>,
}

struct DeviceErrorNotif;
struct StreamRecoveredNotif;
struct StreamFailureNotif;

/// On Linux the untouched-signal-path mode is "native sample rate" (we hand the
/// output rate to PipeWire) rather than an exclusive grab of the device, so the
/// same toggle speaks a different language there.
pub(crate) const NATIVE_RATE_WORDING: bool = cfg!(target_os = "linux");

fn mode_title() -> gpui::SharedString {
    if NATIVE_RATE_WORDING {
        tr().native_rate_title.clone()
    } else {
        tr().exclusive_mode_title.clone()
    }
}

fn mode_tooltip(enabled: bool) -> gpui::SharedString {
    match (NATIVE_RATE_WORDING, enabled) {
        (true, true) => tr().native_rate_click_disable.clone(),
        (true, false) => tr().native_rate_click_enable.clone(),
        (false, true) => tr().exclusive_click_disable.clone(),
        (false, false) => tr().shared_click_enable.clone(),
    }
}

fn mode_failed(err: &str) -> String {
    if NATIVE_RATE_WORDING {
        tr().failed_native_rate(err)
    } else {
        tr().failed_exclusive(err)
    }
}

fn receiver_icon(kind: cast::ReceiverKind) -> &'static str {
    match kind {
        cast::ReceiverKind::Chromecast => "icons/cast.svg",
        cast::ReceiverKind::AirPlay => "icons/airplay.svg",
        cast::ReceiverKind::Dlna => "icons/dlna.svg",
    }
}

fn cast_rows(muted_color: gpui::Hsla, cx: &App) -> Vec<AnyElement> {
    let state = cx.global::<CastState>();
    let strings = cast_strings();
    let muted_text = Colors::muted_foreground(cx);
    let mut rows: Vec<AnyElement> = vec![
        div()
            .mt_1()
            .pt_2()
            .px_1()
            .border_t_1()
            .border_color(Colors::border(cx))
            .text_xs()
            .text_color(muted_text)
            .child(strings.streaming.clone())
            .into_any_element(),
    ];
    if state.receivers.is_empty() {
        let note = if state.searching {
            strings.searching.clone()
        } else {
            strings.none_found.clone()
        };
        rows.push(
            div()
                .px_1()
                .py_1()
                .text_sm()
                .text_color(muted_text)
                .child(note)
                .into_any_element(),
        );
        return rows;
    }
    for (i, receiver) in state.receivers.iter().enumerate() {
        let active = state
            .active
            .as_ref()
            .is_some_and(|active| active.id == receiver.id);
        let connecting = state
            .connecting
            .as_ref()
            .is_some_and(|connecting| connecting.id == receiver.id);
        let clicked = receiver.clone();
        rows.push(
            h_flex()
                .id(("cast-row", i))
                .cursor_pointer()
                .px_1()
                .py_1()
                .rounded(px(4.))
                .hover(move |style| style.bg(muted_color))
                .gap_1()
                .when(active, |el| {
                    el.child(Icon::default().path("icons/check.svg").size(px(14.)))
                })
                .child(
                    Icon::default()
                        .path(receiver_icon(receiver.kind))
                        .size(px(14.))
                        .text_color(muted_text),
                )
                .child(div().text_sm().child(receiver.name.clone()))
                .when(connecting, |el| {
                    el.child(
                        div()
                            .text_xs()
                            .text_color(muted_text)
                            .child(strings.connecting.clone()),
                    )
                })
                .on_click(move |_, _, app_cx| crate::cast::connect(clicked.clone(), app_cx))
                .into_any_element(),
        );
    }
    rows
}

impl AudioSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let is_exclusive = services.output.is_exclusive();
        let settings_store_subscription = cx.observe_global::<SettingsStore>(|_, cx| cx.notify());
        let cast_subscription = cx.observe_global::<CastState>(|this: &mut Self, cx| {
            this.casting_to = cx.global::<CastState>().active.as_ref().map(|receiver| {
                (
                    receiver_icon(receiver.kind),
                    gpui::SharedString::from(cast_strings().playing_on(&receiver.name)),
                )
            });
            cx.notify();
        });
        let lang_event_bus = cx.global::<Services>().lang_event_bus.clone();
        let lang_subscription = cx.subscribe(&lang_event_bus, |this, _, _: &LangChanged, cx| {
            this.bit_perfect_tooltip = None;
            cx.notify();
        });
        Self {
            is_exclusive,
            pending_notification: None,
            bit_perfect_tooltip: None,
            _settings_store_subscription: settings_store_subscription,
            _cast_subscription: cast_subscription,
            _lang_subscription: lang_subscription,
            casting_to: None,
        }
    }

    fn bit_perfect_tooltip(&mut self, status: BitPerfectStatus) -> gpui::SharedString {
        if let Some((cached, tooltip)) = &self.bit_perfect_tooltip
            && *cached == status
        {
            return tooltip.clone();
        }
        let tooltip = crate::bit_perfect_info::tooltip(&status);
        self.bit_perfect_tooltip = Some((status, tooltip.clone()));
        tooltip
    }
}

impl Render for AudioSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(msg) = self.pending_notification.take() {
            window.push_notification(
                Notification::error(msg)
                    .title(tr().audio_device.clone())
                    .id::<DeviceErrorNotif>(),
                cx,
            );
        }

        let (events, is_exclusive, bit_perfect) = {
            let output = &cx.global::<Services>().output;
            let is_exclusive = output.is_exclusive();
            let bit_perfect = is_exclusive.then(|| output.bit_perfect_status());
            (output.drain_events(), is_exclusive, bit_perfect)
        };
        let casting = self.casting_to.is_some();
        let bit_perfect = bit_perfect.filter(|_| !casting).map(|status| {
            let is_perfect = status.is_bit_perfect();
            (is_perfect, self.bit_perfect_tooltip(status))
        });
        let show_hog =
            !casting && native_mode_available() && cx.global::<SettingsStore>().show_hog_button();
        let show_device_picker = cx.global::<SettingsStore>().show_device_picker();
        let (trigger_icon, trigger_tooltip) = match &self.casting_to {
            Some((icon, tooltip)) => (*icon, tooltip.clone()),
            None => ("icons/devices.svg", tr().select_audio_device.clone()),
        };
        let scale = ui_scale(cx);
        for evt in events {
            match evt {
                OutputEvent::Recovered { message } => {
                    window.push_notification(
                        Notification::warning(message)
                            .title(tr().audio_device.clone())
                            .id::<StreamRecoveredNotif>(),
                        cx,
                    );
                }
                OutputEvent::Failure { message } => {
                    window.push_notification(
                        Notification::error(message)
                            .title(tr().audio_device.clone())
                            .id::<StreamFailureNotif>(),
                        cx,
                    );
                }
            }
        }

        self.is_exclusive = is_exclusive;

        h_flex()
            .gap_2()
            .items_center()
            .when_some(bit_perfect, |el, (is_perfect, tooltip_text)| {
                let icon_name = if is_perfect {
                    IconName::Check
                } else {
                    IconName::TriangleAlert
                };
                el.child(
                    Button::new("bit-perfect-indicator")
                        .ghost()
                        .compact()
                        .rounded_full()
                        .w(px(40. * scale))
                        .h(px(40. * scale))
                        .icon(Icon::new(icon_name).size(px(20. * scale)))
                        .tooltip(tooltip_text)
                        .on_click(|_, window, cx| crate::bit_perfect_info::open(window, cx)),
                )
            })
            .when(show_hog, |el| {
                el.child({
                    let view = cx.entity().clone();
                    let icon_path = if self.is_exclusive {
                        "icons/hog-on.svg"
                    } else {
                        "icons/hog-off.svg"
                    };
                    let tooltip = mode_tooltip(self.is_exclusive);
                    Button::new("exclusive-toggle")
                        .ghost()
                        .compact()
                        .rounded_full()
                        .w(px(40. * scale))
                        .h(px(40. * scale))
                        .icon(Icon::default().path(icon_path).size(px(20. * scale)))
                        .tooltip(tooltip)
                        .on_click(move |_, window: &mut Window, app_cx: &mut App| {
                            view.update(app_cx, |this, cx| {
                                let services = cx.global::<Services>();
                                if this.is_exclusive {
                                    let _ = services.output.set_exclusive(false);
                                    this.is_exclusive = false;
                                } else {
                                    match services.output.set_exclusive(true) {
                                        Ok(()) => {
                                            this.is_exclusive = true;
                                        }
                                        Err(e) => {
                                            window.push_notification(
                                                Notification::error(mode_failed(&e.to_string()))
                                                    .title(mode_title())
                                                    .id::<DeviceErrorNotif>(),
                                                cx,
                                            );
                                        }
                                    }
                                }
                                crate::services::publish_remote_state(cx);
                                cx.notify();
                            });
                        })
                })
            })
            .when(show_device_picker, |el| {
                el.child({
                    let view = cx.entity().clone();
                    Popover::new("audio-device-popover")
                        .anchor(Anchor::TopRight)
                        .appearance(false)
                        .trigger(
                            Button::new("audio-device-trigger")
                                .ghost()
                                .compact()
                                .rounded_full()
                                .w(px(40. * scale))
                                .h(px(40. * scale))
                                .icon(Icon::default().path(trigger_icon).size(px(20. * scale)))
                                .tooltip(trigger_tooltip),
                        )
                        .on_open_change(|open, _, cx| {
                            if *open {
                                crate::cast::start_discovery(cx);
                            }
                        })
                        .content(move |_state, _window, pop_cx| {
                            let casting = pop_cx.global::<CastState>().is_casting();
                            let services = pop_cx.global::<Services>();
                            // Enumerate devices once (this may shell out to `pactl`
                            // on Linux) and derive the selected row from the pinned
                            // UID instead of calling `selected_device_index()`, which
                            // would enumerate a second time.
                            let devices = services.output.devices();
                            let selected_uid = services.output.selected_device_uid();
                            let muted_color = Colors::muted(pop_cx);
                            let mut children: Vec<AnyElement> = Vec::new();
                            for (i, d) in devices.into_iter().enumerate() {
                                let view_row = view.clone();
                                let is_selected = !casting
                                    && match &selected_uid {
                                        Some(uid) => *uid == d.uid,
                                        None => d.is_default,
                                    };
                                let device_label = format!(
                                    "{}{}",
                                    d.name,
                                    if d.is_default {
                                        tr().default_suffix.as_str()
                                    } else {
                                        ""
                                    }
                                );
                                children.push(
                                    h_flex()
                                        .id(("device-row", i))
                                        .cursor_pointer()
                                        .px_1()
                                        .py_1()
                                        .rounded(px(4.))
                                        .hover(move |style| style.bg(muted_color))
                                        .gap_1()
                                        .when(is_selected, |el| {
                                            el.child(
                                                Icon::default()
                                                    .path("icons/check.svg")
                                                    .size(px(14.)),
                                            )
                                        })
                                        .child(div().text_sm().child(device_label))
                                        .on_click(move |_, _, app_cx| {
                                            crate::cast::disconnect(app_cx);
                                            view_row.update(app_cx, |this, cx| {
                                                let services = cx.global::<Services>();
                                                if let Err(e) = services.output.select_device(i) {
                                                    this.pending_notification = Some(
                                                        tr().failed_switch_device(&e.to_string()),
                                                    );
                                                }
                                                cx.notify();
                                            });
                                        })
                                        .into_any_element(),
                                );
                            }
                            if pop_cx.global::<SettingsStore>().cast_enabled() {
                                children.extend(cast_rows(muted_color, pop_cx));
                            }
                            v_flex()
                                .id("audio-device-popup")
                                .bg(crate::cover_backdrop::popover_bg(
                                    Colors::popover(pop_cx),
                                    crate::cover_backdrop::veil_factor(pop_cx),
                                ))
                                .border_1()
                                .border_color(Colors::border(pop_cx))
                                .rounded(px(6.))
                                .shadow_md()
                                .p_3()
                                .occlude()
                                .gap_1()
                                .min_w(px(220.))
                                .children(children)
                        })
                })
            })
    }
}
