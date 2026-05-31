use gpui::{ElementId, IntoElement, StatefulInteractiveElement, point, px};
use gpui_component::tooltip::Tooltip;

use super::{RowButtonColors, row_icon_button};
use crate::localization::tr;
use crate::playback_queue::QueueSource;
use crate::playlist_popup::OpenAddToPlaylist;
use crate::services::Services;

const BUTTON_SIZE: f32 = 26.;
const ICON_SIZE: f32 = 14.;

/// "+" button placed next to the heart icon. Opens the global playlist popup
/// anchored under the click position. Hidden when the playlist feature is
/// disabled — caller must check `playlists_enabled` first.
pub fn add_to_playlist_button(track_id: i64, colors: &RowButtonColors) -> impl IntoElement {
    row_icon_button(
        ElementId::NamedInteger("add-to-playlist".into(), track_id as u64),
        BUTTON_SIZE,
        "icons/s1-plus.svg",
        ICON_SIZE,
        colors.icon,
        colors.icon_hover,
        true,
    )
    .tooltip(|window, cx| Tooltip::new(tr().add_to_playlist.clone()).build(window, cx))
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
}

/// "x" button shown when the row is part of a currently-playing playlist.
/// Removes the track from the playlist (and from the active queue).
pub fn remove_from_playlist_button(
    track_id: i64,
    playlist_id: i64,
    colors: &RowButtonColors,
) -> impl IntoElement {
    row_icon_button(
        ElementId::NamedInteger("remove-from-playlist".into(), track_id as u64),
        BUTTON_SIZE,
        "icons/s1-x.svg",
        ICON_SIZE,
        colors.icon,
        colors.icon_hover,
        true,
    )
    .tooltip(|window, cx| Tooltip::new(tr().remove_from_playlist.clone()).build(window, cx))
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
                if next_ix < queue.len() {
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

        // `save_playback` runs centrally in `sync_queue_with_playlist`
        // after the PlaylistTracksChanged event is processed; calling it
        // here would snapshot the stale queue (still holding the removed
        // track) into settings.json.
        services
            .library
            .remove_track_from_playlist(playlist_id, track_id);
    })
}
