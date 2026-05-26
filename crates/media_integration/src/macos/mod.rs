use std::cell::RefCell;
use std::path::PathBuf;

use flume::Sender;
use objc2::rc::Retained;
use objc2_foundation::MainThreadMarker;
use objc2_media_player::MPMediaItemArtwork;

use crate::{MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};

pub mod app_icon;
mod now_playing;
mod remote_command;

use now_playing::{load_artwork, update_now_playing_info, update_position_info};
use remote_command::{RegisteredCommands, register_remote_commands};

pub struct MacOsIntegration {
    _commands: RegisteredCommands,
    cached_artwork_path: RefCell<Option<PathBuf>>,
    cached_artwork: RefCell<Option<Retained<MPMediaItemArtwork>>>,
}

impl MacOsIntegration {
    pub fn new(command_sender: Sender<MediaCommand>) -> Option<Self> {
        MainThreadMarker::new()?;

        let commands = register_remote_commands(command_sender);

        Some(Self {
            _commands: commands,
            cached_artwork_path: RefCell::new(None),
            cached_artwork: RefCell::new(None),
        })
    }

    fn update_cached_artwork(&self, info: &NowPlayingInfo) {
        let new_path = info.artwork_path.clone();
        let should_reload = match self.cached_artwork_path.borrow().as_deref() {
            Some(old) => match &new_path {
                Some(new) => old != new,
                None => true,
            },
            None => new_path.is_some(),
        };

        if !should_reload {
            return;
        }

        self.cached_artwork_path.borrow_mut().clone_from(&new_path);
        self.cached_artwork.borrow_mut().take();

        if let Some(ref path) = new_path
            && let Some(artwork) = load_artwork(path)
        {
            self.cached_artwork.borrow_mut().replace(artwork);
        }
    }
}

impl SystemMediaIntegration for MacOsIntegration {
    fn update_now_playing(&self, info: NowPlayingInfo) {
        self.update_cached_artwork(&info);

        update_now_playing_info(&info, 1.0);

        let cached = self.cached_artwork.borrow();
        if let Some(ref artwork) = *cached {
            unsafe {
                let center = objc2_media_player::MPNowPlayingInfoCenter::defaultCenter();
                let dict: Retained<
                    objc2_foundation::NSMutableDictionary<objc2_foundation::NSString>,
                > = objc2_foundation::NSMutableDictionary::dictionary();
                if let Some(prev) = center.nowPlayingInfo() {
                    dict.addEntriesFromDictionary(&prev);
                }
                dict.setObject_forKey(
                    artwork,
                    objc2::runtime::ProtocolObject::from_ref(
                        objc2_media_player::MPMediaItemPropertyArtwork,
                    ),
                );
                center.setNowPlayingInfo(Some(&dict));
            }
        }
    }

    fn set_playback_state(&self, state: MediaPlaybackState) {
        now_playing::set_playback_state(state);
    }

    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState) {
        let rate = match state {
            MediaPlaybackState::Playing => 1.0,
            _ => 0.0,
        };
        update_position_info(elapsed_secs, rate);
    }
}
