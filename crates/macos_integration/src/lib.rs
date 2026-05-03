use std::cell::RefCell;
use std::path::PathBuf;

use flume::Sender;
use media_integration::{MediaCommand, MediaPlaybackState, NowPlayingInfo, SystemMediaIntegration};
use objc2::rc::Retained;
use objc2::runtime::NSObject;
use objc2::{define_class, msg_send, sel, AnyThread};
use objc2_app_kit::NSImage;
use objc2_app_kit::NSStatusItem;
use objc2_foundation::{MainThreadMarker, NSString};

mod now_playing;
mod remote_command;
mod status_bar;

use now_playing::{load_artwork, update_now_playing_info, update_position_info};
use remote_command::{register_remote_commands, RegisteredCommands};
use status_bar::{create_status_bar_item, update_status_bar, MenuState};

// NOTE: `MediaCommandProxy` is instantiated exactly once per application lifetime
// (NSStatusBar is a singleton). We therefore use a `OnceLock` to give the proxy
// access to the command sender without requiring associated-object gymnastics.
// Remote commands (`MPRemoteCommandCenter`) receive the sender explicitly.
static COMMAND_SENDER: std::sync::OnceLock<Sender<MediaCommand>> = std::sync::OnceLock::new();

define_class!(
    #[unsafe(super(NSObject))]
    struct MediaCommandProxy;

    impl MediaCommandProxy {
        #[unsafe(method(onPlay:))]
        fn on_play(&self, _sender: &NSObject) {
            if let Some(s) = COMMAND_SENDER.get() {
                let _ = s.send(MediaCommand::Play);
            }
        }

        #[unsafe(method(onPause:))]
        fn on_pause(&self, _sender: &NSObject) {
            if let Some(s) = COMMAND_SENDER.get() {
                let _ = s.send(MediaCommand::Pause);
            }
        }

        #[unsafe(method(onNext:))]
        fn on_next(&self, _sender: &NSObject) {
            if let Some(s) = COMMAND_SENDER.get() {
                let _ = s.send(MediaCommand::Next);
            }
        }

        #[unsafe(method(onPrevious:))]
        fn on_previous(&self, _sender: &NSObject) {
            if let Some(s) = COMMAND_SENDER.get() {
                let _ = s.send(MediaCommand::Previous);
            }
        }
    }
);

impl MediaCommandProxy {
    fn new() -> Retained<Self> {
        let this = Self::alloc();
        unsafe { msg_send![this, init] }
    }
}

pub struct MacOsIntegration {
    _proxy: Retained<MediaCommandProxy>,
    _commands: RegisteredCommands,
    status_item: Retained<NSStatusItem>,
    menu_state: RefCell<MenuState>,
    cached_artwork_path: RefCell<Option<PathBuf>>,
    cached_artwork: RefCell<Option<Retained<objc2_media_player::MPMediaItemArtwork>>>,
    cached_status_icon: RefCell<Option<Retained<NSImage>>>,
}

impl MacOsIntegration {
    pub fn new(command_sender: Sender<MediaCommand>) -> Option<Self> {
        let mtm = MainThreadMarker::new()?;
        let _ = COMMAND_SENDER.set(command_sender.clone());

        let proxy = MediaCommandProxy::new();
        let (status_item, menu_state) = create_status_bar_item(&proxy, mtm);

        let commands = register_remote_commands(command_sender);

        Some(Self {
            _proxy: proxy,
            _commands: commands,
            status_item,
            menu_state: RefCell::new(menu_state),
            cached_artwork_path: RefCell::new(None),
            cached_artwork: RefCell::new(None),
            cached_status_icon: RefCell::new(None),
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

        // Clear old cache regardless.
        self.cached_artwork_path.borrow_mut().clone_from(&new_path);
        self.cached_artwork.borrow_mut().take();
        self.cached_status_icon.borrow_mut().take();

        if let Some(ref path) = new_path
            && let Some((artwork, image)) = load_artwork(path)
        {
            self.cached_artwork.borrow_mut().replace(artwork);
            self.cached_status_icon.borrow_mut().replace(image);
        }
    }
}

impl SystemMediaIntegration for MacOsIntegration {
    fn update_now_playing(&self, info: NowPlayingInfo) {
        self.update_cached_artwork(&info);

        update_now_playing_info(&info, 1.0);

        // If we have cached artwork, set it in the Now Playing dict.
        let cached = self.cached_artwork.borrow();
        if let Some(ref artwork) = *cached {
            unsafe {
                let center = objc2_media_player::MPNowPlayingInfoCenter::defaultCenter();
                let dict: Retained<objc2_foundation::NSMutableDictionary<objc2_foundation::NSString>> =
                    objc2_foundation::NSMutableDictionary::dictionary();
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
        drop(cached);

        let mut state = self.menu_state.borrow_mut();
        update_status_bar(&self.status_item, &mut state, &info);

        // Update status-bar icon from cache if available.
        let cached_icon = self.cached_status_icon.borrow();
        if let Some(ref image) = *cached_icon
            && let Some(button) = self.status_item.button(MainThreadMarker::new().unwrap())
        {
            button.setImage(Some(image));
        }
    }

    fn set_playback_state(&self, state: MediaPlaybackState) {
        now_playing::set_playback_state(state);

        let menu_state = self.menu_state.borrow();
        let title = match state {
            MediaPlaybackState::Playing => "Pause",
            _ => "Play",
        };
        let selector = match state {
            MediaPlaybackState::Playing => sel!(onPause:),
            _ => sel!(onPlay:),
        };
        unsafe {
            menu_state
                .play_pause_item
                .setTitle(&NSString::from_str(title));
            menu_state.play_pause_item.setAction(Some(selector));
        }
    }

    fn update_position(&self, elapsed_secs: f64, state: MediaPlaybackState) {
        let rate = match state {
            MediaPlaybackState::Playing => 1.0,
            _ => 0.0,
        };
        update_position_info(elapsed_secs, rate);
    }
}
