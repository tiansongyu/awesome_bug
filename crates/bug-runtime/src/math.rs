//! Small, deterministic two-dimensional math helpers used by the runtime.

use std::f32::consts::{PI, TAU};
use std::ops::{Add, AddAssign, Div, DivAssign, Mul, MulAssign, Neg, Sub, SubAssign};

/// A point or vector in screen coordinates.
///
/// Positive `x` points right and positive `y` points down.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Vec2 {
    pub x: f32,
    pub y: f32,
}

impl Vec2 {
    pub const ZERO: Self = Self { x: 0.0, y: 0.0 };

    #[must_use]
    pub const fn new(x: f32, y: f32) -> Self {
        Self { x, y }
    }

    #[must_use]
    pub fn is_finite(self) -> bool {
        self.x.is_finite() && self.y.is_finite()
    }

    #[must_use]
    pub fn length_squared(self) -> f32 {
        self.x * self.x + self.y * self.y
    }

    #[must_use]
    pub fn length(self) -> f32 {
        self.length_squared().sqrt()
    }

    /// Returns a unit vector, or zero when the magnitude is too small.
    #[must_use]
    pub fn normalized(self) -> Self {
        let magnitude = self.length();
        if magnitude > 0.0001 {
            self / magnitude
        } else {
            Self::ZERO
        }
    }

    #[must_use]
    pub fn dot(self, other: Self) -> f32 {
        self.x * other.x + self.y * other.y
    }

    #[must_use]
    pub fn distance(self, other: Self) -> f32 {
        (self - other).length()
    }

    #[must_use]
    pub fn rotated(self, angle: f32) -> Self {
        let (sine, cosine) = angle.sin_cos();
        Self {
            x: self.x * cosine - self.y * sine,
            y: self.x * sine + self.y * cosine,
        }
    }
}

impl Add for Vec2 {
    type Output = Self;

    fn add(self, other: Self) -> Self {
        Self {
            x: self.x + other.x,
            y: self.y + other.y,
        }
    }
}

impl AddAssign for Vec2 {
    fn add_assign(&mut self, other: Self) {
        self.x += other.x;
        self.y += other.y;
    }
}

impl Sub for Vec2 {
    type Output = Self;

    fn sub(self, other: Self) -> Self {
        Self {
            x: self.x - other.x,
            y: self.y - other.y,
        }
    }
}

impl SubAssign for Vec2 {
    fn sub_assign(&mut self, other: Self) {
        self.x -= other.x;
        self.y -= other.y;
    }
}

impl Mul<f32> for Vec2 {
    type Output = Self;

    fn mul(self, scalar: f32) -> Self {
        Self {
            x: self.x * scalar,
            y: self.y * scalar,
        }
    }
}

impl Mul<Vec2> for f32 {
    type Output = Vec2;

    fn mul(self, vector: Vec2) -> Vec2 {
        vector * self
    }
}

impl MulAssign<f32> for Vec2 {
    fn mul_assign(&mut self, scalar: f32) {
        self.x *= scalar;
        self.y *= scalar;
    }
}

impl Div<f32> for Vec2 {
    type Output = Self;

    fn div(self, scalar: f32) -> Self {
        Self {
            x: self.x / scalar,
            y: self.y / scalar,
        }
    }
}

impl DivAssign<f32> for Vec2 {
    fn div_assign(&mut self, scalar: f32) {
        self.x /= scalar;
        self.y /= scalar;
    }
}

impl Neg for Vec2 {
    type Output = Self;

    fn neg(self) -> Self {
        Self {
            x: -self.x,
            y: -self.y,
        }
    }
}

/// Clamps a finite scalar to an inclusive interval.
#[must_use]
pub fn clamp(value: f32, low: f32, high: f32) -> f32 {
    value.max(low).min(high)
}

/// Wraps a finite angle into `[-PI, PI]`.
///
/// The endpoints follow the legacy runtime: positive odd multiples of `PI`
/// map to `PI`, while negative odd multiples map to `-PI`.
#[must_use]
pub fn wrap_angle(angle: f32) -> f32 {
    if !angle.is_finite() {
        return 0.0;
    }

    let mut wrapped = (angle + PI).rem_euclid(TAU) - PI;
    if wrapped == -PI && angle > 0.0 {
        wrapped = PI;
    }
    canonical_zero(wrapped)
}

/// Rotates a local sprite-space point into screen space.
///
/// Local forward is negative Y; a positive heading turns clockwise on screen.
#[must_use]
pub fn rotate_local(local: Vec2, angle: f32) -> Vec2 {
    local.rotated(angle)
}

#[must_use]
pub fn forward_from_heading(heading: f32) -> Vec2 {
    Vec2::new(heading.sin(), -heading.cos())
}

#[must_use]
pub fn heading_from_direction(direction: Vec2) -> f32 {
    wrap_angle(direction.x.atan2(-direction.y))
}

/// Converts both signed representations of zero to positive zero.
#[must_use]
pub fn canonical_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}
