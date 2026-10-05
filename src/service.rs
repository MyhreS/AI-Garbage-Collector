#[cfg(target_os = "macos")]
#[path = "service_macos.rs"]
mod native;
#[cfg(target_os = "linux")]
#[path = "service_linux.rs"]
mod native;
#[cfg(windows)]
#[path = "service_windows.rs"]
mod native;
pub use native::{install, installed, status, uninstall};
