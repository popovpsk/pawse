use audio_output::{BitPerfectIssue, BitPerfectStatus, OutputEvent};
use gpui::prelude::FluentBuilder;
use gpui::{
    AnyElement, App, Context, Corner, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Window, div, px,
};
use gpui_component::{
    ActiveTheme, Icon, IconName, WindowExt,
    button::{Button, ButtonVariants},
    h_flex,
    notification::Notification,
    popover::Popover,
    v_flex,
};

use crate::services::Services;

pub struct AudioSettings {
    is_exclusive: bool,
    pending_notification: Option<String>,
}

struct DeviceErrorNotif;
struct StreamRecoveredNotif;
struct StreamFailureNotif;

fn format_bit_perfect_tooltip(status: &BitPerfectStatus) -> String {
    if status.is_bit_perfect() {
        return "Bit-perfect playback".to_string();
    }
    let mut lines = vec!["Not bit-perfect:".to_string()];
    for issue in &status.issues {
        let line = match issue {
            BitPerfectIssue::NotExclusive => "• Output is shared (not exclusive)".to_string(),
            BitPerfectIssue::SystemVolumeNotUnity { current } => {
                format!("• System volume not at unity: {:.2}", current)
            }
            BitPerfectIssue::SystemMuted => "• System muted".to_string(),
            BitPerfectIssue::AppVolumeNotUnity { current } => {
                format!("• App volume not at unity: {:.2}", current)
            }
            BitPerfectIssue::SampleRateMismatch { source, device } => format!(
                "• Sample rate mismatch: source {} Hz → device {} Hz",
                source, device
            ),
            BitPerfectIssue::BitDepthExceedsContainer { source } => {
                format!("• Bit depth {} exceeds f32 container (24)", source)
            }
            BitPerfectIssue::NoSource => "• No source loaded".to_string(),
        };
        lines.push(line);
    }
    lines.join("\n")
}

impl AudioSettings {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        Self {
            is_exclusive: services.output.is_exclusive(),
            pending_notification: None,
        }
    }
}

impl Render for AudioSettings {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        if let Some(msg) = self.pending_notification.take() {
            window.push_notification(
                Notification::error(msg)
                    .title("Audio Device")
                    .id::<DeviceErrorNotif>(),
                cx,
            );
        }

        let (events, is_exclusive, bit_perfect) = {
            let services = cx.global::<Services>();
            (
                services.output.drain_events(),
                services.output.is_exclusive(),
                services.output.bit_perfect_status(),
            )
        };
        let has_hw_volume_issue = bit_perfect
            .issues
            .iter()
            .any(|i| matches!(i, BitPerfectIssue::SystemVolumeNotUnity { .. }));
        for evt in events {
            match evt {
                OutputEvent::Recovered { message } => {
                    window.push_notification(
                        Notification::warning(message)
                            .title("Audio Device")
                            .id::<StreamRecoveredNotif>(),
                        cx,
                    );
                }
                OutputEvent::Failure { message } => {
                    window.push_notification(
                        Notification::error(message)
                            .title("Audio Device")
                            .id::<StreamFailureNotif>(),
                        cx,
                    );
                }
            }
        }

        self.is_exclusive = is_exclusive;

        h_flex()
            .items_center()
            .when(self.is_exclusive, |el| {
                let is_perfect = bit_perfect.is_bit_perfect();
                let tooltip_text = format_bit_perfect_tooltip(&bit_perfect);
                let icon_color = if is_perfect {
                    cx.theme().success
                } else {
                    cx.theme().warning
                };
                let icon_name = if is_perfect {
                    IconName::Check
                } else {
                    IconName::TriangleAlert
                };
                el.child(
                    Button::new("bit-perfect-indicator")
                        .ghost()
                        .compact()
                        .icon(Icon::new(icon_name).text_color(icon_color))
                        .tooltip(tooltip_text),
                )
            })
            .when(self.is_exclusive && has_hw_volume_issue, |el| {
                let view = cx.entity().clone();
                el.child(
                    Button::new("fix-hw-volume")
                        .ghost()
                        .compact()
                        .label("Fix volume")
                        .tooltip("Set device volume to 100% for bit-perfect playback")
                        .on_click(move |_, _, app_cx| {
                            view.update(app_cx, |_, cx| {
                                let services = cx.global::<Services>();
                                services.output.set_hw_volume(1.0);
                                cx.notify();
                            });
                        }),
                )
            })
            .child({
                let view = cx.entity().clone();
                let icon_path = if self.is_exclusive {
                    "icons/hog-on.svg"
                } else {
                    "icons/hog-off.svg"
                };
                let tooltip = if self.is_exclusive {
                    "Exclusive mode — click to disable"
                } else {
                    "Shared mode — click to enable exclusive"
                };
                Button::new("exclusive-toggle")
                    .ghost()
                    .compact()
                    .rounded_full()
                    .w(px(36.))
                    .h(px(36.))
                    .icon(Icon::default().path(icon_path))
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
                                            Notification::error(format!(
                                                "Failed to enable exclusive mode: {}",
                                                e
                                            ))
                                            .title("Exclusive Mode")
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
            .child({
                let view = cx.entity().clone();
                Popover::new("audio-device-popover")
                    .anchor(Corner::TopRight)
                    .trigger(
                        Button::new("audio-device-trigger")
                            .ghost()
                            .compact()
                            .rounded_full()
                            .w(px(36.))
                            .h(px(36.))
                            .icon(Icon::default().path("icons/devices.svg").text_color(cx.theme().foreground))
                            .tooltip("Select audio device"),
                    )
                    .content(move |_state, _window, pop_cx| {
                        let services = pop_cx.global::<Services>();
                        let devices = services.output.devices();
                        let selected = services.output.selected_device_index();
                        let muted_color = pop_cx.theme().muted;
                        let mut children: Vec<AnyElement> = Vec::new();
                        for (i, d) in devices.into_iter().enumerate() {
                            let view_row = view.clone();
                            let is_selected =
                                selected == Some(i) || (selected.is_none() && d.is_default);
                            let device_label = format!(
                                "{}{}",
                                d.name,
                                if d.is_default { " (default)" } else { "" }
                            );
                            children.push(
                                h_flex()
                                    .id(("device-row", i))
                                    .cursor_pointer()
                                    .px_2()
                                    .py_1()
                                    .rounded(px(4.))
                                    .hover(move |style| style.bg(muted_color))
                                    .gap_2()
                                    .child(if is_selected {
                                        Icon::default().path("icons/check.svg").into_any_element()
                                    } else {
                                        div().size(px(16.)).into_any_element()
                                    })
                                    .child(div().child(device_label))
                                    .on_click(move |_, _, app_cx| {
                                        view_row.update(app_cx, |this, cx| {
                                            let services = cx.global::<Services>();
                                            if let Err(e) = services.output.select_device(i) {
                                                this.pending_notification =
                                                    Some(format!("Failed to switch device: {}", e));
                                            }
                                            cx.notify();
                                        });
                                    })
                                    .into_any_element(),
                            );
                        }
                        v_flex().gap_1().p_1().min_w(px(220.)).children(children)
                    })
            })
    }
}
