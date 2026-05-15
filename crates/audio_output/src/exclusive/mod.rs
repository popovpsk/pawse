#[cfg(target_os = "macos")]
mod macos;

#[cfg(not(target_os = "macos"))]
mod unsupported;

#[cfg(target_os = "macos")]
pub use macos::ExclusiveOutput;

#[cfg(not(target_os = "macos"))]
pub use unsupported::ExclusiveOutput;

#[cfg(target_os = "macos")]
pub use macos::set_device_sample_rate;