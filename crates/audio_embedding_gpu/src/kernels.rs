use audio_embedding::{HOP, PATCH, PATCH_SAMPLES};
use wgpu::{BindGroupLayout, BufferBindingType, ComputePipeline, Device, ShaderStages};

use crate::engine::DEPTHWISE_TILE;

const PATCH_INFO: &str = include_str!("shaders/patch.wgsl");

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Slot {
    Uniform,
    Read,
    Write,
}

pub(crate) struct Kernel {
    pub(crate) pipeline: ComputePipeline,
    pub(crate) layout: BindGroupLayout,
}

pub(crate) struct Kernels {
    pub(crate) resample: Kernel,
    pub(crate) dft: Kernel,
    pub(crate) mel: Kernel,
    pub(crate) stem: Kernel,
    pub(crate) expand: Kernel,
    pub(crate) depthwise: [Kernel; 4],
    pub(crate) excite: Kernel,
    pub(crate) project: Kernel,
    pub(crate) project_residual: Kernel,
    pub(crate) head: Kernel,
    pub(crate) pool: Kernel,
}

const GEMM_SLOTS: [Slot; 7] = [
    Slot::Uniform,
    Slot::Read,
    Slot::Read,
    Slot::Read,
    Slot::Read,
    Slot::Read,
    Slot::Write,
];

pub(crate) fn depthwise_variant(kernel: usize, stride: usize) -> usize {
    usize::from(kernel == 5) * 2 + usize::from(stride == 2)
}

impl Kernels {
    pub(crate) fn new(device: &Device) -> Self {
        let with_patch = |source: &str| {
            format!(
                "{PATCH_INFO}\nconst PATCH_SAMPLES: u32 = {PATCH_SAMPLES}u;\n\
                 const PATCH_FRAMES: u32 = {PATCH}u;\nconst HOP: u32 = {HOP}u;\n{source}"
            )
        };
        let resample = module(
            device,
            "resample",
            &with_patch(include_str!("shaders/resample.wgsl")),
        );
        let gemm = module(
            device,
            "gemm",
            &with_patch(include_str!("shaders/gemm.wgsl")),
        );
        let mel = module(device, "mel", &with_patch(include_str!("shaders/mel.wgsl")));
        let stem = module(device, "stem", include_str!("shaders/stem.wgsl"));
        let depthwise = module(
            device,
            "depthwise",
            &format!(
                "const TILE: u32 = {DEPTHWISE_TILE}u;\n{}",
                include_str!("shaders/depthwise.wgsl")
            ),
        );
        let excite = module(device, "excite", include_str!("shaders/excite.wgsl"));
        let pool = module(device, "pool", include_str!("shaders/pool.wgsl"));
        let gemm_kernel =
            |label, constants: &[(&str, f64)]| kernel(device, label, &gemm, &GEMM_SLOTS, constants);
        Self {
            resample: kernel(
                device,
                "resample",
                &resample,
                &[Slot::Read, Slot::Read, Slot::Write],
                &[],
            ),
            dft: gemm_kernel("dft", &[("LOAD", 2.0), ("BIAS", 0.0)]),
            mel: kernel(
                device,
                "mel",
                &mel,
                &[
                    Slot::Uniform,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Write,
                ],
                &[],
            ),
            stem: kernel(
                device,
                "stem",
                &stem,
                &[
                    Slot::Uniform,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Write,
                ],
                &[],
            ),
            expand: gemm_kernel("expand", &[("SWISH", 1.0)]),
            depthwise: [(3.0, 1.0), (3.0, 2.0), (5.0, 1.0), (5.0, 2.0)].map(|(k, stride)| {
                kernel(
                    device,
                    "depthwise",
                    &depthwise,
                    &[
                        Slot::Uniform,
                        Slot::Read,
                        Slot::Read,
                        Slot::Read,
                        Slot::Write,
                        Slot::Write,
                    ],
                    &[("KERNEL", k), ("STRIDE", stride)],
                )
            }),
            excite: kernel(
                device,
                "excite",
                &excite,
                &[
                    Slot::Uniform,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Read,
                    Slot::Write,
                ],
                &[],
            ),
            project: gemm_kernel("project", &[("LOAD", 1.0)]),
            project_residual: gemm_kernel("project residual", &[("LOAD", 1.0), ("RESIDUAL", 1.0)]),
            head: gemm_kernel("head", &[("SWISH", 1.0)]),
            pool: kernel(
                device,
                "pool",
                &pool,
                &[Slot::Uniform, Slot::Read, Slot::Write],
                &[],
            ),
        }
    }
}

fn module(device: &Device, label: &str, source: &str) -> wgpu::ShaderModule {
    device.create_shader_module(wgpu::ShaderModuleDescriptor {
        label: Some(label),
        source: wgpu::ShaderSource::Wgsl(source.into()),
    })
}

fn kernel(
    device: &Device,
    label: &str,
    module: &wgpu::ShaderModule,
    slots: &[Slot],
    constants: &[(&str, f64)],
) -> Kernel {
    let entries: Vec<wgpu::BindGroupLayoutEntry> = slots
        .iter()
        .enumerate()
        .map(|(binding, slot)| wgpu::BindGroupLayoutEntry {
            binding: binding as u32,
            visibility: ShaderStages::COMPUTE,
            ty: wgpu::BindingType::Buffer {
                ty: match slot {
                    Slot::Uniform => BufferBindingType::Uniform,
                    Slot::Read => BufferBindingType::Storage { read_only: true },
                    Slot::Write => BufferBindingType::Storage { read_only: false },
                },
                has_dynamic_offset: false,
                min_binding_size: None,
            },
            count: None,
        })
        .collect();
    let layout = device.create_bind_group_layout(&wgpu::BindGroupLayoutDescriptor {
        label: Some(label),
        entries: &entries,
    });
    let pipeline_layout = device.create_pipeline_layout(&wgpu::PipelineLayoutDescriptor {
        label: Some(label),
        bind_group_layouts: &[Some(&layout)],
        immediate_size: 0,
    });
    let pipeline = device.create_compute_pipeline(&wgpu::ComputePipelineDescriptor {
        label: Some(label),
        layout: Some(&pipeline_layout),
        module,
        entry_point: Some("main"),
        compilation_options: wgpu::PipelineCompilationOptions {
            constants,
            zero_initialize_workgroup_memory: false,
        },
        cache: None,
    });
    Kernel { pipeline, layout }
}
