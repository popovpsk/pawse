mod folder;
mod plan;

use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use cover_search::{Candidate, Finder, Source};
use gpui::{
    AnyElement, App, AppContext, Axis, Entity, InteractiveElement, IntoElement, ParentElement,
    RenderImage, SharedString, StatefulInteractiveElement, Styled, UniformListScrollHandle, div,
    px, uniform_list,
};
use gpui_component::{
    Disableable, Sizable,
    button::{Button, ButtonVariants},
    checkbox::Checkbox,
    h_flex, v_flex,
};
use ui_components::cover_thumb::cover_tile;
use ui_components::settings::{SettingField, SettingGroup, SettingItem, SettingPage};
use ui_resources::i18n::{ToolsStrings, tools_strings};

use crate::cover_art_cache::{decode_cover_tile, drop_atlas_tile};
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;

use plan::{AlbumRef, Job, Skip};

const ROW_HEIGHT: f32 = 52.;
const THUMB_SIZE: f32 = 40.;
const THUMB_RADIUS: f32 = 4.;
const LIST_MAX_ROWS: usize = 8;
const SKIPPED_HEIGHT: f32 = 200.;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
enum Phase {
    #[default]
    Idle,
    Searching,
    Applying,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub struct Layout {
    results: bool,
    skipped: bool,
}

struct Row {
    job: Job,
    candidate: Candidate,
    exact: bool,
    checked: bool,
    thumb: Option<Arc<RenderImage>>,
    title: SharedString,
    detail: SharedString,
}

impl Row {
    fn new(job: Job, candidate: Candidate, exact: bool, thumb: Option<Arc<RenderImage>>) -> Self {
        let title = SharedString::from(album_line(&job.album));
        let source = match candidate.source {
            Source::Itunes => "iTunes",
            Source::MusicBrainz => "MusicBrainz",
        };
        let detail = SharedString::from(format!(
            "{source} · {} — {}",
            candidate.artist, candidate.album
        ));
        Self {
            job,
            candidate,
            exact,
            checked: exact,
            thumb,
            title,
            detail,
        }
    }
}

#[derive(Default)]
pub struct CoversState {
    phase: Phase,
    cancel: Arc<AtomicBool>,
    done: usize,
    total: usize,
    found: Option<(usize, usize)>,
    written: Option<usize>,
    rows: Vec<Row>,
    skipped: Vec<(AlbumRef, Skip)>,
    error: Option<SharedString>,
    status: SharedString,
    skipped_text: SharedString,
    apply_label: SharedString,
    list_scroll: UniformListScrollHandle,
}

impl CoversState {
    pub fn layout(&self) -> Layout {
        Layout {
            results: !self.rows.is_empty(),
            skipped: !self.skipped.is_empty(),
        }
    }

    pub fn relabel(&mut self) {
        let s = tools_strings();
        self.skipped_text = SharedString::from(
            self.skipped
                .iter()
                .map(|(album, skip)| skipped_line(s, album, skip))
                .collect::<Vec<_>>()
                .join("\n"),
        );
        self.refresh_status();
    }

    fn push_skipped(&mut self, album: AlbumRef, skip: Skip) {
        let line = skipped_line(tools_strings(), &album, &skip);
        self.skipped_text = SharedString::from(if self.skipped_text.is_empty() {
            line
        } else {
            format!("{}\n{line}", self.skipped_text)
        });
        self.skipped.push((album, skip));
    }

    fn refresh_status(&mut self) {
        let s = tools_strings();
        self.status = SharedString::from(match (self.phase, self.found, self.written) {
            (Phase::Searching, _, _) => s.covers_searching(self.done, self.total),
            (Phase::Applying, _, _) => s.covers_applying(self.done, self.total),
            (Phase::Idle, Some(_), None) if self.total == 0 && self.skipped.is_empty() => {
                s.covers_none_missing.to_string()
            }
            (Phase::Idle, Some((found, total)), None) => s.covers_found(found, total),
            (Phase::Idle, Some((found, total)), Some(written)) => {
                s.covers_summary(found, total, written)
            }
            (Phase::Idle, None, _) => String::new(),
        });
        self.apply_label = SharedString::from(s.covers_apply(self.checked_count()));
    }

    fn checked_count(&self) -> usize {
        self.rows.iter().filter(|row| row.checked).count()
    }

    fn set_all(&mut self, checked: bool) {
        for row in &mut self.rows {
            row.checked = checked;
        }
        self.refresh_status();
    }

    fn toggle(&mut self, ix: usize) {
        if self.phase == Phase::Applying {
            return;
        }
        if let Some(row) = self.rows.get_mut(ix) {
            row.checked = !row.checked;
        }
        self.refresh_status();
    }

    fn clear(&mut self, cx: &mut App) {
        for row in self.rows.drain(..) {
            if let Some(thumb) = row.thumb {
                drop_atlas_tile(thumb, cx);
            }
        }
        self.skipped.clear();
        self.found = None;
        self.written = None;
        self.error = None;
        self.done = 0;
        self.total = 0;
    }

    fn take_row(&mut self, album_id: i64, cx: &mut App) -> Option<Row> {
        let ix = self
            .rows
            .iter()
            .position(|row| row.job.album.album_id == album_id)?;
        let mut row = self.rows.remove(ix);
        if let Some(thumb) = row.thumb.take() {
            drop_atlas_tile(thumb, cx);
        }
        Some(row)
    }
}

fn album_line(album: &AlbumRef) -> String {
    format!("{} — {}", album.artist, album.title)
}

fn skipped_line(s: &ToolsStrings, album: &AlbumRef, skip: &Skip) -> String {
    format!("{}: {}", album_line(album), skip_reason(s, skip))
}

fn skip_reason(s: &ToolsStrings, skip: &Skip) -> String {
    match skip {
        Skip::NoName => s.covers_skip_no_name.to_string(),
        Skip::SharedFolder => s.covers_skip_shared.to_string(),
        Skip::Scattered => s.covers_skip_scattered.to_string(),
        Skip::CoverExists(name) => s.covers_skip_cover_exists(name),
        Skip::ImageExists(name) => s.covers_skip_image_exists(name),
        Skip::ReadOnly => s.covers_skip_read_only.to_string(),
        Skip::NotFound => s.covers_skip_not_found.to_string(),
        Skip::SearchFailed(e) => s.covers_skip_search_failed(e),
        Skip::DownloadFailed(e) => s.covers_skip_download_failed(e),
        Skip::BadImage => s.covers_skip_bad_image.to_string(),
        Skip::WriteFailed(e) => s.covers_skip_write_failed(e),
    }
}

fn find(state: Entity<CoversState>, cx: &mut App) {
    if state.read(cx).phase != Phase::Idle {
        return;
    }
    let repo = cx.global::<Services>().library.repo();
    let cancel = Arc::new(AtomicBool::new(false));
    state.update(cx, |s, cx| {
        s.clear(cx);
        s.phase = Phase::Searching;
        s.cancel = cancel.clone();
        s.relabel();
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let planned = cx
            .background_spawn(async move {
                let albums = repo.albums()?;
                let tracks = repo.all_tracks()?;
                let mut plan = plan::plan(&albums, &tracks);
                let mut jobs = Vec::with_capacity(plan.jobs.len());
                for job in plan.jobs {
                    match folder::check(&job.folder) {
                        Ok(()) => jobs.push(job),
                        Err(skip) => plan.skipped.push((job.album, skip)),
                    }
                }
                plan.jobs = jobs;
                music_library::Result::Ok(plan)
            })
            .await;
        let jobs = match planned {
            Ok(plan) => {
                let total = plan.jobs.len();
                cx.update(|cx| {
                    state.update(cx, |s, cx| {
                        s.skipped = plan.skipped;
                        s.total = total;
                        s.relabel();
                        cx.notify();
                    })
                });
                plan.jobs
            }
            Err(e) => {
                log::warn!("Cover search: failed to read the library: {e}");
                cx.update(|cx| {
                    state.update(cx, |s, cx| {
                        s.phase = Phase::Idle;
                        s.error = Some(SharedString::from(e.to_string()));
                        s.relabel();
                        cx.notify();
                    })
                });
                return;
            }
        };

        let total = jobs.len();
        let mut found = 0;
        let mut finder = Finder::new();
        for job in jobs {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let artist = job.album.artist.clone();
            let title = job.album.title.clone();
            let (back, result) = cx
                .background_spawn(async move {
                    let result = finder.find(&artist, &title).map(|hit| {
                        hit.map(|hit| {
                            let thumb = decode_cover_tile(&hit.thumbnail);
                            (hit, thumb)
                        })
                    });
                    (finder, result)
                })
                .await;
            finder = back;
            if let Ok(Some(_)) = &result {
                found += 1;
            }
            cx.update(|cx| {
                state.update(cx, |s, cx| {
                    match result {
                        Ok(Some((hit, thumb))) => {
                            s.rows.push(Row::new(job, hit.candidate, hit.exact, thumb));
                        }
                        Ok(None) => s.push_skipped(job.album, Skip::NotFound),
                        Err(e) => s.push_skipped(job.album, Skip::SearchFailed(e.to_string())),
                    }
                    s.done += 1;
                    s.refresh_status();
                    cx.notify();
                })
            });
        }
        log::info!("Cover search: found {found} of {total}");
        cx.update(|cx| {
            state.update(cx, |s, cx| {
                s.phase = Phase::Idle;
                s.found = Some((found, s.done));
                s.relabel();
                cx.notify();
            })
        });
    })
    .detach();
}

fn apply(state: Entity<CoversState>, cx: &mut App) {
    let picks: Vec<(Job, Candidate)> = {
        let s = state.read(cx);
        if s.phase != Phase::Idle {
            return;
        }
        s.rows
            .iter()
            .filter(|row| row.checked)
            .map(|row| (row.job.clone(), row.candidate.clone()))
            .collect()
    };
    if picks.is_empty() {
        return;
    }
    let cancel = Arc::new(AtomicBool::new(false));
    state.update(cx, |s, cx| {
        s.phase = Phase::Applying;
        s.cancel = cancel.clone();
        s.done = 0;
        s.total = picks.len();
        s.written = Some(s.written.unwrap_or(0));
        s.relabel();
        cx.notify();
    });
    cx.spawn(async move |cx| {
        let finder = Arc::new(Finder::new());
        let mut written = 0;
        for (job, candidate) in picks {
            if cancel.load(Ordering::Acquire) {
                break;
            }
            let finder = finder.clone();
            let target = job.folder.clone();
            let result = cx
                .background_spawn(async move {
                    let bytes = finder
                        .download(&candidate)
                        .map_err(|e| Skip::DownloadFailed(e.to_string()))?;
                    let extension = folder::validate(&bytes)?;
                    folder::write(&target, &bytes, extension)
                })
                .await;
            match &result {
                Ok(path) => {
                    written += 1;
                    log::info!("Cover search: saved {}", path.display());
                }
                Err(skip) => log::warn!(
                    "Cover search: not saved for {}: {skip:?}",
                    job.folder.display()
                ),
            }
            cx.update(|cx| {
                state.update(cx, |s, cx| {
                    s.take_row(job.album.album_id, cx);
                    match result {
                        Ok(_) => s.written = Some(s.written.unwrap_or(0) + 1),
                        Err(skip) => s.push_skipped(job.album, skip),
                    }
                    s.done += 1;
                    s.refresh_status();
                    cx.notify();
                })
            });
        }
        cx.update(|cx| {
            state.update(cx, |s, cx| {
                s.phase = Phase::Idle;
                s.relabel();
                cx.notify();
            });
            if written > 0 {
                let folders = cx.global::<SettingsStore>().music_folders().to_vec();
                if !folders.is_empty() {
                    cx.global::<Services>()
                        .library
                        .request_rescan(folders, false, false);
                }
            }
        });
    })
    .detach();
}

fn stop(state: &Entity<CoversState>, cx: &App) {
    state.read(cx).cancel.store(true, Ordering::Release);
}

fn muted_text(text: SharedString, cx: &App) -> impl IntoElement {
    div()
        .text_sm()
        .text_color(Colors::muted_foreground(cx))
        .child(text)
}

fn actions_field(state: Entity<CoversState>, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let ui = state.read(cx);
    let busy = ui.phase != Phase::Idle;
    let status = ui.status.clone();
    let error = ui.error.clone();
    let mut buttons = h_flex().gap_2().child(
        Button::new("covers-find")
            .small()
            .primary()
            .label(s.covers_find.clone())
            .loading(ui.phase == Phase::Searching)
            .disabled(busy)
            .on_click({
                let state = state.clone();
                move |_, _, cx| find(state.clone(), cx)
            }),
    );
    if busy {
        buttons = buttons.child(
            Button::new("covers-stop")
                .small()
                .label(s.covers_stop.clone())
                .on_click(move |_, _, cx| stop(&state, cx)),
        );
    }
    let mut column = v_flex().gap_2().child(buttons);
    if !status.is_empty() {
        column = column.child(muted_text(status, cx));
    }
    if let Some(error) = error {
        column = column.child(muted_text(error, cx));
    }
    column.into_any_element()
}

fn row_element(state: &Entity<CoversState>, ix: usize, cx: &App) -> AnyElement {
    let s = tools_strings();
    let Some(row) = state.read(cx).rows.get(ix) else {
        return div().into_any_element();
    };
    let toggle_state = state.clone();
    let applying = state.read(cx).phase == Phase::Applying;
    let mut text = v_flex()
        .flex_1()
        .min_w(px(0.))
        .justify_center()
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .text_ellipsis()
                .text_sm()
                .child(row.title.clone()),
        )
        .child(
            div()
                .w_full()
                .overflow_hidden()
                .text_ellipsis()
                .text_xs()
                .text_color(Colors::muted_foreground(cx))
                .child(row.detail.clone()),
        );
    if !row.exact {
        text = text.child(
            div()
                .text_xs()
                .text_color(Colors::muted_foreground(cx))
                .child(s.covers_uncertain.clone()),
        );
    }
    h_flex()
        .id(("covers-row", ix))
        .h(px(ROW_HEIGHT))
        .gap_3()
        .items_center()
        .child(
            Checkbox::new(("covers-check", ix))
                .checked(row.checked)
                .disabled(applying)
                .on_click(move |_, _, cx| {
                    toggle_state.update(cx, |s, cx| {
                        s.toggle(ix);
                        cx.notify();
                    })
                }),
        )
        .child(cover_tile(
            row.thumb.as_ref(),
            THUMB_SIZE,
            THUMB_RADIUS,
            Colors::muted(cx),
            Colors::muted_foreground(cx),
        ))
        .child(text)
        .into_any_element()
}

fn results_field(state: Entity<CoversState>, cx: &mut App) -> AnyElement {
    let s = tools_strings();
    let ui = state.read(cx);
    let busy = ui.phase != Phase::Idle;
    let applying = ui.phase == Phase::Applying;
    let count = ui.rows.len();
    let scroll = ui.list_scroll.clone();
    let apply_label = ui.apply_label.clone();
    let nothing_checked = ui.rows.iter().all(|row| !row.checked);
    let height = ROW_HEIGHT * count.clamp(1, LIST_MAX_ROWS) as f32;
    let list_state = state.clone();
    let all_state = state.clone();
    let none_state = state.clone();
    v_flex()
        .gap_2()
        .child(
            h_flex()
                .gap_2()
                .child(
                    Button::new("covers-select-all")
                        .small()
                        .label(s.covers_select_all.clone())
                        .disabled(applying)
                        .on_click(move |_, _, cx| {
                            all_state.update(cx, |s, cx| {
                                s.set_all(true);
                                cx.notify();
                            })
                        }),
                )
                .child(
                    Button::new("covers-select-none")
                        .small()
                        .label(s.covers_select_none.clone())
                        .disabled(applying)
                        .on_click(move |_, _, cx| {
                            none_state.update(cx, |s, cx| {
                                s.set_all(false);
                                cx.notify();
                            })
                        }),
                ),
        )
        .child(
            div()
                .h(px(height))
                .rounded_md()
                .border_1()
                .border_color(Colors::border(cx))
                .px_2()
                .child(
                    uniform_list("covers-results", count, move |range, _, cx| {
                        range
                            .map(|ix| row_element(&list_state, ix, cx))
                            .collect::<Vec<_>>()
                    })
                    .track_scroll(&scroll)
                    .size_full(),
                ),
        )
        .child(
            h_flex().child(
                Button::new("covers-apply")
                    .small()
                    .primary()
                    .label(apply_label)
                    .loading(ui.phase == Phase::Applying)
                    .disabled(busy || nothing_checked)
                    .on_click(move |_, _, cx| apply(state.clone(), cx)),
            ),
        )
        .into_any_element()
}

fn skipped_field(state: Entity<CoversState>, cx: &mut App) -> AnyElement {
    let text = state.read(cx).skipped_text.clone();
    div()
        .id("covers-skipped")
        .max_h(px(SKIPPED_HEIGHT))
        .overflow_y_scroll()
        .p_2()
        .rounded_md()
        .border_1()
        .border_color(Colors::border(cx))
        .bg(Colors::muted(cx))
        .text_xs()
        .text_color(Colors::muted_foreground(cx))
        .child(text)
        .into_any_element()
}

pub fn page(state: Entity<CoversState>, layout: Layout) -> SettingPage {
    let s = tools_strings();
    let actions_state = state.clone();
    let mut group = SettingGroup::new()
        .title(s.tools_covers.clone())
        .description(s.covers_intro.clone())
        .item(
            SettingItem::new(
                s.covers_find.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    actions_field(actions_state.clone(), cx)
                }),
            )
            .layout(Axis::Vertical),
        );
    if layout.results {
        let results_state = state.clone();
        group = group.item(
            SettingItem::new(
                s.covers_results.clone(),
                SettingField::render(move |_window, cx: &mut App| {
                    results_field(results_state.clone(), cx)
                }),
            )
            .description(s.covers_results_desc.clone())
            .layout(Axis::Vertical),
        );
    }
    if layout.skipped {
        group = group.item(
            SettingItem::new(
                s.covers_skipped.clone(),
                SettingField::render(move |_window, cx: &mut App| skipped_field(state.clone(), cx)),
            )
            .layout(Axis::Vertical),
        );
    }
    SettingPage::new(s.tools_covers.clone()).group(group)
}
