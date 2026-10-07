pub mod controls;
mod schedule;
pub mod settings;

use std::sync::atomic::Ordering;
use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use audio_output::AudioOutput;
use chrono::NaiveDate;
use gpui::{App, AppContext, Context, Entity, Global, SharedString, Task};
use ui_resources::i18n::tools_strings;

use crate::localization::LangChanged;
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub use schedule::clock_label;

const RESTORE_DELAY: Duration = Duration::from_secs(2);
pub const EXTEND_MIN: u32 = 5;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Armed {
    Until { deadline: Instant, auto: bool },
    EndOfTrack,
}

pub struct SleepTimer {
    armed: Option<Armed>,
    span: Duration,
    fade: Duration,
    badge: Option<SharedString>,
    status: SharedString,
    suppressed_night: Option<NaiveDate>,
    faded: bool,
    tick: Option<Task<()>>,
    restore: Option<Task<()>>,
}

struct SleepTimerGlobal(Entity<SleepTimer>);

impl Global for SleepTimerGlobal {}

pub fn setup(cx: &mut App) {
    let timer = cx.new(|_| SleepTimer::new());
    cx.set_global(SleepTimerGlobal(timer.clone()));

    let engine_bus = cx.global::<Services>().engine_event_bus.clone();
    let on_engine = timer.clone();
    cx.subscribe(&engine_bus, move |_, event: &EngineEvent, cx| {
        if matches!(event, EngineEvent::Playing) {
            on_engine.update(cx, |t, cx| t.on_playing(cx));
        }
    })
    .detach();

    let lang_bus = cx.global::<Services>().lang_event_bus.clone();
    cx.subscribe(&lang_bus, move |_, _: &LangChanged, cx| {
        timer.update(cx, |t, cx| {
            t.relabel();
            cx.notify();
        });
    })
    .detach();
}

pub fn timer(cx: &App) -> Option<Entity<SleepTimer>> {
    cx.try_global::<SleepTimerGlobal>().map(|g| g.0.clone())
}

pub fn stop_at_track_end(cx: &mut App) -> bool {
    let Some(timer) = timer(cx) else {
        return false;
    };
    timer.update(cx, |t, cx| t.take_end_of_track(cx))
}

fn current_night(cx: &App) -> Option<NaiveDate> {
    let settings = cx.global::<SettingsStore>().sleep_timer();
    schedule::window_opened_on(
        chrono::Local::now().naive_local(),
        settings.auto_from_min,
        settings.auto_until_min,
    )
}

impl SleepTimer {
    fn new() -> Self {
        let mut timer = Self {
            armed: None,
            span: Duration::ZERO,
            fade: Duration::ZERO,
            badge: None,
            status: SharedString::default(),
            suppressed_night: None,
            faded: false,
            tick: None,
            restore: None,
        };
        timer.relabel();
        timer
    }

    pub fn armed(&self) -> Option<Armed> {
        self.armed
    }

    pub fn badge(&self) -> Option<SharedString> {
        self.badge.clone()
    }

    pub fn is_countdown(&self) -> bool {
        matches!(self.armed, Some(Armed::Until { .. }))
    }

    pub fn status(&self) -> SharedString {
        self.status.clone()
    }

    pub fn start(&mut self, minutes: u32, cx: &mut Context<Self>) {
        self.arm_until(Duration::from_secs(u64::from(minutes) * 60), false, cx);
    }

    pub fn start_end_of_track(&mut self, cx: &mut Context<Self>) {
        self.armed = Some(Armed::EndOfTrack);
        self.tick = None;
        self.restore_volume(cx);
        self.relabel();
        cx.notify();
    }

    pub fn extend(&mut self, minutes: u32, cx: &mut Context<Self>) {
        let Some(Armed::Until { deadline, .. }) = &mut self.armed else {
            return;
        };
        *deadline += Duration::from_secs(u64::from(minutes) * 60);
        self.span = deadline.saturating_duration_since(Instant::now());
        self.fade = self.wanted_fade(cx);
        self.schedule_tick(cx);
    }

    pub fn cancel(&mut self, cx: &mut Context<Self>) {
        if self.armed.take().is_none() {
            return;
        }
        if let Some(night) = current_night(cx) {
            self.suppressed_night = Some(night);
        }
        self.tick = None;
        self.restore_volume(cx);
        self.relabel();
        cx.notify();
    }

    fn arm_until(&mut self, duration: Duration, auto: bool, cx: &mut Context<Self>) {
        self.armed = Some(Armed::Until {
            deadline: Instant::now() + duration,
            auto,
        });
        self.span = duration;
        self.fade = self.wanted_fade(cx);
        self.restore_volume(cx);
        self.schedule_tick(cx);
    }

    fn on_playing(&mut self, cx: &mut Context<Self>) {
        if self.restore.is_some() {
            self.restore_volume(cx);
        }
        if self.armed.is_some() {
            return;
        }
        let settings = cx.global::<SettingsStore>().sleep_timer();
        if !settings.auto {
            return;
        }
        let Some(night) = current_night(cx) else {
            return;
        };
        if self.suppressed_night == Some(night) {
            return;
        }
        log::info!(
            "sleep timer: armed automatically for {} min",
            settings.auto_duration_min
        );
        self.arm_until(
            Duration::from_secs(u64::from(settings.auto_duration_min) * 60),
            true,
            cx,
        );
    }

    fn take_end_of_track(&mut self, cx: &mut Context<Self>) -> bool {
        if self.armed != Some(Armed::EndOfTrack) {
            return false;
        }
        log::info!("sleep timer: stopping at the end of the track");
        self.armed = None;
        self.relabel();
        cx.notify();
        true
    }

    fn schedule_tick(&mut self, cx: &mut Context<Self>) {
        let first = self.on_tick(cx);
        self.tick = first.map(|delay| {
            cx.spawn(async move |this, cx| {
                let mut delay = delay;
                loop {
                    cx.background_executor().timer(delay).await;
                    match this.update(cx, |t, cx| t.on_tick(cx)) {
                        Ok(Some(next)) => delay = next,
                        _ => break,
                    }
                }
            })
        });
    }

    fn on_tick(&mut self, cx: &mut Context<Self>) -> Option<Duration> {
        let Some(Armed::Until { deadline, .. }) = self.armed else {
            return None;
        };
        let remaining = deadline.saturating_duration_since(Instant::now());
        if remaining.is_zero() {
            self.expire(cx);
            return None;
        }
        self.fade = schedule::settled_fade(self.fade, self.wanted_fade(cx), remaining);
        self.apply_fade(remaining, self.fade, cx);
        self.relabel();
        cx.notify();
        Some(schedule::next_tick(
            remaining,
            self.fade,
            schedule::fade_tick(self.fade),
        ))
    }

    fn wanted_fade(&self, cx: &App) -> Duration {
        let settings = cx.global::<SettingsStore>().sleep_timer();
        schedule::fade_window(settings.fade_out, settings.fade_secs, self.span)
    }

    fn apply_fade(&mut self, remaining: Duration, fade: Duration, cx: &mut Context<Self>) {
        let volume = cx.global::<SettingsStore>().volume();
        let services = cx.global::<Services>();
        if !services.is_playing.load(Ordering::Relaxed) {
            return;
        }
        let gain = schedule::fade_gain(remaining, fade);
        if gain >= 1. {
            if self.faded {
                self.restore_volume(cx);
            }
            return;
        }
        if services.volume_locked() || services.player.is_casting() {
            return;
        }
        services.output.set_volume(volume * gain);
        self.faded = true;
    }

    fn expire(&mut self, cx: &mut Context<Self>) {
        self.armed = None;
        if cx.global::<Services>().is_playing.load(Ordering::Relaxed) {
            log::info!("sleep timer: pausing playback");
            crate::services::pause(cx);
        }
        if self.faded {
            self.restore = Some(cx.spawn(async move |this, cx| {
                cx.background_executor().timer(RESTORE_DELAY).await;
                let _ = this.update(cx, |t, cx| t.apply_full_volume(cx));
            }));
        }
        self.relabel();
        cx.notify();
    }

    fn restore_volume(&mut self, cx: &mut Context<Self>) {
        self.restore = None;
        self.apply_full_volume(cx);
    }

    fn apply_full_volume(&mut self, cx: &mut Context<Self>) {
        if !self.faded {
            return;
        }
        self.faded = false;
        let volume = cx.global::<SettingsStore>().volume();
        cx.global::<Services>().output.set_volume(volume);
    }

    fn relabel(&mut self) {
        let s = tools_strings();
        match self.armed {
            None => {
                self.badge = None;
                self.status = s.timer_off.clone();
            }
            Some(Armed::Until { deadline, auto }) => {
                let remaining = deadline.saturating_duration_since(Instant::now());
                let left = schedule::remaining_label(remaining);
                let status = s.timer_pauses_at(&schedule::end_clock_label(
                    chrono::Local::now().naive_local(),
                    remaining,
                ));
                self.status = SharedString::from(if auto {
                    format!("{status} · {}", s.timer_auto_started)
                } else {
                    status
                });
                self.badge = Some(SharedString::from(left));
            }
            Some(Armed::EndOfTrack) => {
                self.badge = Some(s.timer_end_of_track.clone());
                self.status = s.timer_pauses_after_track.clone();
            }
        }
    }
}
