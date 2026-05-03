use audio_engine::EngineEvent;
use gpui::{
    ClickEvent, Context, InteractiveElement, IntoElement, ParentElement, Render,
    StatefulInteractiveElement, Styled, Subscription, Window, div, px, svg,
};
use gpui_component::ActiveTheme;

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
                    EngineEvent::TrackEnded => {
                        this.state.is_playing = false;
                        cx.notify();
                    }
                    _ => {}
                },
            );

        Self {
            state: PlayButtonState { is_playing: false },
            _subscription: subscription,
        }
    }

    fn on_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        if self.state.is_playing {
            services.engine_manager.pause();
        } else {
            services.engine_manager.play();
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

        div()
            .id("play_button")
            .cursor_pointer()
            .size(px(40.))
            .relative()
            .rounded_full()
            .bg(cx.theme().primary)
            .hover(|style| style.bg(cx.theme().primary_hover))
            .on_click(cx.listener(PlayButton::on_click))
            .child(
                div()
                    .absolute()
                    .top(px(-4.))
                    .left(px(-7.))
                    .size(px(54.))
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(
                        svg()
                            .path(icon_path)
                            .size(px(56.))
                            .text_color(cx.theme().primary_foreground),
                    ),
            )
    }
}
