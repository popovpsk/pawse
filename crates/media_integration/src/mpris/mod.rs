#[cfg(target_os = "linux")]
mod interfaces;
#[cfg(target_os = "linux")]
mod service;
#[cfg_attr(not(target_os = "linux"), allow(dead_code))]
mod state;

#[cfg(target_os = "linux")]
pub(crate) use service::MprisIntegration;
