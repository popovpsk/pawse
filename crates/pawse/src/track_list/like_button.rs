use super::{RowButtonColors, row_icon_button};
use crate::localization::tr;
use crate::services::Services;
use gpui::{ElementId, IntoElement, StatefulInteractiveElement};
use gpui_component::tooltip::Tooltip;

/// Group name used by track rows so the like-button can reveal itself on row hover.
/// Apply `.group(LIKE_ROW_GROUP)` to the row container and the outline heart inside
/// the row will fade in via `.group_hover(...)`.
pub const LIKE_ROW_GROUP: &str = "pawse-track-row";

pub const LIKE_BUTTON_SIZE: f32 = 26.;

pub fn like_button(track_id: i64, liked: bool, colors: &RowButtonColors) -> impl IntoElement {
    let icon_color = if liked { colors.accent } else { colors.icon };
    let icon_path = if liked {
        "icons/s1-heart-fill.svg"
    } else {
        "icons/s1-heart.svg"
    };
    let tooltip_text = if liked {
        tr().remove_from_liked.clone()
    } else {
        tr().add_to_liked.clone()
    };

    row_icon_button(
        ElementId::NamedInteger("like".into(), track_id as u64),
        LIKE_BUTTON_SIZE,
        icon_path,
        15.,
        icon_color,
        colors.icon_hover,
        !liked,
    )
    .tooltip(move |window, cx| Tooltip::new(tooltip_text.clone()).build(window, cx))
    .on_click(move |_, _, cx| {
        cx.stop_propagation();
        let services = cx.global::<Services>();
        services.library.set_liked(track_id, !liked);
    })
}
