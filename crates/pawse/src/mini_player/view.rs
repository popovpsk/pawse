use std::rc::Rc;
use std::sync::Arc;
use std::time::Duration;

use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, AnyElement, App, AppContext, ClickEvent, Context, Div, Entity,
    FocusHandle, FontWeight, InteractiveElement, IntoElement, MouseButton, MouseMoveEvent,
    ParentElement, Render, RenderImage, SharedString, Stateful, StatefulInteractiveElement, Styled,
    Subscription, Task, Window, WindowControlArea, div, ease_out_quint, px, rems, svg,
};
use gpui_component::tooltip::Tooltip;
use gpui_component::{h_flex, v_flex};

use super::lyrics_line::{Around, LyricsLine};
use crate::cover_art_cache::drop_atlas_tile;
use crate::cover_mode_view::{FullCoverSource, cover_layer};
use crate::cover_skin::CoverSkin;
use crate::cover_volume::{self, CoverVolume};
use crate::keyboard_shortcuts::{
    MINI_CONTEXT, NextTrack, PlayPause, PreviousTrack, SeekBackward, SeekForward, VolumeDown,
    VolumeUp,
};
use crate::library_service::LibraryEvent;
use crate::localization::tr;
use crate::next_button::NextButton;
use crate::play_button::PlayButton;
use crate::playback_status::StatusChanged;
use crate::playlist_popup::PlaylistPopup;
use crate::prev_button::PrevButton;
use crate::queue_view::QueueView;
use crate::services::Services;
use crate::settings_store::{SettingsStore, notify_save_error};
use crate::theme_colors::Colors;
use crate::track_progress_slider::TrackProgressSlider;
use crate::volume::{VOLUME_STEP, Volume};
use crate::window_placement::{self, Slot};

const PAD: f32 = 10.;
const GAP: f32 = 12.;
const ROW_GAP: f32 = 8.;
const STRIP_RADIUS: f32 = 6.;
const COVER_LAYOUT_MIN_H: f32 = 200.;
const COMPACT_LYRICS_EXTRA: f32 = 80.;
const TRANSPORT_W: f32 = 36. * 3. + 8. * 2.;
const CONTROL_SIZE: f32 = 30.;
const CONTROL_ICON: f32 = 20.;
const SIDE_GROUP_W: f32 = (CONTROL_SIZE + 4.) * 2. + cover_volume::ICON_SIZE;
const CHROME_SIZE: f32 = 26.;
const CHROME_ICON: f32 = 16.;
const VOLUME_MIN_W: f32 = 78.;
const SCRIM: f32 = 0.82;
const SHOW_DELAY: Duration = Duration::from_millis(90);
const HIDE_DELAY: Duration = Duration::from_millis(350);
const QUEUE_SLIDE: Duration = Duration::from_millis(220);
const LYRIC_SLIDE: Duration = Duration::from_millis(320);

struct Headline {
    id: i64,
    title: SharedString,
    artists: SharedString,
    cover_art_id: Option<i64>,
    path: String,
    liked: bool,
}

pub struct MiniPlayer {
    focus_handle: FocusHandle,
    play_button: Entity<PlayButton>,
    prev_button: Entity<PrevButton>,
    next_button: Entity<NextButton>,
    progress: Entity<TrackProgressSlider>,
    volume: Entity<Volume>,
    cover_volume: Entity<CoverVolume>,
    lyrics: Entity<LyricsLine>,
    lyrics_on: bool,
    queue: Option<Entity<QueueView>>,
    queue_open: bool,
    queue_closing: bool,
    queue_seq: usize,
    _queue_task: Option<Task<()>>,
    playlist_popup: Entity<PlaylistPopup>,
    track: Option<Headline>,
    thumb: Option<Arc<RenderImage>>,
    full: Option<Arc<RenderImage>>,
    full_wanted: bool,
    full_tried: Option<i64>,
    _full_task: Option<Task<()>>,
    remote_task: Option<Task<()>>,
    remote_wanted: bool,
    pointer_over: bool,
    hover_raw: bool,
    hovered: bool,
    _hover_task: Option<Task<()>>,
    on_top: bool,
    can_pin: bool,
    should_move: bool,
    _cover_skin: Entity<CoverSkin>,
    _subscriptions: Vec<Subscription>,
}

impl MiniPlayer {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let focus_handle = cx.focus_handle();
        focus_handle.focus(window, cx);
        let volume = cx.new(|cx| Volume::new(window, cx).fill_width());
        let cover_volume = cx.new(|cx| CoverVolume::new(volume.clone(), window, cx));
        let lyrics = cx.new(LyricsLine::new);
        let services = cx.global::<Services>();
        let playback_status = services.playback_status.clone();
        let library_event_bus = services.library_event_bus.clone();

        let subscriptions = vec![
            cx.subscribe(
                &playback_status,
                |this: &mut Self, status, event: &StatusChanged, cx| {
                    let track_id = status.read(cx).track_id();
                    if event.track_changed || track_id != this.track.as_ref().map(|t| t.id) {
                        this.populate(cx);
                    }
                },
            ),
            cx.subscribe(
                &library_event_bus,
                |this: &mut Self, _, event: &LibraryEvent, cx| {
                    if matches!(
                        event,
                        LibraryEvent::CatalogChanged | LibraryEvent::TagsSaved
                    ) {
                        this.populate(cx);
                    } else if let Some((ids, liked)) = event.liked_update()
                        && let Some(track) = this.track.as_mut()
                        && ids.contains(&track.id)
                    {
                        track.liked = liked;
                        cx.notify();
                    }
                },
            ),
            cx.observe_global::<SettingsStore>(|_, cx| cx.notify()),
            cx.observe(&lyrics, |_, _, cx| cx.notify()),
            cx.observe(&cover_volume, |_, _, cx| cx.notify()),
            cx.observe_window_bounds(window, |_, window, cx| {
                window_placement::remember(window, Slot::Mini, cx);
            }),
            cx.observe_window_activation(window, |this, window, cx| {
                if window.is_window_active() {
                    this.focus_handle.focus(window, cx);
                }
            }),
            cx.on_focus_lost(window, |this, window, cx| {
                this.focus_handle.focus(window, cx);
            }),
        ];

        #[cfg(target_os = "windows")]
        crate::media_bridge::bind_window(crate::media_bridge::window_hwnd(window), cx);

        let can_pin = window_level::supported(&*window);
        let on_top = can_pin && cx.global::<SettingsStore>().mini_player_on_top();
        window.on_next_frame(move |window, _| {
            let limits = super::size_limits(window);
            if let Err(err) = window_level::set_size_limits(&*window, limits, window.scale_factor())
            {
                log::debug!("mini player: no size limits: {err}");
            }
            if on_top && let Err(err) = window_level::set_always_on_top(&*window, true) {
                log::warn!("mini player: keep on top failed: {err}");
            }
        });

        let mut this = Self {
            focus_handle,
            play_button: cx.new(|cx| PlayButton::new(window, cx)),
            prev_button: cx.new(|cx| PrevButton::new(window, cx)),
            next_button: cx.new(|cx| NextButton::new(window, cx)),
            progress: cx.new(|cx| TrackProgressSlider::new(window, cx)),
            volume,
            cover_volume,
            lyrics,
            lyrics_on: super::lyrics_shown(cx),
            queue: None,
            queue_open: false,
            queue_closing: false,
            queue_seq: 0,
            _queue_task: None,
            playlist_popup: cx.new(|cx| PlaylistPopup::new(window, cx)),
            track: None,
            thumb: None,
            full: None,
            full_wanted: false,
            full_tried: None,
            _full_task: None,
            remote_task: None,
            remote_wanted: false,
            pointer_over: false,
            hover_raw: false,
            hovered: false,
            _hover_task: None,
            on_top,
            can_pin,
            should_move: false,
            _cover_skin: crate::cover_skin::shared(cx),
            _subscriptions: subscriptions,
        };
        this.populate(cx);
        this
    }

    fn populate(&mut self, cx: &mut Context<Self>) {
        let services = cx.global::<Services>();
        let current = services
            .playback_queue
            .borrow()
            .current_track()
            .map(|track| {
                (
                    track.id,
                    track.title.clone(),
                    track.cover_art_id,
                    track.path.clone(),
                    track.liked,
                )
            });
        let Some((id, title, cover_art_id, path, liked)) = current else {
            self.track = None;
            self.thumb = None;
            self.release_full(cx);
            cx.notify();
            return;
        };
        let artists = services
            .library
            .unique_track_artists(id)
            .into_iter()
            .map(|(_, name)| name)
            .collect::<Vec<_>>()
            .join(", ");
        let (cache, library) = (services.cover_art_cache.clone(), services.library.clone());
        let thumb = cache.borrow_mut().get_large(cover_art_id, &library, cx);
        if self.track.as_ref().map(|t| t.cover_art_id) != Some(cover_art_id) {
            self.release_full(cx);
        }
        self.thumb = thumb;
        self.track = Some(Headline {
            id,
            title: title.into(),
            artists: artists.into(),
            cover_art_id,
            path,
            liked,
        });
        cx.notify();
    }

    fn want_full(&mut self, wanted: bool, cx: &mut Context<Self>) {
        if self.full_wanted != wanted {
            self.full_wanted = wanted;
            if !wanted {
                self.release_full(cx);
            }
        }
        let cover = self.track.as_ref().and_then(|t| t.cover_art_id);
        if wanted && self.full.is_none() && cover.is_some() && self.full_tried != cover {
            self.load_full(cx);
        }
    }

    fn release_full(&mut self, cx: &mut Context<Self>) {
        self.full_tried = None;
        self._full_task = None;
        if let Some(old) = self.full.take() {
            drop_atlas_tile(old, cx);
        }
    }

    fn load_full(&mut self, cx: &mut Context<Self>) {
        self._full_task = None;
        let Some(track) = self.track.as_ref() else {
            return;
        };
        let Some(id) = track.cover_art_id else {
            return;
        };
        let path = track.path.clone();
        self.full_tried = Some(id);
        let source = FullCoverSource::of(id, cx);
        let is_remote = source.is_remote();
        if is_remote && self.remote_task.is_some() {
            self.remote_wanted = true;
            return;
        }
        let load = source.load(Some(path), cx);
        let task = cx.spawn(async move |this, cx| {
            let loaded = load.await;
            let _ = this.update(cx, |this, cx| {
                if is_remote {
                    this.remote_task = None;
                } else {
                    this._full_task = None;
                }
                let current = this.track.as_ref().and_then(|t| t.cover_art_id) == Some(id);
                if let Some((image, _)) = loaded
                    && current
                    && this.full_wanted
                    && this.full.is_none()
                {
                    this.full = Some(image);
                    cx.notify();
                }
                if is_remote
                    && std::mem::take(&mut this.remote_wanted)
                    && this.full_wanted
                    && this.full.is_none()
                {
                    this.load_full(cx);
                }
            });
        });
        if is_remote {
            self.remote_task = Some(task);
        } else {
            self._full_task = Some(task);
        }
    }

    fn set_pointer_over(&mut self, over: bool, cx: &mut Context<Self>) {
        if self.pointer_over != over {
            self.pointer_over = over;
            cx.notify();
        }
    }

    fn track_hover(&mut self, raw: bool, cx: &mut Context<Self>) {
        if raw == self.hover_raw {
            return;
        }
        self.hover_raw = raw;
        let delay = if raw { SHOW_DELAY } else { HIDE_DELAY };
        self._hover_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(delay).await;
            let _ = this.update(cx, |this, cx| {
                if this.hover_raw == raw && this.hovered != raw {
                    this.hovered = raw;
                    cx.notify();
                }
            });
        }));
    }

    fn toggle_lyrics(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        let on = !self.lyrics_on;
        self.lyrics_on = on;
        super::set_lyrics_shown(on, cx);
        self.lyrics
            .update(cx, |lyrics, cx| lyrics.set_enabled(on, cx));
        cx.notify();
    }

    fn toggle_queue(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        if self.queue_open {
            self.close_queue(cx);
            return;
        }
        let queue = match &self.queue {
            Some(queue) => queue.clone(),
            None => {
                let view = cx.weak_entity();
                let close: Rc<dyn Fn(&mut App)> = Rc::new(move |cx: &mut App| {
                    let _ = view.update(cx, |this, cx| this.close_queue(cx));
                });
                let queue = cx.new(|cx| QueueView::new(window, cx).for_mini_player(close));
                self.queue = Some(queue.clone());
                queue
            }
        };
        queue.update(cx, |queue, _| queue.set_visible(true));
        self.queue_open = true;
        self.queue_closing = false;
        self._queue_task = None;
        self.queue_seq = self.queue_seq.wrapping_add(1);
        cx.notify();
    }

    fn close_queue(&mut self, cx: &mut Context<Self>) {
        if !self.queue_open {
            return;
        }
        if let Some(queue) = &self.queue {
            queue.update(cx, |queue, _| queue.set_visible(false));
        }
        self.queue_open = false;
        self.queue_closing = true;
        self.queue_seq = self.queue_seq.wrapping_add(1);
        self._queue_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(QUEUE_SLIDE).await;
            let _ = this.update(cx, |this, cx| {
                this.queue_closing = false;
                cx.notify();
            });
        }));
        cx.notify();
    }

    fn toggle_on_top(&mut self, _: &ClickEvent, window: &mut Window, cx: &mut Context<Self>) {
        let on = !self.on_top;
        if let Err(err) = window_level::set_always_on_top(&*window, on) {
            log::warn!("mini player: keep on top failed: {err}");
            return;
        }
        self.on_top = on;
        if let Err(err) = cx.global_mut::<SettingsStore>().set_mini_player_on_top(on) {
            notify_save_error(cx, err);
        }
        cx.notify();
    }

    fn on_seek_forward(&mut self, _: &SeekForward, _: &mut Window, cx: &mut Context<Self>) {
        self.progress.update(cx, |p, cx| p.seek_step(1, cx));
    }

    fn on_seek_backward(&mut self, _: &SeekBackward, _: &mut Window, cx: &mut Context<Self>) {
        self.progress.update(cx, |p, cx| p.seek_step(-1, cx));
    }

    fn on_next_track(&mut self, _: &NextTrack, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_next(cx);
    }

    fn on_previous_track(&mut self, _: &PreviousTrack, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_previous(cx);
    }

    fn on_volume_up(&mut self, _: &VolumeUp, _: &mut Window, cx: &mut Context<Self>) {
        self.volume.update(cx, |v, cx| v.nudge(VOLUME_STEP, cx));
    }

    fn on_volume_down(&mut self, _: &VolumeDown, _: &mut Window, cx: &mut Context<Self>) {
        self.volume.update(cx, |v, cx| v.nudge(-VOLUME_STEP, cx));
    }

    fn on_play_pause(&mut self, _: &PlayPause, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::toggle_play_pause(cx);
    }

    fn transport(&self) -> Div {
        h_flex()
            .flex_shrink_0()
            .gap_2()
            .items_center()
            .child(self.prev_button.clone())
            .child(self.play_button.clone())
            .child(self.next_button.clone())
    }

    fn like_button(&self, cx: &Context<Self>) -> Option<Div> {
        if !cx.global::<SettingsStore>().liked_enabled() {
            return None;
        }
        let track = self.track.as_ref()?;
        let (id, liked) = (track.id, track.liked);
        let (icon, color) = if liked {
            ("icons/s1-heart-fill.svg", Colors::primary(cx))
        } else {
            ("icons/s1-heart.svg", Colors::foreground(cx))
        };
        Some(
            div().flex_shrink_0().child(
                control_button("mini_like", icon, color, cx)
                    .tooltip(move |window, cx| {
                        let text = if liked {
                            tr().remove_from_liked.clone()
                        } else {
                            tr().add_to_liked.clone()
                        };
                        Tooltip::new(text).build(window, cx)
                    })
                    .on_click(move |_, _, cx| {
                        cx.global::<Services>().library.set_liked(id, !liked);
                    }),
            ),
        )
    }

    fn lyrics_toggle(&self, cx: &mut Context<Self>) -> Div {
        guard(
            div().flex_shrink_0().child(
                control_button(
                    "mini_lyrics",
                    "icons/s2-lyrics.svg",
                    toggle_color(self.lyrics_on, cx),
                    cx,
                )
                .tooltip(|window, cx| Tooltip::new(tr().lyrics.clone()).build(window, cx))
                .on_click(cx.listener(Self::toggle_lyrics)),
            ),
        )
    }

    fn queue_toggle(&self, cx: &mut Context<Self>) -> Div {
        guard(
            div().flex_shrink_0().child(
                control_button(
                    "mini_queue",
                    "icons/s2-queue.svg",
                    toggle_color(self.queue_open, cx),
                    cx,
                )
                .tooltip(|window, cx| Tooltip::new(tr().queue.clone()).build(window, cx))
                .on_click(cx.listener(Self::toggle_queue)),
            ),
        )
    }

    fn text_block(&self, lyric: Option<SharedString>, cx: &Context<Self>) -> Div {
        let (title, artists) = self
            .track
            .as_ref()
            .map(|t| (t.title.clone(), t.artists.clone()))
            .unwrap_or_default();
        v_flex()
            .w_full()
            .min_w(px(0.))
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_sm()
                    .font_weight(FontWeight::SEMIBOLD)
                    .child(title),
            )
            .child(
                div()
                    .max_w_full()
                    .truncate()
                    .text_xs()
                    .text_color(Colors::muted_foreground(cx))
                    .child(artists),
            )
            .children(lyric.map(|line| {
                div()
                    .max_w_full()
                    .text_xs()
                    .line_height(rems(1.))
                    .max_h(rems(1.))
                    .overflow_hidden()
                    .text_color(Colors::primary(cx))
                    .child(line)
            }))
    }

    fn chrome(&self, cx: &mut Context<Self>) -> Div {
        let pin_color = if self.on_top {
            Colors::primary(cx)
        } else {
            Colors::foreground(cx)
        };
        guard(
            h_flex()
                .absolute()
                .top(px(6.))
                .right(px(6.))
                .gap_1()
                .p(px(2.))
                .rounded_full()
                .bg(Colors::background(cx).opacity(SCRIM))
                .when(self.can_pin, |d| {
                    d.child(
                        chrome_button("mini_pin", "icons/s1-pin.svg", pin_color, cx)
                            .tooltip(|window, cx| {
                                Tooltip::new(tr().keep_on_top.clone()).build(window, cx)
                            })
                            .on_click(cx.listener(Self::toggle_on_top)),
                    )
                })
                .child(
                    chrome_button(
                        "mini_full",
                        "icons/s1-expand.svg",
                        Colors::foreground(cx),
                        cx,
                    )
                    .tooltip(|window, cx| Tooltip::new(tr().full_player.clone()).build(window, cx))
                    .on_click(|_, _, cx| super::leave(cx)),
                )
                .child(
                    chrome_button("mini_close", "icons/s1-x.svg", Colors::foreground(cx), cx)
                        .tooltip(|window, cx| {
                            Tooltip::new(tr().close_window.clone()).build(window, cx)
                        })
                        .on_click(|_, window, _| window.remove_window()),
                ),
        )
    }

    fn strip_body(&self, w: f32, h: f32, rem: f32, cx: &mut Context<Self>) -> AnyElement {
        let lyric = self
            .lyrics_on
            .then(|| self.lyrics.read(cx).line())
            .flatten()
            .filter(|_| h >= PAD * 2. + rem * 3.25 + 36. + 16. + 8.);
        let like = self.like_button(cx);
        let toggles_w = (CONTROL_SIZE + ROW_GAP) * 2. + ROW_GAP;
        let controls_w =
            TRANSPORT_W + like.as_ref().map_or(0., |_| CONTROL_SIZE + ROW_GAP) + toggles_w;
        let side = (h - PAD * 2.).min(w - PAD * 2. - GAP - controls_w).max(0.);
        let column_w = w - PAD * 2. - side - GAP;
        let show_volume = column_w - controls_w >= VOLUME_MIN_W;
        h_flex()
            .size_full()
            .p(px(PAD))
            .gap(px(GAP))
            .items_center()
            .child(
                cover_layer(
                    self.thumb.clone(),
                    None,
                    side,
                    side,
                    STRIP_RADIUS,
                    Colors::secondary(cx),
                    Colors::muted_foreground(cx),
                )
                .flex_shrink_0(),
            )
            .child(
                v_flex()
                    .flex_1()
                    .min_w(px(0.))
                    .h_full()
                    .justify_between()
                    .child(self.text_block(lyric, cx))
                    .child(
                        v_flex()
                            .gap_1()
                            .child(
                                h_flex()
                                    .items_center()
                                    .gap(px(ROW_GAP))
                                    .child(guard(self.transport()))
                                    .children(like.map(guard))
                                    .map(|d| {
                                        if show_volume {
                                            d.child(guard(
                                                div()
                                                    .flex_1()
                                                    .min_w(px(0.))
                                                    .child(self.volume.clone()),
                                            ))
                                        } else {
                                            d.child(div().flex_1())
                                        }
                                    })
                                    .child(self.lyrics_toggle(cx))
                                    .child(self.queue_toggle(cx)),
                            )
                            .child(guard(div().w_full().child(self.progress.read(cx).slider()))),
                    ),
            )
            .into_any_element()
    }

    fn cover_body(
        &self,
        w: f32,
        h: f32,
        rem: f32,
        overlay: bool,
        cx: &mut Context<Self>,
    ) -> AnyElement {
        let side = w.min(h);
        let scrim = Colors::background(cx).opacity(SCRIM);
        let volume_open = self.cover_volume.read(cx).is_expanded();
        let volume_reach = cover_volume::ICON_SIZE + cover_volume::PANEL_H + rem * 0.5;
        let compact = overlay && h < COVER_LAYOUT_MIN_H + COMPACT_LYRICS_EXTRA;
        let around = self
            .lyrics_on
            .then(|| self.lyrics.read(cx).around())
            .flatten()
            .map(|around| {
                if compact {
                    Around {
                        previous: None,
                        next: None,
                        ..around
                    }
                } else {
                    around
                }
            });
        let like = self.like_button(cx);
        let panel = overlay.then(|| {
            v_flex()
                .w_full()
                .gap_1()
                .px(px(PAD))
                .pt(px(PAD))
                .bg(scrim)
                .child(self.text_block(None, cx))
                .child(
                    h_flex()
                        .w_full()
                        .items_center()
                        .justify_between()
                        .child(
                            h_flex()
                                .w(px(SIDE_GROUP_W))
                                .gap_1()
                                .children(like.map(guard)),
                        )
                        .child(guard(self.transport()))
                        .child(
                            h_flex()
                                .w(px(SIDE_GROUP_W))
                                .gap_1()
                                .justify_end()
                                .child(self.lyrics_toggle(cx))
                                .child(self.queue_toggle(cx))
                                .child(guard(
                                    div()
                                        .relative()
                                        .size(px(cover_volume::ICON_SIZE))
                                        .when(volume_open, |d| {
                                            d.child(
                                                div()
                                                    .absolute()
                                                    .bottom_0()
                                                    .left_0()
                                                    .w(px(cover_volume::ICON_SIZE))
                                                    .h(px(volume_reach))
                                                    .occlude(),
                                            )
                                        })
                                        .child(self.cover_volume.clone()),
                                )),
                        ),
                )
        });
        div()
            .size_full()
            .relative()
            .child(
                div()
                    .absolute()
                    .inset_0()
                    .flex()
                    .items_center()
                    .justify_center()
                    .child(cover_layer(
                        self.thumb.clone(),
                        self.full.clone(),
                        side,
                        side,
                        0.,
                        Colors::secondary(cx),
                        Colors::muted_foreground(cx),
                    )),
            )
            .child(
                v_flex()
                    .absolute()
                    .left_0()
                    .right_0()
                    .bottom_0()
                    .items_center()
                    .children(around.map(|around| lyric_block(around, rem, cx)))
                    .children(panel)
                    .child(guard(
                        div()
                            .w_full()
                            .px(px(PAD))
                            .py(px(4.))
                            .bg(scrim)
                            .child(self.progress.read(cx).slider()),
                    )),
            )
            .into_any_element()
    }

    fn queue_layer(&self, h: f32, cx: &mut Context<Self>) -> Option<AnyElement> {
        let queue = self.queue.clone()?;
        if !self.queue_open && !self.queue_closing {
            return None;
        }
        let closing = self.queue_closing;
        let shift = h * 0.12;
        Some(
            div()
                .absolute()
                .left_0()
                .top_0()
                .w_full()
                .h(px(h))
                .occlude()
                .bg(Colors::background(cx))
                .child(queue)
                .with_animation(
                    ("mini-queue", self.queue_seq),
                    Animation::new(QUEUE_SLIDE).with_easing(ease_out_quint()),
                    move |layer, delta| {
                        let t = if closing { 1. - delta } else { delta };
                        layer.top(px(shift * (1. - t))).opacity(t)
                    },
                )
                .into_any_element(),
        )
    }
}

impl Render for MiniPlayer {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let viewport = window.viewport_size();
        let insets = gpui_component::window_paddings(window);
        let w = f32::from(viewport.width - insets.left - insets.right);
        let h = f32::from(viewport.height - insets.top - insets.bottom);
        let rem = f32::from(window.rem_size());
        let cover_layout = h >= COVER_LAYOUT_MIN_H;
        self.want_full(cover_layout, cx);
        let pointer_in = cfg!(target_os = "macos") || window.is_window_hovered();
        self.track_hover(self.pointer_over && pointer_in, cx);
        let seeking = self.progress.read(cx).slider().read(cx).is_interacting();
        let hovered = self.hovered || seeking;
        let overlay = hovered || self.cover_volume.read(cx).is_expanded();
        let queue_shown = self.queue_open || self.queue_closing;
        let body = if cover_layout {
            self.cover_body(w, h, rem, overlay, cx)
        } else {
            self.strip_body(w, h, rem, cx)
        };
        let queue_layer = self.queue_layer(h, cx);
        div()
            .id("mini_player")
            .key_context(MINI_CONTEXT)
            .relative()
            .size_full()
            .overflow_hidden()
            .bg(Colors::background(cx))
            .on_action(cx.listener(Self::on_seek_forward))
            .on_action(cx.listener(Self::on_seek_backward))
            .on_action(cx.listener(Self::on_next_track))
            .on_action(cx.listener(Self::on_previous_track))
            .on_action(cx.listener(Self::on_volume_up))
            .on_action(cx.listener(Self::on_volume_down))
            .on_action(cx.listener(Self::on_play_pause))
            .window_control_area(WindowControlArea::Drag)
            .when(!cfg!(target_os = "windows"), |d| {
                d.on_mouse_down(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.should_move = true),
                )
                .on_mouse_up(
                    MouseButton::Left,
                    cx.listener(|this, _, _, _| this.should_move = false),
                )
                .on_mouse_move(cx.listener(|this, _, window, _| {
                    if std::mem::take(&mut this.should_move) {
                        window.start_window_move();
                    }
                }))
            })
            .child(div().absolute().size_0().track_focus(&self.focus_handle))
            .child(body)
            .when(hovered && !queue_shown, |d| d.child(self.chrome(cx)))
            .children(queue_layer)
            .child(self.playlist_popup.clone())
            .child(
                div()
                    .id("mini_hover")
                    .absolute()
                    .inset_0()
                    .on_hover(cx.listener(|this, hovered: &bool, _, cx| {
                        if !*hovered {
                            this.set_pointer_over(false, cx);
                        }
                    }))
                    .on_mouse_move(cx.listener(|this, _: &MouseMoveEvent, _, cx| {
                        if !cx.has_active_drag() {
                            this.set_pointer_over(true, cx);
                        }
                    }))
                    .on_mouse_up(
                        MouseButton::Left,
                        cx.listener(|this, _, _, cx| this.set_pointer_over(true, cx)),
                    ),
            )
    }
}

fn guard(el: Div) -> Div {
    el.occlude()
        .on_mouse_down(MouseButton::Left, |_, _, cx| cx.stop_propagation())
}

fn toggle_color(active: bool, cx: &App) -> gpui::Hsla {
    if active {
        Colors::primary(cx)
    } else {
        Colors::muted_foreground(cx)
    }
}

fn control_button(
    id: &'static str,
    icon: &'static str,
    color: gpui::Hsla,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(CONTROL_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(Colors::muted(cx)))
        .child(svg().path(icon).size(px(CONTROL_ICON)).text_color(color))
}

fn chrome_button(
    id: &'static str,
    icon: &'static str,
    color: gpui::Hsla,
    cx: &App,
) -> Stateful<Div> {
    div()
        .id(id)
        .size(px(CHROME_SIZE))
        .flex()
        .items_center()
        .justify_center()
        .rounded_full()
        .cursor_pointer()
        .hover(|s| s.bg(Colors::muted(cx)))
        .child(svg().path(icon).size(px(CHROME_ICON)).text_color(color))
}

fn lyric_block(around: Around, rem: f32, cx: &App) -> AnyElement {
    let muted = Colors::muted_foreground(cx);
    let accent = Colors::primary(cx);
    let side_line = |text: Option<SharedString>| {
        div()
            .w_full()
            .text_xs()
            .line_height(rems(1.))
            .max_h(rems(2.))
            .overflow_hidden()
            .text_color(muted)
            .children(text)
    };
    let shift = rem * 1.25;
    div()
        .mx(px(PAD))
        .mb(px(6.))
        .px(px(10.))
        .py(px(4.))
        .rounded(px(8.))
        .overflow_hidden()
        .bg(Colors::background(cx).opacity(SCRIM))
        .child(
            v_flex()
                .relative()
                .gap(px(2.))
                .text_center()
                .child(side_line(around.previous))
                .child(
                    div()
                        .w_full()
                        .text_sm()
                        .font_weight(FontWeight::SEMIBOLD)
                        .line_height(rems(1.25))
                        .max_h(rems(2.5))
                        .overflow_hidden()
                        .text_color(accent)
                        .children(around.current),
                )
                .child(side_line(around.next))
                .with_animation(
                    ("mini-lyrics", around.key),
                    Animation::new(LYRIC_SLIDE).with_easing(ease_out_quint()),
                    move |block, delta| {
                        block
                            .top(px(shift * (1. - delta)))
                            .opacity(0.35 + 0.65 * delta)
                    },
                ),
        )
        .into_any_element()
}
