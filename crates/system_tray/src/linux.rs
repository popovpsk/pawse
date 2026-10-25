use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use ksni::blocking::{Handle, TrayMethods};
use ksni::menu::StandardItem;

use crate::{EventSink, IconImage, TrayConfig, TrayError, TrayEvent, TrayLabels};

pub struct Tray {
    handle: Handle<Item>,
    shown: Arc<AtomicBool>,
}

impl Tray {
    pub fn new(config: TrayConfig, sink: EventSink) -> Result<Self, TrayError> {
        let shown = Arc::new(AtomicBool::new(true));
        let item = Item {
            id: config.id,
            title: config.title,
            show_label: menu_label(&config.labels.show),
            quit_label: menu_label(&config.labels.quit),
            icons: config.icons.iter().map(argb_icon).collect(),
            sink,
            shown: shown.clone(),
        };
        let handle = item
            .disable_dbus_name(is_flatpak())
            .assume_sni_available(false)
            .spawn()
            .map_err(|err| TrayError::Platform(err.to_string()))?;
        Ok(Self { handle, shown })
    }

    pub fn is_shown(&self) -> bool {
        self.shown.load(Ordering::Relaxed)
    }

    pub fn set_labels(&self, labels: &TrayLabels) {
        let show = menu_label(&labels.show);
        let quit = menu_label(&labels.quit);
        self.handle.update(|item| {
            item.show_label = show;
            item.quit_label = quit;
        });
    }
}

impl Drop for Tray {
    fn drop(&mut self) {
        drop(self.handle.shutdown());
    }
}

struct Item {
    id: String,
    title: String,
    show_label: String,
    quit_label: String,
    icons: Vec<ksni::Icon>,
    sink: EventSink,
    shown: Arc<AtomicBool>,
}

impl ksni::Tray for Item {
    fn id(&self) -> String {
        self.id.clone()
    }

    fn title(&self) -> String {
        self.title.clone()
    }

    fn icon_pixmap(&self) -> Vec<ksni::Icon> {
        self.icons.clone()
    }

    fn tool_tip(&self) -> ksni::ToolTip {
        ksni::ToolTip {
            title: self.title.clone(),
            ..Default::default()
        }
    }

    fn activate(&mut self, _x: i32, _y: i32) {
        (self.sink)(TrayEvent::Show);
    }

    fn menu(&self) -> Vec<ksni::MenuItem<Self>> {
        vec![
            StandardItem {
                label: self.show_label.clone(),
                activate: Box::new(|item: &mut Self| (item.sink)(TrayEvent::Show)),
                ..Default::default()
            }
            .into(),
            ksni::MenuItem::Separator,
            StandardItem {
                label: self.quit_label.clone(),
                activate: Box::new(|item: &mut Self| (item.sink)(TrayEvent::Quit)),
                ..Default::default()
            }
            .into(),
        ]
    }

    fn watcher_online(&self) {
        self.shown.store(true, Ordering::Relaxed);
    }

    fn watcher_offline(&self, reason: ksni::OfflineReason) -> bool {
        log::warn!("tray: status notifier watcher went offline: {reason:?}");
        self.shown.store(false, Ordering::Relaxed);
        true
    }
}

fn menu_label(label: &str) -> String {
    label.replace('_', "__")
}

fn argb_icon(image: &IconImage) -> ksni::Icon {
    ksni::Icon {
        width: image.width as i32,
        height: image.height as i32,
        data: rgba_to_argb(&image.rgba),
    }
}

fn rgba_to_argb(rgba: &[u8]) -> Vec<u8> {
    let (pixels, _) = rgba.as_chunks::<4>();
    pixels
        .iter()
        .flat_map(|&[r, g, b, a]| [a, r, g, b])
        .collect()
}

fn is_flatpak() -> bool {
    std::path::Path::new("/.flatpak-info").exists()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pixels_move_alpha_first() {
        assert_eq!(
            rgba_to_argb(&[1, 2, 3, 4, 5, 6, 7, 8]),
            vec![4, 1, 2, 3, 8, 5, 6, 7]
        );
    }

    #[test]
    fn underscores_are_not_mnemonics() {
        assert_eq!(menu_label("Quit_now"), "Quit__now");
    }
}
