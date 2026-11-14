use std::time::Duration;

use gpui::App;

use crate::playback_opener::TrackRequest;
use crate::services::Services;

const NEXT_LEAD: Duration = Duration::from_secs(5);

#[derive(Default)]
pub struct Tracker {
    current: Option<TrackRequest>,
    next_prepared: bool,
}

impl Tracker {
    pub fn loaded(&mut self, cx: &App) {
        self.next_prepared = false;
        let services = cx.global::<Services>();
        let normalization = &services.normalization;
        let queue = services.playback_queue.borrow();
        let current = queue.current_track().map(TrackRequest::from);
        let previous = std::mem::replace(&mut self.current, current.clone());
        if !normalization.is_on() {
            return;
        }
        if let Some(previous) = previous
            && current
                .as_ref()
                .is_none_or(|current| current.id != previous.id)
        {
            normalization.measure(previous);
        }
        if let Some(current) = current {
            normalization.measure(current);
        }
        if let Some(next) = queue.peek_next() {
            normalization.prepare(TrackRequest::from(next.as_ref()));
        }
    }

    pub fn wants_next(&self, position: Duration, duration: Option<Duration>) -> bool {
        !self.next_prepared
            && duration.is_some_and(|duration| duration.saturating_sub(position) <= NEXT_LEAD)
    }

    pub fn prepare_next(&mut self, cx: &App) {
        self.next_prepared = true;
        let services = cx.global::<Services>();
        if let Some(next) = services.playback_queue.borrow().peek_next() {
            services
                .normalization
                .prepare(TrackRequest::from(next.as_ref()));
        }
    }
}

pub fn set_enabled(on: bool, cx: &mut App) {
    let services = cx.global::<Services>();
    services.normalization.set_enabled(on);
    if !on {
        services.output.set_normalized(false);
        return;
    }
    let queue = services.playback_queue.borrow();
    if let Some(current) = queue.current_track() {
        services.normalization.measure(TrackRequest::from(current));
    }
    if let Some(next) = queue.peek_next() {
        services
            .normalization
            .prepare(TrackRequest::from(next.as_ref()));
    }
}
