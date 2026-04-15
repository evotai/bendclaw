mod sandbox;

#[cfg(target_os = "linux")]
mod linux;
#[cfg(target_os = "macos")]
mod macos;

pub use sandbox::check_available;
pub use sandbox::wrap_command;
pub use sandbox::SandboxSupport;
