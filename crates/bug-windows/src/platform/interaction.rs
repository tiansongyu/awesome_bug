//! Pure interaction state machines plus the small Win32 input polling shim.

use bug_runtime::contract::{Rect, ScreenObstacle};
use bug_runtime::math::Vec2;
use windows::Win32::Foundation::POINT;
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetAsyncKeyState, VK_CONTROL, VK_LBUTTON, VK_MENU,
};
use windows::Win32::UI::WindowsAndMessaging::GetCursorPos;

const DRAG_HIT_PADDING: f32 = 4.0;
const DRAG_START_DISTANCE: f32 = 6.0;
const DRAG_OBSTACLE_PADDING: f32 = 12.0;

/// One frame of pointer input.  Keeping this value platform-neutral makes the
/// drag inference deterministic and directly testable.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PointerSample {
    pub position: Vec2,
    pub left_button_down: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
struct DragCandidate {
    source_index: usize,
    source: ScreenObstacle,
    mouse_down: Vec2,
    pointer_offset: Vec2,
}

/// The moving obstacle inferred from an Explorer icon drag.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ActiveIconDrag {
    pub source_index: usize,
    pub source: ScreenObstacle,
    pub obstacle: ScreenObstacle,
}

/// Result of advancing the drag state machine by one pointer sample.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct DragUpdate {
    pub active: Option<ActiveIconDrag>,
    /// Explorer normally settles the icon rectangle shortly after release.
    /// Callers use this edge to bypass the regular 120 ms cache interval.
    pub refresh_requested: bool,
}

/// Infers Explorer's icon drag without installing hooks or intercepting input.
#[derive(Clone, Debug, Default)]
pub struct IconDragTracker {
    was_left_button_down: bool,
    candidate: Option<DragCandidate>,
    dragging: bool,
    settling: Option<ActiveIconDrag>,
}

impl IconDragTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn reset(&mut self) {
        self.was_left_button_down = false;
        self.candidate = None;
        self.dragging = false;
        self.settling = None;
    }

    /// Removes the release-position fallback after Explorer supplied a new,
    /// complete icon snapshot.
    pub fn acknowledge_snapshot(&mut self) {
        self.settling = None;
    }

    /// Advances drag inference.
    ///
    /// `icons` must be the last complete Explorer snapshot.  A failed Explorer
    /// refresh therefore does not cancel an in-progress drag.
    #[must_use]
    pub fn update(
        &mut self,
        sample: PointerSample,
        icons: &[ScreenObstacle],
        enabled: bool,
    ) -> DragUpdate {
        if !enabled || !sample.position.is_finite() {
            self.reset();
            return DragUpdate::default();
        }

        let pressed = sample.left_button_down && !self.was_left_button_down;
        let released = !sample.left_button_down && self.was_left_button_down;
        let mut refresh_requested = false;

        if pressed {
            self.settling = None;
            self.candidate = icons
                .iter()
                .copied()
                .enumerate()
                .find(|(_, icon)| contains(*icon, sample.position, DRAG_HIT_PADDING))
                .map(|(source_index, source)| {
                    let center = obstacle_center(source);
                    DragCandidate {
                        source_index,
                        source,
                        mouse_down: sample.position,
                        pointer_offset: sample.position - center,
                    }
                });
            self.dragging = false;
        } else if released {
            refresh_requested = self.candidate.is_some();
            self.settling = self.candidate.and_then(|candidate| {
                let crossed_threshold =
                    (sample.position - candidate.mouse_down).length() >= DRAG_START_DISTANCE;
                (self.dragging || crossed_threshold)
                    .then(|| active_drag(candidate, sample.position))
            });
            self.candidate = None;
            self.dragging = false;
        }

        if sample.left_button_down
            && !self.dragging
            && self.candidate.is_some_and(|candidate| {
                (sample.position - candidate.mouse_down).length() >= DRAG_START_DISTANCE
            })
        {
            self.dragging = true;
        }

        self.was_left_button_down = sample.left_button_down;
        let active = if self.dragging {
            self.candidate
                .map(|candidate| active_drag(candidate, sample.position))
        } else {
            self.settling
        };

        DragUpdate {
            active,
            refresh_requested,
        }
    }
}

/// Publishes one atomic obstacle snapshot, replacing the dragged icon's stale
/// rectangle with its moving rectangle.  The captured bounds are preferred if
/// Explorer reordered its list; otherwise the stable ListView item index is
/// used while Explorer updates the dragged rectangle itself.
#[must_use]
pub fn compose_icon_obstacles(
    icons: &[ScreenObstacle],
    active_drag: Option<ActiveIconDrag>,
) -> Vec<ScreenObstacle> {
    let mut result = Vec::with_capacity(icons.len() + usize::from(active_drag.is_some()));
    let excluded_index = active_drag.map(|drag| {
        icons
            .iter()
            .position(|icon| same_static_bounds(*icon, drag.source))
            .unwrap_or(drag.source_index)
    });
    for (index, icon) in icons.iter().copied().enumerate() {
        if excluded_index != Some(index) {
            result.push(icon);
        }
    }
    if let Some(drag) = active_drag {
        result.push(drag.obstacle);
    }
    result
}

#[must_use]
fn active_drag(candidate: DragCandidate, cursor: Vec2) -> ActiveIconDrag {
    let center = cursor - candidate.pointer_offset;
    let source = candidate.source;
    ActiveIconDrag {
        source_index: candidate.source_index,
        source,
        obstacle: ScreenObstacle {
            x: center.x - source.width * 0.5 - DRAG_OBSTACLE_PADDING,
            y: center.y - source.height * 0.5 - DRAG_OBSTACLE_PADDING,
            width: source.width + DRAG_OBSTACLE_PADDING * 2.0,
            height: source.height + DRAG_OBSTACLE_PADDING * 2.0,
            moving: true,
        },
    }
}

#[must_use]
fn obstacle_center(obstacle: ScreenObstacle) -> Vec2 {
    Vec2::new(
        obstacle.x + obstacle.width * 0.5,
        obstacle.y + obstacle.height * 0.5,
    )
}

#[must_use]
fn contains(obstacle: ScreenObstacle, point: Vec2, padding: f32) -> bool {
    point.x >= obstacle.x - padding
        && point.x <= obstacle.x + obstacle.width + padding
        && point.y >= obstacle.y - padding
        && point.y <= obstacle.y + obstacle.height + padding
}

#[must_use]
fn same_static_bounds(left: ScreenObstacle, right: ScreenObstacle) -> bool {
    const EPSILON: f32 = 0.25;
    !left.moving
        && !right.moving
        && (left.x - right.x).abs() <= EPSILON
        && (left.y - right.y).abs() <= EPSILON
        && (left.width - right.width).abs() <= EPSILON
        && (left.height - right.height).abs() <= EPSILON
}

/// Keyboard state passed to the safe desktop-pet shortcut controller.
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

/// Handles the single-pet bait shortcut without owning a window or renderer.
#[derive(Clone, Debug)]
pub struct InteractionController {
    enabled: bool,
    work_area: Rect,
    bait_key_was_down: bool,
    bait_position: Option<Vec2>,
}

impl InteractionController {
    #[must_use]
    pub fn new(enabled: bool, work_area: Rect) -> Self {
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

        let bait_placement = (self.enabled
            && bait_pressed
            && shortcuts.control
            && shortcuts.alt
            && point_in_rect(self.work_area, cursor))
        .then_some(cursor);
        if let Some(position) = bait_placement {
            self.bait_position = Some(position);
        }

        InteractionEvents {
            bait_placement,
            quit_requested: shortcuts.quit_key && shortcuts.control && shortcuts.alt,
        }
    }

    /// Polls Win32 keyboard state and advances the safe shortcut controller.
    #[must_use]
    pub fn poll(&mut self, cursor: Vec2) -> InteractionEvents {
        self.update(cursor, poll_shortcuts())
    }
}

#[must_use]
pub(crate) fn left_mouse_button_down() -> bool {
    key_down(i32::from(VK_LBUTTON.0))
}

/// Returns the cursor in physical virtual-screen pixels after PMv2
/// initialization.  Coordinates may be negative on displays left/above the
/// primary monitor.
pub fn cursor_position() -> windows::core::Result<Vec2> {
    let mut position = POINT::default();
    // SAFETY: position is valid writable storage for one POINT.
    unsafe {
        GetCursorPos(&mut position)?;
    }
    Ok(Vec2::new(position.x as f32, position.y as f32))
}

#[must_use]
fn poll_shortcuts() -> ShortcutSample {
    ShortcutSample {
        control: key_down(i32::from(VK_CONTROL.0)),
        alt: key_down(i32::from(VK_MENU.0)),
        bait_key: key_down(i32::from(b'F')),
        quit_key: key_down(i32::from(b'Q')),
    }
}

#[must_use]
fn key_down(virtual_key: i32) -> bool {
    // SAFETY: GetAsyncKeyState reads process-global input state and accepts
    // every i32 virtual-key value.  It does not dereference caller memory.
    unsafe { GetAsyncKeyState(virtual_key) & i16::MIN != 0 }
}

#[must_use]
fn point_in_rect(rectangle: Rect, point: Vec2) -> bool {
    rectangle.is_finite()
        && rectangle.width > 0.0
        && rectangle.height > 0.0
        && point.is_finite()
        && point.x >= rectangle.x
        && point.x < rectangle.right()
        && point.y >= rectangle.y
        && point.y < rectangle.bottom()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn icon() -> ScreenObstacle {
        ScreenObstacle {
            x: 91.0,
            y: 71.0,
            width: 94.0,
            height: 100.0,
            moving: false,
        }
    }

    #[test]
    fn drag_starts_only_after_six_physical_pixels() {
        let icons = [icon()];
        let mut tracker = IconDragTracker::new();
        let down = PointerSample {
            position: Vec2::new(100.0, 80.0),
            left_button_down: true,
        };
        assert_eq!(tracker.update(down, &icons, true).active, None);
        assert_eq!(
            tracker
                .update(
                    PointerSample {
                        position: Vec2::new(105.9, 80.0),
                        ..down
                    },
                    &icons,
                    true
                )
                .active,
            None
        );
        assert!(
            tracker
                .update(
                    PointerSample {
                        position: Vec2::new(106.0, 80.0),
                        ..down
                    },
                    &icons,
                    true
                )
                .active
                .is_some()
        );
    }

    #[test]
    fn moving_icon_replaces_old_rectangle_and_has_twelve_pixel_padding() {
        let icons = [icon(), ScreenObstacle { x: 250.0, ..icon() }];
        let mut tracker = IconDragTracker::new();
        let down = PointerSample {
            position: Vec2::new(100.0, 80.0),
            left_button_down: true,
        };
        let _ = tracker.update(down, &icons, true);
        let update = tracker.update(
            PointerSample {
                position: Vec2::new(112.0, 80.0),
                ..down
            },
            &icons,
            true,
        );
        let published = compose_icon_obstacles(&icons, update.active);

        assert_eq!(published.len(), 2);
        assert_eq!(published[0], icons[1]);
        assert!(published[1].moving);
        assert_eq!(published[1].width, icon().width + 24.0);
        assert_eq!(published[1].height, icon().height + 24.0);
    }

    #[test]
    fn release_requests_immediate_explorer_refresh() {
        let icons = [icon()];
        let mut tracker = IconDragTracker::new();
        let _ = tracker.update(
            PointerSample {
                position: Vec2::new(100.0, 80.0),
                left_button_down: true,
            },
            &icons,
            true,
        );
        let released = tracker.update(
            PointerSample {
                position: Vec2::new(130.0, 80.0),
                left_button_down: false,
            },
            &icons,
            true,
        );
        assert!(released.refresh_requested);
        assert!(released.active.is_some());

        tracker.acknowledge_snapshot();
        let settled = tracker.update(
            PointerSample {
                position: Vec2::new(130.0, 80.0),
                left_button_down: false,
            },
            &icons,
            true,
        );
        assert_eq!(settled.active, None);
    }

    #[test]
    fn work_area_accepts_negative_monitor_coordinates() {
        let mut controller = InteractionController::new(
            true,
            Rect {
                x: -1920.0,
                y: -120.0,
                width: 1920.0,
                height: 1080.0,
            },
        );
        let event = controller.update(
            Vec2::new(-800.0, 40.0),
            ShortcutSample {
                control: true,
                alt: true,
                bait_key: true,
                quit_key: false,
            },
        );
        assert_eq!(event.bait_placement, Some(Vec2::new(-800.0, 40.0)));
    }
}
