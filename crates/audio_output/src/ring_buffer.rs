use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};

use audio_common::AudioSamples;
use parking_lot::Mutex;
use rb::{RB, RbConsumer, RbError, RbInspector, RbProducer, SpscRb};

use crate::tap::SignalTap;

/// Lock-free SPSC ring buffer for audio samples
pub struct AudioRingBuffer {
    rb: SpscRb<f32>,
    scratch: Mutex<Vec<f32>>,
    unexpected_error_logged: AtomicBool,
    tap: Option<Arc<SignalTap>>,
}

impl AudioRingBuffer {
    pub fn new(capacity: usize) -> Self {
        let rb = SpscRb::new(capacity);
        Self {
            rb,
            scratch: Mutex::new(Vec::new()),
            unexpected_error_logged: AtomicBool::new(false),
            tap: None,
        }
    }

    pub fn with_tap(capacity: usize, tap: Arc<SignalTap>) -> Self {
        Self {
            tap: Some(tap),
            ..Self::new(capacity)
        }
    }

    /// Push samples into the buffer (producer side)
    /// Returns the number of samples actually pushed (may be less if buffer is full)
    pub fn push_slice(&self, samples: &[f32]) -> usize {
        self.rb.producer().write(samples).unwrap_or(0)
    }

    pub fn write_samples_blocking(&self, samples: &AudioSamples) -> usize {
        if let AudioSamples::F32(data) = samples {
            return self.write_slice_blocking(data);
        }
        let mut scratch = self.scratch.lock();
        samples.to_f32_into(&mut scratch);
        self.write_slice_blocking(&scratch)
    }

    pub fn write_slice_blocking(&self, samples: &[f32]) -> usize {
        let result = self
            .rb
            .producer()
            .write_blocking_timeout(samples, std::time::Duration::from_millis(10));
        match result {
            Ok(count) => count.unwrap_or_default(),
            Err(RbError::TimedOut) => 0,
            Err(_) => {
                if !self.unexpected_error_logged.swap(true, Ordering::Relaxed) {
                    log::error!(
                        "audio output: unexpected ring buffer write error; dropping samples"
                    );
                }
                0
            }
        }
    }

    /// Pop samples from the buffer (consumer side)
    /// Returns the number of samples actually popped (may be less if buffer is empty)
    pub fn pop_slice(&self, dest: &mut [f32]) -> usize {
        let read = self.rb.consumer().read(dest).unwrap_or(0);
        if let Some(tap) = &self.tap {
            tap.push(&dest[..read]);
        }
        read
    }

    /// Clear all samples from the buffer
    pub fn clear(&self) {
        self.rb.clear();
    }

    /// Returns the number of samples currently in the buffer
    pub fn len(&self) -> usize {
        self.rb.count()
    }

    /// Returns true if the buffer is empty
    pub fn is_empty(&self) -> bool {
        self.rb.is_empty()
    }

    /// Returns the capacity of the buffer
    pub fn capacity(&self) -> usize {
        self.rb.capacity()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_ring_buffer_push_pop() {
        let rb = AudioRingBuffer::new(100);

        let pushed = rb.push_slice(&[1.0, 2.0, 3.0, 4.0]);
        assert_eq!(pushed, 4);
        assert_eq!(rb.len(), 4);

        let mut dest = [0.0; 4];
        let popped = rb.pop_slice(&mut dest);
        assert_eq!(popped, 4);
        assert_eq!(dest, [1.0, 2.0, 3.0, 4.0]);
        assert!(rb.is_empty());
    }

    #[test]
    fn test_ring_buffer_clear() {
        let rb = AudioRingBuffer::new(100);
        rb.push_slice(&[1.0, 2.0, 3.0]);
        assert_eq!(rb.len(), 3);

        rb.clear();
        assert!(rb.is_empty());
    }

    #[test]
    fn test_ring_buffer_empty() {
        let rb = AudioRingBuffer::new(100);
        assert!(rb.is_empty());
        assert_eq!(rb.len(), 0);
    }

    #[test]
    fn write_samples_converts_and_reports_partial_writes() {
        let rb = AudioRingBuffer::new(3);
        let samples = AudioSamples::S16(vec![16384, -16384, 0, 32767]);
        assert_eq!(rb.write_samples_blocking(&samples), 3);

        let mut dest = [0.0; 3];
        assert_eq!(rb.pop_slice(&mut dest), 3);
        assert_eq!(dest, [0.5, -0.5, 0.0]);

        let mut rest = samples;
        rest.discard_front(3);
        assert_eq!(rb.write_samples_blocking(&rest), 1);
        let mut last = [0.0; 1];
        rb.pop_slice(&mut last);
        assert!((last[0] - 32767. / 32768.).abs() < f32::EPSILON);
    }

    #[test]
    fn write_samples_passes_f32_through() {
        let rb = AudioRingBuffer::new(8);
        let samples = AudioSamples::F32(vec![0.25, -0.75]);
        assert_eq!(rb.write_samples_blocking(&samples), 2);
        let mut dest = [0.0; 2];
        rb.pop_slice(&mut dest);
        assert_eq!(dest, [0.25, -0.75]);
    }
}
