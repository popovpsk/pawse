use gpui::{App, Div, Entity, Hsla, ParentElement, Pixels, Styled, Window, div, px};

use crate::cover_backdrop::{self, Veil};
use crate::cover_mode_view::CoverModeView;
use crate::lyrics_view::LyricsPresentation;
use crate::spectrum_view::SpectrumView;

pub(super) struct Stage {
    pub bare_lyrics: bool,
    pub queue_to_top: bool,
    pub title_bar_bg: Hsla,
    pub strip: Option<Strip>,
    pub lyrics: LyricsPresentation,
    #[cfg(target_os = "macos")]
    pub lights_hidden: bool,
}

pub(super) struct Strip {
    spectrum: Entity<SpectrumView>,
    height: Pixels,
    pub under_lyrics: bool,
}

impl Stage {
    #[allow(clippy::too_many_arguments)]
    pub(super) fn new(
        cover_mode: bool,
        cover: &CoverModeView,
        backdrop: bool,
        title_bar: Hsla,
        background: Hsla,
        veil: Option<Veil>,
        window: &Window,
        cx: &App,
    ) -> Self {
        let full_screen = cover_mode && !cover.chrome_visible();
        let title_bar_bg = if full_screen && backdrop {
            gpui::transparent_black()
        } else {
            cover_backdrop::chrome_bg(if full_screen { background } else { title_bar }, veil)
        };
        let strip = cover_mode
            .then(|| cover.strip_height(window, cx))
            .flatten()
            .map(|height| Strip {
                spectrum: cover.spectrum().clone(),
                height: px(height),
                under_lyrics: full_screen,
            });
        let lyrics = LyricsPresentation {
            bare_header: full_screen,
            edge_fade: full_screen,
            bottom_inset: strip
                .as_ref()
                .filter(|strip| strip.under_lyrics)
                .map_or(px(0.), |strip| strip.height),
        };
        Self {
            bare_lyrics: full_screen,
            queue_to_top: full_screen && backdrop,
            title_bar_bg,
            strip,
            lyrics,
            #[cfg(target_os = "macos")]
            lights_hidden: full_screen && !cover.controls_shown() && !window.is_fullscreen(),
        }
    }
}

impl Strip {
    pub(super) fn layer(self) -> Div {
        div()
            .absolute()
            .left_0()
            .right_0()
            .bottom_0()
            .h(self.height)
            .child(self.spectrum)
    }
}
