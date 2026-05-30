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
    ]
);

pub const CONTEXT: &str = "MainView";

pub fn init(cx: &mut App) {
    #[cfg(target_os = "macos")]
    let (next, prev) = ("cmd-right", "cmd-left");
    #[cfg(not(target_os = "macos"))]
    let (next, prev) = ("ctrl-right", "ctrl-left");

    cx.bind_keys([
        KeyBinding::new("right", SeekForward, Some(CONTEXT)),
        KeyBinding::new("left", SeekBackward, Some(CONTEXT)),
        KeyBinding::new(next, NextTrack, Some(CONTEXT)),
        KeyBinding::new(prev, PreviousTrack, Some(CONTEXT)),
        KeyBinding::new("up", VolumeUp, Some(CONTEXT)),
        KeyBinding::new("down", VolumeDown, Some(CONTEXT)),
    ]);
}
