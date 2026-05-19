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
        let slider = cx.new(|cx| {
            Slider::new(cx)
                .default_value(1.0)
                .min(0.0)
                .max(1.0)
                .step(0.01)
        });

        cx.subscribe(&slider, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => {
                this.volume = *value;
                this.is_muted = *value <= 0.0;
                let services = cx.global::<Services>();
                services.output.set_volume(*value);
                cx.notify();
            }
        })
        .detach();

        Self {
            slider,
            volume: 1.0,
            is_muted: false,
            volume_before_mute: 1.0,
        }
    }

    fn on_icon_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if services.output.is_exclusive() {
            return;
        }

        if self.is_muted {
            self.is_muted = false;
            let restored = self.volume_before_mute;
            self.volume = restored;
            services.output.set_volume(restored);
            self.slider
                .update(cx, |slider, cx| slider.set_value_silent(restored, cx));
        } else {
            self.is_muted = true;
            self.volume_before_mute = self.volume;
            self.volume = 0.0;
            services.output.set_volume(0.0);
            self.slider
                .update(cx, |slider, cx| slider.set_value_silent(0.0, cx));
        }

        cx.notify();
    }
}
