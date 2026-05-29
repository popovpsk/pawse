//! Shared building blocks for the track-list views (album tracks, liked,
//! playlist, artist) and the queue. Each view embeds [`TrackRowBase`] for the
//! fields every row needs and adds only the extra fields it actually uses, so
//! a view never pays memory for a field it doesn't render.
//!
//! The submodules hold the shared row controls (`like_button`, the
//! add/remove-to-queue and playlist buttons, and the `current_row` styling)
//! re-exported here so call sites use a single `crate::track_list::` namespace.

mod like_button;
mod playlist_buttons;
mod queue_button;
mod row_style;

pub use like_button::{LIKE_ROW_GROUP, like_button};
pub use playlist_buttons::{add_to_playlist_button, remove_from_playlist_button};
pub use queue_button::{add_album_to_queue_button, add_to_queue_button};
pub use row_style::current_row;

use gpui::{App, Div, Hsla, ParentElement, SharedString, Styled, div, px};

use crate::theme_colors::Colors;

/// Theme colors used by the per-row controls (`like_button`, the queue and
/// playlist buttons). Resolve once per render via [`RowButtonColors::from_cx`]
/// and pass by reference into the button builders, so the buttons don't re-read
/// the theme for every visible row on every frame.
#[derive(Clone, Copy)]
pub struct RowButtonColors {
    pub icon_hover: Hsla,
    pub icon: Hsla,
    pub accent: Hsla,
}

impl RowButtonColors {
    pub fn from_cx(cx: &App) -> Self {
        Self {
            icon_hover: Colors::icon_button_hover_bg(cx),
            icon: Colors::text_secondary(cx),
            accent: Colors::text_accent(cx),
        }
    }
}

/// Fields common to every track row. Embed this in a view's row struct via
/// composition and call [`TrackRowBase::from_track`] from the row constructor.
#[derive(Clone, Debug)]
pub struct TrackRowBase {
    pub id: i64,
    pub title: SharedString,
    pub duration: SharedString,
    pub liked: bool,
}

impl TrackRowBase {
    pub fn from_track(track: &music_library::Track) -> Self {
        Self {
            id: track.id,
            title: track.title.clone().into(),
            duration: fmt_duration(track.duration_ms),
            liked: track.liked,
        }
    }
}

/// `mm:ss` from a millisecond duration; empty string when unknown.
pub fn fmt_duration(duration_ms: Option<i64>) -> SharedString {
    duration_ms
        .map(|ms| {
            let secs = (ms / 1000) as u32;
            format!("{:02}:{:02}", secs / 60, secs % 60)
        })
        .unwrap_or_default()
        .into()
}

/// `N.` from a track number; empty string when unknown.
pub fn fmt_track_num(track_number: Option<i32>) -> SharedString {
    track_number
        .map(|n| format!("{}.", n))
        .unwrap_or_default()
        .into()
}

/// The fixed-width duration cell shared by every track-list row.
pub fn track_duration(cx: &App, duration: SharedString) -> Div {
    div()
        .flex_shrink_0()
        .size(px(40.))
        .flex()
        .items_center()
        .justify_center()
        .text_sm()
        .text_color(Colors::text_secondary(cx))
        .child(duration)
}
