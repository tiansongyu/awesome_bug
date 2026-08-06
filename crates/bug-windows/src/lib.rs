//! Windows and macOS desktop hosts for the scriptable bug runtime.

#[cfg(any(windows, target_os = "macos"))]
pub mod app;
pub mod cli;
#[cfg(any(windows, target_os = "macos"))]
pub mod platform;
#[cfg(any(windows, target_os = "macos"))]
pub mod render;
pub mod resource;
pub mod spawn;
pub mod trace;
pub mod world;
