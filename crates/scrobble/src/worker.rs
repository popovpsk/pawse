use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::{Arc, Mutex, MutexGuard};
use std::thread;
use std::time::{Duration, Instant};

use flume::{Receiver, RecvTimeoutError, Sender};

use crate::queue::{Event, PendingStore};
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
    Pending(usize),
}

enum Msg {
    Configure(Vec<Box<dyn ScrobbleTarget>>),
    NowPlaying(NowPlaying),
    Flush,
}

#[derive(Clone)]
pub struct ScrobbleHandle {
    tx: Sender<Msg>,
    queue: Arc<Mutex<PendingStore>>,
    target_ids: Arc<Mutex<Vec<TargetId>>>,
}

impl ScrobbleHandle {
    pub fn spawn(
        queue_path: PathBuf,
        targets: Vec<Box<dyn ScrobbleTarget>>,
        status: Sender<StatusEvent>,
    ) -> Self {
        let queue = Arc::new(Mutex::new(PendingStore::load(queue_path, CAP)));
        let target_ids = Arc::new(Mutex::new(
            targets.iter().map(|t| t.id()).collect::<Vec<_>>(),
        ));
        let (tx, rx) = flume::unbounded();
        let spawned = {
            let queue = queue.clone();
            let target_ids = target_ids.clone();
            thread::Builder::new()
                .name("scrobble".to_string())
                .spawn(move || {
                    let mut worker = Worker {
                        targets,
                        target_ids,
                        queue,
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
            queue,
            target_ids,
        }
    }

    pub fn configure(&self, targets: Vec<Box<dyn ScrobbleTarget>>) {
        let _ = self.tx.send(Msg::Configure(targets));
    }

    pub fn now_playing(&self, now_playing: NowPlaying) {
        let _ = self.tx.send(Msg::NowPlaying(now_playing));
    }

    pub fn scrobble(&self, scrobble: Scrobble) {
        self.persist(scrobble);
        let _ = self.tx.send(Msg::Flush);
    }

    pub fn persist(&self, scrobble: Scrobble) {
        self.enqueue(Event::Scrobble(scrobble));
    }

    pub fn love(&self, artist: String, title: String, love: bool) {
        self.enqueue(Event::Love {
            artist,
            title,
            love,
        });
        let _ = self.tx.send(Msg::Flush);
    }

    fn enqueue(&self, event: Event) {
        let targets = self.target_ids.lock().unwrap().clone();
        if targets.is_empty() {
            log::debug!("scrobble: no target configured, dropping {event:?}");
            return;
        }
        self.queue.lock().unwrap().push(event, targets);
    }

    pub fn flush(&self) {
        let _ = self.tx.send(Msg::Flush);
    }
}

struct Worker {
    targets: Vec<Box<dyn ScrobbleTarget>>,
    target_ids: Arc<Mutex<Vec<TargetId>>>,
    queue: Arc<Mutex<PendingStore>>,
    backoff: HashMap<TargetId, (Instant, Duration)>,
    disabled: HashSet<TargetId>,
    status: Sender<StatusEvent>,
}

impl Worker {
    fn queue(&self) -> MutexGuard<'_, PendingStore> {
        self.queue.lock().unwrap()
    }

    fn run(&mut self, rx: Receiver<Msg>) {
        {
            let queue = self.queue();
            if !queue.is_empty() {
                log::info!(
                    "scrobble: {} pending item(s) from a previous session",
                    queue.len()
                );
            }
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

    #[cfg(test)]
    fn push(&mut self, event: Event) {
        let targets = self.active_targets();
        self.queue().push(event, targets);
    }

    fn active_targets(&self) -> Vec<TargetId> {
        self.targets.iter().map(|t| t.id()).collect()
    }

    fn next_wakeup(&self) -> Option<Duration> {
        let now = Instant::now();
        let queue = self.queue();
        self.targets
            .iter()
            .map(|t| t.id())
            .filter(|id| !self.disabled.contains(id) && queue.len_for(&[*id]) > 0)
            .map(|id| match self.backoff.get(&id) {
                Some((at, _)) => at.saturating_duration_since(now),
                None => RESUME_SOON,
            })
            .min()
    }

    fn flush(&mut self) {
        let now = Instant::now();
        for id in self.active_targets() {
            if self.disabled.contains(&id) || self.queue().len_for(&[id]) == 0 {
                continue;
            }
            if self.backoff.get(&id).is_some_and(|(at, _)| *at > now) {
                continue;
            }
            self.flush_target(id);
        }
        let active = self.active_targets();
        let pending = self.queue().len_for(&active);
        let _ = self.status.send(StatusEvent::Pending(pending));
    }

    fn flush_target(&mut self, id: TargetId) {
        let Some(pos) = self.targets.iter().position(|t| t.id() == id) else {
            return;
        };
        let max_batch = self.targets[pos].max_batch().max(1);
        let mut sent = 0usize;

        let loves = self.queue().loves(id, RUN_LIMIT);
        for (item_id, artist, title, love) in loves {
            let result = self.targets[pos].love(&artist, &title, love);
            if !self.apply(id, &[item_id], result) {
                return;
            }
            sent += 1;
        }

        while sent < RUN_LIMIT {
            let batch = self.queue().scrobbles(id, max_batch.min(RUN_LIMIT - sent));
            if batch.is_empty() {
                break;
            }
            let item_ids: Vec<u64> = batch.iter().map(|(item_id, _)| *item_id).collect();
            let items: Vec<Scrobble> = batch.into_iter().map(|(_, s)| s).collect();
            let result = self.targets[pos].submit(&items);
            if !self.apply(id, &item_ids, result) {
                return;
            }
            sent += items.len();
        }
    }

    fn apply(&mut self, id: TargetId, item_ids: &[u64], result: Result<(), SubmitError>) -> bool {
        match result {
            Ok(()) => {
                self.queue().resolve(item_ids, id);
                self.backoff.remove(&id);
                true
            }
            Err(SubmitError::Unsupported) => {
                self.queue().resolve(item_ids, id);
                true
            }
            Err(SubmitError::Permanent(message)) => {
                let delay = self.bump(id);
                log::warn!(
                    "scrobble: {} rejected {} item(s) permanently, dropping, next attempt in {}s: {message}",
                    id.label(),
                    item_ids.len(),
                    delay.as_secs()
                );
                self.queue().resolve(item_ids, id);
                let _ = self.status.send(StatusEvent::Rejected {
                    target: id,
                    message,
                });
                false
            }
            Err(SubmitError::Auth(message)) => {
                log::warn!("scrobble: {} needs re-authorization: {message}", id.label());
                self.disabled.insert(id);
                let _ = self.status.send(StatusEvent::AuthFailed {
                    target: id,
                    message,
                });
                false
            }
            Err(SubmitError::Transient(message)) => {
                let delay = self.bump(id);
                log::warn!(
                    "scrobble: {} failed, keeping {} item(s), retry in {}s: {message}",
                    id.label(),
                    item_ids.len(),
                    delay.as_secs()
                );
                false
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

fn next_delay(previous: Option<Duration>) -> Duration {
    match previous {
        None => BACKOFF_MIN,
        Some(delay) => (delay * 2).min(BACKOFF_MAX),
    }
}

#[cfg(test)]
mod tests {
    use std::sync::atomic::{AtomicUsize, Ordering};
    use std::time::{SystemTime, UNIX_EPOCH};

    use super::*;

    struct Fake {
        id: TargetId,
        outcome: Mutex<Vec<Result<(), SubmitError>>>,
        calls: AtomicUsize,
    }

    impl Fake {
        fn new(id: TargetId, outcome: Vec<Result<(), SubmitError>>) -> Self {
            Self {
                id,
                outcome: Mutex::new(outcome),
                calls: AtomicUsize::new(0),
            }
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

        fn love(&self, _artist: &str, _title: &str, _love: bool) -> Result<(), SubmitError> {
            self.next()
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pawse-worker-{tag}-{}-{}.json",
            std::process::id(),
            SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
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

    fn worker(
        path: PathBuf,
        targets: Vec<Box<dyn ScrobbleTarget>>,
    ) -> (Worker, Receiver<StatusEvent>) {
        let (tx, rx) = flume::unbounded();
        let ids: Vec<TargetId> = targets.iter().map(|t| t.id()).collect();
        (
            Worker {
                targets,
                target_ids: Arc::new(Mutex::new(ids)),
                queue: Arc::new(Mutex::new(PendingStore::load(path, CAP))),
                backoff: HashMap::new(),
                disabled: HashSet::new(),
                status: tx,
            },
            rx,
        )
    }

    #[test]
    fn a_failing_target_does_not_hold_back_a_healthy_one() {
        let path = temp_path("partial");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![
                Box::new(Fake::new(TargetId::Lastfm, vec![Ok(())])),
                Box::new(Fake::new(
                    TargetId::ListenBrainz,
                    vec![Err(SubmitError::Transient("offline".to_string()))],
                )),
            ],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();

        assert_eq!(worker.queue().len_for(&[TargetId::Lastfm]), 0);
        assert_eq!(worker.queue().len_for(&[TargetId::ListenBrainz]), 1);
        assert!(worker.backoff.contains_key(&TargetId::ListenBrainz));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_broken_first_target_still_lets_the_second_one_through() {
        let path = temp_path("order");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![
                Box::new(Fake::new(
                    TargetId::Lastfm,
                    vec![Err(SubmitError::Auth("invalid session key".to_string()))],
                )),
                Box::new(Fake::new(TargetId::ListenBrainz, vec![Ok(())])),
            ],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();

        assert_eq!(worker.queue().len_for(&[TargetId::Lastfm]), 1);
        assert_eq!(worker.queue().len_for(&[TargetId::ListenBrainz]), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_permanent_rejection_drops_the_batch_and_stops_the_run() {
        let path = temp_path("permanent");
        let (mut worker, rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Permanent("bad params".to_string()))],
            ))],
        );
        for i in 0..60 {
            worker.push(Event::Scrobble(scrobble_at(i)));
        }

        worker.flush();

        assert_eq!(worker.queue().len_for(&[TargetId::Lastfm]), 10);
        assert!(worker.backoff.contains_key(&TargetId::Lastfm));
        let events: Vec<StatusEvent> = rx.drain().collect();
        assert!(
            events.iter().any(|e| matches!(
                e,
                StatusEvent::Rejected { target: TargetId::Lastfm, message } if message == "bad params"
            )),
            "a silent drop must still reach the user: {events:?}"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn reconfiguring_lets_a_backed_off_target_retry_at_once() {
        let path = temp_path("reconfigure-backoff");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(
                TargetId::ListenBrainz,
                vec![Err(SubmitError::Transient("bad host".to_string())), Ok(())],
            ))],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();
        assert!(worker.backoff.contains_key(&TargetId::ListenBrainz));
        assert_eq!(worker.queue().len_for(&[TargetId::ListenBrainz]), 1);

        worker.disabled.clear();
        worker.backoff.clear();
        worker.flush();

        assert_eq!(
            worker.queue().len_for(&[TargetId::ListenBrainz]),
            0,
            "fixing the settings must not leave the user waiting out the old backoff"
        );
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_auth_failure_disables_the_target_and_keeps_the_queue() {
        let path = temp_path("auth");
        let (mut worker, rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Auth("invalid session key".to_string()))],
            ))],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();

        assert!(worker.disabled.contains(&TargetId::Lastfm));
        assert_eq!(worker.queue().len_for(&[TargetId::Lastfm]), 1);
        assert!(worker.next_wakeup().is_none());
        let events: Vec<StatusEvent> = rx.drain().collect();
        assert!(events.iter().any(|e| matches!(
            e,
            StatusEvent::AuthFailed {
                target: TargetId::Lastfm,
                ..
            }
        )));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unsupported_love_is_dropped_for_that_target_only() {
        let path = temp_path("unsupported");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![
                Box::new(Fake::new(
                    TargetId::ListenBrainz,
                    vec![Err(SubmitError::Unsupported)],
                )),
                Box::new(Fake::new(TargetId::Lastfm, vec![Ok(())])),
            ],
        );
        worker.push(Event::Love {
            artist: "A".to_string(),
            title: "T".to_string(),
            love: true,
        });

        worker.flush();

        assert!(worker.queue().is_empty());
        assert!(worker.backoff.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_run_is_capped_and_resumes_shortly_after() {
        let path = temp_path("run-limit");
        let (mut worker, rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(TargetId::Lastfm, Vec::new()))],
        );
        for i in 0..800 {
            worker.push(Event::Scrobble(scrobble_at(i)));
        }

        worker.flush();
        assert_eq!(worker.queue().len_for(&[TargetId::Lastfm]), 100);
        assert_eq!(worker.next_wakeup(), Some(RESUME_SOON));

        worker.flush();
        assert!(worker.queue().is_empty());
        assert!(worker.next_wakeup().is_none());

        let events: Vec<StatusEvent> = rx.drain().collect();
        assert_eq!(events.last(), Some(&StatusEvent::Pending(0)));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn items_of_an_unconfigured_target_are_not_reported_as_pending() {
        let path = temp_path("pending-count");
        let (mut worker, rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(TargetId::Lastfm, Vec::new()))],
        );
        worker
            .queue()
            .push(Event::Scrobble(scrobble_at(1)), vec![TargetId::Librefm]);

        worker.flush();

        assert_eq!(worker.queue().len(), 1);
        let events: Vec<StatusEvent> = rx.drain().collect();
        assert_eq!(events.last(), Some(&StatusEvent::Pending(0)));
        assert!(worker.next_wakeup().is_none());
        let _ = std::fs::remove_file(&path);
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
        let path = temp_path("reset");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Transient("offline".to_string())), Ok(())],
            ))],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();
        assert!(worker.backoff.contains_key(&TargetId::Lastfm));

        worker.backoff.clear();
        worker.flush();
        assert!(worker.backoff.is_empty());
        assert!(worker.queue().is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_backed_off_target_is_skipped_until_its_deadline() {
        let path = temp_path("skip");
        let (mut worker, _rx) = worker(
            path.clone(),
            vec![Box::new(Fake::new(
                TargetId::Lastfm,
                vec![Err(SubmitError::Transient("x".to_string()))],
            ))],
        );
        worker.push(Event::Scrobble(scrobble_at(1)));

        worker.flush();
        let deadline = worker.backoff.get(&TargetId::Lastfm).unwrap().0;
        worker.flush();

        assert_eq!(worker.backoff.get(&TargetId::Lastfm).unwrap().0, deadline);
        assert!(worker.next_wakeup().unwrap() <= BACKOFF_MIN);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_writes_to_disk_before_the_app_quits() {
        let path = temp_path("persist");
        let (status_tx, _status_rx) = flume::unbounded();
        let offline = Fake::new(
            TargetId::Lastfm,
            vec![Err(SubmitError::Transient("offline".to_string()))],
        );
        let handle = ScrobbleHandle::spawn(path.clone(), vec![Box::new(offline)], status_tx);
        handle.persist(scrobble_at(100));

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"version\":2"));
        assert!(contents.contains("\"timestamp\":100"));
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn persist_with_no_configured_target_is_a_no_op() {
        let path = temp_path("persist-none");
        let (status_tx, _status_rx) = flume::unbounded();
        let handle = ScrobbleHandle::spawn(path.clone(), Vec::new(), status_tx);
        handle.persist(scrobble_at(100));

        assert!(handle.queue.lock().unwrap().is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
