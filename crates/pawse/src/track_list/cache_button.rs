use gpui::{App, ElementId, IntoElement, StatefulInteractiveElement, Window};
use gpui_component::tooltip::Tooltip;

use super::row_icon_button;
use crate::cache_fill::FillProgress;
use crate::localization::tr;
use crate::theme_colors::Colors;

pub fn save_to_cache_button(
    id: ElementId,
    progress: Option<FillProgress>,
    button_size: f32,
    icon_size: f32,
    cx: &App,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let icon = if progress.is_some() {
        "icons/download-stop.svg"
    } else {
        "icons/download.svg"
    };
    row_icon_button(
        id,
        button_size,
        icon,
        icon_size,
        Colors::muted_foreground(cx),
        Colors::muted(cx),
        false,
    )
    .tooltip(move |window, cx| {
        let text: gpui::SharedString = match progress {
            Some(progress) => tr()
                .cache_fill_progress(
                    &tr().size(progress.done_bytes),
                    &tr().size(progress.total_bytes),
                )
                .into(),
            None => tr().cache_fill.clone(),
        };
        Tooltip::new(text).build(window, cx)
    })
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        on_click(window, cx);
    })
}
