use std::sync::atomic::{AtomicBool, AtomicU32, AtomicU64, Ordering};
use std::time::{Duration, Instant};

const CAPACITY: usize = 1 << 18;
const MASK: u64 = CAPACITY as u64 - 1;
const STALE_AFTER: Duration = Duration::from_millis(60);

pub struct SignalTap {
    ring: Box<[AtomicU32]>,
    written: AtomicU64,
    flushed: AtomicU64,
    block_start: AtomicU64,
    block_heard_ns: AtomicU64,
    rate: AtomicU32,
    channels: AtomicU32,
    latency_ns: AtomicU64,
    active: AtomicBool,
    epoch: Instant,
}

impl Default for SignalTap {
    fn default() -> Self {
        Self::new()
    }
}

impl SignalTap {
    pub fn new() -> Self {
        Self {
            ring: (0..CAPACITY).map(|_| AtomicU32::new(0)).collect(),
            written: AtomicU64::new(0),
            flushed: AtomicU64::new(0),
            block_start: AtomicU64::new(0),
            block_heard_ns: AtomicU64::new(0),
            rate: AtomicU32::new(0),
            channels: AtomicU32::new(2),
            latency_ns: AtomicU64::new(0),
            active: AtomicBool::new(false),
            epoch: Instant::now(),
        }
    }

    pub fn capacity() -> usize {
        CAPACITY
    }

    pub fn set_active(&self, active: bool) {
        self.active.store(active, Ordering::Relaxed);
    }

    pub fn is_active(&self) -> bool {
        self.active.load(Ordering::Relaxed)
    }

    pub fn set_format(&self, sample_rate: u32, channels: u16) {
        self.rate.store(sample_rate, Ordering::Relaxed);
        self.channels
            .store(u32::from(channels.max(1)), Ordering::Relaxed);
    }

    pub fn set_latency(&self, latency: Duration) {
        self.latency_ns.store(
            u64::try_from(latency.as_nanos()).unwrap_or(u64::MAX),
            Ordering::Relaxed,
        );
    }

    pub fn sample_rate(&self) -> u32 {
        self.rate.load(Ordering::Relaxed)
    }

    pub fn flush(&self) {
        self.flushed
            .store(self.written.load(Ordering::Acquire), Ordering::Release);
    }

    pub fn latency(&self) -> Duration {
        Duration::from_nanos(self.latency_ns.load(Ordering::Relaxed))
    }

    fn now_ns(&self) -> u64 {
        u64::try_from(self.epoch.elapsed().as_nanos()).unwrap_or(u64::MAX)
    }

    pub fn push(&self, interleaved: &[f32]) {
        if !self.active.load(Ordering::Relaxed) {
            return;
        }
        let channels = self.channels.load(Ordering::Relaxed).max(1) as usize;
        let frames = interleaved.len() / channels;
        if frames == 0 {
            return;
        }
        let heard = self
            .now_ns()
            .saturating_add(self.latency_ns.load(Ordering::Relaxed));
        let start = self.written.load(Ordering::Relaxed);
        let scale = 1.0 / channels as f32;
        for (i, frame) in interleaved.chunks_exact(channels).enumerate() {
            let mono = frame.iter().sum::<f32>() * scale;
            self.ring[((start + i as u64) & MASK) as usize]
                .store(mono.to_bits(), Ordering::Relaxed);
        }
        self.block_start.store(start, Ordering::Relaxed);
        self.block_heard_ns.store(heard, Ordering::Relaxed);
        self.written.store(start + frames as u64, Ordering::Release);
    }

    pub fn read(&self, out: &mut [f32]) -> Option<u32> {
        let written = self.written.load(Ordering::Acquire);
        let start = self.block_start.load(Ordering::Relaxed);
        let heard_ns = self.block_heard_ns.load(Ordering::Relaxed);
        let rate = self.rate.load(Ordering::Relaxed);
        if written == 0 || rate == 0 {
            return None;
        }
        let per_sec = i128::from(rate);
        let since = i128::from(self.now_ns()) - i128::from(heard_ns);
        let heard = i128::from(start) + since * per_sec / 1_000_000_000;
        let stale = per_sec * STALE_AFTER.as_nanos() as i128 / 1_000_000_000;
        if heard > i128::from(written) + stale {
            return None;
        }
        let end = heard.clamp(0, i128::from(written));
        if end <= i128::from(self.flushed.load(Ordering::Acquire)) {
            return None;
        }
        let oldest = i128::from(written) - (CAPACITY as i128 - out.len() as i128).max(0);
        let first = end - out.len() as i128;
        for (i, slot) in out.iter_mut().enumerate() {
            let ix = first + i as i128;
            *slot = if ix < 0 || ix < oldest {
                0.0
            } else {
                f32::from_bits(self.ring[(ix as u64 & MASK) as usize].load(Ordering::Relaxed))
            };
        }
        Some(rate)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn active_tap(rate: u32, channels: u16) -> SignalTap {
        let tap = SignalTap::new();
        tap.set_format(rate, channels);
        tap.set_active(true);
        tap
    }

    #[test]
    fn inactive_tap_records_nothing() {
        let tap = SignalTap::new();
        tap.set_format(48_000, 2);
        tap.push(&[0.5; 64]);
        let mut out = [1.0f32; 8];
        assert_eq!(tap.read(&mut out), None);
    }

    #[test]
    fn mixes_channels_to_mono_and_ends_where_the_newest_block_starts() {
        let tap = active_tap(48_000, 2);
        let samples: Vec<f32> = (0..16).map(|i| i as f32).collect();
        tap.push(&samples);
        tap.push(&[0.0; 2]);
        let mut out = [0.0f32; 4];
        assert_eq!(tap.read(&mut out), Some(48_000));
        assert_eq!(out, [8.5, 10.5, 12.5, 14.5]);
    }

    #[test]
    fn a_short_history_is_padded_with_silence_in_front() {
        let tap = active_tap(48_000, 1);
        tap.push(&[1.0, 2.0]);
        tap.push(&[3.0]);
        let mut out = [9.0f32; 4];
        tap.read(&mut out);
        assert_eq!(out, [0.0, 0.0, 1.0, 2.0]);
    }

    #[test]
    fn latency_moves_the_window_back_to_what_is_heard() {
        let tap = active_tap(1_000, 1);
        let earlier: Vec<f32> = vec![1.0; 500];
        tap.push(&earlier);
        tap.set_latency(Duration::from_millis(200));
        tap.push(&[2.0; 100]);
        let mut out = [0.0f32; 10];
        tap.read(&mut out);
        assert!(out.iter().all(|&s| s == 1.0), "{out:?}");
    }

    #[test]
    fn goes_silent_once_nothing_new_is_pushed() {
        let tap = active_tap(1_000, 1);
        tap.push(&[1.0; 10]);
        std::thread::sleep(STALE_AFTER + Duration::from_millis(40));
        let mut out = [0.0f32; 4];
        assert_eq!(tap.read(&mut out), None);
    }

    #[test]
    fn flushed_frames_are_never_heard() {
        let tap = active_tap(1_000, 1);
        tap.set_latency(Duration::from_secs(2));
        tap.push(&[1.0; 100]);
        tap.flush();
        let mut out = [0.0f32; 4];
        assert_eq!(tap.read(&mut out), None);
        tap.set_latency(Duration::ZERO);
        tap.push(&[2.0; 10]);
        tap.push(&[3.0; 1]);
        assert_eq!(tap.read(&mut out), Some(1_000));
        assert_eq!(out, [2.0; 4]);
    }

    #[test]
    fn wraps_around_the_ring() {
        let tap = active_tap(48_000, 1);
        let block: Vec<f32> = vec![0.25; 4096];
        for _ in 0..(CAPACITY / 4096 + 3) {
            tap.push(&block);
        }
        tap.push(&[0.75; 4]);
        tap.push(&[0.0]);
        let mut out = [0.0f32; 6];
        tap.read(&mut out);
        assert_eq!(out, [0.25, 0.25, 0.75, 0.75, 0.75, 0.75]);
    }
}
