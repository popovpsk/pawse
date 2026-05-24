use gpui::{
    App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, px, svg,
};
use gpui_component::{ActiveTheme, tooltip::Tooltip};

use crate::library_service::LibraryEvent;
use crate::like_button::LIKE_ROW_GROUP;
use crate::services::Services;

pub fn add_to_queue_button(
    track: music_library::Track,
    button_size: f32,
    icon_size: f32,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted_bg = theme.muted;
    let icon_color = theme.muted_foreground;

    div()
        .id(ElementId::NamedInteger(
            "add-to-queue".into(),
            track.id as u64,
        ))
        .size(px(button_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .opacity(0.)
        .group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        .hover(|s| s.bg(muted_bg))
        .tooltip(|window, cx| Tooltip::new("Add to queue").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            cx.global::<Services>()
                .playback_queue
                .borrow_mut()
                .append_track(track.clone());
            let bus = cx.global::<Services>().library_event_bus.clone();
            bus.update(cx, |_, cx| cx.emit(LibraryEvent::QueueChanged));
            crate::services::save_playback(cx);
        })
        .child(
            svg()
                .path("icons/add-queue.svg")
                .size(px(icon_size))
                .text_color(icon_color),
        )
}

pub fn add_album_to_queue_button(
    album_id: i64,
    button_size: f32,
    icon_size: f32,
    cx: &App,
) -> impl IntoElement {
    let theme = cx.theme();
    let muted_bg = theme.muted;
    let icon_color = theme.muted_foreground;

    div()
        .id(ElementId::NamedInteger(
            "add-album-to-queue".into(),
            album_id as u64,
        ))
        .size(px(button_size))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(muted_bg))
        .tooltip(|window, cx| Tooltip::new("Add album to queue").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let tracks = cx.global::<Services>().library.tracks_for_album(album_id);
            cx.global::<Services>()
                .playback_queue
                .borrow_mut()
                .append_tracks(tracks);
            let bus = cx.global::<Services>().library_event_bus.clone();
            bus.update(cx, |_, cx| cx.emit(LibraryEvent::QueueChanged));
            crate::services::save_playback(cx);
        })
        .child(
            svg()
                .path("icons/add-queue.svg")
                .size(px(icon_size))
                .text_color(icon_color),
        )
}
