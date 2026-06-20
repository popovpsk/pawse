use std::rc::Rc;

use gpui::{
    App, ElementId, IntoElement, ParentElement, StatefulInteractiveElement, Window, div, px,
};
use gpui_component::{WindowExt, button::Button, dialog::DialogButtonProps, tooltip::Tooltip};
use music_library::Track;

use crate::theme_colors::Colors;

use super::{RowButtonColors, row_icon_button};
use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::playback_queue::QueueSource;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub fn append_tracks_to_queue(tracks: Vec<Rc<Track>>, cx: &mut App) {
    if tracks.is_empty() {
        return;
    }
    let dedup = cx.global::<SettingsStore>().queue_deduplication();
    cx.global::<Services>()
        .playback_queue
        .borrow_mut()
        .append_tracks(tracks, dedup);
    let bus = cx.global::<Services>().library_event_bus.clone();
    bus.update(cx, |_, cx| cx.emit(LibraryEvent::QueueChanged));
    crate::services::save_playback(cx);
}

fn replace_queue_and_play(tracks: Vec<Rc<Track>>, index: usize, source: QueueSource, cx: &mut App) {
    let services = cx.global::<Services>();
    let track = services
        .playback_queue
        .borrow_mut()
        .set_tracks_and_play_at(tracks, index, source)
        .cloned();
    if let Some(track) = track {
        services.play_track(&track);
        crate::services::save_playback(cx);
    }
}

pub fn play_replacing_queue(
    tracks: Vec<Rc<Track>>,
    index: usize,
    source: QueueSource,
    window: &mut Window,
    cx: &mut App,
) {
    if !cx.global::<Services>().playback_queue.borrow().is_custom() {
        replace_queue_and_play(tracks, index, source, cx);
        return;
    }
    // why: the dialog builder runs per frame; Rc keeps the per-frame clones to refcount bumps
    let tracks = Rc::new(tracks);
    let clicked = tracks.get(index).cloned();
    window.open_dialog(cx, move |dialog, _, _| {
        let tracks = tracks.clone();
        let clicked = clicked.clone();
        dialog
            .confirm()
            .w(px(640.))
            .title(tr().replace_queue_confirm_title.clone())
            .child(div().child(tr().replace_queue_confirm_message.clone()))
            .footer(move |ok, cancel, window, cx| {
                let clicked = clicked.clone();
                vec![
                    Button::new("dialog-add-to-queue")
                        .label(tr().add_to_queue.clone())
                        .on_click(move |_, window, cx| {
                            if let Some(track) = clicked.clone() {
                                append_tracks_to_queue(vec![track], cx);
                            }
                            window.close_dialog(cx);
                        })
                        .into_any_element(),
                    cancel(window, cx),
                    ok(window, cx),
                ]
            })
            .button_props(
                DialogButtonProps::default()
                    .ok_text(tr().replace_queue.clone())
                    .cancel_text(tr().cancel.clone()),
            )
            .on_ok(move |_, _, cx| {
                replace_queue_and_play((*tracks).clone(), index, source, cx);
                true
            })
    });
}

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
        append_tracks_to_queue(vec![track.clone()], cx);
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
        Colors::muted_foreground(cx),
        Colors::muted(cx),
        false,
    )
    .tooltip(|window, cx| Tooltip::new(tr().add_album_to_queue.clone()).build(window, cx))
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        append_album_to_queue(album_id, cx);
    })
}

pub fn append_album_to_queue(album_id: i64, cx: &mut App) {
    let tracks = cx.global::<Services>().library.tracks_for_album(album_id);
    append_tracks_to_queue(tracks.into_iter().map(Rc::new).collect(), cx);
}
