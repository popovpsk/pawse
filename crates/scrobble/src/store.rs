use crate::Scrobble;
use crate::target::TargetId;

#[derive(Debug, thiserror::Error)]
pub enum StoreError {
    #[error("scrobble store: {0}")]
    Backend(String),
}

pub type StoreResult<T> = std::result::Result<T, StoreError>;

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Play {
    pub track_id: Option<i64>,
    pub scrobble: Scrobble,
    pub played_secs: u64,
    pub qualified: bool,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Love {
    pub track_id: Option<i64>,
    pub artist: String,
    pub title: String,
    pub loved: bool,
    pub at: u64,
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Outcome {
    Sent,
    Dropped(String),
    Deferred(String),
}

pub trait ScrobbleStore: Send + Sync {
    fn record_play(&self, play: &Play, targets: &[TargetId]) -> StoreResult<i64>;
    fn record_love(&self, love: &Love, targets: &[TargetId]) -> StoreResult<i64>;
    fn pending_scrobbles(&self, target: TargetId, max: usize) -> StoreResult<Vec<(i64, Scrobble)>>;
    fn pending_loves(&self, target: TargetId, max: usize) -> StoreResult<Vec<(i64, Love)>>;
    fn settle_scrobbles(&self, ids: &[i64], target: TargetId, outcome: &Outcome)
    -> StoreResult<()>;
    fn settle_loves(&self, ids: &[i64], target: TargetId, outcome: &Outcome) -> StoreResult<()>;
    fn pending_count(&self, targets: &[TargetId]) -> StoreResult<usize>;
    fn trim(&self, cap: usize) -> StoreResult<()>;
}
