use std::sync::Arc;
use std::time::{Duration, Instant};

use audio_engine::EngineEvent;
use gpui::prelude::FluentBuilder;
use gpui::{
    Animation, AnimationExt, AppContext, BoxShadow, ClickEvent, Context, Div, Entity, EventEmitter,
    Hsla, Image, ImageFormat, InteractiveElement, IntoElement, ParentElement, Pixels, Render,
    RenderImage, SharedString, Size, StatefulInteractiveElement, Styled, StyledImage, Subscription,
    Task, Transformation, Window, canvas, div, ease_out_quint, img, point, px, size, svg,
};
use gpui_component::{h_flex, v_flex};

use crate::library_service::LibraryEvent;
use crate::now_playing::{NavigateToAlbumRequested, NavigateToArtistRequested};
use crate::services::Services;
use crate::settings_store::SettingsStore;
use crate::theme_colors::Colors;
use crate::track_progress_slider::TrackProgressSlider;

const COVER_RADIUS: f32 = 12.;
const COVER_MARGIN: f32 = 40.;
const COVER_MIN_SIDE: f32 = 120.;
const COVER_TEXT_GAP: f32 = 16.;
const COVER_TEXT_LINE: f32 = 30.;
const COVER_PROGRESS_GAP: f32 = 14.;
const COVER_PROGRESS_H: f32 = 16.;
const COVER_PROGRESS_W_FRAC: f32 = 0.75;
const COVER_GROUP_DROP: f32 = 28.;
const COVER_CTRL_BG_OPACITY: f32 = 0.55;
const COVER_CTRL_SIZE: f32 = 44.;
const COVER_CTRL_ICON: f32 = 26.;
const COVER_PLAY_SIZE: f32 = 64.;
const COVER_PLAY_ICON: f32 = 34.;
const CORNER_HIDE_DELAY: Duration = Duration::from_secs(3);
pub const CORNER_FADE: Duration = Duration::from_millis(200);
const COVER_SLIDE: Duration = Duration::from_millis(300);
const COVER_SLIDE_GAP: f32 = 40.;

pub struct CoverModeView {
    track_title: SharedString,
    artists: Vec<(i64, SharedString)>,
    album_id: Option<i64>,
    cover_art_id: Option<i64>,
    track_path: Option<String>,
    large_cover: Option<Arc<Image>>,
    full_cover: Option<Arc<RenderImage>>,
    cover_aspect: Option<f32>,
    prev_large_cover: Option<Arc<Image>>,
    prev_full_cover: Option<Arc<RenderImage>>,
    sliding: bool,
    slide_forward: bool,
    slide_seq: u64,
    queue_index: Option<usize>,
    active: bool,
    chrome_visible: bool,
    measured: Size<Pixels>,
    is_playing: bool,
    corner_visible: bool,
    corner_hiding: bool,
    corner_hide_at: Instant,
    corner_hide_task: Option<Task<()>>,
    show_artist: bool,
    show_progress: bool,
    show_controls: bool,
    progress: Entity<TrackProgressSlider>,
    _full_cover_task: Option<Task<()>>,
    _slide_task: Option<Task<()>>,
    _engine_subscription: Subscription,
    _library_subscription: Subscription,
    _settings_subscription: Subscription,
}

impl EventEmitter<NavigateToAlbumRequested> for CoverModeView {}
impl EventEmitter<NavigateToArtistRequested> for CoverModeView {}

fn sniff_image_format(bytes: &[u8]) -> Option<ImageFormat> {
    if bytes.starts_with(&[0xFF, 0xD8]) {
        Some(ImageFormat::Jpeg)
    } else if bytes.starts_with(b"\x89PNG") {
        Some(ImageFormat::Png)
    } else if bytes.len() > 12 && bytes.starts_with(b"RIFF") && &bytes[8..12] == b"WEBP" {
        Some(ImageFormat::Webp)
    } else if bytes.starts_with(b"GIF8") {
        Some(ImageFormat::Gif)
    } else if bytes.starts_with(b"BM") {
        Some(ImageFormat::Bmp)
    } else {
        None
    }
}

fn image_aspect(bytes: &[u8]) -> Option<f32> {
    let (w, h) = image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()?;
    if w == 0 || h == 0 {
        return None;
    }
    Some(w as f32 / h as f32)
}

fn fit_cover_box(aspect: Option<f32>, max_w: f32, max_h: f32) -> (f32, f32) {
    match aspect {
        Some(a) if a.is_finite() && a > 0. => {
            let mut w = max_w;
            let mut h = w / a;
            if h > max_h {
                h = max_h;
                w = h * a;
            }
            (w, h)
        }
        _ => {
            let s = max_w.min(max_h);
            (s, s)
        }
    }
}

impl CoverModeView {
    pub fn new(window: &mut Window, cx: &mut Context<Self>) -> Self {
        let engine_event_bus = cx.global::<Services>().engine_event_bus.clone();
        let engine_subscription = cx.subscribe(
            &engine_event_bus,
            |this, _, event: &EngineEvent, cx| match event {
                EngineEvent::Loaded { .. } => {
                    this.populate_current(true, cx);
                }
                EngineEvent::Playing => {
                    this.is_playing = true;
                    cx.notify();
                }
                EngineEvent::Paused => {
                    this.is_playing = false;
                    cx.notify();
                }
                EngineEvent::TrackEnded | EngineEvent::Stopped => {
                    this.is_playing = false;
                    let services = cx.global::<Services>();
                    let queue = services.playback_queue.borrow();
                    if queue.current_track().is_none() {
                        drop(queue);
                        this.clear(cx);
                    } else {
                        drop(queue);
                    }
                    cx.notify();
                }
                _ => {}
            },
        );

        let library_event_bus = cx.global::<Services>().library_event_bus.clone();
        let library_subscription =
            cx.subscribe(&library_event_bus, |this, _, event: &LibraryEvent, cx| {
                if let LibraryEvent::ScanComplete { changed: true } = event {
                    this.populate_current(false, cx);
                }
            });

        let progress = cx.new(|cx| TrackProgressSlider::new(window, cx));

        let settings = cx.global::<SettingsStore>();
        let show_artist = settings.cover_show_artist();
        let show_progress = settings.cover_show_progress();
        let show_controls = settings.cover_show_controls();
        let settings_subscription = cx.observe_global::<SettingsStore>(|this: &mut Self, cx| {
            let settings = cx.global::<SettingsStore>();
            let vals = (
                settings.cover_show_artist(),
                settings.cover_show_progress(),
                settings.cover_show_controls(),
            );
            if vals != (this.show_artist, this.show_progress, this.show_controls) {
                (this.show_artist, this.show_progress, this.show_controls) = vals;
                cx.notify();
            }
        });

        let is_playing = cx
            .global::<Services>()
            .is_playing
            .load(std::sync::atomic::Ordering::Relaxed);

        Self {
            track_title: SharedString::default(),
            artists: Vec::new(),
            album_id: None,
            cover_art_id: None,
            track_path: None,
            large_cover: None,
            full_cover: None,
            cover_aspect: None,
            prev_large_cover: None,
            prev_full_cover: None,
            sliding: false,
            slide_forward: true,
            slide_seq: 0,
            queue_index: None,
            active: false,
            chrome_visible: true,
            measured: size(px(0.), px(0.)),
            is_playing,
            corner_visible: false,
            corner_hiding: false,
            corner_hide_at: Instant::now(),
            corner_hide_task: None,
            show_artist,
            show_progress,
            show_controls,
            progress,
            _full_cover_task: None,
            _slide_task: None,
            _engine_subscription: engine_subscription,
            _library_subscription: library_subscription,
            _settings_subscription: settings_subscription,
        }
    }

    pub fn chrome_visible(&self) -> bool {
        self.chrome_visible
    }

    pub fn corner_visible(&self) -> bool {
        self.corner_visible
    }

    pub fn corner_hiding(&self) -> bool {
        self.corner_hiding
    }

    pub fn toggle_chrome(&mut self, cx: &mut Context<Self>) {
        self.chrome_visible = !self.chrome_visible;
        self.corner_visible = false;
        self.corner_hiding = false;
        self.corner_hide_task = None;
        cx.notify();
    }

    pub fn handle_mouse_move(&mut self, cx: &mut Context<Self>) {
        if !self.active || self.chrome_visible {
            return;
        }
        self.corner_hide_at = Instant::now() + CORNER_HIDE_DELAY;
        if !self.corner_visible {
            self.corner_visible = true;
            self.corner_hiding = false;
            cx.notify();
        }
        if self.corner_hide_task.is_none() {
            self.spawn_corner_hide(cx);
        }
    }

    fn spawn_corner_hide(&mut self, cx: &mut Context<Self>) {
        self.corner_hide_task = Some(cx.spawn(async move |this, cx| {
            loop {
                let Ok(remaining) = this.read_with(cx, |view, _| {
                    view.corner_hide_at
                        .saturating_duration_since(Instant::now())
                }) else {
                    return;
                };
                if !remaining.is_zero() {
                    cx.background_executor().timer(remaining).await;
                    continue;
                }
                let fading = this
                    .update(cx, |view, cx| {
                        if view.active && view.corner_visible {
                            view.corner_visible = false;
                            view.corner_hiding = true;
                            cx.notify();
                            true
                        } else {
                            false
                        }
                    })
                    .unwrap_or(false);
                if fading {
                    cx.background_executor().timer(CORNER_FADE).await;
                    let reshown = this
                        .update(cx, |view, cx| {
                            if view.corner_visible {
                                return true;
                            }
                            if view.corner_hiding {
                                view.corner_hiding = false;
                                cx.notify();
                            }
                            false
                        })
                        .unwrap_or(false);
                    if reshown {
                        continue;
                    }
                }
                let _ = this.update(cx, |view, _| view.corner_hide_task = None);
                return;
            }
        }));
    }

    pub fn set_active(&mut self, active: bool, cx: &mut Context<Self>) {
        if self.active == active {
            return;
        }
        self.active = active;
        self.chrome_visible = true;
        self.corner_visible = false;
        self.corner_hiding = false;
        self.corner_hide_task = None;
        if active {
            self.populate_current(false, cx);
        } else {
            self._full_cover_task = None;
            self.release_full_cover(cx);
            self.end_cover_slide(cx);
        }
        cx.notify();
    }

    fn on_play_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        if let Some(playing) = crate::services::toggle_play_pause(cx) {
            self.is_playing = playing;
            cx.notify();
        }
    }

    fn on_prev_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_previous(cx);
    }

    fn on_next_click(&mut self, _: &ClickEvent, _: &mut Window, cx: &mut Context<Self>) {
        crate::services::play_next(cx);
    }

    #[allow(clippy::too_many_arguments)]
    fn control_button(
        id: &'static str,
        icon: &'static str,
        flipped: bool,
        btn_size: f32,
        icon_size: f32,
        bg: Hsla,
        hover: Hsla,
        fg: Hsla,
        handler: fn(&mut Self, &ClickEvent, &mut Window, &mut Context<Self>),
        cx: &mut Context<Self>,
    ) -> impl IntoElement {
        div().rounded_full().bg(bg).child(
            div()
                .id(id)
                .size(px(btn_size))
                .flex()
                .items_center()
                .justify_center()
                .rounded_full()
                .cursor_pointer()
                .hover(move |s| s.bg(hover))
                .on_click(cx.listener(handler))
                .child(
                    svg()
                        .path(icon)
                        .size(px(icon_size))
                        .when(flipped, |s| {
                            s.with_transformation(Transformation::scale(size(-1.0, 1.0)))
                        })
                        .text_color(fg),
                ),
        )
    }

    fn populate_current(&mut self, animate: bool, cx: &mut Context<Self>) {
        if !self.active {
            return;
        }
        let services = cx.global::<Services>();
        let queue = services.playback_queue.borrow();
        let Some(track) = queue.current_track() else {
            drop(queue);
            self.clear(cx);
            cx.notify();
            return;
        };
        let track_id = track.id;
        let title = track.title.clone();
        let cover = track.cover_art_id;
        let album_id = track.album_id;
        let path = track.path.clone();
        let index = queue.current_index();
        drop(queue);
        self.track_title = title.into();
        self.album_id = album_id;
        self.track_path = Some(path);
        self.artists = services
            .library
            .unique_track_artists(track_id)
            .into_iter()
            .map(|(id, name)| (id, SharedString::from(name)))
            .collect();
        let new_thumb = services
            .cover_art_cache
            .borrow_mut()
            .get_large(cover, &services.library);
        if self.cover_art_id != cover {
            if animate && (self.large_cover.is_some() || self.full_cover.is_some()) {
                self.start_cover_slide(index, cx);
            } else {
                self.release_full_cover(cx);
            }
            self.cover_art_id = cover;
            self.large_cover = new_thumb;
            self.load_full_cover(cx);
        } else {
            self.large_cover = new_thumb;
            if self.full_cover.is_none() {
                self.load_full_cover(cx);
            }
        }
        self.queue_index = index;
        cx.notify();
    }

    fn start_cover_slide(&mut self, new_index: Option<usize>, cx: &mut Context<Self>) {
        self.release_prev_cover(cx);
        self.prev_large_cover = self.large_cover.take();
        self.prev_full_cover = self.full_cover.take();
        self.slide_forward = match (self.queue_index, new_index) {
            (Some(old), Some(new)) => new >= old,
            _ => true,
        };
        self.slide_seq = self.slide_seq.wrapping_add(1);
        self.sliding = true;
        self._slide_task = Some(cx.spawn(async move |this, cx| {
            cx.background_executor().timer(COVER_SLIDE).await;
            let _ = this.update(cx, |view, cx| {
                view.sliding = false;
                view._slide_task = None;
                view.release_prev_cover(cx);
                cx.notify();
            });
        }));
    }

    fn end_cover_slide(&mut self, cx: &mut Context<Self>) {
        self.sliding = false;
        self._slide_task = None;
        self.release_prev_cover(cx);
    }

    fn release_prev_cover(&mut self, cx: &mut Context<Self>) {
        self.prev_large_cover = None;
        if let Some(old) = self.prev_full_cover.take() {
            cx.drop_image(old, None);
        }
    }

    fn release_full_cover(&mut self, cx: &mut Context<Self>) {
        self.cover_aspect = None;
        if let Some(old) = self.full_cover.take() {
            cx.drop_image(old, None);
        }
    }

    fn clear(&mut self, cx: &mut Context<Self>) {
        self.track_title = SharedString::default();
        self.artists.clear();
        self.album_id = None;
        self.cover_art_id = None;
        self.track_path = None;
        self.large_cover = None;
        self.queue_index = None;
        self.release_full_cover(cx);
        self.end_cover_slide(cx);
        self._full_cover_task = None;
    }

    fn cover_layer(
        thumb: Option<Arc<Image>>,
        full: Option<Arc<RenderImage>>,
        w: f32,
        h: f32,
        placeholder_bg: Hsla,
        placeholder_fg: Hsla,
    ) -> Div {
        div()
            .relative()
            .w(px(w))
            .h(px(h))
            .rounded(px(COVER_RADIUS))
            .overflow_hidden()
            .bg(placeholder_bg)
            .map(|d| match (thumb, full) {
                (Some(thumb), Some(full)) => d
                    .child(img(thumb).size_full().object_fit(gpui::ObjectFit::Cover))
                    .child(
                        img(full)
                            .absolute()
                            .top_0()
                            .left_0()
                            .size_full()
                            .object_fit(gpui::ObjectFit::Contain),
                    ),
                (Some(thumb), None) => {
                    d.child(img(thumb).size_full().object_fit(gpui::ObjectFit::Cover))
                }
                (None, Some(full)) => {
                    d.child(img(full).size_full().object_fit(gpui::ObjectFit::Contain))
                }
                (None, None) => d.flex().items_center().justify_center().child(
                    svg()
                        .path("icons/placeholder-notes.svg")
                        .size(px(w.min(h) * 0.5))
                        .text_color(placeholder_fg),
                ),
            })
    }

    fn load_full_cover(&mut self, cx: &mut Context<Self>) {
        self._full_cover_task = None;
        let Some(id) = self.cover_art_id else {
            return;
        };
        let services = cx.global::<Services>();
        let source = services.library.get_cover_art_source(id);
        let track_path = self.track_path.clone();
        let renderer = cx.svg_renderer();
        let load = cx.background_executor().spawn(async move {
            let bytes =
                music_indexer::metadata::load_cover_from_source(source, track_path.as_deref())?;
            let format = sniff_image_format(&bytes)?;
            let aspect = image_aspect(&bytes);
            let image = Image::from_bytes(format, bytes)
                .to_image_data(renderer)
                .ok()?;
            Some((image, aspect))
        });
        self._full_cover_task = Some(cx.spawn(async move |this, cx| {
            let Some((image, aspect)) = load.await else {
                return;
            };
            let _ = this.update(cx, |view, cx| {
                view._full_cover_task = None;
                if view.active && view.cover_art_id == Some(id) {
                    view.full_cover = Some(image);
                    view.cover_aspect = aspect;
                    cx.notify();
                }
            });
        }));
    }
}

impl Render for CoverModeView {
    fn render(&mut self, window: &mut Window, cx: &mut Context<Self>) -> impl IntoElement {
        let avail = if self.measured.width > px(0.) {
            self.measured
        } else {
            window.viewport_size()
        };
        let avail_w = f32::from(avail.width);
        let avail_h = f32::from(avail.height);

        let album_id = self.album_id;
        let title_color = Colors::foreground(cx);
        let artist_color = Colors::muted_foreground(cx);
        let has_track = !self.track_title.is_empty();
        let thumb_img = self.large_cover.clone();
        let full_img = self.full_cover.clone();
        let placeholder_bg = Colors::secondary(cx);
        let placeholder_fg = Colors::muted_foreground(cx);

        let show_artist = self.show_artist;
        let show_progress = self.show_progress;
        let show_controls = self.show_controls;
        let ctrl_bg = Colors::background(cx).opacity(COVER_CTRL_BG_OPACITY);
        let ctrl_hover = Colors::muted(cx);
        let play_icon = if self.is_playing {
            "icons/pause.svg"
        } else {
            "icons/play.svg"
        };

        let show_text = has_track && !self.chrome_visible;
        let show_progress_bar = show_text && show_progress;
        let below = if show_text {
            COVER_TEXT_GAP
                + COVER_TEXT_LINE
                + if show_progress_bar {
                    COVER_PROGRESS_GAP + COVER_PROGRESS_H
                } else {
                    0.
                }
        } else {
            0.
        };
        let max_w = (avail_w - COVER_MARGIN * 2.).max(COVER_MIN_SIDE);
        let max_h = (avail_h - COVER_MARGIN * 2. - below).max(COVER_MIN_SIDE);
        let (cover_w, cover_h) = fit_cover_box(self.cover_aspect, max_w, max_h);

        let shadow = vec![BoxShadow {
            color: gpui::black().opacity(0.5),
            offset: point(px(0.), px(18.)),
            blur_radius: px(28.),
            spread_radius: px(0.),
        }];

        let controls = (show_text && show_controls && (self.corner_visible || self.corner_hiding))
            .then(|| {
                let hiding = self.corner_hiding;
                h_flex()
                    .absolute()
                    .bottom(px(16.))
                    .left(px(0.))
                    .right(px(0.))
                    .items_center()
                    .justify_center()
                    .gap_3()
                    .child(Self::control_button(
                        "cm_prev",
                        "icons/next.svg",
                        true,
                        COVER_CTRL_SIZE,
                        COVER_CTRL_ICON,
                        ctrl_bg,
                        ctrl_hover,
                        title_color,
                        Self::on_prev_click,
                        cx,
                    ))
                    .child(Self::control_button(
                        "cm_play",
                        play_icon,
                        false,
                        COVER_PLAY_SIZE,
                        COVER_PLAY_ICON,
                        ctrl_bg,
                        ctrl_hover,
                        title_color,
                        Self::on_play_click,
                        cx,
                    ))
                    .child(Self::control_button(
                        "cm_next",
                        "icons/next.svg",
                        false,
                        COVER_CTRL_SIZE,
                        COVER_CTRL_ICON,
                        ctrl_bg,
                        ctrl_hover,
                        title_color,
                        Self::on_next_click,
                        cx,
                    ))
                    .with_animation(
                        if hiding {
                            "cm-corner-out"
                        } else {
                            "cm-corner-in"
                        },
                        Animation::new(CORNER_FADE),
                        move |controls, delta| {
                            controls.opacity(if hiding { 1.0 - delta } else { delta })
                        },
                    )
            });

        let cover_square = if self.sliding {
            let forward = self.slide_forward;
            let slide_gap = (avail_w - cover_w) / 2. + COVER_SLIDE_GAP;
            let page = cover_w + slide_gap;
            let seq = self.slide_seq;
            let outgoing = Self::cover_layer(
                self.prev_large_cover.clone(),
                self.prev_full_cover.clone(),
                cover_w,
                cover_h,
                placeholder_bg,
                placeholder_fg,
            )
            .shadow(shadow.clone());
            let incoming = Self::cover_layer(
                thumb_img,
                full_img,
                cover_w,
                cover_h,
                placeholder_bg,
                placeholder_fg,
            )
            .shadow(shadow.clone());
            let (first, second) = if forward {
                (outgoing, incoming)
            } else {
                (incoming, outgoing)
            };
            div()
                .relative()
                .w(px(cover_w))
                .h(px(cover_h))
                .child(
                    div()
                        .absolute()
                        .top_0()
                        .h(px(cover_h))
                        .flex()
                        .flex_row()
                        .gap(px(slide_gap))
                        .child(first.flex_shrink_0())
                        .child(second.flex_shrink_0())
                        .with_animation(
                            ("cm-cover-slide", seq),
                            Animation::new(COVER_SLIDE).with_easing(ease_out_quint()),
                            move |strip, delta| {
                                let left = if forward {
                                    -page * delta
                                } else {
                                    -page * (1.0 - delta)
                                };
                                strip.left(px(left))
                            },
                        ),
                )
                .when_some(controls, |d, c| d.child(c))
        } else {
            Self::cover_layer(
                thumb_img,
                full_img,
                cover_w,
                cover_h,
                placeholder_bg,
                placeholder_fg,
            )
            .shadow(shadow)
            .when_some(controls, |d, c| d.child(c))
        };

        div()
            .size_full()
            .relative()
            .overflow_hidden()
            .bg(Colors::background(cx))
            .child(
                div()
                    .size_full()
                    .flex()
                    .flex_col()
                    .items_center()
                    .justify_center()
                    .when(show_text, |d| d.pt(px(COVER_GROUP_DROP)))
                    .child(cover_square)
                    .when(show_text, |d| {
                        let mut row = h_flex()
                            .px(px(COVER_MARGIN))
                            .items_center()
                            .gap_2()
                            .max_w(px(avail_w))
                            .overflow_hidden()
                            .child({
                                let title_inner = div()
                                    .flex_shrink_0()
                                    .text_xl()
                                    .font_weight(gpui::FontWeight::SEMIBOLD)
                                    .text_color(title_color)
                                    .child(self.track_title.clone());
                                if let Some(aid) = album_id {
                                    div()
                                        .id("cm_title")
                                        .flex_shrink_0()
                                        .cursor_pointer()
                                        .border_b(px(1.))
                                        .hover(move |s| s.border_color(title_color))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(NavigateToAlbumRequested { album_id: aid });
                                        }))
                                        .child(title_inner)
                                        .into_any_element()
                                } else {
                                    title_inner.into_any_element()
                                }
                            });
                        if show_artist && !self.artists.is_empty() {
                            row = row.child(
                                div()
                                    .flex_shrink_0()
                                    .text_xl()
                                    .text_color(artist_color)
                                    .child("\u{b7}"),
                            );
                            let mut artists = h_flex().flex_shrink_0().items_center();
                            for (i, (artist_id, name)) in self.artists.iter().enumerate() {
                                if i > 0 {
                                    artists = artists.child(
                                        div().text_xl().text_color(artist_color).child(", "),
                                    );
                                }
                                let artist_id = *artist_id;
                                artists = artists.child(
                                    div()
                                        .id(("cm_artist", artist_id as u64))
                                        .text_xl()
                                        .text_color(artist_color)
                                        .cursor_pointer()
                                        .border_b(px(1.))
                                        .hover(move |s| s.border_color(artist_color))
                                        .on_click(cx.listener(move |_, _, _, cx| {
                                            cx.emit(NavigateToArtistRequested { artist_id });
                                        }))
                                        .child(name.clone()),
                                );
                            }
                            row = row.child(artists);
                        }
                        let mut group = v_flex()
                            .mt(px(COVER_TEXT_GAP))
                            .items_center()
                            .gap(px(COVER_PROGRESS_GAP))
                            .child(row);
                        if show_progress_bar {
                            group = group.child(
                                div()
                                    .w(px(cover_w * COVER_PROGRESS_W_FRAC))
                                    .child(self.progress.read(cx).slider()),
                            );
                        }
                        d.child(group)
                    }),
            )
            .child({
                let entity = cx.entity();
                canvas(
                    move |bounds, window, cx| {
                        if entity.read(cx).measured != bounds.size {
                            entity.update(cx, |this, _| this.measured = bounds.size);
                            window.on_next_frame(move |_, cx| {
                                entity.update(cx, |_, cx| cx.notify());
                            });
                        }
                    },
                    |_, _, _, _| {},
                )
                .absolute()
                .size_full()
            })
    }
}

#[cfg(test)]
mod tests {
    use super::sniff_image_format;
    use gpui::ImageFormat;

    #[test]
    fn sniffs_known_formats() {
        assert!(matches!(
            sniff_image_format(&[0xFF, 0xD8, 0xFF, 0xE0]),
            Some(ImageFormat::Jpeg)
        ));
        assert!(matches!(
            sniff_image_format(b"\x89PNG\r\n\x1a\n"),
            Some(ImageFormat::Png)
        ));
        assert!(matches!(
            sniff_image_format(b"RIFF\x10\x00\x00\x00WEBPVP8 "),
            Some(ImageFormat::Webp)
        ));
        assert!(matches!(
            sniff_image_format(b"GIF89a"),
            Some(ImageFormat::Gif)
        ));
        assert!(matches!(
            sniff_image_format(b"BM\x36\x00"),
            Some(ImageFormat::Bmp)
        ));
    }

    #[test]
    fn rejects_unknown_and_truncated() {
        assert!(sniff_image_format(b"").is_none());
        assert!(sniff_image_format(b"hello world").is_none());
        assert!(sniff_image_format(b"RIFF\x10\x00\x00\x00WAVE").is_none());
        assert!(sniff_image_format(b"RIFF\x10\x00WEBP").is_none());
        assert!(sniff_image_format(&[0x89]).is_none());
    }
}
