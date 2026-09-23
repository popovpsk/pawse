mod color;
mod palette;
mod scheme;

use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::{App, Context, Global, Subscription, Task, WeakEntity, ease_in_out};
use gpui_component::theme::{Theme, ThemeColor, ThemeMode, ThemeTokens};

use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::{SettingsStore, ThemeChoice, apply_theme};
use color::to_oklch;
use palette::palette_from_thumbnail;
use scheme::{LIGHT_COVER, MIN_TINT_STRENGTH, Tint, anchors, matches, mix, relit, tinted};

const FADE: Duration = Duration::from_millis(320);
const FADE_FRAME: Duration = Duration::from_millis(16);
const FADE_STEPS: usize = 16;

pub struct CoverSkin {
    enabled: bool,
    theme_choice: ThemeChoice,
    base: ThemeColor,
    staged: ThemeColor,
    live: ThemeColor,
    cover_l: Option<f32>,
    current: Tint,
    cover_art_id: Option<i64>,
    evaluated: bool,
    _task: Option<Task<()>>,
    _fade: Option<Task<()>>,
    _engine_subscription: Subscription,
    _library_subscription: Subscription,
    _settings_subscription: Subscription,
}

struct Handle(WeakEntity<CoverSkin>);

impl Global for Handle {}

pub fn reapply(cx: &mut App) {
    let Some(skin) = cx
        .try_global::<Handle>()
        .and_then(|handle| handle.0.upgrade())
    else {
        return;
    };
    skin.update(cx, |this, cx| {
        if !this.enabled {
            return;
        }
        let current = this.current;
        this.apply(this.cover_l, current, false, cx);
    });
}

fn fade_frames(from: ThemeColor, to: ThemeColor) -> Vec<ThemeColor> {
    (0..=FADE_STEPS)
        .map(|step| mix(&from, &to, ease_in_out(step as f32 / FADE_STEPS as f32)))
        .collect()
}

impl CoverSkin {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
        let engine_subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => this.refresh(true, cx),
                EngineEvent::TrackEnded | EngineEvent::Stopped => {
                    let idle = {
                        let queue = cx.global::<Services>().playback_queue.borrow();
                        queue.current_track().is_none()
                    };
                    if idle {
                        this.restore(true, cx);
                    }
                }
                _ => {}
            },
        );
        let library_event_bus = cx.global::<Services>().library_event_bus.clone();
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::ScanComplete { changed: true } = event {
                    this.evaluated = false;
                    this.refresh(true, cx);
                }
            });
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let (enabled, choice) = {
                let store = cx.global::<SettingsStore>();
                (store.dynamic_theme(), store.theme())
            };
            if enabled != this.enabled {
                this.enabled = enabled;
                if enabled {
                    this.refresh(false, cx);
                } else {
                    this.forget();
                    apply_theme(&choice, cx);
                }
            } else if enabled && choice != this.theme_choice {
                let current = this.current;
                this.apply(this.cover_l, current, false, cx);
            }
        });

        let mut this = Self {
            enabled: cx.global::<SettingsStore>().dynamic_theme(),
            theme_choice: cx.global::<SettingsStore>().theme(),
            base: Theme::global(cx).colors,
            staged: Theme::global(cx).colors,
            live: Theme::global(cx).colors,
            cover_l: None,
            current: Tint::none(),
            cover_art_id: None,
            evaluated: false,
            _task: None,
            _fade: None,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        };
        let handle = Handle(cx.weak_entity());
        cx.set_global(handle);
        this.refresh(false, cx);
        this
    }

    fn forget(&mut self) {
        self.current = Tint::none();
        self.cover_l = None;
        self.cover_art_id = None;
        self.evaluated = false;
        self._task = None;
        self._fade = None;
    }

    fn rebase(&mut self, cx: &mut Context<Self>) {
        self.theme_choice = cx.global::<SettingsStore>().theme();
        apply_theme(&self.theme_choice.clone(), cx);
        self.base = Theme::global(cx).colors;
        self.staged = self.base;
    }

    fn commit(&mut self, colors: ThemeColor, cx: &mut Context<Self>) {
        self.live = colors;
        let dark = to_oklch(colors.background).l <= LIGHT_COVER;
        {
            let theme = Theme::global_mut(cx);
            theme.mode = if dark {
                ThemeMode::Dark
            } else {
                ThemeMode::Light
            };
            theme.tokens = ThemeTokens::from(&colors);
            theme.colors = colors;
        }
        Theme::sync_base(cx);
        cx.refresh_windows();
    }

    fn write(&mut self, tint: Tint, animate: bool, cx: &mut Context<Self>) {
        self.current = tint;
        let target = tinted(&self.staged, tint);
        let from = self.live;
        if !animate || matches(&from, &target) {
            self._fade = None;
            self.commit(target, cx);
            return;
        }
        self.commit(from, cx);
        let frames = cx
            .background_executor()
            .spawn(async move { fade_frames(from, target) });
        self._fade = Some(cx.spawn(async move |this, cx| {
            let frames = frames.await;
            let last = frames.len() - 1;
            let started = Instant::now();
            loop {
                let progress = started.elapsed().as_secs_f32() / FADE.as_secs_f32();
                let step = ((progress * last as f32) as usize).min(last);
                let running = this.update(cx, |this, cx| {
                    if !matches(&Theme::global(cx).colors, &this.live) {
                        return false;
                    }
                    this.commit(frames[step], cx);
                    true
                });
                if !running.unwrap_or(false) || step == last {
                    break;
                }
                cx.background_executor().timer(FADE_FRAME).await;
            }
        }));
    }

    fn apply(&mut self, cover_l: Option<f32>, target: Tint, animate: bool, cx: &mut Context<Self>) {
        self.live = Theme::global(cx).colors;
        self.rebase(cx);
        self.cover_l = cover_l;
        if let Some(cover_l) = cover_l {
            self.staged = relit(&self.base, anchors(&self.base, cover_l));
        }
        self.write(target, animate, cx);
    }

    fn refresh(&mut self, animate: bool, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }

        let cover_art_id = {
            let queue = cx.global::<Services>().playback_queue.borrow();
            queue.current_track().and_then(|track| track.cover_art_id)
        };
        if self.evaluated && cover_art_id == self.cover_art_id {
            return;
        }
        self.cover_art_id = cover_art_id;
        self.evaluated = true;

        let services = cx.global::<Services>();
        let thumbnail = services
            .cover_art_cache
            .borrow_mut()
            .get_small(cover_art_id, &services.library);
        let Some(thumbnail) = thumbnail else {
            self.restore(animate, cx);
            return;
        };

        let palette = cx
            .background_executor()
            .spawn(async move { palette_from_thumbnail(&thumbnail) });
        self._task = Some(cx.spawn(async move |this, cx| {
            let palette = palette.await;
            let _ = this.update(cx, |this, cx| {
                this._task = None;
                if !this.enabled || this.cover_art_id != cover_art_id {
                    return;
                }
                let target = match palette
                    .as_ref()
                    .and_then(|palette| palette.accent.map(|accent| (accent, palette.confidence)))
                {
                    Some((accent, confidence)) if confidence >= MIN_TINT_STRENGTH => Tint {
                        hue: accent.h,
                        strength: confidence,
                    },
                    _ => this.current.faded(),
                };
                let cover_l = palette.as_ref().map(|palette| palette.l_median);
                log::info!(
                    "cover skin: hue {:.0} strength {:.2} cover L {:.2}",
                    target.hue,
                    target.strength,
                    cover_l.unwrap_or_default()
                );
                this.apply(cover_l, target, animate, cx);
            });
        }));
    }

    fn restore(&mut self, animate: bool, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.cover_art_id = None;
        self.evaluated = false;
        let target = self.current.faded();
        self.apply(None, target, animate, cx);
    }
}
