use audio_engine::EngineEvent;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, prelude::FluentBuilder, px, svg,
};
use gpui_component::{Sizable, spinner::Spinner, tooltip::Tooltip};

use crate::theme_colors::Colors;

use crate::localization::tr;
use crate::services::Services;

struct PlayButtonState {
    is_playing: bool,
    is_buffering: bool,
}

pub struct PlayButton {
    state: PlayButtonState,
    _subscription: Subscription,
}

impl PlayButton {
    pub fn new(_window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();

        // The engine emits these when a fade completes; they are authoritative
        // and reconcile the optimistic icon set in `on_click`.
        let subscription =
            cx.subscribe(
                &engine_event_bus,
                |this, _, event: &EngineEvent, cx| match event {
                    EngineEvent::Playing => {
                        this.state.is_playing = true;
                        cx.notify();
                    }
                    EngineEvent::Paused => {
                        this.state.is_playing = false;
                        cx.notify();
                    }
                    EngineEvent::TrackEnded | EngineEvent::Stopped => {
                        this.state.is_playing = false;
                        cx.notify();
                    }
                    EngineEvent::Buffering(buffering) => {
                        this.state.is_buffering = *buffering;
                        cx.notify();
                    }
                    _ => {}
                },
            );

        let services = cx.global::<Services>();
        let is_playing = services
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);
        let is_buffering = services
            .is_buffering
            .load(std::sync::atomic::Ordering::Relaxed);

        Self {
            state: PlayButtonState {
                is_playing,
                is_buffering,
            },
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(playing) = crate::services::toggle_play_pause(cx) {
            self.state.is_playing = playing;
            cx.notify();
        }
    }
}

impl Render for PlayButton {
    fn render(&mut self, _: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let icon_path: &str = if !self.state.is_playing {
            "icons/play.svg"
        } else {
            "icons/pause.svg"
        };

        let tooltip_text = if !self.state.is_playing {
            tr().play.clone()
        } else {
            tr().pause.clone()
        };

        div()
            .id("play_button")
            .cursor_pointer()
            .size(px(36.))
            .flex()
            .items_center()
            .justify_center()
            .rounded_full()
            .bg(Colors::primary(cx))
            .hover(|style| style.bg(Colors::primary_hover(cx)))
            .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
            .on_click(cx.listener(PlayButton::on_click))
            .when(self.state.is_buffering, |this| {
                this.child(
                    Spinner::new()
                        .with_size(px(22.))
                        .color(Colors::primary_foreground(cx)),
                )
            })
            .when(!self.state.is_buffering, |this| {
                this.child(
                    svg()
                        .path(icon_path)
                        .size(px(30.))
                        .text_color(Colors::primary_foreground(cx)),
                )
            })
    }
}
