#![cfg(windows)]

#[path = "../src/platform/mod.rs"]
mod platform;

use bug_runtime::contract::ScreenObstacle;
use bug_runtime::math::Vec2;
use platform::dpi::{automatic_body_length, resolution_scale};
use platform::interaction::{IconDragTracker, PointerSample, compose_icon_obstacles};

#[test]
fn automatic_body_size_is_resolution_based_not_dpi_logical_size() {
    assert_eq!(resolution_scale(1920, 1080), 1.0);
    assert_eq!(automatic_body_length(165.0, 2560, 1440), 220.0);
    assert_eq!(automatic_body_length(165.0, 3840, 2160), 330.0);
}

#[test]
fn dragged_icon_is_published_once_at_its_live_position() {
    let source = ScreenObstacle {
        x: -1810.0,
        y: 40.0,
        width: 94.0,
        height: 100.0,
        moving: false,
    };
    let mut tracker = IconDragTracker::new();
    let _ = tracker.update(
        PointerSample {
            position: Vec2::new(-1800.0, 50.0),
            left_button_down: true,
        },
        &[source],
        true,
    );
    let update = tracker.update(
        PointerSample {
            position: Vec2::new(-1780.0, 50.0),
            left_button_down: true,
        },
        &[source],
        true,
    );
    let published = compose_icon_obstacles(&[source], update.active);

    assert_eq!(published.len(), 1);
    assert!(published[0].moving);
    assert_ne!(published[0].x, source.x);
}
