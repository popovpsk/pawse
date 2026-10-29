use std::sync::atomic::Ordering;
use std::time::Duration;

use audio_engine::EngineEvent;
use gpui::{Context, SharedString, Subscription, Task};
use pawse_remote::LibraryReader;

use crate::library_service::LibraryEvent;
use crate::lyrics_fill::{self, LyricRow};
use crate::playback_status::StatusChanged;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub struct LyricsLine {
    enabled: bool,
    track_id: Option<i64>,
    rows: Vec<LyricRow>,
    active: Option<usize>,
    prefs: (bool, bool),
    _load: Option<Task<()>>,
    _engine_subscription: Subscription,
    _status_subscription: Subscription,
    _library_subscription: Subscription,
    _settings_subscription: Subscription,
}

impl LyricsLine {
    pub fn new(cx: &mut Context<Self>) -> Self {
        let services = cx.global::<Services>();
        let engine_event_bus = services.engine_event_bus.clone();
        let playback_status = services.playback_status.clone();
        let library_event_bus = services.library_event_bus.clone();

        let engine_subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::PositionChanged(pos) => this.update_active(*pos, cx),
                EngineEvent::Stopped => this.clear(cx),
                _ => {}
            },
        );
        let status_subscription =
            cx.subscribe(&playback_status, |this, status, _: &StatusChanged, cx| {
                if this.enabled && status.read(cx).track_id() != this.track_id {
                    this.load(cx);
                }
            });
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::LyricsChanged { track_id } = event
                    && this.enabled
                    && this.track_id == Some(*track_id)
                {
                    this.load(cx);
                }
            });
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let prefs = lyrics_prefs(cx.global::<SettingsStore>());
            if prefs != this.prefs {
                this.prefs = prefs;
                if this.enabled {
                    this.load(cx);
                }
            }
        });

        let enabled = super::lyrics_shown(cx);
        let mut this = Self {
            enabled,
            track_id: None,
            rows: Vec::new(),
            active: None,
            prefs: lyrics_prefs(cx.global::<SettingsStore>()),
            _load: None,
            _engine_subscription: engine_subscription,
            _status_subscription: status_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        };
        if enabled {
            this.load(cx);
        }
        this
    }

    pub fn set_enabled(&mut self, enabled: bool, cx: &mut Context<Self>) {
        if self.enabled == enabled {
            return;
        }
        self.enabled = enabled;
        if enabled {
            self.load(cx);
        } else {
            self.clear(cx);
        }
    }

    pub fn line(&self) -> Option<SharedString> {
        let row = self.rows.get(self.active?)?;
        (!row.text.is_empty()).then(|| row.text.clone())
    }

    pub fn around(&self) -> Option<Around> {
        if self.rows.is_empty() {
            return None;
        }
        let text = |ix: Option<usize>| {
            ix.and_then(|ix| self.rows.get(ix))
                .map(|row| row.text.clone())
                .filter(|text| !text.is_empty())
        };
        let next = self.active.map_or(Some(0), |ix| ix.checked_add(1));
        Some(Around {
            key: self.active.map_or(0, |ix| ix + 1),
            previous: text(self.active.and_then(|ix| ix.checked_sub(1))),
            current: text(self.active),
            next: text(next),
        })
    }

    fn load(&mut self, cx: &mut Context<Self>) {
        let current = cx
            .global::<Services>()
            .playback_queue
            .borrow()
            .current_track()
            .map(|track| (track.id, track.duration_ms));
        let Some((id, duration_ms)) = current else {
            self.clear(cx);
            return;
        };
        if self.track_id != Some(id) {
            self.track_id = Some(id);
            self.rows.clear();
            self.active = None;
            cx.notify();
        }
        let services = cx.global::<Services>();
        let access = services
            .library
            .library_access()
            .with_remote_media(services.remote_media.clone());
        let duration_ms = duration_ms.map(|ms| ms.max(0) as u64);
        self._load = Some(cx.spawn(async move |this, cx| {
            let found = net_runtime::unblock(move || access.lyrics(id)).await;
            let _ = this.update(cx, |this, cx| {
                if this.track_id != Some(id) {
                    return;
                }
                this.rows = found
                    .text
                    .as_deref()
                    .map(lyrics::parse_lrc)
                    .filter(|parsed| parsed.synced)
                    .map(|parsed| lyrics_fill::build_rows(&parsed, duration_ms))
                    .unwrap_or_default();
                this.active = None;
                let pos = cx
                    .global::<Services>()
                    .current_position_ms
                    .load(Ordering::Relaxed);
                this.update_active(Duration::from_millis(pos), cx);
                if found.searchable && !found.searching {
                    crate::services::find_lyrics(cx, id);
                }
                cx.notify();
            });
        }));
    }

    fn update_active(&mut self, pos: Duration, cx: &mut Context<Self>) {
        let active = lyrics_fill::active_row(&self.rows, pos.as_millis() as u64);
        if active != self.active {
            self.active = active;
            cx.notify();
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self._load = None;
        if self.track_id.take().is_some() || !self.rows.is_empty() {
            self.rows.clear();
            self.active = None;
            cx.notify();
        }
    }
}

pub struct Around {
    pub key: usize,
    pub previous: Option<SharedString>,
    pub current: Option<SharedString>,
    pub next: Option<SharedString>,
}

fn lyrics_prefs(store: &SettingsStore) -> (bool, bool) {
    (store.lyrics_from_internet(), store.lyrics_prefer_web())
}
