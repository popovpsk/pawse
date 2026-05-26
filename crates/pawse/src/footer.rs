use audio_engine::EngineEvent;
use gpui::{
    AppContext, Context, Entity, EventEmitter, InteractiveElement, IntoElement, ParentElement,
    Render, StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder,
    px, svg,
};
use gpui_component::{h_flex, tooltip::Tooltip, v_flex};

use crate::theme_colors::Colors;

use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::{
    next_button::NextButton,
    now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested, NowPlaying},
    play_button::PlayButton,
    prev_button::PrevButton,
    repeat_button::RepeatButton,
    shuffle_button::ShuffleButton,
    track_progress_slider::TrackProgressSlider,
    volume::Volume,
};

#[derive(Clone, Debug)]
pub struct ToggleQueueEvent {
    pub show: bool,
}

pub struct Footer {
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    pub shuffle_button: Entity<ShuffleButton>,
    repeat_button: Entity<RepeatButton>,
    volume_slider: Entity<Volume>,
    track_progress_slider: Entity<TrackProgressSlider>,
    now_playing: Entity<NowPlaying>,
    show_queue: bool,
    show_repeat_shuffle: bool,
    _subscription: Subscription,
    _settings_subscription: Subscription,
    _np_album_subscription: Subscription,
    _np_artist_subscription: Subscription,
}

impl EventEmitter<ToggleQueueEvent> for Footer {}
impl EventEmitter<NavigateToAlbumRequested> for Footer {}
impl EventEmitter<NavigateToArtistRequested> for Footer {}

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

        let show_repeat_shuffle = cx.global::<SettingsStore>().show_repeat_shuffle();
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let new_val = cx.global::<SettingsStore>().show_repeat_shuffle();
            if new_val != this.show_repeat_shuffle {
                this.show_repeat_shuffle = new_val;
                cx.notify();
            }
        });

        let now_playing = cx.new(|cx| NowPlaying::new(window, cx));

        let np_album_subscription = cx.subscribe(
            &now_playing,
            |_, _, event: &NavigateToAlbumRequested, cx| {
                cx.emit(NavigateToAlbumRequested {
                    album_id: event.album_id,
                });
            },
        );
        let np_artist_subscription = cx.subscribe(
            &now_playing,
            |_, _, event: &NavigateToArtistRequested, cx| {
                cx.emit(NavigateToArtistRequested {
                    artist_id: event.artist_id,
                });
            },
        );

        Self {
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
            shuffle_button: cx.new(|cx| ShuffleButton::new(window, cx)),
            repeat_button: cx.new(|cx| RepeatButton::new(window, cx)),
            volume_slider: cx.new(|cx| Volume::new(window, cx)),
            track_progress_slider: cx.new(|cx| TrackProgressSlider::new(window, cx)),
            now_playing,
            show_queue: false,
            show_repeat_shuffle,
            _subscription: subscription,
            _settings_subscription: settings_subscription,
            _np_album_subscription: np_album_subscription,
            _np_artist_subscription: np_artist_subscription,
        }
    }
}

impl Render for Footer {
    fn render(&mut self, _: &mut gpui::Window, cx: &mut gpui::Context<Self>) -> impl IntoElement {
        let show_repeat_shuffle = self.show_repeat_shuffle;
        h_flex()
            .gap_4()
            .w_full()
            .h_full()
            .items_center()
            .px_4()
            .bg(Colors::app_background(cx))
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
                            .when(show_repeat_shuffle, |b| {
                                b.child(self.shuffle_button.clone())
                            })
                            .child(self.prev_button.clone())
                            .child(self.play_button.clone())
                            .child(self.next_button.clone())
                            .when(show_repeat_shuffle, |b| b.child(self.repeat_button.clone())),
                    )
                    .child(
                        h_flex()
                            .w_full()
                            .px_4()
                            .child(self.track_progress_slider.clone()),
                    ),
            )
            .child({
                let queue_color = if self.show_queue {
                    Colors::text_accent(cx)
                } else {
                    Colors::text_secondary(cx)
                };
                v_flex()
                    .w(px(200.))
                    .items_end()
                    .justify_center()
                    .gap_1()
                    .child(
                        div()
                            .id("queue_toggle")
                            .cursor_pointer()
                            .rounded(px(4.))
                            .hover(|s| s.bg(Colors::control_hover_bg(cx)))
                            .tooltip(|window, cx| Tooltip::new("Queue").build(window, cx))
                            .on_click(cx.listener(|this, _, _, cx| {
                                this.show_queue = !this.show_queue;
                                cx.emit(ToggleQueueEvent {
                                    show: this.show_queue,
                                });
                                cx.notify();
                            }))
                            .child(
                                svg()
                                    .path("icons/s2-queue.svg")
                                    .size(px(22.))
                                    .text_color(queue_color),
                            ),
                    )
                    .child(self.volume_slider.clone())
            })
    }
}
