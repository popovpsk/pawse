use std::sync::{Arc, Mutex, PoisonError};

use wgpu::{Adapter, AdapterInfo, Backend, Backends, DeviceType};

pub(crate) struct Gpu {
    pub(crate) device: wgpu::Device,
    pub(crate) queue: wgpu::Queue,
    pub(crate) info: AdapterInfo,
    fault: Arc<Mutex<Option<String>>>,
}

impl Gpu {
    pub(crate) fn name(&self) -> String {
        describe(&self.info)
    }

    pub(crate) fn fault(&self) -> Option<String> {
        self.fault
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .clone()
    }
}

pub(crate) fn describe(info: &AdapterInfo) -> String {
    format!("{} ({:?})", info.name, info.backend)
}

fn instance() -> wgpu::Instance {
    wgpu::Instance::new(wgpu::InstanceDescriptor {
        backends: Backends::PRIMARY,
        ..wgpu::InstanceDescriptor::new_without_display_handle()
    })
}

pub(crate) fn hardware(info: &AdapterInfo) -> bool {
    info.device_type != DeviceType::Cpu
}

fn rank(info: &AdapterInfo) -> (u8, u8) {
    let kind = match info.device_type {
        DeviceType::DiscreteGpu => 0,
        DeviceType::IntegratedGpu => 1,
        DeviceType::VirtualGpu => 2,
        DeviceType::Other => 3,
        DeviceType::Cpu => 4,
    };
    let backend = match info.backend {
        Backend::Metal => 0,
        Backend::Dx12 => 1,
        Backend::Vulkan => 2,
        _ => 3,
    };
    (kind, backend)
}

pub(crate) fn candidates(allow_software: bool) -> Vec<Adapter> {
    let adapters = pollster::block_on(instance().enumerate_adapters(Backends::PRIMARY));
    let infos: Vec<AdapterInfo> = adapters.iter().map(Adapter::get_info).collect();
    let mut slots: Vec<Option<Adapter>> = adapters.into_iter().map(Some).collect();
    order(&infos, allow_software)
        .into_iter()
        .filter_map(|ix| slots[ix].take())
        .collect()
}

pub(crate) fn order(infos: &[AdapterInfo], allow_software: bool) -> Vec<usize> {
    let mut ix: Vec<usize> = (0..infos.len())
        .filter(|&i| allow_software || hardware(&infos[i]))
        .collect();
    ix.sort_by_key(|&i| rank(&infos[i]));
    ix
}

pub(crate) fn open(adapter: &Adapter) -> Result<Gpu, String> {
    let info = adapter.get_info();
    let limits = wgpu::Limits {
        max_storage_buffers_per_shader_stage: 8,
        ..wgpu::Limits::default()
    };
    let (device, queue) = pollster::block_on(adapter.request_device(&wgpu::DeviceDescriptor {
        label: Some("audio embedding"),
        required_features: wgpu::Features::empty(),
        required_limits: limits,
        experimental_features: wgpu::ExperimentalFeatures::disabled(),
        memory_hints: wgpu::MemoryHints::Performance,
        trace: wgpu::Trace::Off,
    }))
    .map_err(|e| format!("{}: {e}", describe(&info)))?;
    let fault = Arc::new(Mutex::new(None));
    let on_error = fault.clone();
    device.on_uncaptured_error(Arc::new(move |e: wgpu::Error| {
        log::error!("audio embedding: GPU error: {e}");
        on_error
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| e.to_string());
    }));
    let on_lost = fault.clone();
    device.set_device_lost_callback(move |reason, message| {
        on_lost
            .lock()
            .unwrap_or_else(PoisonError::into_inner)
            .get_or_insert_with(|| format!("the GPU was lost ({reason:?}): {message}"));
    });
    Ok(Gpu {
        device,
        queue,
        info,
        fault,
    })
}

pub fn adapter_name() -> Option<String> {
    candidates(false).first().map(|a| describe(&a.get_info()))
}

#[cfg(test)]
pub(crate) fn any() -> Option<Gpu> {
    let allow_software = std::env::var_os("PAWSE_GPU_ALLOW_SOFTWARE").is_some();
    candidates(allow_software).iter().find_map(|a| open(a).ok())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn info(name: &str, device_type: DeviceType, backend: Backend) -> AdapterInfo {
        AdapterInfo {
            name: name.into(),
            vendor: 0,
            device: 0,
            device_type,
            device_pci_bus_id: String::new(),
            driver: String::new(),
            driver_info: String::new(),
            backend,
            subgroup_min_size: 0,
            subgroup_max_size: 0,
            transient_saves_memory: false,
        }
    }

    #[test]
    fn software_adapters_are_never_picked() {
        let infos = [
            info("llvmpipe", DeviceType::Cpu, Backend::Vulkan),
            info(
                "Microsoft Basic Render Driver",
                DeviceType::Cpu,
                Backend::Dx12,
            ),
        ];
        assert!(order(&infos, false).is_empty());
        assert_eq!(order(&infos, true).len(), 2);
    }

    #[test]
    fn a_discrete_gpu_comes_first_then_dx12_before_vulkan() {
        let infos = [
            info("lavapipe", DeviceType::Cpu, Backend::Vulkan),
            info("Intel UHD", DeviceType::IntegratedGpu, Backend::Vulkan),
            info("RTX (Vulkan)", DeviceType::DiscreteGpu, Backend::Vulkan),
            info("RTX (DX12)", DeviceType::DiscreteGpu, Backend::Dx12),
            info("Virtio", DeviceType::VirtualGpu, Backend::Vulkan),
        ];
        assert_eq!(order(&infos, false), vec![3, 2, 1, 4]);
    }
}
