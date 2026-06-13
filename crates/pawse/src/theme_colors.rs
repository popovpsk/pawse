use gpui::{App, Hsla};
use gpui_component::ActiveTheme;

pub struct Colors;

impl Colors {
    /// Default text/icon color: track & album titles, primary labels, prev/next icons, inactive tab icon.
    pub fn foreground(cx: &App) -> Hsla {
        cx.theme().foreground
    }
    /// Muted/secondary text: artist names, durations, hints, year, inactive repeat icon, queue-button icon.
    pub fn muted_foreground(cx: &App) -> Hsla {
        cx.theme().muted_foreground
    }
    /// Accent/active highlight (shuffle-on, repeat-on, active tab icon) and the play-button fill.
    pub fn primary(cx: &App) -> Hsla {
        cx.theme().primary
    }
    /// Play-button fill on hover.
    pub fn primary_hover(cx: &App) -> Hsla {
        cx.theme().primary_hover
    }
    /// Icon/label sitting on the play button (drawn over the primary-colored fill).
    pub fn primary_foreground(cx: &App) -> Hsla {
        cx.theme().primary_foreground
    }
    /// Active tab background and cover-art placeholder background.
    pub fn secondary(cx: &App) -> Hsla {
        cx.theme().secondary
    }
    /// Hover/active bg for icon-only action buttons (like, add-to-playlist, queue) and selected-row bg.
    pub fn accent(cx: &App) -> Hsla {
        cx.theme().accent
    }
    /// Text/icon on a selected (accent-colored) row, e.g. the selected theme in settings.
    pub fn accent_foreground(cx: &App) -> Hsla {
        cx.theme().accent_foreground
    }
    /// Hover bg for transport controls (prev/next/shuffle/repeat) and settings rows.
    pub fn muted(cx: &App) -> Hsla {
        cx.theme().muted
    }
    /// Central content area and fade overlays.
    pub fn background(cx: &App) -> Hsla {
        cx.theme().background
    }
    /// Top/bottom chrome: window title-bar, header bar, footer, and settings section-header background.
    pub fn title_bar(cx: &App) -> Hsla {
        cx.theme().title_bar
    }
    /// Subtle elevated surface for grouped controls: settings group cards and their dropdown triggers.
    pub fn group_box(cx: &App) -> Hsla {
        cx.theme().group_box
    }
    /// Floating-panel / popover background (queue panel, settings popover, playlist popup).
    pub fn popover(cx: &App) -> Hsla {
        cx.theme().popover
    }
    /// Text inside a popover / floating panel.
    pub fn popover_foreground(cx: &App) -> Hsla {
        cx.theme().popover_foreground
    }
    /// Row hover background across all track/album/artist/queue lists.
    pub fn list_hover(cx: &App) -> Hsla {
        cx.theme().list_hover
    }
    /// Background of the currently-playing row.
    pub fn list_active(cx: &App) -> Hsla {
        cx.theme().list_active
    }
    /// Left accent border of the currently-playing row.
    pub fn list_active_border(cx: &App) -> Hsla {
        cx.theme().list_active_border
    }
    /// General panel/section border: lists, popovers, settings, dividers.
    pub fn border(cx: &App) -> Hsla {
        cx.theme().border
    }
    /// Border on the drag-over target row in the queue.
    pub fn drag_border(cx: &App) -> Hsla {
        cx.theme().drag_border
    }
    /// Danger color: the window close button's hover state in the title bar (Linux).
    pub fn danger(cx: &App) -> Hsla {
        cx.theme().danger
    }
}
