use gpui::{App, Styled, px};
use gpui_component::ActiveTheme;

/// Styling for the currently-playing row, shared by every track list and the
/// queue. Fills the row with `list_active` and adds a left accent border;
/// the bottom divider is dropped so only the left accent shows.
///
/// Rows use `pl_4` (16px) and a 1px bottom border. The added 1px left border
/// and dropped bottom border would otherwise inset the content and shift it,
/// so left padding is trimmed to 15px (1px border + 15px = 16px) and 1px is
/// added back to the bottom — content stays aligned with non-current rows.
pub fn current_row<E: Styled>(el: E, cx: &App) -> E {
    let theme = cx.theme();
    el.bg(theme.list_active)
        .border_b(px(0.))
        .pb(px(1.))
        .border_l(px(1.))
        .pl(px(15.))
        .border_color(theme.list_active_border)
}
