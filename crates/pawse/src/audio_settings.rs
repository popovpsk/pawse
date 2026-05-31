use audio_output::{BitPerfectIssue, BitPerfectStatus, OutputEvent};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Corner, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    Icon, IconName, WindowExt,
    button::{Button, ButtonVariants},
    dialog::DialogButtonProps,
    h_flex,
    notification::Notification,
    popover::Popover,
    v_flex,
};

use crate::localization::tr;
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

pub struct AudioSettings {
    is_exclusive: bool,
    pending_notification: Option<String>,
    _settings_store_subscription: gpui::Subscription,
}

struct DeviceErrorNotif;
struct StreamRecoveredNotif;
struct StreamFailureNotif;

fn format_bit_perfect_tooltip(status: &BitPerfectStatus) -> String {
    let s = tr();
    if status.is_bit_perfect() {
        return s.bit_perfect_playback.to_string();
    }
    let mut lines = vec![s.not_bit_perfect.to_string()];
    for issue in &status.issues {
        let line = match issue {
            BitPerfectIssue::NotExclusive => s.bp_not_exclusive.to_string(),
            BitPerfectIssue::SystemVolumeNotUnity { current } => {
                s.bp_system_volume(&format!("{:.2}", current))
            }
            BitPerfectIssue::SystemMuted => s.bp_system_muted.to_string(),
            BitPerfectIssue::AppVolumeNotUnity { current } => {
                s.bp_app_volume(&format!("{:.2}", current))
            }
            BitPerfectIssue::SampleRateMismatch { source, device } => {
                s.bp_sample_rate(*source, *device)
            }
            BitPerfectIssue::BitDepthExceedsContainer { source } => s.bp_bit_depth(*source as u32),
            BitPerfectIssue::NoSource => s.bp_no_source.to_string(),
        };
        lines.push(line);
    }
    lines.join("\n")
}

impl AudioSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let is_exclusive = services.output.is_exclusive();
        let settings_store_subscription = cx.observe_global::<SettingsStore>(|_, cx| cx.notify());
        Self {
            is_exclusive,
            pending_notification: None,
            _settings_store_subscription: settings_store_subscription,
        }
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
        let has_hw_volume_issue = bit_perfect.as_ref().is_some_and(|bp| {
            bp.issues
                .iter()
                .any(|i| matches!(i, BitPerfectIssue::SystemVolumeNotUnity { .. }))
        });
        let show_hog = !cfg!(target_os = "linux") && cx.global::<SettingsStore>().show_hog_button();
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
            .when_some(bit_perfect, |el, bit_perfect| {
                let is_perfect = bit_perfect.is_bit_perfect();
                let tooltip_text = format_bit_perfect_tooltip(&bit_perfect);
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
                        .w(px(40.))
                        .h(px(40.))
                        .icon(Icon::new(icon_name).size(px(20.)))
                        .tooltip(tooltip_text),
                )
            })
            .when(self.is_exclusive && has_hw_volume_issue, |el| {
                el.child(
                    Button::new("fix-hw-volume")
                        .ghost()
                        .compact()
                        .label(tr().fix_volume.clone())
                        .tooltip(tr().fix_volume_tooltip.clone())
                        .on_click(move |_, window: &mut Window, app_cx: &mut App| {
                            window.open_dialog(app_cx, move |dialog, _window, _cx| {
                                dialog
                                    .confirm()
                                    .title(tr().fix_volume_confirm_title.clone())
                                    .child(div().child(tr().fix_volume_confirm_message.clone()))
                                    .button_props(
                                        DialogButtonProps::default()
                                            .ok_text(tr().fix_volume.clone())
                                            .cancel_text(tr().cancel.clone()),
                                    )
                                    .on_ok(|_, _, cx| {
                                        cx.global::<Services>().output.set_hw_volume(1.0);
                                        true
                                    })
                            });
                        }),
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
                    let tooltip = if self.is_exclusive {
                        tr().exclusive_click_disable.clone()
                    } else {
                        tr().shared_click_enable.clone()
                    };
                    Button::new("exclusive-toggle")
                        .ghost()
                        .compact()
                        .rounded_full()
                        .w(px(40.))
                        .h(px(40.))
                        .icon(Icon::default().path(icon_path).size(px(20.)))
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
                                                Notification::error(
                                                    tr().failed_exclusive(&e.to_string()),
                                                )
                                                .title(tr().exclusive_mode_title.clone())
                                                .id::<DeviceErrorNotif>(),
                                                cx,
                                            );
                                        }
                                    }
                                }
                                cx.notify();
                            });
                        })
                })
            })
            .when(!cfg!(target_os = "linux"), |el| {
                el.child({
                    let view = cx.entity().clone();
                    Popover::new("audio-device-popover")
                        .anchor(Corner::TopRight)
                        .trigger(
                            Button::new("audio-device-trigger")
                                .ghost()
                                .compact()
                                .rounded_full()
                                .w(px(40.))
                                .h(px(40.))
                                .icon(Icon::default().path("icons/devices.svg").size(px(20.)))
                                .tooltip(tr().select_audio_device.clone()),
                        )
                        .content(move |_state, _window, pop_cx| {
                            let services = pop_cx.global::<Services>();
                            let devices = services.output.devices();
                            let selected = services.output.selected_device_index();
                            let muted_color = Colors::control_hover_bg(pop_cx);
                            let mut children: Vec<AnyElement> = Vec::new();
                            for (i, d) in devices.into_iter().enumerate() {
                                let view_row = view.clone();
                                let is_selected =
                                    selected == Some(i) || (selected.is_none() && d.is_default);
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
                            v_flex().gap_1().min_w(px(220.)).children(children)
                        })
                })
            })
    }
}
