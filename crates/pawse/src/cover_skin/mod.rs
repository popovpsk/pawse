mod color;
mod palette;
mod scheme;

use audio_engine::EngineEvent;
use gpui::{App, Context, Global, Subscription, Task, WeakEntity};
use gpui_component::theme::{Theme, ThemeColor, ThemeMode, ThemeTokens};

use crate::library_service::LibraryEvent;
use crate::services::Services;
use crate::settings_store::{SettingsStore, ThemeChoice, apply_theme};
use color::to_oklch;
use palette::palette_from_thumbnail;
use scheme::{LIGHT_COVER, MIN_TINT_STRENGTH, Tint, anchors, relit, tinted};

pub struct CoverSkin {
    enabled: bool,
    theme_choice: ThemeChoice,
    base: ThemeColor,
    staged: ThemeColor,
    cover_l: Option<f32>,
    current: Tint,
    cover_art_id: Option<i64>,
    evaluated: bool,
    _task: Option<Task<()>>,
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
        this.apply(this.cover_l, current, cx);
    });
}

impl CoverSkin {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
        let engine_subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => this.refresh(cx),
                EngineEvent::TrackEnded | EngineEvent::Stopped => {
                    let idle = {
                        let queue = cx.global::<Services>().playback_queue.borrow();
                        queue.current_track().is_none()
                    };
                    if idle {
                        this.restore(cx);
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
                    this.refresh(cx);
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
                    this.refresh(cx);
                } else {
                    this.forget();
                    apply_theme(&choice, cx);
                }
            } else if enabled && choice != this.theme_choice {
                let current = this.current;
                this.apply(this.cover_l, current, cx);
            }
        });

        let mut this = Self {
            enabled: cx.global::<SettingsStore>().dynamic_theme(),
            theme_choice: cx.global::<SettingsStore>().theme(),
            base: Theme::global(cx).colors,
            staged: Theme::global(cx).colors,
            cover_l: None,
            current: Tint::none(),
            cover_art_id: None,
            evaluated: false,
            _task: None,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        };
        let handle = Handle(cx.weak_entity());
        cx.set_global(handle);
        this.refresh(cx);
        this
    }

    fn forget(&mut self) {
        self.current = Tint::none();
        self.cover_l = None;
        self.cover_art_id = None;
        self.evaluated = false;
        self._task = None;
    }

    fn rebase(&mut self, cx: &mut Context<Self>) {
        self.theme_choice = cx.global::<SettingsStore>().theme();
        apply_theme(&self.theme_choice.clone(), cx);
        self.base = Theme::global(cx).colors;
        self.staged = self.base;
    }

    fn write(&mut self, tint: Tint, cx: &mut Context<Self>) {
        self.current = tint;
        let colors = tinted(&self.staged, tint);
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

    fn apply(&mut self, cover_l: Option<f32>, target: Tint, cx: &mut Context<Self>) {
        self.rebase(cx);
        self.cover_l = cover_l;
        if let Some(cover_l) = cover_l {
            self.staged = relit(&self.base, anchors(&self.base, cover_l));
        }
        self.write(target, cx);
    }

    fn refresh(&mut self, cx: &mut Context<Self>) {
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
            self.restore(cx);
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
                this.apply(cover_l, target, cx);
            });
        }));
    }

    fn restore(&mut self, cx: &mut Context<Self>) {
        if !self.enabled {
            return;
        }
        self.cover_art_id = None;
        self.evaluated = false;
        let target = self.current.faded();
        self.apply(None, target, cx);
    }
}
