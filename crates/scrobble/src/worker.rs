use std::path::PathBuf;
use std::thread;
use std::time::Duration;

use flume::{Receiver, Sender};

use crate::client::LastfmClient;
use crate::queue::ScrobbleQueue;
use crate::{NowPlaying, Scrobble, Session};

const BATCH: usize = 50;
const CAP: usize = 5000;

enum Msg {
    SetSession(Option<Session>),
    NowPlaying(NowPlaying),
    Scrobble(Scrobble),
    PersistSync(Scrobble, Sender<()>),
    Love {
        artist: String,
        title: String,
        love: bool,
    },
    Flush,
}

#[derive(Clone)]
pub struct ScrobbleHandle {
    tx: Sender<Msg>,
}

impl ScrobbleHandle {
    pub fn spawn(
        api_key: String,
        api_secret: String,
        queue_path: PathBuf,
        session: Option<Session>,
    ) -> Self {
        let (tx, rx) = flume::unbounded();
        let spawned = thread::Builder::new()
            .name("scrobble".to_string())
            .spawn(move || {
                let mut worker = Worker {
                    client: LastfmClient::new(api_key, api_secret),
                    queue: ScrobbleQueue::load(queue_path, CAP),
                    session,
                };
                worker.run(rx);
            });
        if let Err(e) = spawned {
            log::error!("scrobble: failed to spawn worker thread: {e}");
        }
        Self { tx }
    }

    pub fn set_session(&self, session: Option<Session>) {
        let _ = self.tx.send(Msg::SetSession(session));
    }

    pub fn now_playing(&self, now_playing: NowPlaying) {
        let _ = self.tx.send(Msg::NowPlaying(now_playing));
    }

    pub fn scrobble(&self, scrobble: Scrobble) {
        let _ = self.tx.send(Msg::Scrobble(scrobble));
    }

    pub fn persist_blocking(&self, scrobble: Scrobble, timeout: Duration) {
        let (ack_tx, ack_rx) = flume::bounded(1);
        if self.tx.send(Msg::PersistSync(scrobble, ack_tx)).is_err() {
            return;
        }
        let _ = ack_rx.recv_timeout(timeout);
    }

    pub fn love(&self, artist: String, title: String, love: bool) {
        let _ = self.tx.send(Msg::Love {
            artist,
            title,
            love,
        });
    }

    pub fn flush(&self) {
        let _ = self.tx.send(Msg::Flush);
    }
}

struct Worker {
    client: LastfmClient,
    queue: ScrobbleQueue,
    session: Option<Session>,
}

impl Worker {
    fn run(&mut self, rx: Receiver<Msg>) {
        if !self.queue.is_empty() {
            log::info!(
                "scrobble: {} pending scrobble(s) from a previous session",
                self.queue.len()
            );
        }
        self.flush();
        while let Ok(msg) = rx.recv() {
            match msg {
                Msg::SetSession(session) => {
                    self.session = session;
                    self.flush();
                }
                Msg::NowPlaying(now_playing) => self.send_now_playing(&now_playing),
                Msg::Scrobble(scrobble) => {
                    self.queue.push(scrobble);
                    self.flush();
                }
                Msg::PersistSync(scrobble, ack) => {
                    self.queue.push(scrobble);
                    let _ = ack.send(());
                }
                Msg::Love {
                    artist,
                    title,
                    love,
                } => self.send_love(&artist, &title, love),
                Msg::Flush => self.flush(),
            }
        }
    }

    fn flush(&mut self) {
        let Some(key) = self.session.as_ref().map(|s| s.key.clone()) else {
            return;
        };
        while !self.queue.is_empty() {
            let batch = self.queue.peek_batch(BATCH);
            let count = batch.len();
            match self.client.scrobble_batch(&key, &batch) {
                Ok(()) => self.queue.drop_front(count),
                Err(e) => {
                    log::warn!("scrobble: submit failed, keeping {count} queued: {e}");
                    break;
                }
            }
        }
    }

    fn send_now_playing(&self, now_playing: &NowPlaying) {
        let Some(key) = self.session.as_ref().map(|s| s.key.as_str()) else {
            return;
        };
        if let Err(e) = self.client.now_playing(key, now_playing) {
            log::debug!("scrobble: now playing failed: {e}");
        }
    }

    fn send_love(&self, artist: &str, title: &str, love: bool) {
        let Some(key) = self.session.as_ref().map(|s| s.key.as_str()) else {
            return;
        };
        if let Err(e) = self.client.love(key, artist, title, love) {
            log::warn!("scrobble: love failed: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn persist_blocking_writes_to_disk_without_session() {
        let path = std::env::temp_dir().join(format!(
            "pawse-scrobble-worker-{}-{:?}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        let _ = std::fs::remove_file(&path);

        let handle =
            ScrobbleHandle::spawn("key".to_string(), "secret".to_string(), path.clone(), None);
        handle.persist_blocking(
            Scrobble {
                artist: "A".to_string(),
                title: "T".to_string(),
                album: None,
                duration_secs: Some(180),
                timestamp: 123,
            },
            Duration::from_secs(2),
        );

        let contents = std::fs::read_to_string(&path).unwrap();
        assert!(contents.contains("\"timestamp\":123"));
        let _ = std::fs::remove_file(&path);
    }
}
