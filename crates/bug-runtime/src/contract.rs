//! Strongly typed data exchanged by the host, Lua behavior, and motion solver.

use std::error::Error;
use std::f32::consts::PI;
use std::fmt::{self, Display, Formatter};

use crate::math::{Vec2, canonical_zero};

pub const API_VERSION: i64 = 1;
pub const MAX_PARTS: usize = 64;
pub const MAX_IDENTIFIER_BYTES: usize = 64;
pub const MAX_STATE_BYTES: usize = 64;
pub const MAX_COORDINATE: f64 = 1_000_000.0;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ContractError {
    pub path: String,
    pub message: String,
}

impl ContractError {
    #[must_use]
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

impl Display for ContractError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{} {}", self.path, self.message)
    }
}

impl Error for ContractError {}

/// Converts one official-Lua `double` to the runtime's canonical `f32`.
///
/// This is the only conversion Lua readers should use for numeric output. It
/// rejects NaN, infinity and values outside both the caller's contract range
/// and the host `f32` range, then canonicalizes negative zero.
pub fn checked_f32(
    value: f64,
    minimum: f64,
    maximum: f64,
    path: impl Into<String>,
) -> Result<f32, ContractError> {
    let path = path.into();
    if !minimum.is_finite() || !maximum.is_finite() || minimum > maximum {
        return Err(ContractError::new(
            path,
            "has an invalid host validation range",
        ));
    }
    if !value.is_finite() {
        return Err(ContractError::new(
            path,
            "must be a finite number (not NaN or infinity)",
        ));
    }
    if !(minimum..=maximum).contains(&value) {
        return Err(ContractError::new(
            path,
            format!("must be in [{minimum}, {maximum}]"),
        ));
    }
    if !(-f64::from(f32::MAX)..=f64::from(f32::MAX)).contains(&value) {
        return Err(ContractError::new(path, "is outside the host f32 range"));
    }

    let converted = value as f32;
    if !converted.is_finite() {
        return Err(ContractError::new(path, "is outside the host f32 range"));
    }
    Ok(canonical_zero(converted))
}

pub fn checked_i32(
    value: f64,
    minimum: i32,
    maximum: i32,
    path: impl Into<String>,
) -> Result<i32, ContractError> {
    let path = path.into();
    if minimum > maximum {
        return Err(ContractError::new(
            path,
            "has an invalid host validation range",
        ));
    }
    if !value.is_finite() {
        return Err(ContractError::new(
            path,
            "must be a finite number (not NaN or infinity)",
        ));
    }
    if value.fract() != 0.0 || value < f64::from(minimum) || value > f64::from(maximum) {
        return Err(ContractError::new(
            path,
            format!("must be an integer in {minimum}..={maximum}"),
        ));
    }
    Ok(value as i32)
}

#[must_use]
pub fn is_valid_identifier(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= MAX_IDENTIFIER_BYTES
        && value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'_' | b'-'))
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

impl Rect {
    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite()
            && self.y.is_finite()
            && self.width.is_finite()
            && self.height.is_finite()
    }

    #[must_use]
    pub fn right(self) -> f32 {
        self.x + self.width
    }

    #[must_use]
    pub fn bottom(self) -> f32 {
        self.y + self.height
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct SourceRect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ScreenObstacle {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
    pub moving: bool,
}

impl ScreenObstacle {
    #[must_use]
    pub fn bounds(self) -> Rect {
        Rect {
            x: self.x,
            y: self.y,
            width: self.width,
            height: self.height,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CursorInput {
    pub valid: bool,
    pub position: Vec2,
    pub velocity: Vec2,
    pub left_button_down: bool,
    pub left_button_pressed: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BaitInput {
    pub active: bool,
    pub position: Vec2,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct CornerSensor {
    pub position: Vec2,
    pub distance: f32,
    pub blocked: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct ObstacleSensor {
    pub overlapping: bool,
    pub bait_blocked: bool,
    pub nearest_valid: bool,
    pub nearest_moving: bool,
    pub avoidance_direction: Vec2,
    pub obstacle_urgency: f32,
    pub moving_obstacle_urgency: f32,
    pub nearest_point: Vec2,
    pub nearest_away: Vec2,
    pub nearest_distance: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct MotionFeedback {
    pub actual_displacement: Vec2,
    pub overlapping: bool,
    pub blocked_time: f32,
    pub edge_dwell_time: f32,
    pub recovery_direction: Vec2,
    /// Clear travel distance found by the solver's fixed-direction probe.
    pub recovery_clearance: f32,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct BodyState {
    pub position: Vec2,
    pub heading: f32,
    pub speed: f32,
    pub length: f32,
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct FeatureFlags {
    pub single_instance: bool,
    pub extended_behaviors: bool,
    pub bait: bool,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct FrameInput {
    pub dt: f32,
    pub clock: f64,
    pub body: BodyState,
    pub world: Rect,
    pub cursor: CursorInput,
    pub bait: BaitInput,
    pub corners: [CornerSensor; 4],
    pub sensors: ObstacleSensor,
    pub feedback: MotionFeedback,
    pub features: FeatureFlags,
    pub request_corner_rest: bool,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionIntent {
    pub direction: Vec2,
    pub speed: f32,
    pub turn_rate: f32,
    pub acceleration: f32,
    pub lateral_speed: f32,
    pub recovery_probe_phase: f32,
    pub intentionally_still: bool,
    pub stop_immediately: bool,
    pub cancel_recovery: bool,
    pub allow_edge_rest: bool,
    pub initial_heading: Option<f32>,
}

impl Default for MotionIntent {
    fn default() -> Self {
        Self {
            direction: Vec2::ZERO,
            speed: 0.0,
            turn_rate: 0.0,
            acceleration: 0.0,
            lateral_speed: 0.0,
            recovery_probe_phase: 0.0,
            intentionally_still: true,
            stop_immediately: false,
            cancel_recovery: false,
            allow_edge_rest: false,
            initial_heading: None,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionLimits {
    pub maximum_speed: f32,
    pub maximum_turn_rate: f32,
    pub maximum_acceleration: f32,
    pub maximum_lateral_speed: f32,
}

impl Default for MotionLimits {
    fn default() -> Self {
        Self {
            maximum_speed: 4_096.0,
            maximum_turn_rate: 64.0,
            maximum_acceleration: 32_768.0,
            maximum_lateral_speed: 4_096.0,
        }
    }
}

impl MotionIntent {
    pub fn validate(self, limits: MotionLimits) -> Result<(), ContractError> {
        validate_positive_limit(limits.maximum_speed, "motion limit maximum_speed")?;
        validate_positive_limit(limits.maximum_turn_rate, "motion limit maximum_turn_rate")?;
        validate_positive_limit(
            limits.maximum_acceleration,
            "motion limit maximum_acceleration",
        )?;
        validate_positive_limit(
            limits.maximum_lateral_speed,
            "motion limit maximum_lateral_speed",
        )?;

        checked_vector(self.direction, -1.0, 1.0, "step.motion.direction")?;
        checked_f32(
            f64::from(self.speed),
            0.0,
            f64::from(limits.maximum_speed),
            "step.motion.speed",
        )?;
        checked_f32(
            f64::from(self.turn_rate),
            0.0,
            f64::from(limits.maximum_turn_rate),
            "step.motion.turn_rate",
        )?;
        checked_f32(
            f64::from(self.acceleration),
            0.0,
            f64::from(limits.maximum_acceleration),
            "step.motion.acceleration",
        )?;
        checked_f32(
            f64::from(self.lateral_speed),
            -f64::from(limits.maximum_lateral_speed),
            f64::from(limits.maximum_lateral_speed),
            "step.motion.lateral_speed",
        )?;
        checked_f32(
            f64::from(self.recovery_probe_phase),
            -MAX_COORDINATE,
            MAX_COORDINATE,
            "step.motion.recovery_probe_phase",
        )?;

        let direction_length = self.direction.length();
        if direction_length > 1.415 || (direction_length < 0.0001 && !self.intentionally_still) {
            return Err(ContractError::new(
                "step.motion.direction",
                "must be bounded and non-zero while moving",
            ));
        }
        if let Some(initial_heading) = self.initial_heading {
            checked_f32(
                f64::from(initial_heading),
                -f64::from(PI),
                f64::from(PI),
                "step.motion.initial_heading",
            )?;
        }
        Ok(())
    }
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Decision {
    pub state: String,
    pub target: Vec2,
    pub motion: MotionIntent,
    pub consume_bait: bool,
}

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct PartPose {
    pub rotation: f32,
    pub joint_offset: Vec2,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct Pose {
    pub body_offset: Vec2,
    pub body_rotation: f32,
    /// Part poses are stored in the stable manifest order.
    pub parts: Vec<PartPose>,
}

fn validate_positive_limit(value: f32, path: &'static str) -> Result<(), ContractError> {
    if !value.is_finite() || value <= 0.0 {
        return Err(ContractError::new(path, "must be finite and positive"));
    }
    Ok(())
}

fn checked_vector(
    value: Vec2,
    minimum: f32,
    maximum: f32,
    path: &str,
) -> Result<(), ContractError> {
    checked_f32(
        f64::from(value.x),
        f64::from(minimum),
        f64::from(maximum),
        format!("{path}.x"),
    )?;
    checked_f32(
        f64::from(value.y),
        f64::from(minimum),
        f64::from(maximum),
        format!("{path}.y"),
    )?;
    Ok(())
}
