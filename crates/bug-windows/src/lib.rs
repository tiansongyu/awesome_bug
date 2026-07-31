//! Windows SDL and Win32 host for the scriptable bug runtime.

#[cfg(windows)]
pub mod app;
pub mod cli;
#[cfg(windows)]
pub mod platform;
#[cfg(windows)]
pub mod render;
pub mod resource;
pub mod spawn;
pub mod trace;
pub mod world;
