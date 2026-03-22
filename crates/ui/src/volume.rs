use audio_output::AudioOutput;
use gpui::{
    AppContext, Context, Entity, InteractiveElement, IntoElement, ParentElement, Render, Styled,
    Window,
};
use gpui_component::{
    h_flex,
    slider::{Slider, SliderEvent, SliderState},
};

use crate::services::Services;

pub struct Volume {
    state: Entity<SliderState>,
    volume: f32,
}

impl Render for Volume {
    fn render(&mut self, _: &mut Window, _: &mut Context<Self>) -> impl IntoElement {
        h_flex()
            .id("volume_control")
            .gap_2()
            .child("🔊")
            .child(Slider::new(&self.state).w_64())
            .w_full()
            .h_6()
    }
}

impl Volume {
    pub fn new(_: &mut Window, cx: &mut Context<Self>) -> Self {
        let state = cx.new(|_| {
            SliderState::new()
                .default_value(0.5)
                .min(0.0)
                .max(1.0)
                .step(0.01)
        });

        cx.subscribe(&state, |this, _, event: &SliderEvent, cx| match event {
            SliderEvent::Change(value) => {
                let volume_value = value.start();
                this.volume = volume_value;

                let services = cx.global::<Services>();
                services.output.set_volume(volume_value);

                cx.notify();
            }
        })
        .detach();

        Self { state, volume: 0.5 }
    }
}
