use gpui::{App, Hsla};
use gpui_component::ActiveTheme;

pub struct Colors;

impl Colors {
    // ── Text / icon roles ──────────────────────────────────────────────────
    pub fn text_primary(cx: &App) -> Hsla {
        cx.theme().foreground
    }
    pub fn text_secondary(cx: &App) -> Hsla {
        cx.theme().muted_foreground
    }
    /// Active / highlighted state: shuffle on, repeat on, like filled, active tab icon.
    pub fn text_accent(cx: &App) -> Hsla {
        cx.theme().primary
    }
    /// Label on the play button (primary-colored background).
    pub fn text_on_play_button(cx: &App) -> Hsla {
        cx.theme().primary_foreground
    }
    /// Label on a selected/accent-colored row (e.g. selected theme in settings).
    pub fn text_on_selection(cx: &App) -> Hsla {
        cx.theme().accent_foreground
    }
    /// Text inside a popover / floating panel.
    pub fn popover_text(cx: &App) -> Hsla {
        cx.theme().popover_foreground
    }

    // ── Surfaces & backgrounds ─────────────────────────────────────────────
    /// Header bar, settings sidebar, settings section header.
    pub fn header_background(cx: &App) -> Hsla {
        cx.theme().title_bar
    }
    /// Main content area, footer, fade overlays.
    pub fn app_background(cx: &App) -> Hsla {
        cx.theme().background
    }
    /// Floating panel / popover background (queue panel, settings popover, playlist popup).
    pub fn popover_background(cx: &App) -> Hsla {
        cx.theme().popover
    }

    // ── Buttons & interactive surfaces ────────────────────────────────────
    /// Play button fill.
    pub fn play_button_bg(cx: &App) -> Hsla {
        cx.theme().primary
    }
    /// Play button fill on hover.
    pub fn play_button_bg_hover(cx: &App) -> Hsla {
        cx.theme().primary_hover
    }
    /// Hover background for transport controls (prev/next/shuffle/repeat) and settings rows.
    pub fn control_hover_bg(cx: &App) -> Hsla {
        cx.theme().muted
    }
    /// Hover / active background for icon-only action buttons (like, add-to-playlist, queue).
    pub fn icon_button_hover_bg(cx: &App) -> Hsla {
        cx.theme().accent
    }
    /// Active tab background in the main header.
    pub fn tab_active_bg(cx: &App) -> Hsla {
        cx.theme().secondary
    }
    /// Selected-row background (e.g. currently selected theme in settings).
    pub fn selection_bg(cx: &App) -> Hsla {
        cx.theme().accent
    }

    // ── Lists ─────────────────────────────────────────────────────────────
    /// Row hover background in all track/album/artist/queue lists.
    pub fn list_row_hover_bg(cx: &App) -> Hsla {
        cx.theme().list_hover
    }
    /// Background of the currently-playing row.
    pub fn row_current_bg(cx: &App) -> Hsla {
        cx.theme().list_active
    }
    /// Left accent border of the currently-playing row.
    pub fn row_current_border(cx: &App) -> Hsla {
        cx.theme().list_active_border
    }

    // ── Cover art placeholder ─────────────────────────────────────────────
    /// Background of the cover-art placeholder when no image is available.
    pub fn cover_fallback_bg(cx: &App) -> Hsla {
        cx.theme().secondary
    }

    // ── Borders ───────────────────────────────────────────────────────────
    /// General panel/section border (lists, popovers, settings, dividers).
    pub fn panel_border(cx: &App) -> Hsla {
        cx.theme().border
    }
    /// Border shown on the drag-over target row in the queue.
    pub fn drag_over_border(cx: &App) -> Hsla {
        cx.theme().drag_border
    }
}
