use audio_engine::EngineEvent;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui_component::tooltip::Tooltip;

use crate::theme_colors::Colors;

use crate::localization::tr;
use crate::services::Services;

struct PlayButtonState {
    is_playing: bool,
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
                    _ => {}
                },
            );

        let is_playing = cx
            .global::<Services>()
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        Self {
            state: PlayButtonState { is_playing },
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if services.playback_queue.borrow().current_track().is_none() {
            return;
        }
        if self.state.is_playing {
            services.engine_manager.pause();
            self.state.is_playing = false;
        } else {
            services.engine_manager.play();
            self.state.is_playing = true;
        }
        cx.notify();
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
            .child(
                svg()
                    .path(icon_path)
                    .size(px(30.))
                    .text_color(Colors::primary_foreground(cx)),
            )
    }
}
