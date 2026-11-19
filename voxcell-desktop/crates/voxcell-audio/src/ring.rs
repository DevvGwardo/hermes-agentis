//! Lock-free single-producer / single-consumer ring buffer for mic samples.
//!
//! The `cpal` callback thread is the sole producer; the engine processing
//! tick is the sole consumer. No locks, no allocation on the audio path.
//! Overflow policy: drop newest samples (keeps latency bounded) and count
//! them in [`MicRing::dropped`].

use std::cell::UnsafeCell;
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};

#[repr(align(64))]
struct CachePadded<T>(T);

pub struct MicRing {
    buf: UnsafeCell<Box<[f32]>>,
    mask: usize,
    head: CachePadded<AtomicUsize>, // producer write cursor (monotonic)
    tail: CachePadded<AtomicUsize>, // consumer read cursor (monotonic)
    dropped: AtomicU64,
}

// SAFETY: single-producer / single-consumer. The producer only writes free
// slots (head .. tail + capacity), the consumer only reads filled slots
// (tail .. head); the two regions are disjoint, and Acquire/Release ordering
// on head/tail publishes writes before the cursor advances. No slot is ever
// accessed by both sides at once, so sharing the buffer across the producer
// and consumer threads is sound.
unsafe impl Send for MicRing {}
unsafe impl Sync for MicRing {}

impl MicRing {
    /// Capacity is rounded up to a power of two (minimum 64 samples).
    pub fn new(capacity: usize) -> Self {
        let cap = capacity.max(64).next_power_of_two();
        Self {
            buf: UnsafeCell::new(vec![0.0; cap].into_boxed_slice()),
            mask: cap - 1,
            head: CachePadded(AtomicUsize::new(0)),
            tail: CachePadded(AtomicUsize::new(0)),
            dropped: AtomicU64::new(0),
        }
    }

    pub fn capacity(&self) -> usize {
        // Capacity is always a power of two; mask = capacity - 1.
        self.mask + 1
    }

    /// Producer side. Writes as many samples as fit; returns the number
    /// written. Samples that do not fit are dropped (newest) and counted.
    pub fn push(&self, samples: &[f32]) -> usize {
        let head = self.head.0.load(Ordering::Relaxed);
        let tail = self.tail.0.load(Ordering::Acquire);
        let used = head.wrapping_sub(tail);
        let free = self.capacity().saturating_sub(used);
        let n = samples.len().min(free);
        for (i, &s) in samples.iter().take(n).enumerate() {
            // SAFETY: slot is in the free region, exclusive to the producer.
            unsafe { (*self.buf.get())[(head.wrapping_add(i)) & self.mask] = s; }
        }
        self.head.0.store(head.wrapping_add(n), Ordering::Release);
        let lost = samples.len() - n;
        if lost > 0 {
            self.dropped.fetch_add(lost as u64, Ordering::Relaxed);
        }
        n
    }

    /// Consumer side. Reads up to `out.len()` samples; returns count read.
    pub fn pop(&self, out: &mut [f32]) -> usize {
        let tail = self.tail.0.load(Ordering::Relaxed);
        let head = self.head.0.load(Ordering::Acquire);
        let available = head.wrapping_sub(tail);
        let n = out.len().min(available);
        for (i, slot) in out.iter_mut().take(n).enumerate() {
            // SAFETY: slot is in the filled region, exclusive to the consumer.
            *slot = unsafe { (*self.buf.get())[(tail.wrapping_add(i)) & self.mask] };
        }
        self.tail.0.store(tail.wrapping_add(n), Ordering::Release);
        n
    }

    pub fn len(&self) -> usize {
        self.head
            .0
            .load(Ordering::Acquire)
            .wrapping_sub(self.tail.0.load(Ordering::Acquire))
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// Total samples dropped due to overflow since creation.
    pub fn dropped(&self) -> u64 {
        self.dropped.load(Ordering::Relaxed)
    }

    /// Consumer-side reset (e.g. when listening is disabled). Must only be
    /// called while the producer is paused or from the consumer thread.
    pub fn clear(&self) {
        let head = self.head.0.load(Ordering::Acquire);
        self.tail.0.store(head, Ordering::Release);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    #[test]
    fn roundtrips_samples_in_order() {
        let ring = MicRing::new(128);
        let input: Vec<f32> = (0..64).map(|i| i as f32 * 0.01).collect();
        assert_eq!(ring.push(&input), 64);
        assert_eq!(ring.len(), 64);
        let mut out = vec![0.0; 64];
        assert_eq!(ring.pop(&mut out), 64);
        assert_eq!(out, input);
        assert!(ring.is_empty());
    }

    #[test]
    fn capacity_rounds_up_to_power_of_two() {
        assert_eq!(MicRing::new(100).capacity(), 128);
        assert_eq!(MicRing::new(1).capacity(), 64);
        assert_eq!(MicRing::new(1024).capacity(), 1024);
    }

    #[test]
    fn overflow_drops_newest_and_counts() {
        let ring = MicRing::new(64);
        let block = vec![1.0; 64];
        assert_eq!(ring.push(&block), 64);
        // Full: nothing fits, all 10 dropped.
        assert_eq!(ring.push(&vec![2.0; 10]), 0);
        assert_eq!(ring.dropped(), 10);
        // Partial fit after draining 5.
        let mut tmp = vec![0.0; 5];
        assert_eq!(ring.pop(&mut tmp), 5);
        assert_eq!(ring.push(&vec![3.0; 10]), 5);
        assert_eq!(ring.dropped(), 15);
        // Oldest samples survived (drop-newest policy): 64 - 5 drained = 59
        // originals at indices 0..58, then the 5 newly written samples.
        let mut all = vec![0.0; 64];
        assert_eq!(ring.pop(&mut all), 64);
        assert_eq!(all[0], 1.0);
        assert_eq!(all[58], 1.0);
        assert_eq!(all[59], 3.0);
    }

    #[test]
    fn wraparound_preserves_order() {
        let ring = MicRing::new(64);
        let mut expected = Vec::new();
        let mut got = Vec::new();
        for round in 0..10 {
            let chunk: Vec<f32> = (0..40).map(|i| (round * 40 + i) as f32).collect();
            expected.extend_from_slice(&chunk);
            assert_eq!(ring.push(&chunk), 40);
            let mut out = vec![0.0; 40];
            assert_eq!(ring.pop(&mut out), 40);
            got.extend_from_slice(&out);
        }
        assert_eq!(got, expected);
        assert_eq!(ring.dropped(), 0);
    }

    #[test]
    fn clear_empties_without_touching_producer_cursor() {
        let ring = MicRing::new(64);
        ring.push(&[1.0, 2.0, 3.0]);
        ring.clear();
        assert!(ring.is_empty());
        ring.push(&[4.0]);
        let mut out = [0.0; 1];
        assert_eq!(ring.pop(&mut out), 1);
        assert_eq!(out[0], 4.0);
    }

    #[test]
    fn concurrent_spsc_producer_consumer_lossless_when_throttled() {
        let ring = Arc::new(MicRing::new(4096));
        let total = 100_000usize;
        let producer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut sent = 0usize;
                while sent < total {
                    // Truncate the last chunk so exactly `total` samples are
                    // pushed (100_000 is not a multiple of 256).
                    let take = (total - sent).min(256);
                    let chunk: Vec<f32> = (0..take).map(|i| (sent + i) as f32).collect();
                    let mut off = 0;
                    while off < chunk.len() {
                        // Throttle to free space: only offer samples that
                        // fit. push() counts non-fitting samples as drops
                        // even if the caller retries them, so a blind
                        // push-and-retry loop would inflate `dropped`.
                        // Space can only grow between len() and push()
                        // (the consumer is the sole other cursor writer).
                        let free = ring.capacity() - ring.len();
                        if free == 0 {
                            std::thread::yield_now();
                            continue;
                        }
                        let take = free.min(chunk.len() - off);
                        let n = ring.push(&chunk[off..off + take]);
                        off += n;
                    }
                    sent += chunk.len();
                }
            })
        };
        let consumer = {
            let ring = Arc::clone(&ring);
            std::thread::spawn(move || {
                let mut received = Vec::with_capacity(total);
                let mut buf = vec![0.0; 512];
                while received.len() < total {
                    let n = ring.pop(&mut buf);
                    received.extend_from_slice(&buf[..n]);
                    if n == 0 {
                        std::thread::yield_now();
                    }
                }
                received
            })
        };
        producer.join().unwrap();
        let received = consumer.join().unwrap();
        assert_eq!(received.len(), total);
        for (i, &s) in received.iter().enumerate() {
            assert_eq!(s, i as f32, "sample {i} out of order");
        }
        assert_eq!(ring.dropped(), 0);
    }
}
