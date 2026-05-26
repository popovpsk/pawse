use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use atomic_float::AtomicF32;

use crate::cpal_stream::{FadeState, apply_fade_gain};
use crate::ring_buffer::AudioRingBuffer;

pub(crate) const STATE_IDLE: u8 = 0;
pub(crate) const STATE_PLAYING: u8 = 1;

/// State shared between the ring-buffer writer (`Backend::write`) and the
/// platform render path (macOS IOProc callback, or the Windows/Linux render
/// thread). Everything here must be lock-free.
pub(crate) struct RenderCtx {
    pub(crate) buffer: Arc<AudioRingBuffer>,
    pub(crate) volume: AtomicF32,
    pub(crate) playing: AtomicU8,
    /// Fade envelope; shared with `apply_fade_gain` (same logic as shared mode).
    pub(crate) fade: FadeState,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u8,
}

/// Fills `out` (an interleaved f32 output slice) with the next block of audio,
/// applying app volume and any active fade ramp.
///
/// Emits silence when not playing. When the fade is frozen (post fade-out) it
/// also emits silence but leaves the ring buffer intact, so a later resume can
/// fade the same samples back in seamlessly. The near-unity skip inside
/// `apply_fade_gain` keeps output bit-perfect when volume is 1.0 and no fade is
/// active.
pub(crate) fn fill(ctx: &RenderCtx, out: &mut [f32]) {
    if ctx.playing.load(Ordering::Relaxed) != STATE_PLAYING || ctx.fade.is_frozen() {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        return;
    }

    let read = ctx.buffer.pop_slice(out);

    let vol = ctx.volume.load(Ordering::Relaxed);
    apply_fade_gain(&ctx.fade, vol, ctx.channels as usize, &mut out[..read]);

    for s in &mut out[read..] {
        *s = 0.0;
    }
}
