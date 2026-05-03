use media_integration::{MediaPlaybackState, NowPlayingInfo};
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2::AnyThread;
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState,
    MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist,
    MPMediaItemPropertyPlaybackDuration, MPMediaItemPropertyTitle,
};

pub fn update_now_playing_info(info: &NowPlayingInfo, playback_rate: f64) {
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();

        let dict: Retained<NSMutableDictionary<NSString>> = NSMutableDictionary::dictionary();

        // Preserve any existing metadata (e.g. elapsed time set by update_position).
        if let Some(prev) = center.nowPlayingInfo() {
            dict.addEntriesFromDictionary(&prev);
        }

        if !info.title.is_empty() {
            let ns = NSString::from_str(&info.title);
            dict.setObject_forKey(&ns, ProtocolObject::from_ref(MPMediaItemPropertyTitle));
        }

        if !info.artist.is_empty() {
            let ns = NSString::from_str(&info.artist);
            dict.setObject_forKey(&ns, ProtocolObject::from_ref(MPMediaItemPropertyArtist));
        }

        if !info.album.is_empty() {
            let ns = NSString::from_str(&info.album);
            dict.setObject_forKey(&ns, ProtocolObject::from_ref(MPMediaItemPropertyAlbumTitle));
        }

        if info.duration_secs > 0.0 {
            let ns = NSNumber::new_f64(info.duration_secs);
            dict.setObject_forKey(
                &ns,
                ProtocolObject::from_ref(MPMediaItemPropertyPlaybackDuration),
            );
        }

        if let Some(elapsed) = info.elapsed_secs
            && elapsed >= 0.0
        {
            let ns = NSNumber::new_f64(elapsed);
            dict.setObject_forKey(
                &ns,
                ProtocolObject::from_ref(MPNowPlayingInfoPropertyElapsedPlaybackTime),
            );
        }

        let rate = NSNumber::new_f64(playback_rate);
        dict.setObject_forKey(
            &rate,
            ProtocolObject::from_ref(MPNowPlayingInfoPropertyPlaybackRate),
        );

        center.setNowPlayingInfo(Some(&dict));
    }
}

pub fn update_position_info(elapsed_secs: f64, playback_rate: f64) {
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();

        let dict: Retained<NSMutableDictionary<NSString>> = NSMutableDictionary::dictionary();

        if let Some(prev) = center.nowPlayingInfo() {
            dict.addEntriesFromDictionary(&prev);
        }

        let elapsed = NSNumber::new_f64(elapsed_secs);
        dict.setObject_forKey(
            &elapsed,
            ProtocolObject::from_ref(MPNowPlayingInfoPropertyElapsedPlaybackTime),
        );

        let rate = NSNumber::new_f64(playback_rate);
        dict.setObject_forKey(
            &rate,
            ProtocolObject::from_ref(MPNowPlayingInfoPropertyPlaybackRate),
        );

        center.setNowPlayingInfo(Some(&dict));
    }
}

pub fn set_playback_state(state: MediaPlaybackState) {
    unsafe {
        let center = MPNowPlayingInfoCenter::defaultCenter();
        let mp_state = match state {
            MediaPlaybackState::Playing => MPNowPlayingPlaybackState::Playing,
            MediaPlaybackState::Paused => MPNowPlayingPlaybackState::Paused,
            MediaPlaybackState::Stopped => MPNowPlayingPlaybackState::Stopped,
        };
        center.setPlaybackState(mp_state);
    }
}

/// Load artwork from a file path and create both the `MPMediaItemArtwork`
/// (for Now Playing) and an `NSImage` suitable for the status bar.
pub fn load_artwork(
    path: &std::path::Path,
) -> Option<(
    Retained<objc2_media_player::MPMediaItemArtwork>,
    Retained<objc2_app_kit::NSImage>,
)> {
    use block2::RcBlock;
    use core::ptr::NonNull;
    use objc2_app_kit::NSImage;
    use objc2_core_foundation::CGSize;
    use objc2_media_player::MPMediaItemArtwork;

    let path_str = path.to_str()?;
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path_str))?;

    let image_for_block = image.clone();

    // SAFETY: `image_for_block` is moved into the block which is retained by
    // `MPMediaItemArtwork`. The block (and thus the image) lives as long as
    // the artwork object.
    let block = RcBlock::new(move |_size: CGSize| -> NonNull<NSImage> {
        NonNull::from(&*image_for_block)
    });

    let artwork = unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            CGSize::new(512.0, 512.0),
            &block,
        )
    };

    Some((artwork, image))
}
