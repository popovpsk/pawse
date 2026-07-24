use std::path::PathBuf;

use crate::Scrobble;

pub struct ScrobbleQueue {
    path: PathBuf,
    items: Vec<Scrobble>,
    cap: usize,
}

impl ScrobbleQueue {
    pub fn load(path: PathBuf, cap: usize) -> Self {
        let items = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str(&s).ok())
            .unwrap_or_default();
        Self { path, items, cap }
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn push(&mut self, scrobble: Scrobble) {
        self.items.push(scrobble);
        let len = self.items.len();
        if len > self.cap {
            self.items.drain(0..len - self.cap);
        }
        self.persist();
    }

    pub fn peek_batch(&self, n: usize) -> Vec<Scrobble> {
        self.items[..self.items.len().min(n)].to_vec()
    }

    pub fn drop_front(&mut self, n: usize) {
        let n = n.min(self.items.len());
        self.items.drain(0..n);
        self.persist();
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let json = match serde_json::to_string(&self.items) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("scrobble: failed to serialize queue: {e}");
                return;
            }
        };
        let tmp = self.path.with_extension("json.tmp");
        if let Err(e) = std::fs::write(&tmp, &json).and_then(|_| std::fs::rename(&tmp, &self.path))
        {
            log::warn!("scrobble: failed to persist queue: {e}");
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scrobble(ts: u64) -> Scrobble {
        Scrobble {
            artist: "Artist".to_string(),
            title: format!("Track {ts}"),
            album: None,
            duration_secs: Some(180),
            timestamp: ts,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        let unique = format!(
            "pawse-scrobble-test-{tag}-{}-{:?}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );
        std::env::temp_dir().join(unique)
    }

    #[test]
    fn load_missing_file_is_empty() {
        let queue = ScrobbleQueue::load(temp_path("missing"), 10);
        assert!(queue.is_empty());
    }

    #[test]
    fn push_persists_and_reloads() {
        let path = temp_path("roundtrip");
        {
            let mut queue = ScrobbleQueue::load(path.clone(), 10);
            queue.push(scrobble(1));
            queue.push(scrobble(2));
        }
        let queue = ScrobbleQueue::load(path.clone(), 10);
        assert_eq!(queue.len(), 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn cap_drops_oldest() {
        let path = temp_path("cap");
        let mut queue = ScrobbleQueue::load(path.clone(), 2);
        queue.push(scrobble(1));
        queue.push(scrobble(2));
        queue.push(scrobble(3));
        assert_eq!(queue.len(), 2);
        assert_eq!(queue.peek_batch(1)[0].timestamp, 2);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn drop_front_removes_sent_items() {
        let path = temp_path("drop");
        let mut queue = ScrobbleQueue::load(path.clone(), 10);
        queue.push(scrobble(1));
        queue.push(scrobble(2));
        queue.push(scrobble(3));
        let batch = queue.peek_batch(2);
        assert_eq!(batch.len(), 2);
        queue.drop_front(2);
        assert_eq!(queue.len(), 1);
        assert_eq!(queue.peek_batch(10)[0].timestamp, 3);
        let _ = std::fs::remove_file(&path);
    }
}
