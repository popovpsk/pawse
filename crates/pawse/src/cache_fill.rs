use std::collections::{HashMap, HashSet};
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use gpui::{App, AppContext, Context, Entity, ParentElement, Window, div, px};
use gpui_component::{
    Disableable, WindowExt,
    button::{Button, ButtonVariants},
    dialog::{Cancel, Confirm, DialogFooter},
};
use music_library::Track;
use music_library::remote::{self, Location};

use crate::library_service::LibraryService;
use crate::localization::tr;
use crate::remote_media::RemoteMedia;
use crate::services::Services;
use crate::settings_store::SettingsStore;

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FillTarget {
    Album(i64),
    Artist(i64),
}

#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct FillPlan {
    pub files: Vec<PathBuf>,
    pub sizes: Vec<u64>,
}

impl FillPlan {
    pub fn total(&self) -> u64 {
        self.sizes.iter().sum()
    }

    pub fn fitting(&self, limit: u64) -> usize {
        fitting_prefix(&self.sizes, limit)
    }

    fn entries(&self, count: usize) -> Vec<(PathBuf, u64)> {
        self.files
            .iter()
            .cloned()
            .zip(self.sizes.iter().copied())
            .take(count)
            .collect()
    }
}

pub fn fitting_prefix(sizes: &[u64], limit: u64) -> usize {
    let mut used = 0u64;
    sizes
        .iter()
        .take_while(|size| {
            used = used.saturating_add(**size);
            used <= limit
        })
        .count()
}

pub fn missing_files<'a>(
    tracks: impl IntoIterator<Item = &'a Track>,
    media: &RemoteMedia,
) -> Vec<(i64, String, PathBuf)> {
    let mut seen = HashSet::new();
    tracks
        .into_iter()
        .filter_map(|track| match remote::location(&track.path) {
            Location::Remote(reference) => Some((reference, PathBuf::from(&track.path))),
            Location::File(_) | Location::Invalid => None,
        })
        .filter(|(_, path)| seen.insert(path.clone()))
        .filter(|(_, path)| !media.is_cached(path))
        .map(|(reference, path)| (reference.source_id, reference.key, path))
        .collect()
}

pub fn has_missing<'a>(tracks: impl IntoIterator<Item = &'a Track>, media: &RemoteMedia) -> bool {
    !missing_files(tracks, media).is_empty()
}

pub fn plan(tracks: &[Track], media: &RemoteMedia, library: &LibraryService) -> FillPlan {
    let missing = missing_files(tracks, media);
    let mut keys: HashMap<i64, Vec<String>> = HashMap::new();
    for (source_id, key, _) in &missing {
        keys.entry(*source_id).or_default().push(key.clone());
    }
    let sizes: HashMap<(i64, String), i64> = keys
        .into_iter()
        .flat_map(|(source_id, keys)| {
            library
                .remote_file_sizes(source_id, &keys)
                .into_iter()
                .map(move |(key, size)| ((source_id, key), size))
        })
        .collect();
    let mut plan = FillPlan::default();
    for (source_id, key, path) in missing {
        let size = sizes.get(&(source_id, key)).copied().unwrap_or(0).max(0) as u64;
        plan.files.push(path);
        plan.sizes.push(size);
    }
    plan
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FillProgress {
    pub done_bytes: u64,
    pub total_bytes: u64,
}

struct Job {
    progress: FillProgress,
    cancel: Arc<AtomicBool>,
}

#[derive(Default)]
pub struct CacheFill {
    jobs: HashMap<FillTarget, Job>,
    revision: u64,
}

impl CacheFill {
    pub fn progress(&self, target: FillTarget) -> Option<FillProgress> {
        self.jobs.get(&target).map(|job| job.progress)
    }

    pub fn revision(&self) -> u64 {
        self.revision
    }

    pub fn cache_changed(&mut self, cx: &mut Context<Self>) {
        self.revision += 1;
        cx.notify();
    }

    pub fn cancel(&mut self, target: FillTarget) {
        if let Some(job) = self.jobs.get(&target) {
            job.cancel.store(true, Ordering::Release);
        }
    }

    fn start(&mut self, target: FillTarget, files: Vec<(PathBuf, u64)>, cx: &mut Context<Self>) {
        if files.is_empty() || self.jobs.contains_key(&target) {
            return;
        }
        let cancel = Arc::new(AtomicBool::new(false));
        self.jobs.insert(
            target,
            Job {
                progress: FillProgress {
                    done_bytes: 0,
                    total_bytes: files.iter().map(|(_, size)| size).sum(),
                },
                cancel: cancel.clone(),
            },
        );
        cx.notify();
        let media = cx.global::<Services>().remote_media.clone();
        cx.spawn(async move |this, cx| {
            for (path, size) in files {
                if cancel.load(Ordering::Acquire) {
                    break;
                }
                let media = media.clone();
                let stop = cancel.clone();
                let fetched = cx
                    .background_spawn(async move {
                        media.resolve(&path, &|| stop.load(Ordering::Acquire))
                    })
                    .await;
                if let Err(e) = fetched {
                    log::warn!("saving to the cache failed: {e}");
                }
                let alive = this.update(cx, |this, cx| {
                    if let Some(job) = this.jobs.get_mut(&target) {
                        job.progress.done_bytes += size;
                    }
                    cx.notify();
                });
                if alive.is_err() {
                    return;
                }
            }
            let _ = this.update(cx, |this, cx| {
                this.jobs.remove(&target);
                this.revision += 1;
                cx.notify();
            });
        })
        .detach();
    }
}

pub fn request(target: FillTarget, tracks: Vec<Track>, window: &mut Window, cx: &mut App) {
    let fill = cx.global::<Services>().cache_fill.clone();
    if fill.read(cx).progress(target).is_some() {
        fill.update(cx, |fill, _| fill.cancel(target));
        return;
    }
    let plan = {
        let services = cx.global::<Services>();
        plan(&tracks, &services.remote_media, &services.library)
    };
    let limit = cx.global::<SettingsStore>().network_cache_bytes();
    let fitting = plan.fitting(limit);
    if fitting == plan.files.len() {
        start(&fill, target, plan.entries(fitting), cx);
    } else {
        confirm(fill, target, plan, fitting, limit, window, cx);
    }
}

fn start(fill: &Entity<CacheFill>, target: FillTarget, files: Vec<(PathBuf, u64)>, cx: &mut App) {
    fill.update(cx, |fill, cx| fill.start(target, files, cx));
}

fn confirm(
    fill: Entity<CacheFill>,
    target: FillTarget,
    plan: FillPlan,
    fitting: usize,
    limit: u64,
    window: &mut Window,
    cx: &mut App,
) {
    let message = tr().cache_fill_too_big(
        &tr().size(plan.total()),
        &tr().size(limit),
        fitting,
        plan.files.len(),
    );
    let files = plan.entries(fitting);
    window.open_dialog(cx, move |dialog, _window, _cx| {
        let files = files.clone();
        let fill = fill.clone();
        dialog
            .overlay_closable(false)
            .close_button(false)
            .w(px(520.))
            .title(tr().cache_fill_too_big_title.clone())
            .child(div().child(message.clone()))
            .footer(
                DialogFooter::new()
                    .child(
                        Button::new("cache-fill-cancel")
                            .label(tr().cancel.clone())
                            .on_click(|_, window, cx| window.dispatch_action(Box::new(Cancel), cx)),
                    )
                    .child(
                        Button::new("cache-fill-ok")
                            .label(tr().cache_fill_part.clone())
                            .primary()
                            .disabled(fitting == 0)
                            .on_click(|_, window, cx| {
                                window.dispatch_action(Box::new(Confirm { secondary: false }), cx)
                            }),
                    ),
            )
            .on_ok(move |_, _, cx| {
                start(&fill, target, files.clone(), cx);
                true
            })
    });
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_the_files_that_fit_the_limit_are_taken_in_order() {
        assert_eq!(fitting_prefix(&[], 10), 0);
        assert_eq!(fitting_prefix(&[3, 3, 3], 10), 3);
        assert_eq!(fitting_prefix(&[4, 4, 4], 10), 2);
        assert_eq!(fitting_prefix(&[11, 1], 10), 0);
        assert_eq!(fitting_prefix(&[5, 5], 10), 2);
    }
}
