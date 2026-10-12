use std::rc::Rc;

use gpui::{App, AppContext};
use music_library::Track;

use super::{OnApplied, State};
use crate::playback_queue::QueueSnapshot;
use crate::services::Services;
use crate::settings_store::SettingsStore;

type Built = music_library::Result<Option<Vec<Track>>>;

pub fn set_queue_refresh(refresh: OnApplied, cx: &mut App) {
    if cx.has_global::<State>() {
        cx.global_mut::<State>().queue_refresh = Some(refresh);
    }
}

pub fn continue_ended_queue(last_id: i64, cx: &mut App) {
    if !cx.global::<SettingsStore>().similar_autoplay() {
        return;
    }
    let Some(similar) = super::current(cx) else {
        return;
    };
    let snapshot = cx.global::<Services>().playback_queue.borrow().snapshot();
    let queue = snapshot.ids().to_vec();
    let familiarity = cx.global::<SettingsStore>().similar_familiarity();
    let rng_seed: u64 = rand::random();
    let task =
        cx.background_spawn(async move { similar.radio(last_id, &queue, familiarity, rng_seed) });
    cx.spawn(async move |cx| {
        let built = task.await;
        cx.update(|cx| apply(&snapshot, built, cx));
    })
    .detach();
}

fn apply(snapshot: &QueueSnapshot, built: Built, cx: &mut App) {
    if !super::is_running(cx) {
        return;
    }
    let picks = match built {
        Ok(Some(picks)) if !picks.is_empty() => picks,
        Ok(Some(_)) => {
            log::info!("similar tracks: no similar tracks to continue the queue with");
            return;
        }
        Ok(None) => {
            log::info!("similar tracks: the queue ended on a track without a vector");
            return;
        }
        Err(e) => {
            log::error!("similar tracks: could not continue the queue: {e}");
            return;
        }
    };
    let services = cx.global::<Services>().clone();
    let Some(first) = services
        .playback_queue
        .borrow_mut()
        .continue_with(snapshot, picks.into_iter().map(Rc::new).collect())
    else {
        return;
    };
    let next = services
        .playback_queue
        .borrow_mut()
        .play_track_at(first)
        .cloned();
    if let Some(track) = next {
        services.play_track(&track);
    }
    let refresh = cx
        .try_global::<State>()
        .and_then(|state| state.queue_refresh.clone());
    if let Some(refresh) = refresh {
        refresh(cx);
    }
    crate::services::queue_mutated(cx);
}
