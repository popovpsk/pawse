use crate::MediaCommand;
use block2::RcBlock;
use core::ptr::NonNull;
use flume::Sender;
use objc2::rc::Retained;
use objc2::runtime::AnyObject;
use objc2_media_player::{
    MPChangePlaybackPositionCommandEvent, MPRemoteCommandCenter, MPRemoteCommandEvent,
    MPRemoteCommandHandlerStatus,
};

/// Holds references to the targets registered with `MPRemoteCommandCenter`.
/// The command center retains the handlers internally, but we keep these
/// references to ensure they are not dropped prematurely.
pub struct RegisteredCommands {
    #[allow(dead_code)]
    targets: Vec<Retained<AnyObject>>,
}

pub fn register_remote_commands(sender: Sender<MediaCommand>) -> RegisteredCommands {
    let mut targets = Vec::new();

    unsafe {
        let center = MPRemoteCommandCenter::sharedCommandCenter();

        center.playCommand().removeTarget(None);
        center.pauseCommand().removeTarget(None);
        center.togglePlayPauseCommand().removeTarget(None);
        center.nextTrackCommand().removeTarget(None);
        center.previousTrackCommand().removeTarget(None);
        center.changePlaybackPositionCommand().removeTarget(None);

        // Play
        let play_tx = sender.clone();
        let play_block = RcBlock::new(
            move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let _ = play_tx.send(MediaCommand::Play);
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.playCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&play_block));

        // Pause
        let pause_tx = sender.clone();
        let pause_block = RcBlock::new(
            move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let _ = pause_tx.send(MediaCommand::Pause);
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.pauseCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&pause_block));

        // Toggle Play/Pause
        let toggle_tx = sender.clone();
        let toggle_block = RcBlock::new(
            move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let _ = toggle_tx.send(MediaCommand::TogglePlayPause);
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.togglePlayPauseCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&toggle_block));

        // Next Track
        let next_tx = sender.clone();
        let next_block = RcBlock::new(
            move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let _ = next_tx.send(MediaCommand::Next);
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.nextTrackCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&next_block));

        // Previous Track
        let prev_tx = sender.clone();
        let prev_block = RcBlock::new(
            move |_event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                let _ = prev_tx.send(MediaCommand::Previous);
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.previousTrackCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&prev_block));

        // Seek / Change Playback Position
        let seek_tx = sender.clone();
        let seek_block = RcBlock::new(
            move |event: NonNull<MPRemoteCommandEvent>| -> MPRemoteCommandHandlerStatus {
                // Cast to the concrete event type to read the target position.
                if let Some(ev) = Retained::retain(event.as_ptr()) {
                    let Ok(ev) = ev.downcast::<MPChangePlaybackPositionCommandEvent>() else {
                        return MPRemoteCommandHandlerStatus::CommandFailed;
                    };
                    let _ = seek_tx.send(MediaCommand::Seek(ev.positionTime()));
                }
                MPRemoteCommandHandlerStatus::Success
            },
        );
        let cmd = center.changePlaybackPositionCommand();
        cmd.setEnabled(true);
        targets.push(cmd.addTargetWithHandler(&seek_block));
    }

    RegisteredCommands { targets }
}
