use std::rc::Rc;

use gpui::{App, AppContext};
use gpui_component::{WindowExt, notification::Notification};
use music_library::Track;
use rand::Rng;
use ui_resources::i18n::similar_strings;

use super::mix::GAP;
use crate::playback_queue::{MixPlan, MixSnapshot, QueueSource};
use crate::services::Services;
use crate::settings_store::SettingsStore;

pub type OnApplied = Rc<dyn Fn(&mut App)>;

type Built = music_library::Result<Option<Vec<Track>>>;

#[derive(Clone, Copy)]
enum Seed {
    Track,
    Queue,
}

pub fn start_radio(on_applied: OnApplied, cx: &mut App) {
    let Some(similar) = super::current(cx) else {
        return;
    };
    let Some(seed_id) = cx
        .global::<Services>()
        .playback_queue
        .borrow()
        .current_track()
        .map(|t| t.id)
    else {
        return;
    };
    let familiarity = cx.global::<SettingsStore>().similar_familiarity();
    let rng_seed: u64 = rand::random();
    let task = cx.background_spawn(async move { similar.radio(seed_id, familiarity, rng_seed) });
    cx.spawn(async move |cx| {
        let built = task.await;
        cx.update(|cx| apply_radio(seed_id, built, &on_applied, cx));
    })
    .detach();
}

pub fn mix_queue(on_applied: OnApplied, cx: &mut App) {
    let Some(similar) = super::current(cx) else {
        return;
    };
    let MixPlan {
        basis,
        keep,
        anchors,
        snapshot,
    } = cx.global::<Services>().playback_queue.borrow().mix_plan();
    if anchors == 0 {
        return;
    }
    let mut rng = rand::rng();
    let gaps: Vec<usize> = (0..anchors).map(|_| rng.random_range(GAP)).collect();
    let count = gaps.iter().sum::<usize>();
    let familiarity = cx.global::<SettingsStore>().similar_familiarity();
    let rng_seed: u64 = rng.random();
    let task = cx
        .background_spawn(async move { similar.mix(&basis, &keep, count, familiarity, rng_seed) });
    cx.spawn(async move |cx| {
        let built = task.await;
        cx.update(|cx| apply_mix(&snapshot, &gaps, built, &on_applied, cx));
    })
    .detach();
}

fn apply_radio(seed_id: i64, built: Built, on_applied: &OnApplied, cx: &mut App) {
    let Some(picks) = usable(built, Seed::Track, cx) else {
        return;
    };
    {
        let mut queue = cx.global::<Services>().playback_queue.borrow_mut();
        let Some(seed) = queue.current_entry().filter(|t| t.id == seed_id).cloned() else {
            return;
        };
        let tracks = std::iter::once(seed)
            .chain(picks.into_iter().map(Rc::new))
            .collect();
        queue.set_tracks_and_play_at(tracks, 0, QueueSource::Unknown);
    }
    on_applied(cx);
    crate::services::queue_mutated(cx);
}

fn apply_mix(
    snapshot: &MixSnapshot,
    gaps: &[usize],
    built: Built,
    on_applied: &OnApplied,
    cx: &mut App,
) {
    let Some(picks) = usable(built, Seed::Queue, cx) else {
        return;
    };
    let applied = cx.global::<Services>().playback_queue.borrow_mut().mix_in(
        snapshot,
        picks.into_iter().map(Rc::new).collect(),
        gaps,
    );
    if applied {
        on_applied(cx);
        crate::services::queue_mutated(cx);
    }
}

fn usable(built: Built, seed: Seed, cx: &mut App) -> Option<Vec<Track>> {
    let s = similar_strings();
    let notification = match built {
        Ok(Some(picks)) if !picks.is_empty() => return Some(picks),
        Ok(Some(_)) => Notification::info(s.nothing_found.clone()),
        Ok(None) => Notification::warning(match seed {
            Seed::Track => s.not_analysed.clone(),
            Seed::Queue => s.queue_not_analysed.clone(),
        }),
        Err(e) => {
            log::error!("similar tracks: could not build the list: {e}");
            Notification::error(s.failed.clone())
        }
    };
    if let Some(handle) = cx.windows().into_iter().next() {
        let _ = handle.update(cx, |_, window, cx| {
            window.push_notification(notification, cx);
        });
    }
    None
}
