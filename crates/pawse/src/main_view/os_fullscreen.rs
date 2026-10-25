use gpui::{Context, Window};

use super::MainView;
use crate::settings_store::{FullScreenMode, SettingsStore};

impl MainView {
    pub(super) fn sync_os_fullscreen(&mut self, window: &mut Window, cx: &mut Context<Self>) {
        let want = cfg!(not(target_os = "macos"))
            && cx.global::<SettingsStore>().full_screen_mode() == FullScreenMode::System
            && self.cover_mode
            && !self.cover_mode_view.read(cx).chrome_visible();
        if want && !self.os_fullscreen {
            if window.is_maximized() && !window.is_fullscreen() {
                window.toggle_fullscreen();
                self.os_fullscreen = true;
            }
        } else if !want && self.os_fullscreen {
            self.os_fullscreen = false;
            if window.is_fullscreen() {
                window.toggle_fullscreen();
            }
        }
    }
}
