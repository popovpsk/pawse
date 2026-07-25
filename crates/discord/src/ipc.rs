use std::path::PathBuf;
use std::thread;
use std::time::{Duration, Instant};

use discord_rich_presence::activity::{Activity, ActivityType, Assets, Timestamps};
use discord_rich_presence::{DiscordIpc, DiscordIpcClient};
use flume::{Receiver, RecvTimeoutError, Sender};

use crate::Presence;
use crate::art::ArtCache;

const TICK: Duration = Duration::from_secs(1);
const MIN_INTERVAL: Duration = Duration::from_secs(5);
const RECONNECT_BACKOFF: Duration = Duration::from_secs(15);

enum Msg {
    Set(Presence),
    Clear,
    Shutdown,
}

#[derive(Clone)]
pub struct DiscordHandle {
    tx: Sender<Msg>,
}

impl DiscordHandle {
    pub fn spawn(client_id: String, art_cache_path: PathBuf) -> Option<Self> {
        let (tx, rx) = flume::unbounded();
        thread::Builder::new()
            .name("discord-rpc".to_string())
            .spawn(move || {
                let mut worker = Worker {
                    client: DiscordIpcClient::new(&client_id),
                    art: ArtCache::load(art_cache_path),
                    connected: false,
                    desired: None,
                    dirty: false,
                    last_apply: Instant::now()
                        .checked_sub(MIN_INTERVAL)
                        .unwrap_or_else(Instant::now),
                    next_connect: Instant::now(),
                };
                worker.run(rx);
            })
            .map_err(|e| log::error!("discord: failed to spawn worker thread: {e}"))
            .ok()?;
        Some(Self { tx })
    }

    pub fn set(&self, presence: Presence) {
        let _ = self.tx.send(Msg::Set(presence));
    }

    pub fn clear(&self) {
        let _ = self.tx.send(Msg::Clear);
    }

    pub fn shutdown(&self) {
        let _ = self.tx.send(Msg::Shutdown);
    }
}

struct Worker {
    client: DiscordIpcClient,
    art: ArtCache,
    connected: bool,
    desired: Option<Presence>,
    dirty: bool,
    last_apply: Instant,
    next_connect: Instant,
}

impl Worker {
    fn run(&mut self, rx: Receiver<Msg>) {
        loop {
            let received = if self.dirty {
                rx.recv_timeout(TICK)
            } else {
                rx.recv().map_err(|_| RecvTimeoutError::Disconnected)
            };
            match received {
                Ok(msg) => {
                    if self.handle(msg) {
                        return;
                    }
                    while let Ok(msg) = rx.try_recv() {
                        if self.handle(msg) {
                            return;
                        }
                    }
                }
                Err(RecvTimeoutError::Timeout) => {}
                Err(RecvTimeoutError::Disconnected) => return,
            }
            self.apply(Instant::now());
        }
    }

    fn handle(&mut self, msg: Msg) -> bool {
        match msg {
            Msg::Set(p) => {
                if self.desired.as_ref() != Some(&p) {
                    self.desired = Some(p);
                    self.dirty = true;
                }
            }
            Msg::Clear => {
                if self.desired.is_some() {
                    self.desired = None;
                    self.dirty = true;
                }
            }
            Msg::Shutdown => {
                if self.connected {
                    let _ = self.client.clear_activity();
                    let _ = self.client.close();
                }
                return true;
            }
        }
        false
    }

    fn apply(&mut self, now: Instant) {
        if !self.dirty {
            return;
        }
        if !self.connected && self.desired.is_none() {
            self.dirty = false;
            return;
        }
        if !self.connected {
            if now < self.next_connect {
                return;
            }
            match self.client.connect() {
                Ok(()) => self.connected = true,
                Err(_) => {
                    self.next_connect = now + RECONNECT_BACKOFF;
                    return;
                }
            }
        }
        if now.duration_since(self.last_apply) < MIN_INTERVAL {
            return;
        }
        let result = match &self.desired {
            Some(p) => {
                let art = self.art.resolve(&p.artist, p.album.as_deref());
                let mut assets = Assets::new().small_image("play").small_text("Playing");
                if let Some(url) = art.as_deref() {
                    assets = assets.large_image(url);
                    if let Some(album) = p.album.as_deref() {
                        assets = assets.large_text(album);
                    }
                }
                let (start, end) = spans(p);
                let mut ts = Timestamps::new().start(start);
                if let Some(end) = end {
                    ts = ts.end(end);
                }
                let activity = Activity::new()
                    .activity_type(ActivityType::Listening)
                    .details(p.title.as_str())
                    .state(p.artist.as_str())
                    .assets(assets)
                    .timestamps(ts);
                self.client.set_activity(activity)
            }
            None => self.client.clear_activity(),
        };
        match result {
            Ok(()) => {
                self.last_apply = now;
                self.dirty = false;
            }
            Err(_) => {
                self.connected = false;
                self.next_connect = now + RECONNECT_BACKOFF;
            }
        }
    }
}

fn spans(p: &Presence) -> (i64, Option<i64>) {
    let dur = p.duration.as_secs() as i64;
    (p.started_at, (dur > 0).then_some(p.started_at + dur))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn end_span_present_only_with_known_duration() {
        let known = Presence {
            artist: "A".to_string(),
            title: "T".to_string(),
            album: Some("Alb".to_string()),
            duration: Duration::from_secs(180),
            started_at: 1000,
        };
        assert_eq!(spans(&known), (1000, Some(1180)));

        let unknown = Presence {
            duration: Duration::ZERO,
            ..known
        };
        assert_eq!(spans(&unknown), (1000, None));
    }
}
