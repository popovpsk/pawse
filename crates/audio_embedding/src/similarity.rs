use std::cmp::{Ordering, Reverse};
use std::collections::BinaryHeap;

pub const SCAN_CHUNK: usize = 1024;

pub struct MeanAccumulator {
    sum: Vec<f64>,
    count: u64,
}

impl MeanAccumulator {
    pub fn new(dim: usize) -> Self {
        Self {
            sum: vec![0.0; dim],
            count: 0,
        }
    }

    pub fn feed(&mut self, vectors: &[f32]) {
        for row in vectors.chunks_exact(self.sum.len()) {
            for (acc, &v) in self.sum.iter_mut().zip(row) {
                *acc += v as f64;
            }
            self.count += 1;
        }
    }

    pub fn count(&self) -> u64 {
        self.count
    }

    pub fn finish(self) -> Box<[f32]> {
        let n = self.count.max(1) as f64;
        self.sum.into_iter().map(|s| (s / n) as f32).collect()
    }
}

#[derive(Debug, Clone, Copy)]
struct Scored {
    score: f32,
    id: i64,
}

impl PartialEq for Scored {
    fn eq(&self, other: &Self) -> bool {
        self.cmp(other) == Ordering::Equal
    }
}

impl Eq for Scored {}

impl PartialOrd for Scored {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Scored {
    fn cmp(&self, other: &Self) -> Ordering {
        self.score
            .total_cmp(&other.score)
            .then_with(|| other.id.cmp(&self.id))
    }
}

pub fn centered_cosine(x: &[f32], seed_centered: &[f32], seed_norm: f32, mean: &[f32]) -> f32 {
    let mut dot = 0f32;
    let mut norm = 0f32;
    for ((&v, &m), &s) in x.iter().zip(mean).zip(seed_centered) {
        let c = v - m;
        dot += c * s;
        norm += c * c;
    }
    let denom = norm.sqrt() * seed_norm;
    if denom > 0.0 { dot / denom } else { 0.0 }
}

pub struct TopN {
    exclude: i64,
    seed: Box<[f32]>,
    seed_norm: f32,
    mean: Box<[f32]>,
    n: usize,
    heap: BinaryHeap<Reverse<Scored>>,
}

impl TopN {
    pub fn new(seed_id: i64, seed: &[f32], mean: &[f32], n: usize) -> Self {
        let seed: Box<[f32]> = seed.iter().zip(mean).map(|(s, m)| s - m).collect();
        let seed_norm = seed.iter().map(|v| v * v).sum::<f32>().sqrt();
        Self {
            exclude: seed_id,
            seed,
            seed_norm,
            mean: mean.into(),
            n,
            heap: BinaryHeap::with_capacity(n + 1),
        }
    }

    pub fn feed(&mut self, ids: &[i64], vectors: &[f32]) {
        if self.n == 0 {
            return;
        }
        for (&id, row) in ids.iter().zip(vectors.chunks_exact(self.seed.len())) {
            if id == self.exclude {
                continue;
            }
            let candidate = Scored {
                score: centered_cosine(row, &self.seed, self.seed_norm, &self.mean),
                id,
            };
            if self.heap.len() < self.n {
                self.heap.push(Reverse(candidate));
            } else if self
                .heap
                .peek()
                .is_some_and(|Reverse(worst)| candidate > *worst)
            {
                self.heap.pop();
                self.heap.push(Reverse(candidate));
            }
        }
    }

    pub fn finish(self) -> Vec<(i64, f32)> {
        self.heap
            .into_sorted_vec()
            .into_iter()
            .map(|Reverse(s)| (s.id, s.score))
            .collect()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use rstest::rstest;

    fn random(n: usize, dim: usize, seed: u64) -> Vec<f32> {
        let mut state = seed;
        (0..n * dim)
            .map(|_| {
                state = state
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                ((state >> 40) as f32 / (1u64 << 24) as f32) * 2.0 - 1.0 + 0.7
            })
            .collect()
    }

    #[test]
    fn the_mean_streams_over_chunks() {
        let dim = 5;
        let vectors = random(37, dim, 1);
        let mut whole = MeanAccumulator::new(dim);
        whole.feed(&vectors);
        let mut chunked = MeanAccumulator::new(dim);
        for chunk in vectors.chunks(4 * dim) {
            chunked.feed(chunk);
        }
        assert_eq!(chunked.count(), 37);
        let expected: Vec<f32> = (0..dim)
            .map(|d| {
                (vectors
                    .iter()
                    .skip(d)
                    .step_by(dim)
                    .map(|&v| v as f64)
                    .sum::<f64>()
                    / 37.0) as f32
            })
            .collect();
        assert_eq!(&*whole.finish(), expected.as_slice());
        assert_eq!(&*chunked.finish(), expected.as_slice());
    }

    #[test]
    fn an_empty_mean_is_zero() {
        assert_eq!(&*MeanAccumulator::new(3).finish(), &[0.0, 0.0, 0.0]);
    }

    #[rstest]
    #[case::one_row(1)]
    #[case::odd(7)]
    #[case::bigger_than_everything(1000)]
    fn top_n_in_chunks_equals_sorting_every_cosine(#[case] chunk: usize) {
        let dim = 16;
        let count = 300;
        let vectors = random(count, dim, 42);
        let ids: Vec<i64> = (0..count as i64).map(|i| i * 3 + 1).collect();
        let mut mean = MeanAccumulator::new(dim);
        mean.feed(&vectors);
        let mean = mean.finish();
        let seed_id = ids[17];
        let seed = &vectors[17 * dim..18 * dim];

        let centered: Vec<f32> = seed.iter().zip(mean.iter()).map(|(s, m)| s - m).collect();
        let norm = centered.iter().map(|v| v * v).sum::<f32>().sqrt();
        let mut expected: Vec<(i64, f32)> = ids
            .iter()
            .zip(vectors.chunks_exact(dim))
            .filter(|(id, _)| **id != seed_id)
            .map(|(&id, row)| (id, centered_cosine(row, &centered, norm, &mean)))
            .collect();
        expected.sort_by(|a, b| b.1.total_cmp(&a.1).then(a.0.cmp(&b.0)));
        expected.truncate(25);

        let mut top = TopN::new(seed_id, seed, &mean, 25);
        for (id_chunk, vec_chunk) in ids.chunks(chunk).zip(vectors.chunks(chunk * dim)) {
            top.feed(id_chunk, vec_chunk);
        }
        assert_eq!(top.finish(), expected);
    }

    #[test]
    fn centering_turns_a_shared_offset_into_contrast() {
        let mean = [10.0, 10.0];
        let seed = [11.0, 10.0];
        let mut top = TopN::new(0, &seed, &mean, 2);
        top.feed(&[1, 2], &[12.0, 10.0, 9.0, 10.0]);
        let found = top.finish();
        assert_eq!(found[0].0, 1);
        assert!((found[0].1 - 1.0).abs() < 1e-6);
        assert_eq!(found[1].0, 2);
        assert!((found[1].1 + 1.0).abs() < 1e-6);
    }

    #[test]
    fn a_vector_at_the_mean_scores_zero_instead_of_nan() {
        let mean = [1.0, 2.0];
        let mut top = TopN::new(0, &[2.0, 2.0], &mean, 5);
        top.feed(&[7], &[1.0, 2.0]);
        assert_eq!(top.finish(), vec![(7, 0.0)]);
    }

    #[test]
    fn asking_for_nothing_returns_nothing() {
        let mut top = TopN::new(0, &[1.0], &[0.0], 0);
        top.feed(&[1], &[1.0]);
        assert!(top.finish().is_empty());
    }
}
