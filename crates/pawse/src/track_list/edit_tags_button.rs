use std::rc::Rc;

use gpui::{App, ElementId, IntoElement, StatefulInteractiveElement};
use gpui_component::tooltip::Tooltip;

use super::{RowButtonColors, row_icon_button};
use crate::localization::tr;
use crate::theme_colors::Colors;

const BUTTON_SIZE: f32 = 26.;
const ICON_SIZE: f32 = 14.;

/// Pencil next to the row's other action buttons. Opens the tag-editor dialog.
/// Hidden when the tag editor is off in settings — the caller must check
/// `tag_editor_enabled` first, once per frame rather than per row.
pub fn edit_tags_button(
    track: Rc<music_library::Track>,
    colors: &RowButtonColors,
) -> impl IntoElement {
    let track_id = track.id;
    row_icon_button(
        ElementId::NamedInteger("edit-tags".into(), track_id as u64),
        BUTTON_SIZE,
        "icons/s1-pencil.svg",
        ICON_SIZE,
        colors.icon,
        colors.icon_hover,
        true,
    )
    .tooltip(|window, cx| Tooltip::new(tr().edit_tags.clone()).build(window, cx))
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        crate::tag_editor_view::open_for_track(track.clone(), window, cx);
    })
}

/// Album-header pencil. Edits the fields shared by the whole album and writes
/// them to every one of its files, which is the only place those fields can be
/// changed without the album's tracks disagreeing with each other.
pub fn edit_album_tags_button(
    album_id: i64,
    button_size: f32,
    icon_size: f32,
    cx: &App,
) -> impl IntoElement {
    row_icon_button(
        ElementId::NamedInteger("edit-album-tags".into(), album_id as u64),
        button_size,
        "icons/s1-pencil.svg",
        icon_size,
        Colors::muted_foreground(cx),
        Colors::muted(cx),
        false,
    )
    .tooltip(|window, cx| Tooltip::new(tr().edit_album_tags.clone()).build(window, cx))
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        crate::tag_editor_view::open_for_album(album_id, window, cx);
    })
}
