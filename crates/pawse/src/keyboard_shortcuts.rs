use gpui::{App, KeyBinding, actions};

use crate::app_menu::ToggleMiniPlayer;

actions!(
    pawse,
    [
        SeekForward,
        SeekBackward,
        NextTrack,
        PreviousTrack,
        VolumeUp,
        VolumeDown,
        PlayPause,
        ExitCoverMode,
    ]
);

pub const CONTEXT: &str = "MainView";
pub const MINI_CONTEXT: &str = "MiniPlayer";

const BINDING: &str = "MainView && !Input";
const MINI_BINDING: &str = "MiniPlayer && !Input";

#[cfg(target_os = "macos")]
pub const MINI_PLAYER_KEY: &str = "cmd-alt-m";
#[cfg(not(target_os = "macos"))]
pub const MINI_PLAYER_KEY: &str = "ctrl-shift-m";

pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let (next, prev) = ("cmd-right", "cmd-left");
    #[cfg(not(target_os = "macos"))]
    let (next, prev) = ("ctrl-right", "ctrl-left");

    cx.bind_keys([
        KeyBinding::new("right", SeekForward, Some(BINDING)),
        KeyBinding::new("left", SeekBackward, Some(BINDING)),
        KeyBinding::new(next, NextTrack, Some(BINDING)),
        KeyBinding::new(prev, PreviousTrack, Some(BINDING)),
        KeyBinding::new("up", VolumeUp, Some(BINDING)),
        KeyBinding::new("down", VolumeDown, Some(BINDING)),
        KeyBinding::new("space", PlayPause, Some(BINDING)),
        KeyBinding::new("escape", ExitCoverMode, Some(BINDING)),
        KeyBinding::new("right", SeekForward, Some(MINI_BINDING)),
        KeyBinding::new("left", SeekBackward, Some(MINI_BINDING)),
        KeyBinding::new(next, NextTrack, Some(MINI_BINDING)),
        KeyBinding::new(prev, PreviousTrack, Some(MINI_BINDING)),
        KeyBinding::new("up", VolumeUp, Some(MINI_BINDING)),
        KeyBinding::new("down", VolumeDown, Some(MINI_BINDING)),
        KeyBinding::new("space", PlayPause, Some(MINI_BINDING)),
        KeyBinding::new(MINI_PLAYER_KEY, ToggleMiniPlayer, None),
    ]);

    #[cfg(target_os = "macos")]
    {
        use crate::app_menu::{Hide, HideOthers, Minimize, Quit};
        cx.bind_keys([
            KeyBinding::new("cmd-q", Quit, None),
            KeyBinding::new("cmd-h", Hide, None),
            KeyBinding::new("cmd-alt-h", HideOthers, None),
            KeyBinding::new("cmd-m", Minimize, None),
        ]);
    }
}
