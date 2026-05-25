use std::collections::HashSet;
use std::path::Path;

use flume::Sender;

use crate::pipeline;
use crate::types::ScanEvent;

/// Convenience wrapper that scans a single directory with no prior cover-hash
/// knowledge. Production rescans call [`pipeline::collect_sources`] +
/// [`pipeline::run`] directly so they can take the fast path and reuse existing
/// cover thumbnails; this keeps the simple one-shot entry point for tests and
/// ad-hoc use.
pub struct DirectoryScanner;

impl DirectoryScanner {
    pub fn scan(path: impl AsRef<Path>, tx: Sender<ScanEvent>) {
        let sources = pipeline::collect_sources(&[path.as_ref().to_path_buf()]);
        pipeline::run(sources, HashSet::new(), tx);
    }
}
