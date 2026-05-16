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

pub struct AudioSettings {
    device_select: Entity<SelectState<Vec<AudioDeviceItem>>>,
    is_exclusive: bool,
    pending_notification: Option<String>,
    needs_device_refresh: bool,
    _subscription: Subscription,
}

struct DeviceErrorNotif;

impl AudioSettings {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let devices = services.output.devices();
        let items = build_device_items(&devices);
        let selected_index = services.output.selected_device_index();
        let is_exclusive = services.output.is_exclusive();

        let selected_path = if !items.is_empty() {
            Some(IndexPath::default().row(selected_index))
        } else {
            None
        };

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
                    this.needs_device_refresh = true;
                }
                cx.notify();
            },
        );

        Self {
            device_select,
            is_exclusive,
            pending_notification: None,
            needs_device_refresh: false,
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

        let (recreate_error, is_exclusive) = {
            let services = cx.global::<Services>();
            (services.output.take_recreate_error(), services.output.is_exclusive())
        };
        if let Some(err) = recreate_error {
            window.push_notification(
                Notification::error(err).title("Stream Error"),
                cx,
            );
        }

        if self.needs_device_refresh {
            self.needs_device_refresh = false;
            let (devices, selected_index) = {
                let services = cx.global::<Services>();
                (services.output.devices(), services.output.selected_device_index())
            };
            let items = build_device_items(&devices);
            let selected_path = if !items.is_empty() {
                Some(IndexPath::default().row(selected_index))
            } else {
                None
            };
            self.device_select.update(cx, |state, cx| {
                state.set_items(items, window, cx);
                state.set_selected_index(selected_path, window, cx);
            });
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
                                            .title("Exclusive Mode"),
                                            cx,
                                        );
                                    }
                                }
                            } else if let Err(e) = services.output.set_exclusive(false) {
                                window.push_notification(
                                    Notification::error(format!(
                                        "Failed to disable exclusive mode: {}",
                                        e
                                    )),
                                    cx,
                                );
                            } else {
                                this.is_exclusive = false;
                            }
                            this.needs_device_refresh = true;
                            cx.notify();
                        });
                    })
            })
    }
}