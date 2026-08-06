//! Narrow platform services used by the desktop-pet host.
//!
//! Win32 handles and polling stay inside this module.  The application and the
//! platform-independent runtime exchange only owned values such as
//! [`bug_runtime::contract::ScreenObstacle`].

#[cfg(windows)]
pub mod desktop_icons;
#[cfg(target_os = "macos")]
#[path = "macos/desktop_icons.rs"]
pub mod desktop_icons;

#[cfg(windows)]
pub mod dpi;
#[cfg(target_os = "macos")]
#[path = "macos/dpi.rs"]
pub mod dpi;

#[cfg(windows)]
pub mod interaction;
#[cfg(target_os = "macos")]
#[path = "macos/interaction.rs"]
pub mod interaction;

#[cfg(windows)]
pub mod layered_window;
#[cfg(target_os = "macos")]
#[path = "macos/layered_window.rs"]
pub mod layered_window;
