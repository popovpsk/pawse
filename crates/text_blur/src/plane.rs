#[derive(Debug, Clone, PartialEq)]
pub struct Plane {
    pub data: Vec<f32>,
    pub width: usize,
    pub height: usize,
}

impl Plane {
    pub fn new(width: usize, height: usize) -> Plane {
        Plane {
            data: vec![0.; width * height],
            width,
            height,
        }
    }

    pub fn row(&self, y: usize) -> &[f32] {
        &self.data[y * self.width..(y + 1) * self.width]
    }

    pub fn ink(&self) -> f32 {
        self.data.iter().sum()
    }

    pub fn halved(&self) -> Plane {
        let mut out = Plane::new(self.width / 2, self.height / 2);
        if out.width == 0 {
            return out;
        }
        let span = out.width * 2;
        for (y, dst) in out.data.chunks_exact_mut(out.width).enumerate() {
            let (top, _) = self.data[2 * y * self.width..][..span].as_chunks::<2>();
            let (bottom, _) = self.data[(2 * y + 1) * self.width..][..span].as_chunks::<2>();
            for ((o, a), b) in dst.iter_mut().zip(top).zip(bottom) {
                *o = (a[0] + a[1] + b[0] + b[1]) * 0.25;
            }
        }
        out
    }

    pub fn padded(&self, pad: usize) -> Plane {
        let mut out = Plane::new(self.width + 2 * pad, self.height + 2 * pad);
        if self.width == 0 {
            return out;
        }
        for (y, src) in self.data.chunks_exact(self.width).enumerate() {
            let start = (y + pad) * out.width + pad;
            out.data[start..start + self.width].copy_from_slice(src);
        }
        out
    }

    pub fn gaussian(&mut self, sigma: f32) {
        if sigma <= 0. || self.width == 0 || self.height == 0 {
            return;
        }
        let kernel = kernel(sigma);
        let radius = kernel.len() / 2;
        let width = self.width;

        let mut tmp = vec![0f32; self.data.len()];
        let mut live = vec![false; self.height];
        for ((src, dst), live) in self
            .data
            .chunks_exact(width)
            .zip(tmp.chunks_exact_mut(width))
            .zip(&mut live)
        {
            if src.iter().any(|&v| v != 0.) {
                *live = true;
                convolve_row(src, dst, &kernel, radius);
            }
        }

        self.data.fill(0.);
        for (y, dst) in self.data.chunks_exact_mut(width).enumerate() {
            for (k, &w) in kernel.iter().enumerate() {
                let Some(sy) = (y + k).checked_sub(radius) else {
                    continue;
                };
                if !live.get(sy).copied().unwrap_or(false) {
                    continue;
                }
                axpy(dst, &tmp[sy * width..(sy + 1) * width], w);
            }
        }
    }

    pub fn to_bgra(&self, rgba: [f32; 4]) -> Vec<u8> {
        let channel = |v: f32| (v.clamp(0., 1.) * 255.).round() as u8;
        let (width, height) = (self.width + 2, self.height + 2);
        let mut bytes =
            [channel(rgba[2]), channel(rgba[1]), channel(rgba[0]), 0].repeat(width * height);
        if self.width == 0 {
            return bytes;
        }
        let alpha = rgba[3];
        for (y, src) in self.data.chunks_exact(self.width).enumerate() {
            let start = ((y + 1) * width + 1) * 4;
            let row = &mut bytes[start..start + self.width * 4];
            for (dst, &v) in row.as_chunks_mut::<4>().0.iter_mut().zip(src) {
                dst[3] = channel(v * alpha);
            }
        }
        bytes
    }
}

pub fn overlap(sharper: &Plane, blurrier: &Plane, dx: usize, dy: usize) -> f32 {
    debug_assert!(dx + sharper.width <= blurrier.width);
    debug_assert!(dy + sharper.height <= blurrier.height);
    const LANES: usize = 8;
    let (mut both, mut ink) = (0f64, 0f64);
    for y in 0..sharper.height {
        let a = sharper.row(y);
        let b = &blurrier.row(y + dy)[dx..dx + sharper.width];
        let (mut ab_lanes, mut a_lanes) = ([0f32; LANES], [0f32; LANES]);
        let (a_chunks, a_rest) = a.as_chunks::<LANES>();
        let (b_chunks, b_rest) = b.as_chunks::<LANES>();
        for (ca, cb) in a_chunks.iter().zip(b_chunks) {
            for i in 0..LANES {
                ab_lanes[i] += ca[i] * cb[i];
                a_lanes[i] += ca[i];
            }
        }
        for (va, vb) in a_rest.iter().zip(b_rest) {
            ab_lanes[0] += va * vb;
            a_lanes[0] += va;
        }
        both += ab_lanes.iter().sum::<f32>() as f64;
        ink += a_lanes.iter().sum::<f32>() as f64;
    }
    if ink <= 0. { 0. } else { (both / ink) as f32 }
}

fn kernel(sigma: f32) -> Vec<f32> {
    let radius = (sigma * 3.).ceil() as isize;
    let mut kernel: Vec<f32> = (-radius..=radius)
        .map(|i| (-((i * i) as f32) / (2. * sigma * sigma)).exp())
        .collect();
    let sum: f32 = kernel.iter().sum();
    kernel.iter_mut().for_each(|w| *w /= sum);
    kernel
}

fn convolve_row(src: &[f32], dst: &mut [f32], kernel: &[f32], radius: usize) {
    let width = src.len();
    for (k, &w) in kernel.iter().enumerate() {
        if k >= radius {
            let d = k - radius;
            if d < width {
                axpy(&mut dst[..width - d], &src[d..], w);
            }
        } else {
            let d = radius - k;
            if d < width {
                axpy(&mut dst[d..], &src[..width - d], w);
            }
        }
    }
}

fn axpy(dst: &mut [f32], src: &[f32], w: f32) {
    for (o, i) in dst.iter_mut().zip(src) {
        *o += w * i;
    }
}
