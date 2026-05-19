use media_integration::{MediaPlaybackState, NowPlayingInfo};
use objc2::AnyThread;
use objc2::rc::Retained;
use objc2::runtime::ProtocolObject;
use objc2_foundation::{NSMutableDictionary, NSNumber, NSString};
use objc2_media_player::{
    MPMediaItemPropertyAlbumTitle, MPMediaItemPropertyArtist, MPMediaItemPropertyPlaybackDuration,
    MPMediaItemPropertyTitle, MPNowPlayingInfoCenter, MPNowPlayingInfoPropertyElapsedPlaybackTime,
    MPNowPlayingInfoPropertyPlaybackRate, MPNowPlayingPlaybackState,
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

/// Load artwork from a file path and create `MPMediaItemArtwork`
/// for the Now Playing widget.
pub fn load_artwork(
    path: &std::path::Path,
) -> Option<Retained<objc2_media_player::MPMediaItemArtwork>> {
    use block2::RcBlock;
    use core::ptr::NonNull;
    use objc2_app_kit::NSImage;
    use objc2_core_foundation::CGSize;
    use objc2_media_player::MPMediaItemArtwork;

    let path_str = path.to_str()?;
    let image = NSImage::initWithContentsOfFile(NSImage::alloc(), &NSString::from_str(path_str))?;

    let block = RcBlock::new(move |_size: CGSize| -> NonNull<NSImage> { NonNull::from(&*image) });

    let artwork = unsafe {
        MPMediaItemArtwork::initWithBoundsSize_requestHandler(
            MPMediaItemArtwork::alloc(),
            CGSize::new(512.0, 512.0),
            &block,
        )
    };

    Some(artwork)
}
