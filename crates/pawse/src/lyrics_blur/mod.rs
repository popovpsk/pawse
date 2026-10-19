mod raster;

use std::mem;
use std::sync::Arc;
use std::time::Instant;

use gpui::{
    App, AppContext, Context, Div, Entity, Hsla, IntoElement, ParentElement, Pixels, SharedString,
    Size, Styled, Task, Window, canvas, div, px,
};
use text_blur::{MAX_SIGMA, Mix};

pub use raster::GlyphRaster;
use raster::{BlurredLine, GlyphCache};

use crate::cover_art_cache::drop_atlas_tile;
use crate::lyrics_fill::{LineFont, LyricRow};

const STEP: f32 = 1.5;
const TAU: f32 = 0.11;
const DT_MAX: f32 = 0.05;
const EPS: f32 = 0.02;
const FIT: Pixels = px(1.);
const BATCH: usize = 12;
const PENDING_FADE: f32 = 0.5;
const BACKING_FADE: f32 = 2.;
const NEAR: f32 = 0.25;

#[derive(Clone, Copy, PartialEq)]
pub struct BlurStyle {
    pub font: LineFont,
    pub color: Hsla,
    pub scale: f32,
}

pub struct Frame<'a> {
    pub style: Option<BlurStyle>,
    pub rows: &'a [LyricRow],
    pub active: Option<usize>,
    pub following: bool,
    pub lit: &'a dyn Fn(usize) -> bool,
}

pub struct RowBlur {
    pub text_alpha: f32,
    pub backing_alpha: f32,
    line: Option<(Arc<BlurredLine>, Mix)>,
}

#[derive(Clone, PartialEq)]
struct Key {
    text: SharedString,
    measured: Size<Pixels>,
    style: BlurStyle,
}

#[derive(Default)]
struct Slot {
    measured: Size<Pixels>,
    key: Option<Key>,
    line: Option<Arc<BlurredLine>>,
    sigma: f32,
    far: bool,
}

#[derive(Default)]
pub struct LyricsBlur {
    slots: Vec<Slot>,
    style: Option<BlurStyle>,
    dirty: bool,
    baking: bool,
    seq: usize,
    tick: Option<Instant>,
    redraw_pending: bool,
    glyphs: GlyphCache,
    _task: Option<Task<()>>,
}

impl Slot {
    fn current(&self, style: BlurStyle) -> Option<&Arc<BlurredLine>> {
        let key = self.key.as_ref()?;
        if key.style != style {
            return None;
        }
        self.line.as_ref()
    }
}

impl RowBlur {
    pub fn layers(&self) -> impl Iterator<Item = Div> + '_ {
        self.line.iter().flat_map(|(line, mix)| {
            mix.layers
                .into_iter()
                .flatten()
                .map(move |(level, opacity)| layer(line.clone(), level, opacity))
        })
    }
}

impl LyricsBlur {
    pub fn is_on(&self) -> bool {
        self.style.is_some()
    }

    pub fn row(&self, ix: usize) -> Option<RowBlur> {
        let style = self.style?;
        let slot = self.slots.get(ix).filter(|slot| slot.sigma > 0.)?;
        let line = slot.current(style).filter(|_| !slot.far);
        let mix = match line {
            Some(line) => line.mix(slot.sigma),
            None => Mix {
                text: 1. - slot.sigma / MAX_SIGMA * PENDING_FADE,
                layers: [None, None],
            },
        };
        Some(RowBlur {
            text_alpha: mix.text,
            backing_alpha: (1. - slot.sigma / BACKING_FADE).clamp(0., 1.),
            line: line.map(|line| (line.clone(), mix)),
        })
    }

    pub fn release(&mut self, cx: &mut Context<Self>) {
        self.seq = self.seq.wrapping_add(1);
        self._task = None;
        self.baking = false;
        self.tick = None;
        self.dirty = true;
        self.glyphs = GlyphCache::default();
        for slot in self.slots.drain(..) {
            if let Some(line) = slot.line {
                release_line(&line, cx);
            }
        }
    }

    pub fn sync(&mut self, frame: Frame, window: &mut Window, cx: &mut Context<Self>) {
        let Some(style) = frame.style else {
            if self.style.take().is_some() || !self.slots.is_empty() {
                self.release(cx);
            }
            return;
        };
        if self.slots.len() != frame.rows.len() {
            self.slots.resize_with(frame.rows.len(), Slot::default);
        }
        if self.style != Some(style) {
            self.style = Some(style);
            self.glyphs = GlyphCache::default();
            self.dirty = true;
        }
        self.step(&frame, window);
        self.schedule(style, &frame, window, cx);
    }

    fn step(&mut self, frame: &Frame, window: &mut Window) {
        let now = Instant::now();
        let dt = self
            .tick
            .map_or(0., |tick| now.duration_since(tick).as_secs_f32())
            .min(DT_MAX);
        let k = 1. - (-dt / TAU).exp();
        let mut moving = false;
        for (ix, slot) in self.slots.iter_mut().enumerate() {
            let timed = frame.rows.get(ix).is_some_and(|row| row.time_ms.is_some());
            let target = if frame.following && timed && !(frame.lit)(ix) {
                let distance = frame.active.map_or(ix + 1, |active| ix.abs_diff(active));
                (distance as f32 * STEP).min(MAX_SIGMA)
            } else {
                0.
            };
            let next = slot.sigma + (target - slot.sigma) * k;
            if (target - next).abs() < EPS {
                slot.sigma = target;
            } else {
                slot.sigma = next;
                moving = true;
            }
        }
        if moving {
            self.tick = Some(now);
            window.request_animation_frame();
        } else {
            self.tick = None;
        }
    }

    fn schedule(
        &mut self,
        style: BlurStyle,
        frame: &Frame,
        window: &mut Window,
        cx: &mut Context<Self>,
    ) {
        if self.baking || !self.dirty {
            return;
        }
        self.dirty = false;
        let Some(raster) = cx
            .try_global::<GlyphRaster>()
            .map(|raster| raster.0.clone())
        else {
            return;
        };
        let mut stale = Vec::new();
        for (ix, (row, slot)) in frame.rows.iter().zip(&self.slots).enumerate() {
            let measured = slot.measured;
            if row.text.is_empty() || measured.width <= px(0.) || measured.height <= px(0.) {
                continue;
            }
            let key = Key {
                text: row.text.clone(),
                measured,
                style,
            };
            if slot.key.as_ref() != Some(&key) {
                stale.push((ix, key));
            }
        }
        if stale.is_empty() {
            return;
        }
        let anchor = frame.active.unwrap_or(0);
        stale.sort_by_key(|(ix, _)| ix.abs_diff(anchor));
        stale.truncate(BATCH);
        let jobs: Vec<_> = stale
            .into_iter()
            .map(|(ix, key)| {
                let measured = key.measured;
                let glyphs = raster::line_glyphs(window, &key.text, measured.width, style.font)
                    .filter(|glyphs| (glyphs.size.height - measured.height).abs() <= FIT);
                (ix, key, glyphs)
            })
            .collect();
        self.baking = true;
        let seq = self.seq;
        let mut cache = mem::take(&mut self.glyphs);
        let baking = cx.background_spawn(async move {
            let baked: Vec<_> = jobs
                .into_iter()
                .map(|(ix, key, glyphs)| {
                    let line = glyphs.and_then(|glyphs| {
                        raster::bake(
                            raster.as_ref(),
                            &mut cache,
                            &glyphs,
                            key.style.color,
                            key.style.scale,
                        )
                    });
                    (ix, key, line.map(Arc::new))
                })
                .collect();
            (baked, cache)
        });
        self._task = Some(cx.spawn(async move |this, cx| {
            let (baked, cache) = baking.await;
            this.update(cx, |this, cx| {
                if this.seq != seq {
                    return;
                }
                this.baking = false;
                this.dirty = true;
                if this.style == Some(style) {
                    this.glyphs = cache;
                }
                for (ix, key, line) in baked {
                    let Some(slot) = this.slots.get_mut(ix) else {
                        continue;
                    };
                    if let Some(old) = slot.line.take() {
                        release_line(&old, cx);
                    }
                    slot.key = Some(key);
                    slot.line = line;
                }
                cx.notify();
            })
            .ok();
        }));
    }
}

pub fn measure(entity: Entity<LyricsBlur>, ix: usize) -> impl IntoElement {
    canvas(
        move |bounds, window, cx| {
            let view = window.content_mask().bounds;
            let far = !bounds.intersects(&view.dilate(view.size.height * NEAR));
            let (resized, was_far) = match entity.read(cx).slots.get(ix) {
                Some(slot) => (slot.measured != bounds.size, slot.far),
                None => return,
            };
            if !resized && was_far == far {
                return;
            }
            let redraw = entity.update(cx, |this, _| {
                if let Some(slot) = this.slots.get_mut(ix) {
                    slot.far = far;
                    if resized {
                        slot.measured = bounds.size;
                        this.dirty = true;
                    }
                }
                (resized || (was_far && !far)) && !mem::replace(&mut this.redraw_pending, true)
            });
            if redraw {
                window.on_next_frame(move |_, cx| {
                    entity.update(cx, |this, cx| {
                        this.redraw_pending = false;
                        cx.notify();
                    });
                });
            }
        },
        |_, _, _, _| {},
    )
    .absolute()
    .top_0()
    .left_0()
    .size_full()
}

fn layer(line: Arc<BlurredLine>, level: usize, opacity: f32) -> Div {
    div()
        .absolute()
        .top_0()
        .left_0()
        .size_full()
        .opacity(opacity)
        .child(
            canvas(
                |_, _, _| {},
                move |bounds, _, window, _| line.paint(level, bounds.origin, window),
            )
            .absolute()
            .top_0()
            .left_0()
            .size_full(),
        )
}

fn release_line(line: &BlurredLine, cx: &mut App) {
    for image in line.images() {
        drop_atlas_tile(image, cx);
    }
}
