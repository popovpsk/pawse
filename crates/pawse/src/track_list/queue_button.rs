use std::rc::Rc;

use gpui::{App, ElementId, IntoElement, StatefulInteractiveElement};
use gpui_component::tooltip::Tooltip;
use music_library::Track;

use crate::theme_colors::Colors;

use super::{RowButtonColors, row_icon_button};
use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::services::Services;

pub fn add_to_queue_button(
    track: Rc<Track>,
    button_size: f32,
    icon_size: f32,
    colors: &RowButtonColors,
) -> impl IntoElement {
    row_icon_button(
        ElementId::NamedInteger("add-to-queue".into(), track.id as u64),
        button_size,
        "icons/add-queue.svg",
        icon_size,
        colors.icon,
        colors.icon_hover,
        true,
    )
    .tooltip(|window, cx| Tooltip::new(tr().add_to_queue.clone()).build(window, cx))
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
}

pub fn add_album_to_queue_button(
    album_id: i64,
    button_size: f32,
    icon_size: f32,
    cx: &App,
) -> impl IntoElement {
    row_icon_button(
        ElementId::NamedInteger("add-album-to-queue".into(), album_id as u64),
        button_size,
        "icons/add-queue.svg",
        icon_size,
        Colors::text_secondary(cx),
        Colors::control_hover_bg(cx),
        false,
    )
    .tooltip(|window, cx| Tooltip::new(tr().add_album_to_queue.clone()).build(window, cx))
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        let tracks = cx.global::<Services>().library.tracks_for_album(album_id);
        cx.global::<Services>()
            .playback_queue
            .borrow_mut()
            .append_tracks(tracks.into_iter().map(Rc::new).collect());
        let bus = cx.global::<Services>().library_event_bus.clone();
        bus.update(cx, |_, cx| cx.emit(LibraryEvent::QueueChanged));
        crate::services::save_playback(cx);
    })
}
