use media_integration::NowPlayingInfo;
use objc2::rc::Retained;
use objc2::{sel, MainThreadOnly};
use objc2_app_kit::{
    NSImage, NSMenu, NSMenuItem, NSStatusBar, NSStatusItem, NSSquareStatusItemLength,
};
use objc2_foundation::{ns_string, MainThreadMarker, NSString};

use crate::MediaCommandProxy;

pub struct MenuState {
    pub title_item: Retained<NSMenuItem>,
    pub artist_item: Retained<NSMenuItem>,
    pub play_pause_item: Retained<NSMenuItem>,
    pub has_track: bool,
}

pub fn create_status_bar_item(
    proxy: &MediaCommandProxy,
    mtm: MainThreadMarker,
) -> (Retained<NSStatusItem>, MenuState) {
    let status_bar = NSStatusBar::systemStatusBar();
    let item = status_bar.statusItemWithLength(NSSquareStatusItemLength);

    let menu = NSMenu::new(mtm);

    let title_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Not Playing"),
            None,
            ns_string!(""),
        )
    };
    title_item.setEnabled(false);
    menu.addItem(&title_item);

    let artist_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!(""),
            None,
            ns_string!(""),
        )
    };
    artist_item.setEnabled(false);
    menu.addItem(&artist_item);

    menu.addItem(&NSMenuItem::separatorItem(mtm));

    let play_pause_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Play"),
            Some(sel!(onPlay:)),
            ns_string!(""),
        )
    };
    unsafe {
        play_pause_item.setTarget(Some(proxy.as_ref()));
    }
    menu.addItem(&play_pause_item);

    let next_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Next Track"),
            Some(sel!(onNext:)),
            ns_string!(""),
        )
    };
    unsafe {
        next_item.setTarget(Some(proxy.as_ref()));
    }
    menu.addItem(&next_item);

    let prev_item = unsafe {
        NSMenuItem::initWithTitle_action_keyEquivalent(
            NSMenuItem::alloc(mtm),
            ns_string!("Previous Track"),
            Some(sel!(onPrevious:)),
            ns_string!(""),
        )
    };
    unsafe {
        prev_item.setTarget(Some(proxy.as_ref()));
    }
    menu.addItem(&prev_item);

    item.setMenu(Some(&menu));

    // Set a default template icon.
    set_default_status_icon(&item);

    let menu_state = MenuState {
        title_item,
        artist_item,
        play_pause_item,
        has_track: false,
    };

    (item, menu_state)
}

pub fn update_status_bar(
    item: &NSStatusItem,
    state: &mut MenuState,
    info: &NowPlayingInfo,
) {
    if info.title.is_empty() {
        state.title_item.setTitle(ns_string!("Not Playing"));
        state.artist_item.setTitle(ns_string!(""));
        state.has_track = false;
        set_default_status_icon(item);
        return;
    }

    state.has_track = true;
    state
        .title_item
        .setTitle(&NSString::from_str(&info.title));
    state
        .artist_item
        .setTitle(&NSString::from_str(&info.artist));

    // Icon updates are handled by the caller (MacOsIntegration) from the artwork cache.
}

pub fn set_default_status_icon(item: &NSStatusItem) {
    let mtm = MainThreadMarker::new().unwrap();
    if let Some(button) = item.button(mtm) {
        let image = NSImage::imageNamed(ns_string!("NSImageNameAudioOutputVolumeHigh"));
        // Fallback: try a system template image name.
        let image = image.or_else(|| NSImage::imageNamed(ns_string!("NSActionTemplate")));
        button.setImage(image.as_deref());
    }
}
