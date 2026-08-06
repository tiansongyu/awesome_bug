//! macOS display geometry helpers.

use std::convert::Infallible;

use bug_runtime::contract::Rect;

pub const REFERENCE_DISPLAY_WIDTH: i32 = 1920;
pub const REFERENCE_DISPLAY_HEIGHT: i32 = 1080;
pub const REFERENCE_BODY_LENGTH: f32 = 165.0;
pub const MINIMUM_RESOLUTION_SCALE: f32 = 0.60;
pub const MAXIMUM_RESOLUTION_SCALE: f32 = 2.0;

/// An integer screen rectangle in the logical desktop coordinate space used
/// by SDL and Core Graphics on macOS.
#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct PixelRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

impl PixelRect {
    #[must_use]
    pub const fn new(x: i32, y: i32, width: i32, height: i32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    #[must_use]
    pub const fn is_valid(self) -> bool {
        self.width > 0 && self.height > 0
    }

    #[must_use]
    pub fn to_runtime_rect(self) -> Rect {
        Rect {
            x: self.x as f32,
            y: self.y as f32,
            width: self.width as f32,
            height: self.height as f32,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DisplayGeometry {
    pub display_bounds: PixelRect,
    pub work_area: PixelRect,
    pub body_length: f32,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum BodySizePolicy {
    Automatic { reference_length: f32 },
    Fixed(f32),
}

impl Default for BodySizePolicy {
    fn default() -> Self {
        Self::Automatic {
            reference_length: REFERENCE_BODY_LENGTH,
        }
    }
}

/// macOS uses a single logical point coordinate space, so no process-wide DPI
/// mode needs to be enabled before SDL starts.
pub const fn enable_per_monitor_v2() -> Result<(), Infallible> {
    Ok(())
}

#[must_use]
pub fn query_display_geometry(
    display_bounds: PixelRect,
    work_area: PixelRect,
    size_policy: BodySizePolicy,
) -> DisplayGeometry {
    let work_area = if work_area.is_valid() {
        work_area
    } else {
        display_bounds
    };
    DisplayGeometry {
        display_bounds,
        work_area,
        body_length: resolve_body_length(size_policy, display_bounds),
    }
}

#[must_use]
pub fn resolve_body_length(policy: BodySizePolicy, display_bounds: PixelRect) -> f32 {
    match policy {
        BodySizePolicy::Automatic { reference_length } => automatic_body_length(
            reference_length,
            display_bounds.width,
            display_bounds.height,
        ),
        BodySizePolicy::Fixed(length) => length,
    }
}

#[must_use]
pub fn resolution_scale(width: i32, height: i32) -> f32 {
    if width <= 0 || height <= 0 {
        return 1.0;
    }
    let proportional = ((width as f32) / (REFERENCE_DISPLAY_WIDTH as f32))
        .min((height as f32) / (REFERENCE_DISPLAY_HEIGHT as f32));
    proportional.clamp(MINIMUM_RESOLUTION_SCALE, MAXIMUM_RESOLUTION_SCALE)
}

#[must_use]
pub fn automatic_body_length(reference_length: f32, width: i32, height: i32) -> f32 {
    (reference_length * resolution_scale(width, height)).round()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn usable_bounds_are_preferred_and_invalid_bounds_fall_back() {
        let display = PixelRect::new(0, 0, 1728, 1117);
        let usable = PixelRect::new(0, 37, 1728, 1037);
        let geometry = query_display_geometry(display, usable, BodySizePolicy::Fixed(180.0));
        assert_eq!(geometry.work_area, usable);
        assert_eq!(geometry.body_length, 180.0);

        let fallback =
            query_display_geometry(display, PixelRect::default(), BodySizePolicy::Fixed(180.0));
        assert_eq!(fallback.work_area, display);
    }
}
