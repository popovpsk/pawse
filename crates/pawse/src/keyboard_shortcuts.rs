use gpui::{App, KeyBinding, actions};

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
    ]
);

pub const CONTEXT: &str = "MainView";

const BINDING: &str = "MainView && !Input";

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
