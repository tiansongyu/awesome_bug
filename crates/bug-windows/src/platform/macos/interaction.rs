//! Core Graphics global cursor and shortcut polling for macOS.

use std::error::Error;
use std::ffi::c_void;
use std::fmt::{self, Display, Formatter};

use bug_runtime::contract::Rect;
use bug_runtime::math::Vec2;

const CONTROL_KEY_CODE: u16 = 0x3B;
const OPTION_KEY_CODE: u16 = 0x3A;
const BAIT_KEY_CODE: u16 = 0x03; // F
const QUIT_KEY_CODE: u16 = 0x0C; // Q
const COMBINED_SESSION_STATE: i32 = 0;
const LEFT_MOUSE_BUTTON: u32 = 0;

#[repr(C)]
#[derive(Clone, Copy, Debug, Default)]
struct CGPoint {
    x: f64,
    y: f64,
}

type CGEventRef = *const c_void;

#[link(name = "ApplicationServices", kind = "framework")]
unsafe extern "C" {
    fn CGEventCreate(source: *const c_void) -> CGEventRef;
    fn CGEventGetLocation(event: CGEventRef) -> CGPoint;
    fn CGEventSourceKeyState(state_id: i32, key: u16) -> bool;
    fn CGEventSourceButtonState(state_id: i32, button: u32) -> bool;
    fn CFRelease(value: *const c_void);
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct InputError(&'static str);

impl Display for InputError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(self.0)
    }
}

impl Error for InputError {}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct ShortcutSample {
    pub control: bool,
    pub alt: bool,
    pub bait_key: bool,
    pub quit_key: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct InteractionEvents {
    pub bait_placement: Option<Vec2>,
    pub quit_requested: bool,
}

#[derive(Clone, Debug)]
pub struct InteractionController {
    enabled: bool,
    work_area: Rect,
    bait_key_was_down: bool,
    bait_position: Option<Vec2>,
}

impl InteractionController {
    #[must_use]
    pub const fn new(enabled: bool, work_area: Rect) -> Self {
        Self {
            enabled,
            work_area,
            bait_key_was_down: false,
            bait_position: None,
        }
    }

    pub fn set_work_area(&mut self, work_area: Rect) {
        self.work_area = work_area;
        if self
            .bait_position
            .is_some_and(|position| !point_in_rect(work_area, position))
        {
            self.bait_position = None;
        }
    }

    #[must_use]
    pub const fn bait_position(&self) -> Option<Vec2> {
        self.bait_position
    }

    pub fn clear_bait(&mut self) {
        self.bait_position = None;
    }

    pub fn place_bait(&mut self, position: Vec2) {
        if self.enabled && point_in_rect(self.work_area, position) {
            self.bait_position = Some(position);
        }
    }

    #[must_use]
    pub fn update(&mut self, cursor: Vec2, shortcuts: ShortcutSample) -> InteractionEvents {
        let bait_pressed = shortcuts.bait_key && !self.bait_key_was_down;
        self.bait_key_was_down = shortcuts.bait_key;
        InteractionEvents {
            bait_placement: (self.enabled
                && bait_pressed
                && shortcuts.control
                && shortcuts.alt
                && point_in_rect(self.work_area, cursor))
            .then_some(cursor),
            quit_requested: shortcuts.quit_key && shortcuts.control && shortcuts.alt,
        }
    }

    #[must_use]
    pub fn poll(&mut self, cursor: Vec2) -> InteractionEvents {
        self.update(cursor, poll_shortcuts())
    }
}

pub fn cursor_position() -> Result<Vec2, InputError> {
    // SAFETY: A null source asks Core Graphics to create an ordinary event;
    // the returned retained object is released exactly once below.
    let event = unsafe { CGEventCreate(std::ptr::null()) };
    if event.is_null() {
        return Err(InputError(
            "Core Graphics did not provide the cursor position",
        ));
    }
    // SAFETY: event is a live CGEvent returned above.
    let point = unsafe { CGEventGetLocation(event) };
    // SAFETY: event is a retained Core Foundation object and has not yet been
    // released.
    unsafe { CFRelease(event) };
    let position = Vec2::new(point.x as f32, point.y as f32);
    if position.is_finite() {
        Ok(position)
    } else {
        Err(InputError(
            "Core Graphics returned a non-finite cursor position",
        ))
    }
}

#[must_use]
fn poll_shortcuts() -> ShortcutSample {
    ShortcutSample {
        control: key_down(CONTROL_KEY_CODE),
        alt: key_down(OPTION_KEY_CODE),
        bait_key: key_down(BAIT_KEY_CODE),
        quit_key: key_down(QUIT_KEY_CODE),
    }
}

#[must_use]
fn key_down(key: u16) -> bool {
    // SAFETY: The state selector and virtual key code are value parameters;
    // Core Graphics retains no pointer.
    unsafe { CGEventSourceKeyState(COMBINED_SESSION_STATE, key) }
}

#[allow(dead_code)]
#[must_use]
pub(crate) fn left_mouse_button_down() -> bool {
    // SAFETY: The state selector and mouse button are value parameters.
    unsafe { CGEventSourceButtonState(COMBINED_SESSION_STATE, LEFT_MOUSE_BUTTON) }
}

#[must_use]
fn point_in_rect(rectangle: Rect, point: Vec2) -> bool {
    rectangle.x.is_finite()
        && rectangle.y.is_finite()
        && rectangle.width.is_finite()
        && rectangle.height.is_finite()
        && rectangle.width > 0.0
        && rectangle.height > 0.0
        && point.is_finite()
        && point.x >= rectangle.x
        && point.x <= rectangle.x + rectangle.width
        && point.y >= rectangle.y
        && point.y <= rectangle.y + rectangle.height
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn shortcut_controller_is_edge_triggered() {
        let mut controller = InteractionController::new(
            true,
            Rect {
                x: 0.0,
                y: 0.0,
                width: 100.0,
                height: 100.0,
            },
        );
        let shortcut = ShortcutSample {
            control: true,
            alt: true,
            bait_key: true,
            quit_key: false,
        };
        assert_eq!(
            controller
                .update(Vec2::new(20.0, 30.0), shortcut)
                .bait_placement,
            Some(Vec2::new(20.0, 30.0))
        );
        assert_eq!(
            controller
                .update(Vec2::new(40.0, 50.0), shortcut)
                .bait_placement,
            None
        );
    }
}
