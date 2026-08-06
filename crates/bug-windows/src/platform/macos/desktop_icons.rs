//! Finder desktop collision adapter.
//!
//! Finder does not expose desktop item rectangles through a stable public API.
//! The macOS host therefore keeps the same atomic tracker interface while
//! publishing no static icon obstacles. Cursor avoidance, display bounds and
//! every runtime collision invariant remain active.

use bug_runtime::contract::ScreenObstacle;
use bug_runtime::math::Vec2;

#[derive(Debug, Default)]
pub struct DesktopIconTracker;

impl DesktopIconTracker {
    #[must_use]
    pub const fn new() -> Self {
        Self
    }

    pub const fn preload(&mut self) {}

    #[must_use]
    pub const fn cached_icons(&self) -> &[ScreenObstacle] {
        &[]
    }

    pub const fn update(&mut self, _cursor: Vec2, _allow_drag: bool) {}

    #[must_use]
    pub const fn obstacles(&self) -> &[ScreenObstacle] {
        &[]
    }

    pub const fn invalidate(&mut self) {}
}
