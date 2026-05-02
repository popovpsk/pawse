use std::path::PathBuf;

use audio_engine::EngineEvent;
use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div, px};
use gpui_component::{h_flex, v_flex};

use crate::{
    next_button::NextButton, now_playing::NowPlaying, play_button::PlayButton,
    prev_button::PrevButton, track_progress_slider::TrackProgressSlider, volume::Volume,
};
use crate::services::Services;

pub struct Footer {
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    volume_slider: Entity<Volume>,
    track_progress_slider: Entity<TrackProgressSlider>,
    now_playing: Entity<NowPlaying>,
    _subscription: Subscription,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |_this, _, event: &EngineEvent, cx| {
                if let EngineEvent::TrackEnded = event {
                    let services = cx.global::<Services>();
                    let mut queue = services.playback_queue.borrow_mut();
                    if let Some(track) = queue.next_track() {
                        let path = PathBuf::from(&track.path);
                        drop(queue);
                        services.engine_manager.set_track(path);
                        services.engine_manager.play();
                    } else {
                        drop(queue);
                        cx.notify();
                    }
                }
            },
        );

        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
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
                            .child(self.prev_button.clone())
                            .child(self.play_button.clone())
                            .child(self.next_button.clone()),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .px_8()
                            .child(self.track_progress_slider.clone()),
                    ),
            )
            .child(div().w(px(140.)).child(self.volume_slider.clone()))
    }
}
