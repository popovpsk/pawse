use std::sync::{Mutex, PoisonError, mpsc};
use std::time::Duration;

use audio_embedding::{BANDS, DIM, FRAME, PATCH, PATCH_SAMPLES, STYLES, hann_window, mel_filters};
use wgpu::util::DeviceExt;
use wgpu::{BindGroup, Buffer, BufferUsages, ComputePipeline};

use crate::arch::{
    BATCH, BLOCKS, HEAD_INPUT, INPUT, Layer, NETS, Net, Plane, Role, STEM, head_plane, planes,
    stem_plane,
};
use crate::gpu::Gpu;
use crate::input::{INFO_WORDS, PCM_CAP};
use crate::kernels::{Kernel, Kernels, depthwise_variant};
use crate::weights::{Slice, Weights};

const PARAM_STRIDE: u64 = 256;
const BINS: usize = FRAME / 2 + 1;
const FRAMES: usize = BATCH * PATCH;
const WAIT: Duration = Duration::from_secs(60);
#[cfg(test)]
pub(crate) const FRONTEND_STEPS: usize = 2;
pub(crate) const OUT_LEN: usize = BATCH * DIM;
pub(crate) const STYLES_LEN: usize = BATCH * STYLES;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) enum Buf {
    Weights(Slice),
    Dft,
    MelWeights,
    MelFilters,
    Info,
    M,
    P,
    Q,
    E,
    D,
    Pool,
    Gate,
    Out,
    Styles,
    Dummy,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Binding {
    Params(usize),
    Buf(Buf),
}

struct Step {
    pipeline: ComputePipeline,
    bind_group: BindGroup,
    groups: [u32; 3],
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(crate) struct Tap {
    pub(crate) after: usize,
    pub(crate) buffer: Buf,
    pub(crate) len: usize,
}

struct Pcm {
    buffer: Buffer,
    bind_group: BindGroup,
}

struct Buffers {
    weights: Option<Buffer>,
    dft: Buffer,
    mel_weights: Buffer,
    mel_filters: Buffer,
    info: Buffer,
    m: Buffer,
    p: Buffer,
    q: Buffer,
    e: Buffer,
    d: Buffer,
    pool: Buffer,
    gate: Buffer,
    out: Buffer,
    styles: Buffer,
    dummy: Buffer,
}

pub(crate) struct Engine {
    pub(crate) gpu: Gpu,
    resample: Kernel,
    buffers: Buffers,
    steps: Vec<Step>,
    pcm: Mutex<Pcm>,
    #[cfg(test)]
    pub(crate) block_taps: Vec<Vec<Tap>>,
}

pub(crate) struct Pending {
    index: wgpu::SubmissionIndex,
    readback: Vec<Buffer>,
    mapped: mpsc::Receiver<Result<(), wgpu::BufferAsyncError>>,
}

struct Spec {
    pipeline: ComputePipeline,
    layout: wgpu::BindGroupLayout,
    bindings: Vec<Binding>,
    groups: [u32; 3],
}

fn groups(n: usize, per: usize) -> u32 {
    n.div_ceil(per) as u32
}

fn storage(device: &wgpu::Device, label: &str, floats: usize) -> Buffer {
    device.create_buffer(&wgpu::BufferDescriptor {
        label: Some(label),
        size: (floats.max(1) * 4) as u64,
        usage: BufferUsages::STORAGE | BufferUsages::COPY_SRC | BufferUsages::COPY_DST,
        mapped_at_creation: false,
    })
}

fn init(device: &wgpu::Device, label: &str, bytes: &[u8], usage: BufferUsages) -> Buffer {
    device.create_buffer_init(&wgpu::util::BufferInitDescriptor {
        label: Some(label),
        contents: bytes,
        usage,
    })
}

pub(crate) fn dft_table() -> Vec<f32> {
    let window = hann_window();
    let mut table = vec![0f32; FRAME * 2 * BINS];
    for (k, &w) in window.iter().enumerate() {
        for bin in 0..BINS {
            let phase = 2.0 * std::f64::consts::PI * ((bin * k) % FRAME) as f64 / FRAME as f64;
            table[k * 2 * BINS + bin] = (w as f64 * phase.cos()) as f32;
            table[k * 2 * BINS + BINS + bin] = (w as f64 * phase.sin()) as f32;
        }
    }
    table
}

fn mel_tables() -> (Vec<f32>, Vec<u32>) {
    let mut weights = Vec::new();
    let mut index = Vec::new();
    for filter in mel_filters() {
        index.extend([
            filter.first as u32,
            filter.weights.len() as u32,
            weights.len() as u32,
        ]);
        weights.extend_from_slice(&filter.weights);
    }
    (weights, index)
}

struct Sizes {
    pq: usize,
    e: usize,
    d: usize,
    channels: usize,
}

fn sizes() -> Sizes {
    let mut pq = (BATCH * INPUT.area()).max(STEM * BATCH * stem_plane().area());
    let mut e = 2 * BINS * FRAMES;
    let mut d = BATCH * PATCH_SAMPLES;
    let mut channels = 0;
    for (b, (inp, out)) in BLOCKS.iter().zip(planes()) {
        pq = pq.max(b.output * BATCH * out.area());
        e = e.max(b.expanded * BATCH * inp.area());
        d = d.max(b.expanded * BATCH * out.area());
        channels = channels.max(b.expanded);
    }
    e = e.max(DIM * BATCH * head_plane().area());
    d = d.max(STYLES * BATCH * head_plane().area());
    Sizes { pq, e, d, channels }
}

impl Engine {
    pub(crate) fn new(gpu: Gpu, weights: Option<&Weights>) -> Self {
        let device = &gpu.device;
        let kernels = Kernels::new(device);
        let sizes = sizes();
        let (mel_weights, mel_filters) = mel_tables();
        let buffers = Buffers {
            weights: weights.map(|w| init(device, "weights", w.bytes(), BufferUsages::STORAGE)),
            dft: init(
                device,
                "dft",
                bytemuck::cast_slice(&dft_table()),
                BufferUsages::STORAGE,
            ),
            mel_weights: init(
                device,
                "mel weights",
                bytemuck::cast_slice(&mel_weights),
                BufferUsages::STORAGE,
            ),
            mel_filters: init(
                device,
                "mel filters",
                bytemuck::cast_slice(&mel_filters),
                BufferUsages::STORAGE,
            ),
            info: storage(device, "patches", BATCH * INFO_WORDS),
            m: storage(device, "mel", BATCH * INPUT.area()),
            p: storage(device, "activations p", sizes.pq),
            q: storage(device, "activations q", sizes.pq),
            e: storage(device, "expanded", sizes.e),
            d: storage(device, "depthwise", sizes.d),
            pool: storage(device, "pool", sizes.channels * BATCH),
            gate: storage(device, "gate", sizes.channels * BATCH),
            out: storage(device, "embeddings", OUT_LEN),
            styles: storage(device, "styles", STYLES_LEN),
            dummy: storage(device, "dummy", 64),
        };
        let mut params: Vec<Vec<u32>> = Vec::new();
        let mut specs: Vec<Spec> = Vec::new();
        let mut block_taps = vec![Vec::new(); NETS.len()];
        let mut add = |kernel: &Kernel, words: Vec<u32>, bufs: &[Buf], groups: [u32; 3]| {
            let mut bindings = Vec::with_capacity(bufs.len() + 1);
            if !words.is_empty() {
                bindings.push(Binding::Params(params.len()));
                params.push(words);
            }
            bindings.extend(bufs.iter().map(|&b| Binding::Buf(b)));
            specs.push(Spec {
                pipeline: kernel.pipeline.clone(),
                layout: kernel.layout.clone(),
                bindings,
                groups,
            });
            specs.len()
        };
        let gemm = |m: usize, n: usize, k: usize, hw: usize| {
            (
                vec![
                    m as u32,
                    n as u32,
                    k as u32,
                    hw as u32,
                    BATCH as u32,
                    0,
                    0,
                    0,
                ],
                [groups(n, 64), groups(m, 64), 1],
            )
        };
        let (words, dims) = gemm(2 * BINS, FRAMES, FRAME, 1);
        add(
            &kernels.dft,
            words,
            &[Buf::Dft, Buf::Dummy, Buf::D, Buf::Dummy, Buf::Dummy, Buf::E],
            dims,
        );
        add(
            &kernels.mel,
            vec![FRAMES as u32, BINS as u32, BANDS as u32, 0],
            &[Buf::Info, Buf::E, Buf::MelWeights, Buf::MelFilters, Buf::M],
            [groups(FRAMES, 256), BANDS as u32, 1],
        );
        for (net, weights) in weights.into_iter().flat_map(|w| NETS.map(|net| (net, w))) {
            let w = |layer| Buf::Weights(weights.slice(Role::Layer(net, layer)));
            let stem = stem_plane();
            add(
                &kernels.stem,
                vec![
                    BATCH as u32,
                    INPUT.h as u32,
                    INPUT.w as u32,
                    stem.h as u32,
                    stem.w as u32,
                    STEM as u32,
                    0,
                    0,
                ],
                &[w(Layer::StemWeight), w(Layer::StemBias), Buf::M, Buf::P],
                [groups(BATCH * stem.area(), 256), 1, 1],
            );
            let (mut cur, mut other) = (Buf::P, Buf::Q);
            for (ix, (b, (inp, out))) in BLOCKS.iter().zip(planes()).enumerate() {
                let dw_src = if b.expands() {
                    let (words, dims) = gemm(b.expanded, BATCH * inp.area(), b.input, inp.area());
                    add(
                        &kernels.expand,
                        words,
                        &[
                            w(Layer::ExpandWeight(ix)),
                            w(Layer::ExpandBias(ix)),
                            cur,
                            Buf::Dummy,
                            Buf::Dummy,
                            Buf::E,
                        ],
                        dims,
                    );
                    Buf::E
                } else {
                    cur
                };
                let lanes = depthwise_lanes(inp, out, b.kernel);
                add(
                    &kernels.depthwise[depthwise_variant(b.kernel, b.stride)],
                    vec![
                        b.expanded as u32,
                        BATCH as u32,
                        inp.h as u32,
                        inp.w as u32,
                        out.h as u32,
                        out.w as u32,
                        lanes as u32,
                        0,
                    ],
                    &[
                        w(Layer::DepthwiseWeight(ix)),
                        w(Layer::DepthwiseBias(ix)),
                        dw_src,
                        Buf::D,
                        Buf::Pool,
                    ],
                    [groups(b.expanded * BATCH, 256 / lanes), 1, 1],
                );
                let squeeze_lanes = 256 / b.squeeze.next_power_of_two();
                add(
                    &kernels.excite,
                    vec![
                        b.expanded as u32,
                        b.squeeze as u32,
                        BATCH as u32,
                        squeeze_lanes as u32,
                    ],
                    &[
                        w(Layer::SqueezeWeight(ix)),
                        w(Layer::SqueezeBias(ix)),
                        w(Layer::ExciteWeight(ix)),
                        w(Layer::ExciteBias(ix)),
                        Buf::Pool,
                        Buf::Gate,
                    ],
                    [BATCH as u32, 1, 1],
                );
                let (words, dims) = gemm(b.output, BATCH * out.area(), b.expanded, out.area());
                let (kernel, residual) = if b.residual() {
                    (&kernels.project_residual, cur)
                } else {
                    (&kernels.project, Buf::Dummy)
                };
                let after = add(
                    kernel,
                    words,
                    &[
                        w(Layer::ProjectWeight(ix)),
                        w(Layer::ProjectBias(ix)),
                        Buf::D,
                        Buf::Gate,
                        residual,
                        other,
                    ],
                    dims,
                );
                block_taps[net as usize].push(Tap {
                    after,
                    buffer: other,
                    len: b.output * BATCH * out.area(),
                });
                std::mem::swap(&mut cur, &mut other);
            }
            let head = head_plane();
            let (words, dims) = gemm(DIM, BATCH * head.area(), HEAD_INPUT, head.area());
            add(
                &kernels.head,
                words,
                &[
                    w(Layer::HeadWeight),
                    w(Layer::HeadBias),
                    cur,
                    Buf::Dummy,
                    Buf::Dummy,
                    Buf::E,
                ],
                dims,
            );
            let (pooled, channels, out) = match net {
                Net::Multi => (Buf::E, DIM, Buf::Out),
                Net::Styles => {
                    let (words, dims) = gemm(STYLES, BATCH * head.area(), DIM, head.area());
                    add(
                        &kernels.classifier,
                        words,
                        &[
                            Buf::Weights(weights.slice(Role::ClassifierWeight)),
                            Buf::Weights(weights.slice(Role::ClassifierBias)),
                            Buf::E,
                            Buf::Dummy,
                            Buf::Dummy,
                            Buf::D,
                        ],
                        dims,
                    );
                    (Buf::D, STYLES, Buf::Styles)
                }
            };
            add(
                &kernels.pool,
                vec![channels as u32, BATCH as u32, head.area() as u32, 0],
                &[pooled, out],
                [groups(channels * BATCH, 256), 1, 1],
            );
        }
        let mut param_bytes = vec![0u8; params.len() * PARAM_STRIDE as usize];
        for (ix, words) in params.iter().enumerate() {
            let at = ix * PARAM_STRIDE as usize;
            let bytes: &[u8] = bytemuck::cast_slice(words);
            param_bytes[at..at + bytes.len()].copy_from_slice(bytes);
        }
        let param_buffer = init(device, "params", &param_bytes, BufferUsages::UNIFORM);
        let pcm_buffer = storage(device, "pcm", BATCH * PATCH_SAMPLES * 3);
        let mut engine = Self {
            pcm: Mutex::new(Pcm {
                bind_group: resample_bind_group(&gpu, &kernels.resample, &buffers, &pcm_buffer),
                buffer: pcm_buffer,
            }),
            steps: Vec::new(),
            resample: kernels.resample,
            buffers,
            gpu,
            #[cfg(test)]
            block_taps,
        };
        engine.steps = specs
            .into_iter()
            .map(|spec| {
                let entries: Vec<wgpu::BindGroupEntry> = spec
                    .bindings
                    .iter()
                    .enumerate()
                    .map(|(binding, b)| wgpu::BindGroupEntry {
                        binding: binding as u32,
                        resource: match *b {
                            Binding::Params(ix) => {
                                wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                                    buffer: &param_buffer,
                                    offset: ix as u64 * PARAM_STRIDE,
                                    size: wgpu::BufferSize::new((params[ix].len() * 4) as u64),
                                })
                            }
                            Binding::Buf(buf) => engine.resource(buf),
                        },
                    })
                    .collect();
                Step {
                    pipeline: spec.pipeline,
                    bind_group: engine
                        .gpu
                        .device
                        .create_bind_group(&wgpu::BindGroupDescriptor {
                            label: None,
                            layout: &spec.layout,
                            entries: &entries,
                        }),
                    groups: spec.groups,
                }
            })
            .collect();
        engine
    }

    fn buffer(&self, buf: Buf) -> &Buffer {
        let b = &self.buffers;
        match buf {
            Buf::Weights(_) => b.weights.as_ref().expect("the network has weights"),
            Buf::Dft => &b.dft,
            Buf::MelWeights => &b.mel_weights,
            Buf::MelFilters => &b.mel_filters,
            Buf::Info => &b.info,
            Buf::M => &b.m,
            Buf::P => &b.p,
            Buf::Q => &b.q,
            Buf::E => &b.e,
            Buf::D => &b.d,
            Buf::Pool => &b.pool,
            Buf::Gate => &b.gate,
            Buf::Out => &b.out,
            Buf::Styles => &b.styles,
            Buf::Dummy => &b.dummy,
        }
    }

    fn resource(&self, buf: Buf) -> wgpu::BindingResource<'_> {
        match buf {
            Buf::Weights(slice) => wgpu::BindingResource::Buffer(wgpu::BufferBinding {
                buffer: self.buffer(buf),
                offset: slice.offset,
                size: wgpu::BufferSize::new(slice.size),
            }),
            _ => self.buffer(buf).as_entire_binding(),
        }
    }

    pub(crate) fn submit(
        &self,
        infos: &[[u32; INFO_WORDS]],
        pcm: &[&[f32]],
    ) -> Result<Pending, String> {
        let after = self.steps.len();
        self.submit_with_taps(
            infos,
            pcm,
            after,
            &[
                Tap {
                    after,
                    buffer: Buf::Out,
                    len: OUT_LEN,
                },
                Tap {
                    after,
                    buffer: Buf::Styles,
                    len: STYLES_LEN,
                },
            ],
        )
    }

    pub(crate) fn submit_with_taps(
        &self,
        infos: &[[u32; INFO_WORDS]],
        pcm: &[&[f32]],
        until: usize,
        taps: &[Tap],
    ) -> Result<Pending, String> {
        let state = self.upload(infos, pcm)?;
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let mut encoder = device.create_command_encoder(&wgpu::CommandEncoderDescriptor {
            label: Some("embedding batch"),
        });
        let readback: Vec<Buffer> = taps
            .iter()
            .map(|tap| {
                device.create_buffer(&wgpu::BufferDescriptor {
                    label: Some("readback"),
                    size: (tap.len * 4) as u64,
                    usage: BufferUsages::MAP_READ | BufferUsages::COPY_DST,
                    mapped_at_creation: false,
                })
            })
            .collect();
        let mut order: Vec<&Tap> = taps.iter().collect();
        order.sort_by_key(|t| t.after);
        let mut done = 0;
        let mut resampled = false;
        for (tap, target) in order.iter().zip(&readback_order(taps, &readback)) {
            let stop = tap.after.min(until).max(done);
            if !resampled || stop > done {
                self.encode(&mut encoder, &state.bind_group, !resampled, done..stop);
                (resampled, done) = (true, stop);
            }
            encoder.copy_buffer_to_buffer(
                self.buffer(tap.buffer),
                0,
                target,
                0,
                (tap.len * 4) as u64,
            );
        }
        if !resampled || done < until {
            self.encode(&mut encoder, &state.bind_group, !resampled, done..until);
        }
        let index = queue.submit([encoder.finish()]);
        drop(state);
        let (tx, mapped) = mpsc::channel();
        for buffer in &readback {
            let tx = tx.clone();
            buffer.map_async(wgpu::MapMode::Read, .., move |result| {
                let _ = tx.send(result);
            });
        }
        Ok(Pending {
            index,
            readback,
            mapped,
        })
    }

    fn upload(
        &self,
        infos: &[[u32; INFO_WORDS]],
        pcm: &[&[f32]],
    ) -> Result<std::sync::MutexGuard<'_, Pcm>, String> {
        if let Some(fault) = self.gpu.fault() {
            return Err(fault);
        }
        let total: usize = pcm.iter().map(|s| s.len()).sum();
        if total > PCM_CAP || infos.len() > BATCH {
            return Err(format!("a run of {total} samples does not fit"));
        }
        let device = &self.gpu.device;
        let queue = &self.gpu.queue;
        let mut state = self.pcm.lock().unwrap_or_else(PoisonError::into_inner);
        if (total * 4) as u64 > state.buffer.size() {
            let grown = (total + total / 2).clamp(1, PCM_CAP);
            let buffer = storage(device, "pcm", grown);
            state.bind_group =
                resample_bind_group(&self.gpu, &self.resample, &self.buffers, &buffer);
            state.buffer = buffer;
        }
        let mut words = vec![0u32; BATCH * INFO_WORDS];
        for (slot, chunk) in words.as_chunks_mut::<INFO_WORDS>().0.iter_mut().enumerate() {
            chunk.copy_from_slice(infos.get(slot).unwrap_or(&crate::input::EMPTY_INFO));
        }
        queue.write_buffer(&self.buffers.info, 0, bytemuck::cast_slice(&words));
        if let Some(size) = wgpu::BufferSize::new((total * 4) as u64) {
            let mut view = queue
                .write_buffer_with(&state.buffer, 0, size)
                .ok_or("the PCM upload was refused")?;
            let mut at = 0;
            for samples in pcm {
                let bytes: &[u8] = bytemuck::cast_slice(samples);
                view.slice(at..at + bytes.len()).copy_from_slice(bytes);
                at += bytes.len();
            }
        }
        Ok(state)
    }

    fn encode(
        &self,
        encoder: &mut wgpu::CommandEncoder,
        resample: &BindGroup,
        with_resample: bool,
        steps: std::ops::Range<usize>,
    ) {
        let mut pass = encoder.begin_compute_pass(&wgpu::ComputePassDescriptor {
            label: None,
            timestamp_writes: None,
        });
        if with_resample {
            pass.set_pipeline(&self.resample.pipeline);
            pass.set_bind_group(0, resample, &[]);
            pass.dispatch_workgroups(groups(PATCH_SAMPLES, 256), BATCH as u32, 1);
        }
        for step in &self.steps[steps] {
            pass.set_pipeline(&step.pipeline);
            pass.set_bind_group(0, &step.bind_group, &[]);
            pass.dispatch_workgroups(step.groups[0], step.groups[1], step.groups[2]);
        }
    }

    pub(crate) fn wait(&self, pending: Pending) -> Result<Vec<Vec<f32>>, String> {
        self.gpu
            .device
            .poll(wgpu::PollType::Wait {
                submission_index: Some(pending.index),
                timeout: Some(WAIT),
            })
            .map_err(|e| format!("waiting for the GPU: {e}"))?;
        for _ in &pending.readback {
            pending
                .mapped
                .recv_timeout(WAIT)
                .map_err(|_| "the GPU did not return the results".to_string())?
                .map_err(|e| format!("reading the results: {e}"))?;
        }
        if let Some(fault) = self.gpu.fault() {
            return Err(fault);
        }
        Ok(pending
            .readback
            .iter()
            .map(|buffer| {
                let values = {
                    let view = buffer.get_mapped_range(..);
                    view.as_chunks::<4>()
                        .0
                        .iter()
                        .map(|b| f32::from_le_bytes(*b))
                        .collect()
                };
                buffer.unmap();
                values
            })
            .collect())
    }

    #[cfg(test)]
    pub(crate) fn steps(&self) -> usize {
        self.steps.len()
    }
}

pub(crate) const DEPTHWISE_TILE: usize = 3300;

fn depthwise_lanes(input: Plane, out: Plane, kernel: usize) -> usize {
    let pad = kernel / 2;
    let padded = (input.h + 2 * pad) * (input.w + 2 * pad);
    let mut lanes = (out.area().next_power_of_two() / 4).clamp(16, 256);
    while lanes < 256 && (256 / lanes) * padded > DEPTHWISE_TILE {
        lanes *= 2;
    }
    lanes
}

fn readback_order<'a>(taps: &[Tap], readback: &'a [Buffer]) -> Vec<&'a Buffer> {
    let mut ix: Vec<usize> = (0..taps.len()).collect();
    ix.sort_by_key(|&i| taps[i].after);
    ix.into_iter().map(|i| &readback[i]).collect()
}

fn resample_bind_group(gpu: &Gpu, kernel: &Kernel, buffers: &Buffers, pcm: &Buffer) -> BindGroup {
    gpu.device.create_bind_group(&wgpu::BindGroupDescriptor {
        label: Some("resample"),
        layout: &kernel.layout,
        entries: &[
            wgpu::BindGroupEntry {
                binding: 0,
                resource: buffers.info.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 1,
                resource: pcm.as_entire_binding(),
            },
            wgpu::BindGroupEntry {
                binding: 2,
                resource: buffers.d.as_entire_binding(),
            },
        ],
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn every_depthwise_layer_fits_its_tile() {
        for (b, (inp, out)) in BLOCKS.iter().zip(planes()) {
            let lanes = depthwise_lanes(inp, out, b.kernel);
            let pad = b.kernel / 2;
            let padded = (inp.h + 2 * pad) * (inp.w + 2 * pad);
            assert!((256 / lanes) * padded <= DEPTHWISE_TILE, "{b:?}");
            assert!((256 / lanes) * b.kernel * b.kernel <= 400, "{b:?}");
            assert!(lanes.is_power_of_two() && (16..=256).contains(&lanes));
        }
    }
}
