use gpui::{App, ElementId, IntoElement, SharedString, StatefulInteractiveElement, Window};
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
    progress_button(
        id,
        progress,
        "icons/download.svg",
        button_size,
        icon_size,
        cx,
        |progress| match progress {
            Some(progress) => tr()
                .cache_fill_progress(
                    &tr().size(progress.done_bytes),
                    &tr().size(progress.total_bytes),
                )
                .into(),
            None => tr().cache_fill.clone(),
        },
        on_click,
    )
}

pub fn move_to_local_button(
    id: ElementId,
    progress: Option<FillProgress>,
    button_size: f32,
    icon_size: f32,
    cx: &App,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    progress_button(
        id,
        progress,
        "icons/folder-down.svg",
        button_size,
        icon_size,
        cx,
        |progress| match progress {
            Some(progress) => tr()
                .album_to_local_progress(
                    &tr().size(progress.done_bytes),
                    &tr().size(progress.total_bytes),
                )
                .into(),
            None => tr().album_to_local.clone(),
        },
        on_click,
    )
}

#[allow(clippy::too_many_arguments)]
fn progress_button(
    id: ElementId,
    progress: Option<FillProgress>,
    idle_icon: &'static str,
    button_size: f32,
    icon_size: f32,
    cx: &App,
    tooltip: fn(Option<FillProgress>) -> SharedString,
    on_click: impl Fn(&mut Window, &mut App) + 'static,
) -> impl IntoElement {
    let icon = if progress.is_some() {
        "icons/download-stop.svg"
    } else {
        idle_icon
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
    .tooltip(move |window, cx| Tooltip::new(tooltip(progress)).build(window, cx))
    .on_click(move |_, window, cx| {
        cx.stop_propagation();
        on_click(window, cx);
    })
}
