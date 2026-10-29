use gpui::{App, Bounds, Pixels, Size, Window, WindowBounds, WindowOptions, point, px, size};

use crate::settings_store::{SettingsStore, WindowPlacement};

#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Slot {
    Main,
    Mini,
}

impl Slot {
    fn saved(self, store: &SettingsStore) -> Option<WindowPlacement> {
        match self {
            Slot::Main => store.window_placement(),
            Slot::Mini => store.mini_window_placement(),
        }
    }

    fn max(self) -> Option<Size<Pixels>> {
        match self {
            Slot::Main => None,
            Slot::Mini => Some(crate::mini_player::max_size()),
        }
    }

    fn store(self, store: &SettingsStore, placement: WindowPlacement) {
        match self {
            Slot::Main => store.remember_window_placement(placement),
            Slot::Mini => store.remember_mini_window_placement(placement),
        }
    }
}

pub fn restore(options: &mut WindowOptions, slot: Slot, cx: &App) {
    let Some(saved) = slot.saved(cx.global::<SettingsStore>()) else {
        return;
    };
    let min = options.window_min_size.unwrap_or_default();
    let saved_size = size(px(saved.width), px(saved.height));
    let saved_size = slot.max().map_or(saved_size, |max| saved_size.min(&max));
    let display = saved.display.as_deref().and_then(|uuid| {
        cx.displays()
            .into_iter()
            .find(|display| display.uuid().is_ok_and(|id| id.to_string() == uuid))
    });
    let on_display = display.and_then(|display| {
        let bounds = Bounds::new(
            point(px(saved.x), px(saved.y)),
            fit(saved_size, display.visible_bounds().size, min),
        );
        display
            .bounds()
            .contains(&bounds.center())
            .then(|| (display.id(), bounds))
    });
    let bounds = match on_display {
        Some((display_id, bounds)) => {
            options.display_id = Some(display_id);
            bounds
        }
        None => {
            options.display_id = None;
            let available = cx
                .primary_display()
                .map_or(saved_size, |display| display.visible_bounds().size);
            Bounds::centered(None, fit(saved_size, available, min), cx)
        }
    };
    options.window_bounds = Some(if saved.maximized {
        WindowBounds::Maximized(bounds)
    } else {
        WindowBounds::Windowed(bounds)
    });
}

pub fn remember(window: &Window, slot: Slot, cx: &App) {
    if !window.is_window_active() {
        return;
    }
    let (bounds, maximized) = match window.inner_window_bounds() {
        WindowBounds::Windowed(bounds) => (bounds, false),
        WindowBounds::Maximized(bounds) if cfg!(target_os = "windows") && slot == Slot::Main => {
            (bounds, true)
        }
        WindowBounds::Maximized(_) | WindowBounds::Fullscreen(_) => return,
    };
    let display = window.display(cx);
    let minimized = cfg!(target_os = "windows")
        && !display
            .as_ref()
            .is_some_and(|display| display.bounds().intersects(&window.bounds()));
    if minimized {
        return;
    }
    let display = display
        .and_then(|display| display.uuid().ok())
        .map(|id| id.to_string());
    slot.store(
        cx.global::<SettingsStore>(),
        WindowPlacement {
            display,
            x: f32::from(bounds.origin.x),
            y: f32::from(bounds.origin.y),
            width: f32::from(bounds.size.width),
            height: f32::from(bounds.size.height),
            maximized,
        },
    );
}

fn fit(wanted: Size<Pixels>, available: Size<Pixels>, min: Size<Pixels>) -> Size<Pixels> {
    wanted.min(&available).max(&min)
}
