mod lyrics_line;
mod view;

use gpui::{
    AnyWindowHandle, App, AppContext, Bounds, DisplayId, Global, Pixels, Size, Window,
    WindowBounds, point, px, size,
};
use gpui_component::Root;

use crate::settings_store::SettingsStore;
use crate::window_placement::{self, Slot};
pub use view::MiniPlayer;

const DEFAULT_W: f32 = 480.;
const DEFAULT_H: f32 = 136.;
const MIN_W: f32 = 360.;
const MIN_H: f32 = 112.;
const MAX_W: f32 = 520.;
const MAX_H: f32 = 520.;
const CORNER_MARGIN: f32 = 24.;
#[cfg(target_os = "linux")]
pub(crate) const FRAME: f32 = 42.;
#[cfg(not(target_os = "linux"))]
pub(crate) const FRAME: f32 = 0.;

struct MiniState {
    window: Option<AnyWindowHandle>,
    current: bool,
    lyrics: bool,
}

impl Default for MiniState {
    fn default() -> Self {
        Self {
            window: None,
            current: false,
            lyrics: true,
        }
    }
}

impl Global for MiniState {}

pub fn window(cx: &App) -> Option<AnyWindowHandle> {
    let handle = cx.try_global::<MiniState>()?.window?;
    cx.windows().contains(&handle).then_some(handle)
}

pub fn is_current(cx: &App) -> bool {
    cx.try_global::<MiniState>()
        .is_some_and(|state| state.current)
}

pub(crate) fn max_size() -> Size<Pixels> {
    size(px(MAX_W + FRAME), px(MAX_H + FRAME))
}

pub(crate) fn size_limits(window: &Window) -> window_level::SizeLimits {
    let pad = gpui_component::window_paddings(window);
    let frame = |a: Pixels, b: Pixels| {
        let inset = f32::from(a + b);
        if inset > 0. { inset + 2. } else { 0. }
    };
    let (w, h) = (frame(pad.left, pad.right), frame(pad.top, pad.bottom));
    window_level::SizeLimits {
        min: (MIN_W + w, MIN_H + h),
        max: (MAX_W + w, MAX_H + h),
    }
}

pub fn open(cx: &mut App) {
    to_mini(cx);
}

pub(crate) fn lyrics_shown(cx: &App) -> bool {
    cx.try_global::<MiniState>()
        .is_none_or(|state| state.lyrics)
}

pub(crate) fn set_lyrics_shown(shown: bool, cx: &mut App) {
    cx.default_global::<MiniState>().lyrics = shown;
}

pub fn toggle(cx: &mut App) {
    cx.defer(|cx| {
        if window(cx).is_some() {
            to_main(cx);
        } else {
            to_mini(cx);
        }
    });
}

pub fn enter(cx: &mut App) {
    cx.defer(to_mini);
}

pub fn leave(cx: &mut App) {
    cx.defer(to_main);
}

fn to_mini(cx: &mut App) {
    if window(cx).is_some() || !cx.global::<SettingsStore>().onboarding_complete() {
        return;
    }
    let others = cx.windows();
    let display = others.first().and_then(|handle| {
        handle
            .update(cx, |_, window, cx| {
                window
                    .display(cx)
                    .map(|display| (display.id(), display.visible_bounds()))
            })
            .ok()
            .flatten()
    });
    let mut options = crate::build_window_options(cx);
    options.window_min_size = Some(size(px(MIN_W + FRAME), px(MIN_H + FRAME)));
    options.window_bounds = Some(WindowBounds::Windowed(corner_bounds(display, cx)));
    options.display_id = display.map(|(id, _)| id);
    #[cfg(target_os = "macos")]
    if let Some(titlebar) = options.titlebar.as_mut() {
        titlebar.traffic_light_position = Some(point(
            px(crate::window_title_bar::TRAFFIC_LIGHT_HIDDEN_X),
            px(crate::window_title_bar::TRAFFIC_LIGHT_Y),
        ));
    }
    window_placement::restore(&mut options, Slot::Mini, cx);
    let opened = cx.open_window(options, |window, cx| {
        let view = cx.new(|cx| MiniPlayer::new(window, cx));
        cx.new(|cx| Root::new(view, window, cx))
    });
    match opened {
        Ok(handle) => {
            let state = cx.default_global::<MiniState>();
            state.window = Some(handle.into());
            state.current = true;
        }
        Err(err) => {
            log::error!("mini player: could not open the window: {err:?}");
            return;
        }
    }
    close_later(others, cx);
}

fn to_main(cx: &mut App) {
    let Some(mini) = window(cx) else {
        return;
    };
    cx.default_global::<MiniState>().current = false;
    crate::open_main_window(cx, false);
    close_later(vec![mini], cx);
}

fn close_later(windows: Vec<AnyWindowHandle>, cx: &mut App) {
    cx.spawn(async move |cx| {
        cx.update(|cx| {
            for handle in windows {
                let _ = handle.update(cx, |_, window, _| window.remove_window());
            }
        });
    })
    .detach();
}

fn corner_bounds(display: Option<(DisplayId, Bounds<Pixels>)>, cx: &App) -> Bounds<Pixels> {
    let wanted = size(px(DEFAULT_W + FRAME), px(DEFAULT_H + FRAME));
    let area = display
        .map(|(_, area)| area)
        .or_else(|| cx.primary_display().map(|display| display.visible_bounds()));
    let Some(area) = area else {
        return Bounds::centered(None, wanted, cx);
    };
    let margin = px(CORNER_MARGIN);
    let origin = point(
        area.right() - wanted.width - margin,
        area.bottom() - wanted.height - margin,
    );
    Bounds::new(origin, wanted)
}
