use gpui::{
    App, ElementId, InteractiveElement, IntoElement, MouseButton, ParentElement,
    StatefulInteractiveElement, Styled, div, point, px, svg,
};
use gpui_component::{ActiveTheme, tooltip::Tooltip};

use crate::like_button::LIKE_ROW_GROUP;
use crate::playback_queue::QueueSource;
use crate::playlist_popup::OpenAddToPlaylist;
use crate::services::Services;

const BUTTON_SIZE: f32 = 26.;
const ICON_SIZE: f32 = 14.;

/// "+" button placed next to the heart icon. Opens the global playlist popup
/// anchored under the click position. Hidden when the playlist feature is
/// disabled — caller must check `playlists_enabled` first.
pub fn add_to_playlist_button(track_id: i64, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted_bg = theme.muted;
    let icon_color = theme.muted_foreground;

    div()
        .id(ElementId::NamedInteger(
            "add-to-playlist".into(),
            track_id as u64,
        ))
        .size(px(BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .opacity(0.)
        .group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        .hover(|s| s.bg(muted_bg))
        .tooltip(|window, cx| Tooltip::new("Add to playlist").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |event, _, cx| {
            cx.stop_propagation();
            let click_pos = event.position();
            // Anchor the popup just below the click point so the popup
            // appears near the button but doesn't cover the row.
            let anchor = point(click_pos.x - px(220.), click_pos.y + px(8.));
            let bus = cx.global::<Services>().playlist_popup_bus.clone();
            bus.update(cx, |_, cx| {
                cx.emit(OpenAddToPlaylist { track_id, anchor });
            });
        })
        .child(
            svg()
                .path("icons/s1-plus.svg")
                .size(px(ICON_SIZE))
                .text_color(icon_color),
        )
}

/// "x" button shown when the row is part of a currently-playing playlist.
/// Removes the track from the playlist (and from the active queue).
pub fn remove_from_playlist_button(track_id: i64, playlist_id: i64, cx: &App) -> impl IntoElement {
    let theme = cx.theme();
    let muted_bg = theme.muted;
    let icon_color = theme.muted_foreground;

    div()
        .id(ElementId::NamedInteger(
            "remove-from-playlist".into(),
            track_id as u64,
        ))
        .size(px(BUTTON_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .opacity(0.)
        .group_hover(LIKE_ROW_GROUP, |s| s.opacity(1.))
        .hover(|s| s.bg(muted_bg))
        .tooltip(|window, cx| Tooltip::new("Remove from playlist").build(window, cx))
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
        .on_click(move |_, _, cx| {
            cx.stop_propagation();
            let services = cx.global::<Services>();

            // The `PlaylistTracksChanged` event will sync the queue contents
            // via `sync_queue_with_playlist`. We only need to handle the case
            // where the playing track itself was the one removed — advance
            // before the DB mutation so we can pick the *next* track in the
            // queue order, and only if the playing queue is backed by this
            // playlist (otherwise we'd skip songs in an unrelated album/etc).
            let queue_matches = matches!(
                services.playback_queue.borrow().source(),
                QueueSource::Playlist(id) if id == playlist_id,
            );
            let advance_needed = queue_matches
                && services
                    .playback_queue
                    .borrow()
                    .current_track()
                    .map(|t| t.id)
                    == Some(track_id);

            if advance_needed {
                // Advance forward one position regardless of repeat mode —
                // `next_track()` under `RepeatMode::One` would loop on the
                // very track we're removing.
                let next = {
                    let mut queue = services.playback_queue.borrow_mut();
                    let next_ix = queue.current_index().map(|i| i + 1).unwrap_or(0);
                    if next_ix < queue.tracks_vec().len() {
                        queue.play_track_at(next_ix).cloned()
                    } else {
                        None
                    }
                };
                if let Some(next) = next {
                    services.play_track(&next);
                } else {
                    services.engine_manager.pause();
                }
            }

            services
                .library
                .remove_track_from_playlist(playlist_id, track_id);
            crate::services::save_playback(cx);
        })
        .child(
            svg()
                .path("icons/s1-x.svg")
                .size(px(ICON_SIZE))
                .text_color(icon_color),
        )
}
