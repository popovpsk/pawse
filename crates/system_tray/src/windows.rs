use std::sync::{Mutex, Once, PoisonError};

use tray_icon::menu::{Menu, MenuEvent, MenuItem, PredefinedMenuItem};
use tray_icon::{Icon, MouseButton, MouseButtonState, TrayIcon, TrayIconBuilder, TrayIconEvent};

use crate::{EventSink, IconImage, TrayConfig, TrayError, TrayEvent, TrayLabels};

const SHOW_ID: &str = "pawse-tray-show";
const QUIT_ID: &str = "pawse-tray-quit";
const APP_ICON_RESOURCE: u16 = 1;

static SINK: Mutex<Option<EventSink>> = Mutex::new(None);
static HANDLERS: Once = Once::new();

pub struct Tray {
    icon: TrayIcon,
    show: MenuItem,
    quit: MenuItem,
}

impl Tray {
    pub fn new(config: TrayConfig, sink: EventSink) -> Result<Self, TrayError> {
        install_handlers();
        *SINK.lock().unwrap_or_else(PoisonError::into_inner) = Some(sink);
        let menu = Menu::new();
        let show = MenuItem::with_id(SHOW_ID, menu_label(&config.labels.show), true, None);
        let quit = MenuItem::with_id(QUIT_ID, menu_label(&config.labels.quit), true, None);
        menu.append_items(&[&show, &PredefinedMenuItem::separator(), &quit])
            .map_err(platform_error)?;
        let icon = TrayIconBuilder::new()
            .with_id(config.id)
            .with_tooltip(&config.title)
            .with_icon(app_icon(&config.icons)?)
            .with_menu(Box::new(menu))
            .with_menu_on_left_click(false)
            .build()
            .map_err(platform_error)?;
        Ok(Self { icon, show, quit })
    }

    pub fn is_shown(&self) -> bool {
        true
    }

    pub fn set_labels(&self, labels: &TrayLabels) {
        self.show.set_text(menu_label(&labels.show));
        self.quit.set_text(menu_label(&labels.quit));
    }

    pub fn window_handle(&self) -> *mut std::ffi::c_void {
        self.icon.window_handle()
    }
}

fn install_handlers() {
    HANDLERS.call_once(|| {
        TrayIconEvent::set_event_handler(Some(|event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                emit(TrayEvent::Show);
            }
        }));
        MenuEvent::set_event_handler(Some(|event: MenuEvent| {
            if event.id == SHOW_ID {
                emit(TrayEvent::Show);
            } else if event.id == QUIT_ID {
                emit(TrayEvent::Quit);
            }
        }));
    });
}

fn emit(event: TrayEvent) {
    let sink = SINK.lock().unwrap_or_else(PoisonError::into_inner).clone();
    if let Some(sink) = sink {
        sink(event);
    }
}

fn app_icon(fallback: &[IconImage]) -> Result<Icon, TrayError> {
    match Icon::from_resource(APP_ICON_RESOURCE, None) {
        Ok(icon) => Ok(icon),
        Err(err) => {
            log::warn!("tray: app icon resource unavailable: {err}");
            let image = fallback
                .first()
                .ok_or_else(|| TrayError::Platform("no tray icon image".into()))?;
            Icon::from_rgba(image.rgba.clone(), image.width, image.height).map_err(platform_error)
        }
    }
}

fn menu_label(label: &str) -> String {
    label.replace('&', "&&")
}

fn platform_error(err: impl std::fmt::Display) -> TrayError {
    TrayError::Platform(err.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ampersands_are_not_mnemonics() {
        assert_eq!(menu_label("Play & pause"), "Play && pause");
    }
}
