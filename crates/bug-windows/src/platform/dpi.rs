//! Physical-pixel display geometry and per-monitor DPI initialization.

use std::mem::size_of;

use bug_runtime::contract::Rect;
use windows::Win32::Foundation::POINT;
use windows::Win32::Graphics::Gdi::{
    GetMonitorInfoW, MONITOR_DEFAULTTONEAREST, MONITORINFO, MonitorFromPoint,
};
use windows::Win32::UI::HiDpi::{
    AreDpiAwarenessContextsEqual, DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
    GetThreadDpiAwarenessContext, SetProcessDpiAwarenessContext,
};

pub const REFERENCE_DISPLAY_WIDTH: i32 = 1920;
pub const REFERENCE_DISPLAY_HEIGHT: i32 = 1080;
pub const REFERENCE_BODY_LENGTH: f32 = 165.0;
pub const MINIMUM_RESOLUTION_SCALE: f32 = 0.60;
pub const MAXIMUM_RESOLUTION_SCALE: f32 = 2.0;

/// An integer screen rectangle in physical desktop pixels.
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
    fn center(self) -> POINT {
        POINT {
            x: midpoint(self.x, self.width),
            y: midpoint(self.y, self.height),
        }
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

/// Automatic sizing remains tied to the full selected display, while movement
/// is constrained to the taskbar-excluding work area.
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

/// Must be called before SDL video initialization.
pub fn enable_per_monitor_v2() -> windows::core::Result<()> {
    // SAFETY: These process/thread APIs take predefined values and no pointers.
    // The caller invokes this before SDL creates any windows.  A manifest may
    // already have selected PMv2, in which case SetProcessDpiAwarenessContext
    // can report access denied; equality before/after makes that case success.
    unsafe {
        let is_per_monitor_v2 = || {
            AreDpiAwarenessContextsEqual(
                GetThreadDpiAwarenessContext(),
                DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2,
            )
            .as_bool()
        };
        if is_per_monitor_v2() {
            return Ok(());
        }
        match SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2) {
            Ok(()) => Ok(()),
            Err(_) if is_per_monitor_v2() => Ok(()),
            Err(error) => Err(error),
        }
    }
}

/// Resolves the selected SDL display to a Windows work area in the same
/// physical coordinate space as Explorer and `GetCursorPos`.
#[must_use]
pub fn query_display_geometry(
    display_bounds: PixelRect,
    size_policy: BodySizePolicy,
) -> DisplayGeometry {
    let work_area = work_area_for_display(display_bounds);
    let body_length = resolve_body_length(size_policy, display_bounds);
    DisplayGeometry {
        display_bounds,
        work_area,
        body_length,
    }
}

/// Returns the monitor work area, falling back to the SDL display rectangle if
/// Windows cannot provide a valid monitor.  Negative origins are preserved.
#[must_use]
pub fn work_area_for_display(display_bounds: PixelRect) -> PixelRect {
    if !display_bounds.is_valid() {
        return display_bounds;
    }

    // SAFETY: MonitorFromPoint receives a value. GetMonitorInfoW writes to a
    // correctly sized, initialized MONITORINFO for the returned monitor.
    unsafe {
        let monitor = MonitorFromPoint(display_bounds.center(), MONITOR_DEFAULTTONEAREST);
        if monitor.is_invalid() {
            return display_bounds;
        }

        let mut information = MONITORINFO {
            cbSize: size_of::<MONITORINFO>() as u32,
            ..MONITORINFO::default()
        };
        if !GetMonitorInfoW(monitor, &mut information).as_bool() {
            return display_bounds;
        }

        pixel_rect_from_edges(
            information.rcWork.left,
            information.rcWork.top,
            information.rcWork.right,
            information.rcWork.bottom,
        )
        .filter(|rectangle| rectangle.is_valid())
        .unwrap_or(display_bounds)
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

#[must_use]
fn pixel_rect_from_edges(left: i32, top: i32, right: i32, bottom: i32) -> Option<PixelRect> {
    Some(PixelRect {
        x: left,
        y: top,
        width: right.checked_sub(left)?,
        height: bottom.checked_sub(top)?,
    })
}

#[must_use]
fn midpoint(origin: i32, length: i32) -> i32 {
    let value = i64::from(origin) + i64::from(length) / 2;
    value.clamp(i64::from(i32::MIN), i64::from(i32::MAX)) as i32
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn automatic_size_matches_supported_resolution_formula() {
        assert_eq!(automatic_body_length(165.0, 1920, 1080), 165.0);
        assert_eq!(automatic_body_length(165.0, 3840, 2160), 330.0);
        assert_eq!(automatic_body_length(165.0, 1280, 720), 110.0);
        assert_eq!(automatic_body_length(165.0, 640, 480), 99.0);
        assert_eq!(automatic_body_length(165.0, 7680, 4320), 330.0);
    }

    #[test]
    fn fixed_size_is_not_changed_by_display_topology() {
        let left = resolve_body_length(
            BodySizePolicy::Fixed(217.5),
            PixelRect::new(-3840, 0, 3840, 2160),
        );
        let right = resolve_body_length(
            BodySizePolicy::Fixed(217.5),
            PixelRect::new(0, -200, 1280, 720),
        );
        assert_eq!(left, 217.5);
        assert_eq!(right, 217.5);
    }

    #[test]
    fn edge_conversion_preserves_negative_monitor_coordinates() {
        assert_eq!(
            pixel_rect_from_edges(-1920, -120, 0, 960),
            Some(PixelRect::new(-1920, -120, 1920, 1080))
        );
    }

    #[test]
    fn invalid_dimensions_use_neutral_scale() {
        assert_eq!(resolution_scale(0, 1080), 1.0);
        assert_eq!(resolution_scale(1920, -1), 1.0);
    }
}
