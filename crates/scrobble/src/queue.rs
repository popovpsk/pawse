use std::io::Write;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::{Scrobble, TargetId};

const VERSION: u32 = 2;

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum Event {
    Scrobble(Scrobble),
    Love {
        artist: String,
        title: String,
        love: bool,
    },
}

#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct Pending {
    pub id: u64,
    pub event: Event,
    #[serde(deserialize_with = "known_targets")]
    pub targets: Vec<TargetId>,
}

fn known_targets<'de, D: serde::Deserializer<'de>>(d: D) -> Result<Vec<TargetId>, D::Error> {
    let raw = Vec::<String>::deserialize(d)?;
    Ok(raw
        .iter()
        .filter_map(|key| TargetId::from_key(key))
        .collect())
}

#[derive(Serialize, Deserialize)]
struct FileV2 {
    version: u32,
    items: Vec<Pending>,
}

pub struct PendingStore {
    path: PathBuf,
    items: Vec<Pending>,
    cap: usize,
    next_id: u64,
}

impl PendingStore {
    pub fn load(path: PathBuf, cap: usize) -> Self {
        let raw = std::fs::read_to_string(&path).unwrap_or_default();
        let mut items = parse(&raw, &path);
        items.retain(|i| !i.targets.is_empty());
        let trimmed = items.len() > cap;
        if trimmed {
            let excess = items.len() - cap;
            items.drain(0..excess);
        }
        let next_id = items.iter().map(|i| i.id + 1).max().unwrap_or(1);
        let store = Self {
            path,
            items,
            cap,
            next_id,
        };
        if trimmed {
            store.persist();
        }
        store
    }

    pub fn is_empty(&self) -> bool {
        self.items.is_empty()
    }

    pub fn len(&self) -> usize {
        self.items.len()
    }

    pub fn len_for(&self, targets: &[TargetId]) -> usize {
        self.items
            .iter()
            .filter(|i| i.targets.iter().any(|t| targets.contains(t)))
            .count()
    }

    pub fn push(&mut self, event: Event, targets: Vec<TargetId>) {
        if targets.is_empty() {
            return;
        }
        let id = self.next_id;
        self.next_id += 1;
        self.items.push(Pending { id, event, targets });
        let len = self.items.len();
        if len > self.cap {
            self.items.drain(0..len - self.cap);
        }
        self.persist();
    }

    pub fn scrobbles(&self, target: TargetId, max: usize) -> Vec<(u64, Scrobble)> {
        self.items
            .iter()
            .filter(|i| i.targets.contains(&target))
            .filter_map(|i| match &i.event {
                Event::Scrobble(s) => Some((i.id, s.clone())),
                Event::Love { .. } => None,
            })
            .take(max)
            .collect()
    }

    pub fn loves(&self, target: TargetId, max: usize) -> Vec<(u64, String, String, bool)> {
        self.items
            .iter()
            .filter(|i| i.targets.contains(&target))
            .filter_map(|i| match &i.event {
                Event::Love {
                    artist,
                    title,
                    love,
                } => Some((i.id, artist.clone(), title.clone(), *love)),
                Event::Scrobble(_) => None,
            })
            .take(max)
            .collect()
    }

    pub fn resolve(&mut self, ids: &[u64], target: TargetId) {
        if ids.is_empty() {
            return;
        }
        for item in self.items.iter_mut() {
            if ids.contains(&item.id) {
                item.targets.retain(|t| *t != target);
            }
        }
        self.items.retain(|i| !i.targets.is_empty());
        self.persist();
    }

    fn persist(&self) {
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        let file = FileV2 {
            version: VERSION,
            items: self.items.clone(),
        };
        let json = match serde_json::to_string(&file) {
            Ok(json) => json,
            Err(e) => {
                log::warn!("scrobble: failed to serialize queue: {e}");
                return;
            }
        };
        let tmp = self.path.with_extension("json.tmp");
        let written = std::fs::File::create(&tmp).and_then(|mut file| {
            file.write_all(json.as_bytes())?;
            file.sync_all()
        });
        if let Err(e) = written.and_then(|()| std::fs::rename(&tmp, &self.path)) {
            log::warn!("scrobble: failed to persist queue: {e}");
        }
    }
}

fn parse(raw: &str, path: &Path) -> Vec<Pending> {
    if raw.trim().is_empty() {
        return Vec::new();
    }
    if let Ok(file) = serde_json::from_str::<FileV2>(raw) {
        return file.items;
    }
    match serde_json::from_str::<Vec<Scrobble>>(raw) {
        Ok(legacy) => legacy
            .into_iter()
            .enumerate()
            .map(|(i, s)| Pending {
                id: i as u64 + 1,
                event: Event::Scrobble(s),
                targets: vec![TargetId::Lastfm],
            })
            .collect(),
        Err(e) => {
            let kept = path.with_extension("json.unreadable");
            let moved = std::fs::rename(path, &kept).is_ok();
            log::warn!(
                "scrobble: unreadable queue file, starting empty ({e}); {}",
                if moved {
                    format!("kept a copy at {}", kept.display())
                } else {
                    "the old file could not be kept".to_string()
                }
            );
            Vec::new()
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
            album_artist: None,
            track_number: None,
            duration_secs: Some(180),
            timestamp: ts,
        }
    }

    fn temp_path(tag: &str) -> PathBuf {
        std::env::temp_dir().join(format!(
            "pawse-scrobble-{tag}-{}-{}.json",
            std::process::id(),
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ))
    }

    #[test]
    fn legacy_array_migrates_to_lastfm_items() {
        let path = temp_path("legacy");
        std::fs::write(
            &path,
            r#"[{"artist":"A","title":"T","album":null,"duration_secs":180,"timestamp":7}]"#,
        )
        .unwrap();

        let store = PendingStore::load(path.clone(), 100);
        assert_eq!(store.len(), 1);
        assert_eq!(store.scrobbles(TargetId::Lastfm, 10).len(), 1);
        assert_eq!(store.scrobbles(TargetId::ListenBrainz, 10).len(), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn load_applies_cap_to_an_oversized_file() {
        let path = temp_path("cap");
        let mut store = PendingStore::load(path.clone(), 100);
        for i in 0..10 {
            store.push(Event::Scrobble(scrobble(i)), vec![TargetId::Lastfm]);
        }

        let reloaded = PendingStore::load(path.clone(), 4);
        assert_eq!(reloaded.len(), 4);
        assert_eq!(reloaded.scrobbles(TargetId::Lastfm, 10)[0].1.timestamp, 6);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn resolving_one_target_keeps_the_item_for_the_others() {
        let path = temp_path("partial");
        let mut store = PendingStore::load(path.clone(), 100);
        store.push(
            Event::Scrobble(scrobble(1)),
            vec![TargetId::Lastfm, TargetId::ListenBrainz],
        );

        let ids: Vec<u64> = store
            .scrobbles(TargetId::Lastfm, 10)
            .into_iter()
            .map(|(id, _)| id)
            .collect();
        store.resolve(&ids, TargetId::Lastfm);

        assert_eq!(store.scrobbles(TargetId::Lastfm, 10).len(), 0);
        assert_eq!(store.scrobbles(TargetId::ListenBrainz, 10).len(), 1);

        store.resolve(&ids, TargetId::ListenBrainz);
        assert!(store.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn pushing_with_no_targets_keeps_the_queue_empty() {
        let path = temp_path("no-targets");
        let mut store = PendingStore::load(path.clone(), 100);
        store.push(Event::Scrobble(scrobble(1)), Vec::new());
        assert!(store.is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn len_for_counts_only_the_listed_targets() {
        let path = temp_path("len-for");
        let mut store = PendingStore::load(path.clone(), 100);
        store.push(Event::Scrobble(scrobble(1)), vec![TargetId::Lastfm]);
        store.push(Event::Scrobble(scrobble(2)), vec![TargetId::Librefm]);
        store.push(
            Event::Scrobble(scrobble(3)),
            vec![TargetId::Lastfm, TargetId::CsvLog],
        );

        assert_eq!(store.len(), 3);
        assert_eq!(store.len_for(&[TargetId::Lastfm]), 2);
        assert_eq!(store.len_for(&[TargetId::CsvLog]), 1);
        assert_eq!(store.len_for(&[]), 0);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn an_unknown_target_is_dropped_without_losing_the_rest() {
        let path = temp_path("unknown-target");
        std::fs::write(
            &path,
            r#"{"version":2,"items":[
                {"id":1,"event":{"kind":"scrobble","artist":"A","title":"T","album":null,"duration_secs":180,"timestamp":1},"targets":["lastfm","maloja"]},
                {"id":2,"event":{"kind":"scrobble","artist":"B","title":"T","album":null,"duration_secs":180,"timestamp":2},"targets":["maloja"]}
            ]}"#,
        )
        .unwrap();

        let store = PendingStore::load(path.clone(), 100);
        assert_eq!(store.len(), 1);
        assert_eq!(store.scrobbles(TargetId::Lastfm, 10).len(), 1);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn a_corrupt_file_is_kept_aside_instead_of_being_overwritten() {
        let path = temp_path("kept-aside");
        std::fs::write(&path, "{not json").unwrap();

        let mut store = PendingStore::load(path.clone(), 100);
        assert!(store.is_empty());
        let kept = path.with_extension("json.unreadable");
        assert_eq!(std::fs::read_to_string(&kept).unwrap(), "{not json");

        store.push(Event::Scrobble(scrobble(1)), vec![TargetId::Lastfm]);
        assert!(
            std::fs::read_to_string(&path)
                .unwrap()
                .contains("\"version\":2")
        );
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&kept);
    }

    #[test]
    fn a_corrupt_file_does_not_panic() {
        let path = temp_path("corrupt");
        std::fs::write(&path, "{not json").unwrap();
        assert!(PendingStore::load(path.clone(), 100).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn loves_round_trip_through_the_file() {
        let path = temp_path("loves");
        {
            let mut store = PendingStore::load(path.clone(), 100);
            store.push(
                Event::Love {
                    artist: "A".to_string(),
                    title: "T".to_string(),
                    love: true,
                },
                vec![TargetId::Lastfm],
            );
        }
        let store = PendingStore::load(path.clone(), 100);
        let loves = store.loves(TargetId::Lastfm, 10);
        assert_eq!(loves.len(), 1);
        assert_eq!(loves[0].1, "A");
        assert!(loves[0].3);
        assert_eq!(store.scrobbles(TargetId::Lastfm, 10).len(), 0);
        let _ = std::fs::remove_file(&path);
    }
}
