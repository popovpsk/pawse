use audio_output::OutputEvent;
use gpui::{
    App, AppContext, Context, Entity, IntoElement, ParentElement, Render, SharedString, Styled,
    Subscription, Window, px,
};
use gpui_component::{
    h_flex, ActiveTheme, IndexPath, Sizable, WindowExt,
    notification::Notification,
    select::{Select, SelectEvent, SelectItem, SelectState},
    switch::Switch,
};

use crate::services::Services;

#[derive(Debug, Clone)]
struct AudioDeviceItem {
    name: SharedString,
    is_default: bool,
}

impl SelectItem for AudioDeviceItem {
    type Value = SharedString;

    fn title(&self) -> SharedString {
        if self.is_default {
            format!("{} (default)", self.name).into()
        } else {
            self.name.clone()
        }
    }

    fn value(&self) -> &Self::Value {
        &self.name
    }
}

fn build_device_items(devices: &[audio_output::device::OutputDeviceInfo]) -> Vec<AudioDeviceItem> {
    devices
        .iter()
        .map(|d| AudioDeviceItem {
            name: d.name.clone().into(),
            is_default: d.is_default,
        })
        .collect()
}

/// Snapshot of the device list we last pushed into the dropdown. Used to skip
/// updates when nothing changed (avoids flicker on every render tick).
#[derive(Default)]
struct DeviceListSnapshot {
    fingerprint: Vec<(String, bool)>,
    selected_index: Option<usize>,
}

impl DeviceListSnapshot {
    fn from(
        devices: &[audio_output::device::OutputDeviceInfo],
        selected_index: Option<usize>,
    ) -> Self {
        Self {
            fingerprint: devices.iter().map(|d| (d.name.clone(), d.is_default)).collect(),
            selected_index,
        }
    }

    fn matches(
        &self,
        devices: &[audio_output::device::OutputDeviceInfo],
        selected_index: Option<usize>,
    ) -> bool {
        if self.selected_index != selected_index || self.fingerprint.len() != devices.len() {
            return false;
        }
        self.fingerprint
            .iter()
            .zip(devices.iter())
            .all(|((n, d), info)| n == &info.name && *d == info.is_default)
    }
}

pub struct AudioSettings {
    device_select: Entity<SelectState<Vec<AudioDeviceItem>>>,
    is_exclusive: bool,
    pending_notification: Option<String>,
    last_device_snapshot: DeviceListSnapshot,
    _subscription: Subscription,
}

struct DeviceErrorNotif;
struct StreamRecoveredNotif;
struct StreamFailureNotif;

impl AudioSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let devices = services.output.devices();
        let items = build_device_items(&devices);
        let selected_index = services.output.selected_device_index();
        let is_exclusive = services.output.is_exclusive();

        let selected_path = selected_index.map(|i| IndexPath::default().row(i));

        let device_select = cx.new(|cx| SelectState::new(items, selected_path, window, cx));

        let subscription = cx.subscribe(
            &device_select,
            |this: &mut AudioSettings,
             _entity,
             event: &SelectEvent<Vec<AudioDeviceItem>>,
             cx: &mut Context<AudioSettings>| {
                if let SelectEvent::Confirm(Some(device_name)) = event {
                    let services = cx.global::<Services>();
                    let devices = services.output.devices();
                    if let Some(index) = devices.iter().position(|d| d.name == *device_name)
                        && let Err(e) = services.output.select_device(index)
                    {
                        this.pending_notification =
                            Some(format!("Failed to switch device: {}", e));
                    }
                }
                cx.notify();
            },
        );

        Self {
            device_select,
            is_exclusive,
            pending_notification: None,
            last_device_snapshot: DeviceListSnapshot::from(&devices, selected_index),
            _subscription: subscription,
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

        // Drain background events from Output: device disconnect, recovery, etc.
        // Notification IDs are stable per kind so a repeated event replaces the
        // existing toast instead of stacking.
        let (events, is_exclusive, devices, selected_index) = {
            let services = cx.global::<Services>();
            (
                services.output.drain_events(),
                services.output.is_exclusive(),
                services.output.devices(),
                services.output.selected_device_index(),
            )
        };
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

        // Refresh dropdown only when the live enumeration actually differs from
        // what's currently displayed — this lets hot-plugged devices appear
        // automatically without triggering an update on every render.
        if !self.last_device_snapshot.matches(&devices, selected_index) {
            let items = build_device_items(&devices);
            let selected_path = selected_index.map(|i| IndexPath::default().row(i));
            self.device_select.update(cx, |state, cx| {
                state.set_items(items, window, cx);
                state.set_selected_index(selected_path, window, cx);
            });
            self.last_device_snapshot = DeviceListSnapshot::from(&devices, selected_index);
        }

        self.is_exclusive = is_exclusive;

        h_flex()
            .gap_3()
            .items_center()
            .px_3()
            .py_1()
            .rounded(px(6.))
            .bg(cx.theme().secondary)
            .child(
                Select::new(&self.device_select)
                    .placeholder("Audio Device")
                    .small(),
            )
            .child({
                let view = cx.entity().clone();
                Switch::new("exclusive-mode")
                    .checked(self.is_exclusive)
                    .with_size(gpui_component::Size::Small)
                    .label("Exclusive")
                    .on_click(move |checked: &bool, window: &mut Window, app_cx: &mut App| {
                        view.update(app_cx, |this, cx| {
                            let services = cx.global::<Services>();
                            if *checked {
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
                            } else {
                                // Leaving exclusive mode is infallible by contract.
                                let _ = services.output.set_exclusive(false);
                                this.is_exclusive = false;
                            }
                            cx.notify();
                        });
                    })
            })
    }
}
