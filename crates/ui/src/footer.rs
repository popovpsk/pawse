use gpui::{div, AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window};
use gpui_component::h_flex;

use crate::{play_button::PlayButton, volume::Volume};

pub struct Footer {
    play_button: Entity<PlayButton>,
    volume_slider: Entity<Volume>,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let this = Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            volume_slider: cx.new(|cx| Volume::new(window, cx)),
        };

        this
    }
}

impl Render for Footer {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        h_flex()
            .pb_3()
            .gap_4()
            .h_10()
            .w_full()
            .child(div().ml_4().child("current track"))
            .child(
                h_flex()
                    .w_full()
                    .justify_center()
                    .child(self.play_button.clone()),
            )
            .child(div().mr_4().w_56().child(self.volume_slider.clone()))
    }
}
