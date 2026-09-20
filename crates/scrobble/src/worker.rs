use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::thread;
use std::time::{Duration, Instant};

use flume::{Receiver, RecvTimeoutError, Sender};

use crate::store::{Love, Outcome, Play, ScrobbleStore};
use crate::target::{ScrobbleTarget, SubmitError, TargetId};
use crate::{NowPlaying, Scrobble};

const CAP: usize = 5000;
const RUN_LIMIT: usize = 700;
const BACKOFF_MIN: Duration = Duration::from_secs(60);
const BACKOFF_MAX: Duration = Duration::from_secs(30 * 60);
const RESUME_SOON: Duration = Duration::from_secs(5);

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum StatusEvent {
    AuthFailed { target: TargetId, message: String },
    Rejected { target: TargetId, message: String },
}

enum Msg {
    Configure(Vec<Box<dyn ScrobbleTarget>>),
    NowPlaying(NowPlaying),
    Flush,
}

#[derive(Clone)]
pub struct ScrobbleHandle {
    tx: Sender<Msg>,
    store: Arc<dyn ScrobbleStore>,
    target_ids: Arc<Mutex<Vec<TargetId>>>,
    love_target_ids: Arc<Mutex<Vec<TargetId>>>,
}

impl ScrobbleHandle {
    pub fn spawn(
        store: Arc<dyn ScrobbleStore>,
        targets: Vec<Box<dyn ScrobbleTarget>>,
        status: Sender<StatusEvent>,
    ) -> Self {
        let target_ids = Arc::new(Mutex::new(
            targets.iter().map(|t| t.id()).collect::<Vec<_>>(),
        ));
        let love_target_ids = Arc::new(Mutex::new(love_targets(&targets)));
        let (tx, rx) = flume::unbounded();
        let spawned = {
            let store = store.clone();
            let target_ids = target_ids.clone();
            thread::Builder::new()
                .name("scrobble".to_string())
                .spawn(move || {
                    let mut worker = Worker {
                        targets,
                        target_ids,
                        store,
                        backoff: HashMap::new(),
                        disabled: HashSet::new(),
                        status,
                    };
                    worker.run(rx);
                })
        };
        if let Err(e) = spawned {
            log::error!("scrobble: failed to spawn worker thread: {e}");
        }
        Self {
            tx,
            store,
            target_ids,
            love_target_ids,
        }
    }

    pub fn configure(&self, targets: Vec<Box<dyn ScrobbleTarget>>) {
        *self.target_ids.lock().unwrap() = targets.iter().map(|t| t.id()).collect();
        *self.love_target_ids.lock().unwrap() = love_targets(&targets);
        let _ = self.tx.send(Msg::Configure(targets));
    }

    pub fn now_playing(&self, now_playing: NowPlaying) {
        let _ = self.tx.send(Msg::NowPlaying(now_playing));
    }

    pub fn scrobble(&self, play: Play) -> Option<i64> {
        let qualified = play.qualified;
        let id = self.persist(play);
        if qualified {
            let _ = self.tx.send(Msg::Flush);
        }
        id
    }

    pub fn persist(&self, play: Play) -> Option<i64> {
        let targets = if play.qualified {
            self.target_ids.lock().unwrap().clone()
        } else {
            Vec::new()
        };
        if play.qualified && targets.is_empty() {
            log::debug!("scrobble: play qualified but no target is configured");
        }
        match self.store.record_play(&play, &targets) {
            Ok(id) => Some(id),
            Err(e) => {
                log::error!("scrobble: failed to record play: {e}");
                None
            }
        }
    }

    pub fn love(&self, love: Love) {
        let targets = self.love_target_ids.lock().unwrap().clone();
        if targets.is_empty() {
            return;
        }
        if let Err(e) = self.store.record_love(&love, &targets) {
            log::error!("scrobble: failed to record love: {e}");
            return;
        }
        let _ = self.tx.send(Msg::Flush);
    }

    pub fn flush(&self) {
        let _ = self.tx.send(Msg::Flush);
    }
}

struct Worker {
    targets: Vec<Box<dyn ScrobbleTarget>>,
    target_ids: Arc<Mutex<Vec<TargetId>>>,
    store: Arc<dyn ScrobbleStore>,
    backoff: HashMap<TargetId, (Instant, Duration)>,
    disabled: HashSet<TargetId>,
    status: Sender<StatusEvent>,
}

impl Worker {
    fn pending_for(&self, targets: &[TargetId]) -> usize {
        match self.store.pending_count(targets) {
            Ok(count) => count,
            Err(e) => {
                log::warn!("scrobble: failed to count pending items: {e}");
                0
            }
        }
    }

    fn run(&mut self, rx: Receiver<Msg>) {
        if let Err(e) = self.store.trim(CAP) {
            log::warn!("scrobble: failed to trim the queue: {e}");
        }
        let carried = self.pending_for(&self.active_targets());
        if carried > 0 {
            log::info!("scrobble: {carried} pending item(s) from a previous session");
        }
        self.flush();
        loop {
            let msg = match self.next_wakeup() {
                Some(timeout) => match rx.recv_timeout(timeout) {
                    Ok(msg) => Some(msg),
                    Err(RecvTimeoutError::Timeout) => None,
                    Err(RecvTimeoutError::Disconnected) => break,
                },
                None => match rx.recv() {
                    Ok(msg) => Some(msg),
                    Err(_) => break,
                },
            };
            match msg {
                Some(Msg::Configure(targets)) => {
                    let ids: Vec<TargetId> = targets.iter().map(|t| t.id()).collect();
                    *self.target_ids.lock().unwrap() = ids;
                    self.targets = targets;
                    self.disabled.clear();
                    self.backoff.clear();
                    self.flush();
                }
                Some(Msg::NowPlaying(now_playing)) => self.send_now_playing(&now_playing),
                Some(Msg::Flush) | None => self.flush(),
            }
        }
    }

    fn active_targets(&self) -> Vec<TargetId> {
        self.targets.iter().map(|t| t.id()).collect()
    }

    fn next_wakeup(&self) -> Option<Duration> {
        let now = Instant::now();
        self.targets
            .iter()
            .map(|t| t.id())
            .filter(|id| !self.disabled.contains(id) && self.pending_for(&[*id]) > 0)
            .map(|id| match self.backoff.get(&id) {
                Some((at, _)) => at.saturating_duration_since(now),
                None => RESUME_SOON,
            })
            .min()
    }

    fn flush(&mut self) {
        let now = Instant::now();
        for id in self.active_targets() {
            if self.disabled.contains(&id) || self.pending_for(&[id]) == 0 {
                continue;
            }
            if self.backoff.get(&id).is_some_and(|(at, _)| *at > now) {
                continue;
            }
            self.flush_target(id);
        }
        if let Err(e) = self.store.trim(CAP) {
            log::warn!("scrobble: failed to trim the queue: {e}");
        }
    }

    fn flush_target(&mut self, id: TargetId) {
        let Some(pos) = self.targets.iter().position(|t| t.id() == id) else {
            return;
        };
        let max_batch = self.targets[pos].max_batch().max(1);
        let mut sent = 0usize;

        let loves = match self.store.pending_loves(id, RUN_LIMIT) {
            Ok(loves) => loves,
            Err(e) => {
                log::warn!("scrobble: {} failed to read pending loves: {e}", id.label());
                return;
            }
        };
        for (item_id, love) in loves {
            let result = self.targets[pos].love(&love.artist, &love.title, love.loved, love.at);
            let (outcome, keep_going) = self.classify(id, 1, result);
            if !self.settle_loves(id, &[item_id], &outcome) {
                self.bump(id);
                return;
            }
            if !keep_going {
                return;
            }
            sent += 1;
        }

        while sent < RUN_LIMIT {
            let batch = match self
                .store
                .pending_scrobbles(id, max_batch.min(RUN_LIMIT - sent))
            {
                Ok(batch) => batch,
                Err(e) => {
                    log::warn!(
                        "scrobble: {} failed to read pending scrobbles: {e}",
                        id.label()
                    );
                    return;
                }
            };
            if batch.is_empty() {
                break;
            }
            let item_ids: Vec<i64> = batch.iter().map(|(item_id, _)| *item_id).collect();
            let items: Vec<Scrobble> = batch.into_iter().map(|(_, s)| s).collect();
            let result = self.targets[pos].submit(&items);
            let (outcome, keep_going) = self.classify(id, items.len(), result);
            if !self.settle_scrobbles(id, &item_ids, &outcome) {
                self.bump(id);
                return;
            }
            if !keep_going {
                return;
            }
            sent += items.len();
        }
    }

    fn settle_scrobbles(&self, id: TargetId, ids: &[i64], outcome: &Outcome) -> bool {
        match self.store.settle_scrobbles(ids, id, outcome) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "scrobble: {} delivered {} item(s) but could not record it, pausing: {e}",
                    id.label(),
                    ids.len()
                );
                false
            }
        }
    }

    fn settle_loves(&self, id: TargetId, ids: &[i64], outcome: &Outcome) -> bool {
        match self.store.settle_loves(ids, id, outcome) {
            Ok(()) => true,
            Err(e) => {
                log::warn!(
                    "scrobble: {} delivered a love but could not record it, pausing: {e}",
                    id.label()
                );
                false
            }
        }
    }

    fn classify(
        &mut self,
        id: TargetId,
        count: usize,
        result: Result<(), SubmitError>,
    ) -> (Outcome, bool) {
        match result {
            Ok(()) => {
                self.backoff.remove(&id);
                (Outcome::Sent, true)
            }
            Err(SubmitError::Unsupported) => (Outcome::Dropped("unsupported".to_string()), true),
            Err(SubmitError::Permanent(message)) => {
                let delay = self.bump(id);
                log::warn!(
                    "scrobble: {} rejected {count} item(s) permanently, dropping, next attempt in {}s: {message}",
                    id.label(),
                    delay.as_secs()
                );
                let _ = self.status.send(StatusEvent::Rejected {
                    target: id,
                    message: message.clone(),
                });
                (Outcome::Dropped(message), false)
            }
            Err(SubmitError::Auth(message)) => {
                log::warn!("scrobble: {} needs re-authorization: {message}", id.label());
                self.disabled.insert(id);
                let _ = self.status.send(StatusEvent::AuthFailed {
                    target: id,
                    message: message.clone(),
                });
                (Outcome::Deferred(message), false)
            }
            Err(SubmitError::Transient(message)) => {
                let delay = self.bump(id);
                log::warn!(
                    "scrobble: {} failed, keeping {count} item(s), retry in {}s: {message}",
                    id.label(),
                    delay.as_secs()
                );
                (Outcome::Deferred(message), false)
            }
        }
    }

    fn bump(&mut self, id: TargetId) -> Duration {
        let previous = self.backoff.get(&id).map(|(_, delay)| *delay);
        let delay = next_delay(previous);
        self.backoff.insert(id, (Instant::now() + delay, delay));
        delay
    }

    fn send_now_playing(&mut self, now_playing: &NowPlaying) {
        for pos in 0..self.targets.len() {
            let id = self.targets[pos].id();
            if self.disabled.contains(&id) {
                continue;
            }
            match self.targets[pos].now_playing(now_playing) {
                Ok(()) | Err(SubmitError::Unsupported) => {}
                Err(SubmitError::Auth(message)) => {
                    log::warn!("scrobble: {} needs re-authorization: {message}", id.label());
                    self.disabled.insert(id);
                    let _ = self.status.send(StatusEvent::AuthFailed {
                        target: id,
                        message,
                    });
                }
                Err(e) => log::debug!("scrobble: {} now playing failed: {e}", id.label()),
            }
        }
    }
}

fn love_targets(targets: &[Box<dyn ScrobbleTarget>]) -> Vec<TargetId> {
    targets
        .iter()
        .filter(|t| t.accepts_loves())
        .map(|t| t.id())
        .collect()
}

fn next_delay(previous: Option<Duration>) -> Duration {
    match previous {
        None => BACKOFF_MIN,
        Some(delay) => (delay * 2).min(BACKOFF_MAX),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};

    use super::*;
    use crate::store::StoreResult;

    struct Fake {
        id: TargetId,
        outcome: Mutex<Vec<Result<(), SubmitError>>>,
        calls: Arc<AtomicUsize>,
    }

    impl Fake {
        fn new(id: TargetId, outcome: Vec<Result<(), SubmitError>>) -> Self {
            Self {
                id,
                outcome: Mutex::new(outcome),
                calls: Arc::new(AtomicUsize::new(0)),
            }
        }

        fn counted(
            id: TargetId,
            outcome: Vec<Result<(), SubmitError>>,
        ) -> (Self, Arc<AtomicUsize>) {
            let fake = Self::new(id, outcome);
            let calls = fake.calls.clone();
            (fake, calls)
        }

        fn next(&self) -> Result<(), SubmitError> {
            self.calls.fetch_add(1, Ordering::Relaxed);
            let mut outcome = self.outcome.lock().unwrap();
            if outcome.is_empty() {
                Ok(())
            } else {
                outcome.remove(0)
            }
        }
    }

    impl ScrobbleTarget for Fake {
        fn id(&self) -> TargetId {
            self.id
        }

        fn max_batch(&self) -> usize {
            50
        }

        fn now_playing(&self, _now_playing: &NowPlaying) -> Result<(), SubmitError> {
            self.next()
        }

        fn submit(&self, _items: &[Scrobble]) -> Result<(), SubmitError> {
            self.next()
        }

        fn love(
            &self,
            _artist: &str,
            _title: &str,
            _love: bool,
            _at: u64,
        ) -> Result<(), SubmitError> {
            self.next()
        }
    }

    #[derive(Default)]
    struct Rows {
        next_id: i64,
        plays: Vec<(i64, Scrobble)>,
        loves: Vec<(i64, Love)>,
        play_deliveries: Vec<(i64, TargetId, i64)>,
        love_deliveries: Vec<(i64, TargetId, i64)>,
        errors: Vec<(TargetId, String)>,
    }

    #[derive(Default)]
    struct FakeStore {
        rows: Mutex<Rows>,
        settle_fails: AtomicUsize,
    }

    const PENDING: i64 = 0;
    const SENT: i64 = 1;
    const DROPPED: i64 = 2;

    impl FakeStore {
        fn pending_ids(&self, target: TargetId) -> Vec<i64> {
            let rows = self.rows.lock().unwrap();
            rows.play_deliveries
                .iter()
                .filter(|(_, t, state)| *t == target && *state == PENDING)
                .map(|(id, _, _)| *id)
                .collect()
        }

        fn pending_love_ids(&self, target: TargetId) -> Vec<i64> {
            let rows = self.rows.lock().unwrap();
            rows.love_deliveries
                .iter()
                .filter(|(_, t, state)| *t == target && *state == PENDING)
                .map(|(id, _, _)| *id)
                .collect()
        }

        fn love_state(&self, target: TargetId) -> Option<i64> {
            let rows = self.rows.lock().unwrap();
            rows.love_deliveries
                .iter()
                .find(|(_, t, _)| *t == target)
                .map(|(_, _, state)| *state)
        }

        fn play_count(&self) -> usize {
            self.rows.lock().unwrap().plays.len()
        }

        fn delivery_count(&self) -> usize {
            self.rows.lock().unwrap().play_deliveries.len()
        }

        fn errors(&self) -> Vec<(TargetId, String)> {
            self.rows.lock().unwrap().errors.clone()
        }

        fn settle(
            deliveries: &mut [(i64, TargetId, i64)],
            errors: &mut Vec<(TargetId, String)>,
            ids: &[i64],
            target: TargetId,
            outcome: &Outcome,
        ) {
            for (id, t, state) in deliveries.iter_mut() {
                if *t != target || !ids.contains(id) {
                    continue;
                }
                match outcome {
                    Outcome::Sent => *state = SENT,
                    Outcome::Dropped(msg) => {
                        *state = DROPPED;
                        errors.push((target, msg.clone()));
                    }
                    Outcome::Deferred(msg) => errors.push((target, msg.clone())),
                }
            }
        }
    }

    impl ScrobbleStore for FakeStore {
        fn record_play(&self, play: &Play, targets: &[TargetId]) -> StoreResult<i64> {
            let mut rows = self.rows.lock().unwrap();
            rows.next_id += 1;
            let id = rows.next_id;
            rows.plays.push((id, play.scrobble.clone()));
            if play.qualified {
                for target in targets {
                    rows.play_deliveries.push((id, *target, PENDING));
                }
            }
            Ok(id)
        }

        fn record_love(&self, love: &Love, targets: &[TargetId]) -> StoreResult<i64> {
            let mut rows = self.rows.lock().unwrap();
            rows.next_id += 1;
            let id = rows.next_id;
            rows.loves.push((id, love.clone()));
            for target in targets {
                rows.love_deliveries.push((id, *target, PENDING));
            }
            Ok(id)
        }

        fn pending_scrobbles(
            &self,
            target: TargetId,
            max: usize,
        ) -> StoreResult<Vec<(i64, Scrobble)>> {
            let rows = self.rows.lock().unwrap();
            let mut ids: Vec<i64> = rows
                .play_deliveries
                .iter()
                .filter(|(_, t, state)| *t == target && *state == PENDING)
                .map(|(id, _, _)| *id)
                .collect();
            ids.sort_unstable();
            Ok(ids
                .into_iter()
                .take(max)
                .filter_map(|id| {
                    rows.plays
                        .iter()
                        .find(|(play_id, _)| *play_id == id)
                        .map(|(_, s)| (id, s.clone()))
                })
                .collect())
        }

        fn pending_loves(&self, target: TargetId, max: usize) -> StoreResult<Vec<(i64, Love)>> {
            let rows = self.rows.lock().unwrap();
            let mut ids: Vec<i64> = rows
                .love_deliveries
                .iter()
                .filter(|(_, t, state)| *t == target && *state == PENDING)
                .map(|(id, _, _)| *id)
                .collect();
            ids.sort_unstable();
            Ok(ids
                .into_iter()
                .take(max)
                .filter_map(|id| {
                    rows.loves
                        .iter()
                        .find(|(love_id, _)| *love_id == id)
                        .map(|(_, l)| (id, l.clone()))
                })
                .collect())
        }

        fn settle_scrobbles(
            &self,
            ids: &[i64],
            target: TargetId,
            outcome: &Outcome,
        ) -> StoreResult<()> {
            if self.settle_fails.load(Ordering::Relaxed) > 0 {
                self.settle_fails.fetch_sub(1, Ordering::Relaxed);
                return Err(crate::StoreError::Backend("disk full".to_string()));
            }
            let rows = &mut *self.rows.lock().unwrap();
            FakeStore::settle(
                &mut rows.play_deliveries,
                &mut rows.errors,
                ids,
                target,
                outcome,
            );
            Ok(())
        }

        fn settle_loves(
            &self,
            ids: &[i64],
            target: TargetId,
            outcome: &Outcome,
        ) -> StoreResult<()> {
            let rows = &mut *self.rows.lock().unwrap();
            FakeStore::settle(
                &mut rows.love_deliveries,
                &mut rows.errors,
                ids,
                target,
                outcome,
            );
            Ok(())
        }

        fn pending_count(&self, targets: &[TargetId]) -> StoreResult<usize> {
            let rows = self.rows.lock().unwrap();
            let mut ids: Vec<(i64, bool)> = rows
                .play_deliveries
                .iter()
                .filter(|(_, t, state)| targets.contains(t) && *state == PENDING)
                .map(|(id, _, _)| (*id, false))
                .collect();
            ids.extend(
                rows.love_deliveries
                    .iter()
                    .filter(|(_, t, state)| targets.contains(t) && *state == PENDING)
                    .map(|(id, _, _)| (*id, true)),
            );
            ids.sort_unstable();
            ids.dedup();
            Ok(ids.len())
        }

        fn trim(&self, cap: usize) -> StoreResult<()> {
            let rows = &mut *self.rows.lock().unwrap();
            let mut items: Vec<i64> = rows
                .play_deliveries
                .iter()
                .chain(rows.love_deliveries.iter())
                .filter(|(_, _, state)| *state == PENDING)
                .map(|(id, _, _)| *id)
                .collect();
            items.sort_unstable();
            items.dedup();
            if items.len() <= cap {
                return Ok(());
            }
            let doomed: Vec<i64> = items[..items.len() - cap].to_vec();
            for (id, _, state) in rows
                .play_deliveries
                .iter_mut()
                .chain(rows.love_deliveries.iter_mut())
            {
                if *state == PENDING && doomed.contains(id) {
                    *state = DROPPED;
                }
            }
            Ok(())
        }
    }

    fn scrobble_at(timestamp: u64) -> Scrobble {
        Scrobble {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: None,
            album_artist: None,
            track_number: None,
            duration_secs: Some(180),
            timestamp,
        }
    }

    fn play_at(timestamp: u64) -> Play {
        Play {
            track_id: None,
            scrobble: scrobble_at(timestamp),
            played_secs: 120,
            qualified: true,
        }
    }

    fn a_love() -> Love {
        Love {
            track_id: None,
            artist: "A".to_string(),
            title: "T".to_string(),
            loved: true,
            at: 1_700_000_000,
        }
    }

    fn worker(
        targets: Vec<Box<dyn ScrobbleTarget>>,
    ) -> (Worker, Arc<FakeStore>, Receiver<StatusEvent>) {
        let (tx, rx) = flume::unbounded();
        let ids: Vec<TargetId> = targets.iter().map(|t| t.id()).collect();
        let store = Arc::new(FakeStore::default());
        (
            Worker {
                targets,
                target_ids: Arc::new(Mutex::new(ids)),
                store: store.clone(),
                backoff: HashMap::new(),
                disabled: HashSet::new(),
                status: tx,
            },
            store,
            rx,
        )
    }

    fn push_play(worker: &Worker, store: &FakeStore, timestamp: u64) {
        let targets = worker.active_targets();
        store.record_play(&play_at(timestamp), &targets).unwrap();
    }

    #[test]
    fn a_failing_target_does_not_hold_back_a_healthy_one() {
        let (mut worker, store, _rx) = worker(vec![
            Box::new(Fake::new(TargetId::Lastfm, vec![Ok(())])),
            Box::new(Fake::new(
                TargetId::ListenBrainz,
                vec![Err(SubmitError::Transient("offline".to_string()))],
            )),
        ]);
        push_play(&worker, &store, 1);

        worker.flush();

        assert!(store.pending_ids(TargetId::Lastfm).is_empty());
        assert_eq!(store.pending_ids(TargetId::ListenBrainz).len(), 1);
        assert!(worker.backoff.contains_key(&TargetId::ListenBrainz));
    }

    #[test]
    fn a_broken_first_target_still_lets_the_second_one_through() {
        let (mut worker, store, _rx) = worker(vec![
            Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Auth("bad session".to_string()))],
            )),
            Box::new(Fake::new(TargetId::ListenBrainz, vec![Ok(())])),
        ]);
        push_play(&worker, &store, 1);

        worker.flush();

        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 1);
        assert!(store.pending_ids(TargetId::ListenBrainz).is_empty());
    }

    #[test]
    fn a_permanent_rejection_drops_the_batch_and_stops_the_run() {
        let (mut worker, store, rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Permanent("bad params".to_string()))],
        ))]);
        for n in 0..60 {
            push_play(&worker, &store, n);
        }

        worker.flush();

        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 10);
        assert!(worker.backoff.contains_key(&TargetId::Lastfm));
        let events: Vec<StatusEvent> = rx.try_iter().collect();
        assert!(
            events.contains(&StatusEvent::Rejected {
                target: TargetId::Lastfm,
                message: "bad params".to_string(),
            }),
            "a silent drop must still reach the user, got {events:?}"
        );
    }

    #[test]
    fn a_dropped_batch_keeps_its_history_and_records_why() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Permanent("bad params".to_string()))],
        ))]);
        push_play(&worker, &store, 1);

        worker.flush();

        assert_eq!(store.play_count(), 1);
        assert!(store.pending_ids(TargetId::Lastfm).is_empty());
        assert_eq!(
            store.errors(),
            vec![(TargetId::Lastfm, "bad params".to_string())]
        );
    }

    #[test]
    fn a_transient_failure_records_the_error_but_keeps_the_item() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(
            TargetId::ListenBrainz,
            vec![Err(SubmitError::Transient("unverified email".to_string()))],
        ))]);
        push_play(&worker, &store, 1);

        worker.flush();

        assert_eq!(store.pending_ids(TargetId::ListenBrainz).len(), 1);
        assert_eq!(
            store.errors(),
            vec![(TargetId::ListenBrainz, "unverified email".to_string())]
        );
    }

    #[test]
    fn reconfiguring_lets_a_backed_off_target_retry_at_once() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Transient("offline".to_string())), Ok(())],
        ))]);
        push_play(&worker, &store, 1);
        worker.flush();
        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 1);

        worker.disabled.clear();
        worker.backoff.clear();
        worker.flush();

        assert!(store.pending_ids(TargetId::Lastfm).is_empty());
    }

    #[test]
    fn an_auth_failure_disables_the_target_and_keeps_the_queue() {
        let (mut worker, store, rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Auth("bad session".to_string()))],
        ))]);
        push_play(&worker, &store, 1);

        worker.flush();

        assert!(worker.disabled.contains(&TargetId::Lastfm));
        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 1);
        assert_eq!(worker.next_wakeup(), None);
        let events: Vec<StatusEvent> = rx.try_iter().collect();
        assert!(events.contains(&StatusEvent::AuthFailed {
            target: TargetId::Lastfm,
            message: "bad session".to_string(),
        }));
    }

    #[test]
    fn a_settle_failure_stops_the_run_instead_of_resubmitting() {
        let (target, calls) = Fake::counted(TargetId::Lastfm, vec![]);
        let (mut worker, store, _rx) = worker(vec![Box::new(target)]);
        store.settle_fails.store(5, Ordering::Relaxed);
        for n in 0..300 {
            push_play(&worker, &store, n);
        }

        worker.flush();

        assert_eq!(
            calls.load(Ordering::Relaxed),
            1,
            "a batch that could not be marked sent must never be submitted again in the same run"
        );
        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 300);
        assert!(
            worker.backoff.contains_key(&TargetId::Lastfm),
            "the target must back off so the retry is paced, not every 5s"
        );
    }

    #[test]
    fn an_unsupported_love_is_dropped_for_that_target_only() {
        let (mut worker, store, _rx) = worker(vec![
            Box::new(Fake::new(TargetId::Lastfm, vec![Ok(())])),
            Box::new(Fake::new(
                TargetId::ListenBrainz,
                vec![Err(SubmitError::Unsupported)],
            )),
        ]);
        let targets = worker.active_targets();
        store.record_love(&a_love(), &targets).unwrap();

        worker.flush();

        assert_eq!(
            worker.pending_for(&[TargetId::Lastfm, TargetId::ListenBrainz]),
            0
        );
        assert!(store.pending_love_ids(TargetId::Lastfm).is_empty());
        assert_eq!(
            store.love_state(TargetId::Lastfm),
            Some(SENT),
            "last.fm really accepted it"
        );
        assert_eq!(
            store.love_state(TargetId::ListenBrainz),
            Some(DROPPED),
            "listenbrainz cannot love, and the row must say so rather than claim delivery"
        );
        assert!(worker.backoff.is_empty());
    }

    #[test]
    fn a_run_is_capped_and_resumes_shortly_after() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(TargetId::Lastfm, vec![]))]);
        for n in 0..800 {
            push_play(&worker, &store, n);
        }

        worker.flush();
        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 100);
        assert_eq!(worker.next_wakeup(), Some(RESUME_SOON));

        worker.flush();
        assert!(store.pending_ids(TargetId::Lastfm).is_empty());
        assert_eq!(worker.next_wakeup(), None);
    }

    #[test]
    fn items_of_an_unconfigured_target_never_wake_the_worker() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(TargetId::Lastfm, vec![]))]);
        store
            .record_play(&play_at(1), &[TargetId::Librefm])
            .unwrap();

        worker.flush();

        assert_eq!(
            store.delivery_count(),
            1,
            "the debt survives so a re-login still delivers it"
        );
        assert_eq!(worker.pending_for(&worker.active_targets()), 0);
        assert_eq!(
            worker.next_wakeup(),
            None,
            "but it must never keep the worker spinning"
        );
    }

    #[test]
    fn backoff_doubles_and_is_capped() {
        assert_eq!(next_delay(None), BACKOFF_MIN);
        assert_eq!(next_delay(Some(BACKOFF_MIN)), BACKOFF_MIN * 2);
        assert_eq!(next_delay(Some(BACKOFF_MAX)), BACKOFF_MAX);
        assert_eq!(
            next_delay(Some(BACKOFF_MAX / 2 + Duration::from_secs(1))),
            BACKOFF_MAX
        );
    }

    #[test]
    fn backoff_resets_after_a_success() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Transient("offline".to_string())), Ok(())],
        ))]);
        push_play(&worker, &store, 1);
        worker.flush();
        assert!(worker.backoff.contains_key(&TargetId::Lastfm));

        worker.backoff.clear();
        worker.flush();

        assert!(worker.backoff.is_empty());
        assert!(store.pending_ids(TargetId::Lastfm).is_empty());
    }

    #[test]
    fn a_backed_off_target_is_skipped_until_its_deadline() {
        let (mut worker, store, _rx) = worker(vec![Box::new(Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Transient("offline".to_string()))],
        ))]);
        push_play(&worker, &store, 1);
        worker.flush();
        let first = worker.backoff.get(&TargetId::Lastfm).map(|(at, _)| *at);

        worker.flush();

        assert_eq!(
            worker.backoff.get(&TargetId::Lastfm).map(|(at, _)| *at),
            first
        );
        assert!(worker.next_wakeup().is_some_and(|d| d <= BACKOFF_MIN));
    }

    #[test]
    fn persist_reaches_the_store_before_returning() {
        let (status_tx, _status_rx) = flume::unbounded();
        let store = Arc::new(FakeStore::default());
        let handle = ScrobbleHandle::spawn(
            store.clone(),
            vec![Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Transient("offline".to_string()))],
            ))],
            status_tx,
        );

        handle.persist(play_at(100));

        assert_eq!(store.play_count(), 1);
        assert_eq!(store.pending_ids(TargetId::Lastfm).len(), 1);
    }

    #[test]
    fn history_is_kept_even_with_no_target_configured() {
        let (status_tx, _status_rx) = flume::unbounded();
        let store = Arc::new(FakeStore::default());
        let handle = ScrobbleHandle::spawn(store.clone(), Vec::new(), status_tx);

        handle.persist(play_at(100));

        assert_eq!(store.play_count(), 1);
        assert_eq!(store.delivery_count(), 0);
    }

    #[test]
    fn an_unqualified_play_is_never_queued_for_delivery() {
        let (status_tx, _status_rx) = flume::unbounded();
        let store = Arc::new(FakeStore::default());
        let handle = ScrobbleHandle::spawn(
            store.clone(),
            vec![Box::new(Fake::new(TargetId::Lastfm, vec![]))],
            status_tx,
        );

        let mut play = play_at(100);
        play.qualified = false;
        play.played_secs = 20;
        handle.persist(play);

        assert_eq!(store.play_count(), 1);
        assert_eq!(store.delivery_count(), 0);
    }
}
