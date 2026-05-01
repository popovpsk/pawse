use std::path::PathBuf;

use audio_engine::EngineEvent;
use gpui::{AppContext, Context, Entity, IntoElement, ParentElement, Render, Styled, Subscription, Window, div};
use gpui_component::{h_flex, v_flex};

use crate::{next_button::NextButton, play_button::PlayButton, prev_button::PrevButton, track_progress_slider::TrackProgressSlider, volume::Volume};
use crate::services::Services;

pub struct Footer {
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    volume_slider: Entity<Volume>,
    track_progress_slider: Entity<TrackProgressSlider>,
    track_title: String,
    _subscription: Subscription,
}

impl Footer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        let subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => {
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    this.track_title = queue
                        .current_track()
                        .map(|t| t.title.clone())
                        .unwrap_or_else(|| "Unknown".to_string());
                    drop(queue);
                    cx.notify();
                }
                EngineEvent::TrackEnded => {
                    let services = cx.global::<Services>();
                    let mut queue = services.playback_queue.borrow_mut();
                    if let Some(track) = queue.next_track() {
                        let path = PathBuf::from(&track.path);
                        drop(queue);
                        services.engine_manager.set_track(path);
                        services.engine_manager.play();
                    } else {
                        this.track_title = "current track".to_string();
                        drop(queue);
                        cx.notify();
                    }
                }
                _ => {}
            },
        );

        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
            volume_slider: cx.new(|cx| Volume::new(window, cx)),
            track_progress_slider: cx.new(|cx| TrackProgressSlider::new(window, cx)),
            track_title: "current track".to_string(),
            _subscription: subscription,
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
            .child(div().ml_4().w_48().child(self.track_title.clone()))
            .child(
                v_flex()
                    .w_full()
                    .gap_1()
                    .mb_10()
                    .child(
                        h_flex()
                            .w_full()
                            .justify_center()
                            .gap_2()
                            .child(self.prev_button.clone())
                            .child(self.play_button.clone())
                            .child(self.next_button.clone()),
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
