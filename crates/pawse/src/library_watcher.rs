use std::path::PathBuf;

use gpui::App;
use notify::{Event, RecommendedWatcher, RecursiveMode, Watcher};

use crate::services::Services;
use crate::settings_store::SettingsStore;

pub struct LibraryWatcher {
    _watcher: RecommendedWatcher,
}

impl LibraryWatcher {
    fn start(folders: &[PathBuf], ping: flume::Sender<()>) -> Option<Self> {
        if folders.is_empty() {
            return None;
        }

        let mut watcher =
            match notify::recommended_watcher(move |res: notify::Result<Event>| match res {
                Ok(_) => {
                    let _ = ping.try_send(());
                }
                Err(e) => log::warn!("Library watcher error: {e}"),
            }) {
                Ok(w) => w,
                Err(e) => {
                    log::warn!("Failed to start library watcher: {e}");
                    return None;
                }
            };

        let mut watched_any = false;
        for folder in folders {
            match watcher.watch(folder.as_path(), RecursiveMode::Recursive) {
                Ok(()) => watched_any = true,
                Err(e) => log::warn!("Failed to watch {}: {e}", folder.display()),
            }
        }

        watched_any.then_some(Self { _watcher: watcher })
    }
}

pub fn rebuild(cx: &mut App) {
    let folders = cx.global::<SettingsStore>().music_folders().to_vec();
    let services = cx.global::<Services>();
    let watcher = LibraryWatcher::start(&folders, services.watcher_ping_tx.clone());
    *services.library_watcher.borrow_mut() = watcher;
}
