use audio_engine::EngineEvent;
use gpui::{
    AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window,
    div, px,
};
use gpui_component::{h_flex, v_flex};

use crate::services::Services;
use crate::{
    next_button::NextButton, now_playing::NowPlaying, play_button::PlayButton,
    prev_button::PrevButton, repeat_button::RepeatButton, shuffle_button::ShuffleButton,
    track_progress_slider::TrackProgressSlider, volume::Volume,
};

pub struct Footer {
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    shuffle_button: Entity<ShuffleButton>,
    repeat_button: Entity<RepeatButton>,
    volume_slider: Entity<Volume>,
    track_progress_slider: Entity<TrackProgressSlider>,
    now_playing: Entity<NowPlaying>,
    _subscription: Subscription,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(&engine_event_bus, |_this, _, event: &EngineEvent, cx| {
            if let EngineEvent::TrackEnded = event {
                let services = cx.global::<Services>();
                let mut queue = services.playback_queue.borrow_mut();
                if let Some(track) = queue.next_track().cloned() {
                    drop(queue);
                    services.play_track(&track);
                    crate::services::save_playback(cx);
                } else {
                    drop(queue);
                    cx.notify();
                }
            }
        });

        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
            shuffle_button: cx.new(|cx| ShuffleButton::new(window, cx)),
            repeat_button: cx.new(|cx| RepeatButton::new(window, cx)),
            volume_slider: cx.new(|cx| Volume::new(window, cx)),
            track_progress_slider: cx.new(|cx| TrackProgressSlider::new(window, cx)),
            now_playing: cx.new(|cx| NowPlaying::new(window, cx)),
            _subscription: subscription,
        }
    }
}

impl Render for Footer {
    fn render(&mut self, _: &mut gpui::Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        h_flex()
            .gap_4()
            .w_full()
            .h_full()
            .items_center()
            .px_4()
            .child(self.now_playing.clone())
            .child(
                v_flex()
                    .flex_1()
                    .w_full()
                    .gap_1()
                    .items_center()
                    .child(
                        h_flex()
                            .gap_2()
                            .items_center()
                            .child(self.shuffle_button.clone())
                            .child(self.prev_button.clone())
                            .child(self.play_button.clone())
                            .child(self.next_button.clone())
                            .child(self.repeat_button.clone()),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .px_4()
                            .child(self.track_progress_slider.clone()),
                    ),
            )
            .child(
                div()
                    .w(px(200.))
                    .flex()
                    .justify_end()
                    .child(self.volume_slider.clone()),
            )
    }
}
