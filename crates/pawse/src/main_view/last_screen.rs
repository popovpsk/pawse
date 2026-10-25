use gpui::{App, Context, Global, Window};

use super::MainView;
use crate::library_views::library_view::NavKey;

#[derive(Clone)]
pub(super) struct LastScreen {
    screen: Screen,
    library: Vec<NavKey>,
    queue: Panel,
    lyrics: Panel,
}

impl Global for LastScreen {}

#[derive(Clone, Copy)]
enum Screen {
    Library,
    Settings(usize),
    Tools(usize),
    Cover { full_screen: bool },
}

#[derive(Clone, Copy)]
struct Panel {
    shown: bool,
    width: f32,
}

impl MainView {
    pub(super) fn remember_screen(&self, cx: &mut App) {
        let screen = if self.cover_mode {
            Screen::Cover {
                full_screen: !self.cover_mode_view.read(cx).chrome_visible(),
            }
        } else if self.show_settings {
            Screen::Settings(self.settings_screen.read(cx).page_ix())
        } else if self.show_tools {
            Screen::Tools(self.tools_view.read(cx).page_ix())
        } else {
            Screen::Library
        };
        let last = LastScreen {
            screen,
            library: self.library_view.read(cx).nav_keys(),
            queue: Panel {
                shown: self.show_queue,
                width: self.queue_width,
            },
            lyrics: Panel {
                shown: self.show_lyrics,
                width: self.lyrics_width,
            },
        };
        cx.set_global(last);
    }

    pub(super) fn restore_screen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let Some(last) = cx.try_global::<LastScreen>().cloned() else {
            return;
        };
        self.library_view
            .update(cx, |view, cx| view.restore(&last.library, window, cx));
        let view = self.library_view.read(cx);
        self.is_drilled_in = view.is_drilled_in();
        self.view_menu_tab = view.view_menu_tab();
        self.current_tab = view.root_tab();

        self.queue_width = last.queue.width;
        self.lyrics_width = last.lyrics.width;
        if last.queue.shown {
            self.footer.update(cx, |f, cx| f.set_show_queue(true, cx));
            self.set_queue_visible(true, cx);
        }
        if last.lyrics.shown {
            self.footer.update(cx, |f, cx| f.set_show_lyrics(true, cx));
            self.set_lyrics_visible(true, cx);
        }

        match last.screen {
            Screen::Library => {}
            Screen::Settings(page_ix) => self.open_settings_at(page_ix, None, window, cx),
            Screen::Tools(page_ix) => self.open_tools_page(page_ix, window, cx),
            Screen::Cover { full_screen } => {
                self.set_cover_mode(true, cx);
                if full_screen {
                    self.cover_mode_view
                        .update(cx, |view, cx| view.toggle_chrome(cx));
                }
            }
        }
    }
}
