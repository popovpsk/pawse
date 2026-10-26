#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "linux")]
pub(crate) use linux::{read, write};
#[cfg(target_os = "windows")]
mod windows;
#[cfg(target_os = "windows")]
pub(crate) use windows::{read, write};

const SERVICE: &str = "pawse";
const ACCOUNT: &str = "data-key";
