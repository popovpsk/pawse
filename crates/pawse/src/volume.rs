use audio_output::AudioOutput;
use gpui::{
    AppContext, ClickEvent, Context, Entity, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Window, div, px, svg,
};
use gpui_component::{ActiveTheme, h_flex};
use ui_components::slider::{Slider, SliderEvent};

use crate::services::Services;

pub struct Volume {
    slider: Entity<Slider>,
    volume: f32,
    is_muted: bool,
    volume_before_mute: f32,
}

impl Render for Volume {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let services = cx.global::<Services>();
        let is_exclusive = services.output.is_exclusive();

        let icon_path: &str = if is_exclusive {
            "icons/volume_unmute.svg"
        } else if self.is_muted || self.volume <= 0. {
            "icons/volume_mute.svg"
        } else if self.volume < 0.5 {
            "icons/volume_low.svg"
        } else {
            "icons/volume_unmute.svg"
        };

        let mut container = h_flex()
            .id("volume_control")
            .gap_2()
            .justify_end()
            .items_center()
            .child(
                div()
                    .id("volume_icon")
                    .cursor_pointer()
                    .on_click(cx.listener(Self::on_icon_click))
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(22.))
                            .text_color(cx.theme().foreground),
                    ),
            );

        let is_exclusive = services.output.is_exclusive();
        self.slider
            .update(cx, |slider, cx| slider.set_disabled(is_exclusive, cx));
        container = container.child(div().w(px(100.)).child(self.slider.clone()));

        container.w_full().h_6()
    }
}

impl Volume {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let initial = cx.global::<crate::settings_store::SettingsStore>().volume();
        let slider = cx.new(|cx| {
            Slider::new(cx)
                .default_value(initial)
                .min(0.0)
                .max(1.0)
                .step(0.01)
        });

        cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => {
                let volume = *value;
                this.volume = volume;
                this.is_muted = volume <= 0.0;
                cx.global::<Services>().output.set_volume(volume);
                if let Err(e) = cx
                    .global_mut::<crate::settings_store::SettingsStore>()
                    .set_volume(volume)
                {
                    crate::settings_store::notify_save_error(cx, e);
                }
                cx.notify();
            }
        })
        .detach();

        Self {
            slider,
            volume: initial,
            is_muted: initial <= 0.0,
            volume_before_mute: if initial > 0.0 { initial } else { 1.0 },
        }
    }

    fn on_icon_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if cx.global::<Services>().output.is_exclusive() {
            return;
        }

        let new_volume = if self.is_muted {
            self.is_muted = false;
            self.volume = self.volume_before_mute;
            self.volume_before_mute
        } else {
            self.is_muted = true;
            self.volume_before_mute = self.volume;
            self.volume = 0.0;
            0.0
        };

        cx.global::<Services>().output.set_volume(new_volume);
        self.slider
            .update(cx, |slider, cx| slider.set_value_silent(new_volume, cx));
        if let Err(e) = cx
            .global_mut::<crate::settings_store::SettingsStore>()
            .set_volume(new_volume)
        {
            crate::settings_store::notify_save_error(cx, e);
        }
        cx.notify();
    }
}
