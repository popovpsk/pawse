use std::time::Duration;

use audio_engine::EngineEvent;
use gpui::{App, AppContext, Context, Entity, EventEmitter, Subscription};

use crate::services::{EngineEventsBus, Services};

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Phase {
    Idle,
    Preparing,
    Ready {
        sample_rate: u32,
        bit_depth: u8,
        dsd_rate: Option<u32>,
    },
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct StatusChanged {
    pub track_changed: bool,
}

pub struct PlaybackStatus {
    track_id: Option<i64>,
    phase: Phase,
    duration: Option<Duration>,
    _subscription: Subscription,
}

impl EventEmitter<StatusChanged> for PlaybackStatus {}

impl PlaybackStatus {
    pub fn create(engine_event_bus: &Entity<EngineEventsBus>, cx: &mut App) -> Entity<Self> {
        let bus = engine_event_bus.clone();
        cx.new(|cx| Self {
            track_id: None,
            phase: Phase::Idle,
            duration: None,
            _subscription: cx.subscribe(&bus, |this: &mut Self, _, event: &EngineEvent, cx| {
                this.apply(event, cx)
            }),
        })
    }

    pub fn track_id(&self) -> Option<i64> {
        self.track_id
    }

    pub fn phase(&self) -> Phase {
        self.phase
    }

    pub fn duration(&self) -> Option<Duration> {
        self.duration
    }

    fn apply(&mut self, event: &EngineEvent, cx: &mut Context<Self>) {
        let Some((phase, duration)) = transition(event) else {
            return;
        };
        let track_id = cx
            .global::<Services>()
            .playback_queue
            .borrow()
            .current_track()
            .map(|track| track.id);
        let track_changed = track_id != self.track_id;
        self.track_id = track_id;
        self.phase = phase;
        self.duration = duration;
        cx.emit(StatusChanged { track_changed });
        cx.notify();
    }
}

fn transition(event: &EngineEvent) -> Option<(Phase, Option<Duration>)> {
    match event {
        EngineEvent::Preparing { duration } => Some((Phase::Preparing, *duration)),
        EngineEvent::Loaded { params, duration } => Some((
            Phase::Ready {
                sample_rate: params.sample_rate,
                bit_depth: params.bit_depth,
                dsd_rate: params.dsd_rate,
            },
            Some(*duration),
        )),
        EngineEvent::TrackEnded | EngineEvent::Stopped | EngineEvent::Error(_) => {
            Some((Phase::Idle, None))
        }
        EngineEvent::Playing
        | EngineEvent::Paused
        | EngineEvent::PositionChanged(_)
        | EngineEvent::Buffering(_) => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_track_lifecycle_events_move_the_phase() {
        assert_eq!(
            transition(&EngineEvent::Preparing {
                duration: Some(Duration::from_secs(3))
            }),
            Some((Phase::Preparing, Some(Duration::from_secs(3))))
        );
        assert_eq!(
            transition(&EngineEvent::Error("x".into())),
            Some((Phase::Idle, None))
        );
        assert_eq!(
            transition(&EngineEvent::TrackEnded),
            Some((Phase::Idle, None))
        );
        assert_eq!(transition(&EngineEvent::Buffering(true)), None);
        assert_eq!(
            transition(&EngineEvent::PositionChanged(Duration::ZERO)),
            None
        );
        assert_eq!(transition(&EngineEvent::Playing), None);
    }
}
