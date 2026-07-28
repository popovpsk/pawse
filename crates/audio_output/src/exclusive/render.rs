use std::sync::Arc;
use std::sync::atomic::{AtomicU8, Ordering};

use crate::cpal_stream::{FadeState, apply_fade_gain};
use crate::ring_buffer::AudioRingBuffer;

pub(crate) const STATE_IDLE: u8 = 0;
pub(crate) const STATE_PLAYING: u8 = 1;

/// State shared between the ring-buffer writer (`Backend::write`) and the
/// platform render path (macOS IOProc callback, or the Windows/Linux render
/// thread). Everything here must be lock-free.
pub(crate) struct RenderCtx {
    pub(crate) buffer: Arc<AudioRingBuffer>,
    pub(crate) playing: AtomicU8,
    /// Fade envelope; shared with `apply_fade_gain` (same logic as shared mode).
    pub(crate) fade: FadeState,
    pub(crate) sample_rate: u32,
    pub(crate) channels: u8,
}

/// Fills `out` (an interleaved f32 output slice) with the next block of audio,
/// applying any active fade ramp. App volume is deliberately NOT part of this
/// path: exclusive mode locks the in-app slider, so the digital gain stays at
/// unity and the samples reach the device untouched.
///
/// Emits silence when not playing. When the fade is frozen (post fade-out) it
/// also emits silence but leaves the ring buffer intact, so a later resume can
/// fade the same samples back in seamlessly. The near-unity skip inside
/// `apply_fade_gain` makes the steady state (no fade running) a no-op.
pub(crate) fn fill(ctx: &RenderCtx, out: &mut [f32]) {
    if ctx.playing.load(Ordering::Relaxed) != STATE_PLAYING || ctx.fade.is_frozen() {
        for s in out.iter_mut() {
            *s = 0.0;
        }
        return;
    }

    let read = ctx.buffer.pop_slice(out);

    apply_fade_gain(&ctx.fade, 1.0, ctx.channels as usize, &mut out[..read]);

    for s in &mut out[read..] {
        *s = 0.0;
    }
}
