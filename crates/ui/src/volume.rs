use audio_output::AudioOutput;
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    Window, div, px,
};
use gpui_component::h_flex;
use ui_components::slider::{Slider, SliderEvent};

use crate::services::Services;

pub struct Volume {
    slider: Entity<Slider>,
    volume: f32,
}

impl Render for Volume {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("volume_control")
            .gap_2()
            .justify_end()
            .child(div().w(px(100.)).child(self.slider.clone()))
            .w_full()
            .h_6()
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
                let services = cx.global::<Services>();
                services.output.set_volume(*value);
                cx.notify();
            }
        })
        .detach();

        Self {
            slider,
            volume: 1.0,
        }
    }
}
