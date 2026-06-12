use gpui::{App, IntoElement, RenderOnce, Styled, Window, div, px};

#[cfg(target_os = "linux")]
use gpui::{
    Decorations, InteractiveElement, MouseButton, ParentElement, Render,
    StatefulInteractiveElement as _, WindowControlArea, prelude::FluentBuilder, svg,
};

#[cfg(target_os = "linux")]
use gpui_component::{InteractiveElementExt as _, h_flex};

use crate::theme_colors::Colors;

const HEIGHT: f32 = 34.;
const FULLSCREEN_TOP_INSET: f32 = 8.;

pub fn title_bar_height(window: &Window) -> f32 {
    if window.is_fullscreen() {
        FULLSCREEN_TOP_INSET
    } else {
        HEIGHT
    }
}

#[derive(IntoElement, Default)]
pub struct WindowTitleBar;

impl WindowTitleBar {
    pub fn new() -> Self {
        Self
    }
}

#[cfg(not(target_os = "linux"))]
impl RenderOnce for WindowTitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        if window.is_fullscreen() {
            return div()
                .w_full()
                .flex_shrink_0()
                .h(px(FULLSCREEN_TOP_INSET))
                .bg(Colors::background(cx))
                .into_any_element();
        }
        gpui_component::TitleBar::new()
            .bg(Colors::background(cx))
            .border_color(Colors::background(cx))
            .into_any_element()
    }
}

#[cfg(target_os = "linux")]
struct DragState {
    should_move: bool,
}

#[cfg(target_os = "linux")]
impl Render for DragState {
    fn render(&mut self, _: &mut Window, _: &mut gpui::Context<Self>) -> impl IntoElement {
        div()
    }
}

#[cfg(target_os = "linux")]
impl RenderOnce for WindowTitleBar {
    fn render(self, window: &mut Window, cx: &mut App) -> impl IntoElement {
        let is_client_decorated = matches!(window.window_decorations(), Decorations::Client { .. });
        let state = window.use_state(cx, |_, _| DragState { should_move: false });
        let bg = Colors::background(cx);
        let text_secondary = Colors::muted_foreground(cx);
        let text_primary = Colors::foreground(cx);
        let danger = Colors::danger(cx);

        let minimize_btn = div()
            .id("minimize")
            .group("ctrl-min")
            .flex()
            .items_center()
            .justify_center()
            .h_full()
            .px(px(8.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(|_, window, cx| {
                cx.stop_propagation();
                window.minimize_window();
            })
            .child(
                svg()
                    .path("icons/window-minimize.svg")
                    .size(px(14.))
                    .text_color(text_secondary)
                    .group_hover("ctrl-min", |s| s.text_color(text_primary)),
            );

        let max_restore_btn = if window.is_maximized() {
            div()
                .id("restore")
                .group("ctrl-max")
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .px(px(8.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(|_, window, cx| {
                    cx.stop_propagation();
                    window.zoom_window();
                })
                .child(
                    svg()
                        .path("icons/window-restore.svg")
                        .size(px(14.))
                        .text_color(text_secondary)
                        .group_hover("ctrl-max", |s| s.text_color(text_primary)),
                )
        } else {
            div()
                .id("maximize")
                .group("ctrl-max")
                .flex()
                .items_center()
                .justify_center()
                .h_full()
                .px(px(8.))
                .cursor_pointer()
                .on_mouse_down(MouseButton::Left, |_, window, cx| {
                    window.prevent_default();
                    cx.stop_propagation();
                })
                .on_click(|_, window, cx| {
                    cx.stop_propagation();
                    window.zoom_window();
                })
                .child(
                    svg()
                        .path("icons/window-maximize.svg")
                        .size(px(14.))
                        .text_color(text_secondary)
                        .group_hover("ctrl-max", |s| s.text_color(text_primary)),
                )
        };

        let close_btn = div()
            .id("close")
            .group("ctrl-close")
            .flex()
            .items_center()
            .justify_center()
            .h_full()
            .px(px(8.))
            .cursor_pointer()
            .on_mouse_down(MouseButton::Left, |_, window, cx| {
                window.prevent_default();
                cx.stop_propagation();
            })
            .on_click(|_, window, cx| {
                cx.stop_propagation();
                window.remove_window();
            })
            .child(
                svg()
                    .path("icons/window-close.svg")
                    .size(px(14.))
                    .text_color(text_secondary)
                    .group_hover("ctrl-close", |s| s.text_color(danger)),
            );

        div().flex_shrink_0().child(
            div()
                .id("title-bar")
                .flex()
                .flex_row()
                .items_center()
                .justify_between()
                .h(px(HEIGHT))
                .pl(px(12.))
                .border_b_1()
                .border_color(bg)
                .bg(bg)
                .on_double_click(|_, window, _| window.zoom_window())
                .on_mouse_down_out(window.listener_for(&state, |state, _, _, _| {
                    state.should_move = false;
                }))
                .on_mouse_down(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| {
                        state.should_move = true;
                    }),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    window.listener_for(&state, |state, _, _, _| {
                        state.should_move = false;
                    }),
                )
                .on_mouse_move(window.listener_for(&state, |state, _, window, _| {
                    if state.should_move {
                        state.should_move = false;
                        window.start_window_move();
                    }
                }))
                .child(
                    h_flex()
                        .id("bar")
                        .window_control_area(WindowControlArea::Drag)
                        .h_full()
                        .justify_between()
                        .flex_shrink_0()
                        .flex_1()
                        .when(is_client_decorated, |this| {
                            this.child(
                                div()
                                    .top_0()
                                    .left_0()
                                    .absolute()
                                    .size_full()
                                    .h_full()
                                    .on_mouse_down(MouseButton::Right, move |ev, window, _| {
                                        window.show_window_menu(ev.position)
                                    }),
                            )
                        }),
                )
                .child(
                    h_flex()
                        .id("window-controls")
                        .items_center()
                        .flex_shrink_0()
                        .h_full()
                        .pr(px(10.))
                        .child(minimize_btn)
                        .child(max_restore_btn)
                        .child(close_btn),
                ),
        )
    }
}
