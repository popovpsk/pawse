use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Window, div};
use gpui_component::{h_flex, v_flex};

use crate::{play_button::PlayButton, track_progress_slider::TrackProgressSlider, volume::Volume};

pub struct Footer {
    play_button: Entity<PlayButton>,
    volume_slider: Entity<Volume>,
    track_progress_slider: Entity<TrackProgressSlider>,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            volume_slider: cx.new(|cx| Volume::new(window, cx)),
            track_progress_slider: cx.new(|cx| TrackProgressSlider::new(window, cx)),
        }
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
                v_flex()
                    .w_full()
                    .gap_1()
                    .mb_10()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_center()
                            .child(self.play_button.clone()),
                    )
                    .child(
                        div()
                            .pr_5()
                            .pl_5()
                            .child(self.track_progress_slider.clone()),
                    ),
            )
            .child(div().mr_4().w_56().child(self.volume_slider.clone()))
    }
}
