//! Narrow Windows platform services used by the desktop-pet host.
//!
//! Win32 handles and polling stay inside this module.  The application and the
//! platform-independent runtime exchange only owned values such as
//! [`bug_runtime::contract::ScreenObstacle`].

#![cfg(windows)]

pub mod desktop_icons;
pub mod dpi;
pub mod interaction;
