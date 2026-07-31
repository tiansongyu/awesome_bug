//! Platform-independent hard motion constraints.
//!
//! The solver owns geometry, not behavior.  It never chooses a state, target,
//! speed, or random recovery duration.  When progress is poor it reports the
//! best of 24 deterministic probes so Lua can decide what to do next.

use std::error::Error;
use std::f32::consts::{PI, TAU};
use std::fmt;

use crate::contract::{
    BaitInput, BodyState, CornerSensor, MotionFeedback, MotionIntent, ObstacleSensor, Rect,
    ScreenObstacle,
};
use crate::math::{Vec2, clamp, forward_from_heading, heading_from_direction, wrap_angle};

const WORK_AREA_GAP: f32 = 10.0;
const STATIC_PADDING: f32 = 2.0;
const MOVING_PADDING: f32 = 8.0;
const EDGE_DWELL_DISTANCE: f32 = 22.0;
const MAX_DT: f32 = 0.05;
const MAX_ROTATION_SUBSTEP: f32 = 5.0 * PI / 180.0;
const COLLISION_EPSILON: f32 = 0.01;
const PROBE_DIRECTION_COUNT: usize = 24;
const MAX_TIMER: f32 = 60.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MotionSolverConfig {
    pub world: Rect,
    pub body_length: f32,
    pub collider_half_width: f32,
    pub collider_half_length: f32,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub enum MotionError {
    InvalidWorld,
    InvalidBodyLength,
    InvalidCollider,
    InvalidInitialState,
}

impl fmt::Display for MotionError {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorld => write!(formatter, "motion world must be a finite positive rect"),
            Self::InvalidBodyLength => {
                write!(formatter, "motion body length must be finite and positive")
            }
            Self::InvalidCollider => write!(
                formatter,
                "motion collider half-width and half-length must be finite values in (0, 1]"
            ),
            Self::InvalidInitialState => {
                write!(
                    formatter,
                    "motion initial position and heading must be finite"
                )
            }
        }
    }
}

impl Error for MotionError {}

#[derive(Clone, Debug)]
pub struct MotionSolver {
    config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    desired_heading: f32,
    speed: f32,
    blocked_motion_time: f32,
    edge_dwell_time: f32,
    step_count: u64,
    feedback: MotionFeedback,
    overlap_escape_direction: Vec2,
}

impl MotionSolver {
    pub fn new(
        config: MotionSolverConfig,
        initial_position: Vec2,
        initial_heading: f32,
    ) -> Result<Self, MotionError> {
        validate_config(config)?;
        if !initial_position.is_finite() || !initial_heading.is_finite() {
            return Err(MotionError::InvalidInitialState);
        }
        let heading = wrap_angle(initial_heading);
        let position = clamp_to_world(config, initial_position, heading);
        Ok(Self {
            config,
            position,
            heading,
            desired_heading: heading,
            speed: 0.0,
            blocked_motion_time: 0.0,
            edge_dwell_time: 0.0,
            step_count: 0,
            feedback: MotionFeedback::default(),
            overlap_escape_direction: Vec2::ZERO,
        })
    }

    /// Replaces display/collider geometry while preserving behavior-owned
    /// motion state.  The position is clamped to the nearest legal work-area
    /// point; it is never wrapped to another edge.
    pub fn reconfigure(&mut self, config: MotionSolverConfig) -> Result<(), MotionError> {
        validate_config(config)?;
        self.config = config;
        self.position = clamp_to_world(self.config, self.position, self.heading);
        self.overlap_escape_direction = Vec2::ZERO;
        Ok(())
    }

    #[must_use]
    pub fn body(&self) -> BodyState {
        BodyState {
            position: self.position,
            heading: self.heading,
            speed: self.speed,
            length: self.config.body_length,
        }
    }

    #[must_use]
    pub const fn feedback(&self) -> MotionFeedback {
        self.feedback
    }

    /// Reports whether the current body intersects any static hard obstacle.
    ///
    /// The Windows host uses this only as a visibility gate when Explorer
    /// publishes a new snapshot underneath an existing pet. Motion and
    /// separation remain governed by `step`.
    #[must_use]
    pub fn overlaps_static(&self, obstacles: &[ScreenObstacle]) -> bool {
        obstacles.iter().copied().any(|obstacle| {
            valid_obstacle(obstacle)
                && !obstacle.moving
                && body_overlaps_obstacle(
                    self.config,
                    self.position,
                    self.heading,
                    obstacle,
                    STATIC_PADDING,
                )
        })
    }

    #[must_use]
    pub fn corner(&self, index: usize, obstacles: &[ScreenObstacle]) -> CornerSensor {
        let half_length = self.config.body_length * self.config.collider_half_length;
        let half_width = self.config.body_length * self.config.collider_half_width;
        let safe_extent = half_length.hypot(half_width);
        let margin = safe_extent + 12.0;
        let world = self.config.world;
        let corners = [
            Vec2::new(world.x + margin, world.y + margin),
            Vec2::new(world.right() - margin, world.y + margin),
            Vec2::new(world.x + margin, world.bottom() - margin),
            Vec2::new(world.right() - margin, world.bottom() - margin),
        ];
        let position = corners[index % corners.len()];
        let blocked = obstacles.iter().any(|obstacle| {
            valid_obstacle(*obstacle)
                && expanded_bounds(*obstacle, Vec2::new(safe_extent, safe_extent), 8.0)
                    .contains(position)
        });
        CornerSensor {
            position,
            distance: position.distance(self.position),
            blocked,
        }
    }

    /// Builds the narrow, allocation-free obstacle summary exposed to Lua.
    #[must_use]
    pub fn sensors(&self, obstacles: &[ScreenObstacle], bait: BaitInput) -> ObstacleSensor {
        let mut result = ObstacleSensor::default();
        let extents = collision_extents(self.config, self.heading);
        let current_forward = forward_from_heading(self.heading);
        let look_ahead_distance = clamp(
            self.speed * 0.12 + self.config.body_length * 0.18,
            self.config.body_length * 0.25,
            self.config.body_length * 0.90,
        );
        let look_ahead = self.position + current_forward * look_ahead_distance;
        let mut steering = Vec2::ZERO;
        let mut nearest = f32::MAX;

        for &obstacle in obstacles.iter().filter(|item| valid_obstacle(**item)) {
            let padding = obstacle_padding(obstacle);
            let collision_area = expanded_bounds(obstacle, extents, padding);
            let overlaps =
                body_overlaps_obstacle(self.config, self.position, self.heading, obstacle, padding);
            result.overlapping |= overlaps;
            if bait.active && collision_area.contains(bait.position) {
                result.bait_blocked = true;
            }

            let nearest_point = collision_area.closest_point(self.position);
            let mut away = self.position - nearest_point;
            let distance = away.length();
            if distance < nearest {
                nearest = distance;
                if away.length() < 0.0001 {
                    away = self.position - collision_area.center();
                }
                if away.length() < 0.0001 {
                    away = Vec2::new(-current_forward.y, current_forward.x);
                }
                result.nearest_valid = true;
                result.nearest_moving = obstacle.moving;
                result.nearest_point = nearest_point;
                result.nearest_away = away.normalized();
                result.nearest_distance = distance;
            }

            let influence_padding = if obstacle.moving { 10.0 } else { 4.0 };
            let influence_area = expanded_bounds(obstacle, extents, influence_padding);
            let sample = if overlaps { self.position } else { look_ahead };
            let closest = influence_area.closest_point(sample);
            let mut avoidance = sample - closest;
            let avoidance_distance = avoidance.length();
            let influence_distance =
                self.config.body_length * if obstacle.moving { 0.68 } else { 0.46 };

            let urgency = if overlaps || influence_area.contains(sample) {
                avoidance = sample - influence_area.center();
                if avoidance.length() < 0.0001 {
                    avoidance = Vec2::new(-current_forward.y, current_forward.x);
                }
                1.0
            } else if avoidance_distance < influence_distance {
                1.0 - avoidance_distance / influence_distance
            } else {
                continue;
            };

            avoidance = avoidance.normalized();
            let mut tangent = Vec2::new(-avoidance.y, avoidance.x);
            if tangent.dot(current_forward) < 0.0 {
                tangent = -tangent;
            }
            let away_weight = if obstacle.moving { 3.45 } else { 2.55 };
            let tangent_weight = if obstacle.moving { 1.05 } else { 0.78 };
            steering += (avoidance * away_weight + tangent * tangent_weight) * urgency;
            result.obstacle_urgency = result.obstacle_urgency.max(urgency);
            if obstacle.moving {
                result.moving_obstacle_urgency = result.moving_obstacle_urgency.max(urgency);
            }
        }

        result.avoidance_direction = steering.normalized();
        result
    }

    pub fn step(
        &mut self,
        dt: f32,
        intent: MotionIntent,
        obstacles: &[ScreenObstacle],
    ) -> MotionFeedback {
        let dt = if dt.is_finite() {
            clamp(dt, 0.0, MAX_DT)
        } else {
            0.0
        };
        let frame_start = self.position;

        if self.step_count == 0
            && let Some(initial_heading) = intent.initial_heading
            && initial_heading.is_finite()
        {
            self.heading = wrap_angle(initial_heading);
            self.desired_heading = self.heading;
            self.position = clamp_to_world(self.config, self.position, self.heading);
        }
        self.step_count = self.step_count.saturating_add(1);

        if intent.stop_immediately {
            self.speed = 0.0;
        }
        if intent.cancel_recovery {
            self.blocked_motion_time = 0.0;
            self.edge_dwell_time = 0.0;
        }

        let current_forward = forward_from_heading(self.heading);
        let mut direction = intent.direction.normalized();
        if direction.length_squared() < 0.000_001 {
            direction = current_forward;
        }
        if direction.is_finite() {
            self.desired_heading = heading_from_direction(direction);
        }

        self.update_edge_dwell(dt, intent.allow_edge_rest);
        self.apply_turn(dt, intent.turn_rate, obstacles);

        let target_speed = finite_nonnegative(intent.speed);
        let acceleration = finite_nonnegative(intent.acceleration);
        let speed_difference = target_speed - self.speed;
        self.speed += clamp(speed_difference, -acceleration * dt, acceleration * dt);
        self.speed = finite_nonnegative(self.speed);

        let forward = forward_from_heading(self.heading);
        let sideways = Vec2::new(self.heading.cos(), self.heading.sin());
        let lateral_speed = if intent.lateral_speed.is_finite() {
            intent.lateral_speed
        } else {
            0.0
        };
        let intended_displacement = (forward * self.speed + sideways * lateral_speed) * dt;

        let intended_destination = clamp_to_world(
            self.config,
            self.position + intended_displacement,
            self.heading,
        );
        let bounded_displacement = intended_destination - self.position;
        let allowed_fraction = earliest_static_collision_fraction(
            self.config,
            self.position,
            bounded_displacement,
            self.heading,
            obstacles,
        );
        self.position += bounded_displacement * allowed_fraction;
        self.position = clamp_to_world(self.config, self.position, self.heading);

        let overlapped = self.separate_existing_overlaps(dt, obstacles);
        self.position = clamp_to_world(self.config, self.position, self.heading);

        let actual_displacement = self.position - frame_start;
        let intended_distance = intended_displacement.length();
        let actual_distance = actual_displacement.length();
        let commanded_to_move = !intent.intentionally_still || overlapped;
        let insufficient_progress = intended_distance > 0.55
            && (actual_distance < 0.35 || actual_distance < intended_distance * 0.16);
        if commanded_to_move && (insufficient_progress || (overlapped && actual_distance < 0.75)) {
            self.blocked_motion_time = (self.blocked_motion_time + dt).min(MAX_TIMER);
        } else {
            self.blocked_motion_time = (self.blocked_motion_time - dt * 2.8).max(0.0);
        }

        let should_probe = !intent.cancel_recovery
            && (self.blocked_motion_time >= 0.16
                || (!intent.allow_edge_rest && self.edge_dwell_time >= 0.72));
        let (recovery_direction, recovery_clearance) = if should_probe {
            self.recovery_probe(intent.recovery_probe_phase, obstacles)
        } else {
            (Vec2::ZERO, 0.0)
        };

        self.feedback = MotionFeedback {
            actual_displacement,
            overlapping: overlapped,
            blocked_time: self.blocked_motion_time,
            edge_dwell_time: self.edge_dwell_time,
            recovery_direction,
            recovery_clearance,
        };
        self.feedback
    }

    fn update_edge_dwell(&mut self, dt: f32, allow_edge_rest: bool) {
        let extents = collision_extents(self.config, self.heading);
        let world = self.config.world;
        let nearest = (self.position.x - extents.x - world.x)
            .min(world.right() - self.position.x - extents.x)
            .min(self.position.y - extents.y - world.y)
            .min(world.bottom() - self.position.y - extents.y);
        if !allow_edge_rest && nearest < EDGE_DWELL_DISTANCE {
            self.edge_dwell_time = (self.edge_dwell_time + dt).min(MAX_TIMER);
        } else {
            self.edge_dwell_time = (self.edge_dwell_time - dt * 3.2).max(0.0);
        }
    }

    fn apply_turn(&mut self, dt: f32, turn_rate: f32, obstacles: &[ScreenObstacle]) {
        let rate = finite_nonnegative(turn_rate);
        let heading_error = wrap_angle(self.desired_heading - self.heading);
        let turn = clamp(heading_error, -rate * dt, rate * dt);
        if turn.abs() < f32::EPSILON {
            return;
        }

        // Checking small rotation increments prevents the body from rotating
        // through a nearby static icon even when translation is zero.
        let steps = (turn.abs() / MAX_ROTATION_SUBSTEP).ceil().max(1.0) as usize;
        let increment = turn / steps as f32;
        for _ in 0..steps {
            let candidate_heading = wrap_angle(self.heading + increment);
            let candidate_position = clamp_to_world(self.config, self.position, candidate_heading);
            let enters_new_static = obstacles.iter().any(|&obstacle| {
                valid_obstacle(obstacle)
                    && !obstacle.moving
                    && !body_overlaps_obstacle(
                        self.config,
                        self.position,
                        self.heading,
                        obstacle,
                        STATIC_PADDING,
                    )
                    && body_overlaps_obstacle(
                        self.config,
                        candidate_position,
                        candidate_heading,
                        obstacle,
                        STATIC_PADDING,
                    )
            });
            if enters_new_static {
                break;
            }
            self.heading = candidate_heading;
            self.position = candidate_position;
        }
    }

    /// Applies only a bounded hard correction.  It does not alter heading,
    /// speed, target, or any recovery timer.
    fn separate_existing_overlaps(&mut self, dt: f32, obstacles: &[ScreenObstacle]) -> bool {
        let mut remaining = (420.0 * dt + 1.5).min(12.0);
        let mut saw_overlap = false;

        for moving_pass in [true, false] {
            for _ in 0..8 {
                if remaining <= 0.001 {
                    break;
                }
                let overlap_indices: Vec<usize> = obstacles
                    .iter()
                    .enumerate()
                    .filter(|(_, obstacle)| {
                        valid_obstacle(**obstacle)
                            && obstacle.moving == moving_pass
                            && body_overlaps_obstacle(
                                self.config,
                                self.position,
                                self.heading,
                                **obstacle,
                                obstacle_padding(**obstacle),
                            )
                    })
                    .map(|(index, _)| index)
                    .collect();
                if overlap_indices.is_empty() {
                    break;
                }
                saw_overlap = true;

                let before_score =
                    total_penetration(self.config, self.position, self.heading, obstacles);
                let mut best_position = self.position;
                let mut best_improvement = 0.0_f32;
                let mut best_distance = 0.0_f32;

                for &index in &overlap_indices {
                    let obstacle = obstacles[index];
                    for direction in
                        separation_directions(self.config, self.position, self.heading, obstacle)
                    {
                        let candidate = clamp_to_world(
                            self.config,
                            self.position + direction * remaining,
                            self.heading,
                        );
                        let distance = candidate.distance(self.position);
                        if distance < 0.01
                            || enters_unrelated_static(
                                self.config,
                                self.position,
                                candidate,
                                self.heading,
                                obstacles,
                            )
                        {
                            continue;
                        }
                        let score =
                            total_penetration(self.config, candidate, self.heading, obstacles);
                        let improvement = before_score - score;
                        if improvement > best_improvement + 0.0001
                            || ((improvement - best_improvement).abs() <= 0.0001
                                && distance > best_distance)
                        {
                            best_position = candidate;
                            best_improvement = improvement;
                            best_distance = distance;
                        }
                    }
                }

                if best_improvement <= 0.0001 {
                    // Two opposing obstacles can form a penetration plateau:
                    // every short move reduces one overlap by exactly the
                    // amount it deepens the other. Requiring an immediate
                    // scalar improvement leaves the body hidden forever.
                    //
                    // Persist a geometry-probed route to clear space and take
                    // one bounded step along it. A short step may temporarily
                    // deepen one of the overlaps in a symmetric pinch, but it
                    // can never enter a previously unrelated static obstacle
                    // or exceed the per-frame separation budget.
                    if self.overlap_escape_direction.length_squared() < 0.5 {
                        let Some(direction) = self.overlap_escape_probe(obstacles) else {
                            break;
                        };
                        self.overlap_escape_direction = direction;
                    }
                    let candidate = clamp_to_world(
                        self.config,
                        self.position + self.overlap_escape_direction * remaining,
                        self.heading,
                    );
                    let distance = candidate.distance(self.position);
                    if distance < 0.01
                        || enters_unrelated_static(
                            self.config,
                            self.position,
                            candidate,
                            self.heading,
                            obstacles,
                        )
                    {
                        self.overlap_escape_direction = Vec2::ZERO;
                        break;
                    }
                    self.position = candidate;
                    remaining -= distance;
                    continue;
                }
                self.position = best_position;
                remaining -= best_distance;
            }
        }
        if !collides_with_any(self.config, self.position, self.heading, obstacles) {
            self.overlap_escape_direction = Vec2::ZERO;
        }
        saw_overlap
    }

    /// Finds the shortest deterministic path that exits only the obstacles
    /// covering the body at the start of the probe.
    ///
    /// Sampling is a route proof, not an integration step: `step` still
    /// advances by at most the separation budget. Rejecting any direction
    /// that touches a previously unrelated obstacle prevents an escape from
    /// trading one desktop-icon collision for another.
    fn overlap_escape_probe(&self, obstacles: &[ScreenObstacle]) -> Option<Vec2> {
        let mut initially_overlapping = vec![false; obstacles.len()];
        let mut overlap_count = 0_usize;
        for (index, &obstacle) in obstacles.iter().enumerate() {
            if valid_obstacle(obstacle)
                && body_overlaps_obstacle(
                    self.config,
                    self.position,
                    self.heading,
                    obstacle,
                    obstacle_padding(obstacle),
                )
            {
                initially_overlapping[index] = true;
                overlap_count += 1;
            }
        }
        if overlap_count == 0 {
            return None;
        }

        let bounds = legal_center_bounds(self.config, self.heading);
        let world_center = Vec2::new(
            self.config.world.x + self.config.world.width * 0.5,
            self.config.world.y + self.config.world.height * 0.5,
        );
        let inward = (world_center - self.position).normalized();
        let current_forward = forward_from_heading(self.heading);
        let probe_step = (self.config.body_length * 0.045).max(6.0);
        let probe_distance = (self.config.body_length * 3.2)
            .max(560.0)
            .min(self.config.world.width.hypot(self.config.world.height));
        let mut best_direction = None;
        let mut best_distance = f32::INFINITY;
        let mut best_bias = f32::NEG_INFINITY;

        for index in 0..PROBE_DIRECTION_COUNT {
            let angle = self.heading + TAU * index as f32 / PROBE_DIRECTION_COUNT as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let mut distance = probe_step;
            while distance <= probe_distance {
                let sample = self.position + direction * distance;
                if !bounds.contains(sample) {
                    break;
                }

                let mut blocked = false;
                let mut entered_unrelated = false;
                for (obstacle_index, &obstacle) in obstacles.iter().enumerate() {
                    if !valid_obstacle(obstacle)
                        || !body_overlaps_obstacle(
                            self.config,
                            sample,
                            self.heading,
                            obstacle,
                            obstacle_padding(obstacle),
                        )
                    {
                        continue;
                    }
                    blocked = true;
                    if !initially_overlapping[obstacle_index] {
                        entered_unrelated = true;
                        break;
                    }
                }
                if entered_unrelated {
                    break;
                }
                if !blocked {
                    let bias = direction.dot(inward) * 2.0 + direction.dot(current_forward);
                    if distance < best_distance - 0.001
                        || ((distance - best_distance).abs() <= 0.001 && bias > best_bias)
                    {
                        best_direction = Some(direction);
                        best_distance = distance;
                        best_bias = bias;
                    }
                    break;
                }
                distance += probe_step;
            }
        }
        best_direction.map(Vec2::normalized)
    }

    fn recovery_probe(
        &self,
        recovery_probe_phase: f32,
        obstacles: &[ScreenObstacle],
    ) -> (Vec2, f32) {
        let phase = if recovery_probe_phase.is_finite() {
            recovery_probe_phase
        } else {
            0.0
        };
        let world_center = Vec2::new(
            self.config.world.x + self.config.world.width * 0.5,
            self.config.world.y + self.config.world.height * 0.5,
        );
        let current_forward = forward_from_heading(self.heading);
        let mut inward = (world_center - self.position).normalized();
        if inward.length_squared() < 0.000_001 {
            inward = current_forward;
        }

        let bounds = legal_center_bounds(self.config, self.heading);
        let probe_step = (self.config.body_length * 0.045).max(6.0);
        let probe_distance = (self.config.body_length * 1.45).max(160.0);
        let starts_blocked = collides_with_any(self.config, self.position, self.heading, obstacles);

        let mut best_direction = inward;
        let mut best_clearance = 0.0;
        let mut best_score = f32::NEG_INFINITY;
        let angle_offset = self.heading + phase;

        for index in 0..PROBE_DIRECTION_COUNT {
            let angle = angle_offset + TAU * index as f32 / PROBE_DIRECTION_COUNT as f32;
            let direction = Vec2::new(angle.cos(), angle.sin());
            let mut reached_clear_space = !starts_blocked;
            let mut blocked_prefix = 0.0;
            let mut clear_distance = 0.0;
            let mut distance = probe_step;
            while distance <= probe_distance {
                let sample = self.position + direction * distance;
                if !bounds.contains(sample) {
                    break;
                }
                let blocked = collides_with_any(self.config, sample, self.heading, obstacles);
                if !reached_clear_space {
                    if blocked {
                        blocked_prefix += probe_step;
                        distance += probe_step;
                        continue;
                    }
                    reached_clear_space = true;
                } else if blocked {
                    break;
                }
                clear_distance += probe_step;
                distance += probe_step;
            }

            let mut score = clear_distance - blocked_prefix * 0.62
                + direction.dot(inward) * 18.0
                + direction.dot(current_forward) * 5.0;
            if !reached_clear_space {
                score -= probe_distance * 1.35;
            }
            if score > best_score {
                best_score = score;
                best_direction = direction;
                best_clearance = clear_distance;
            }
        }
        (best_direction.normalized(), best_clearance)
    }
}

fn validate_config(config: MotionSolverConfig) -> Result<(), MotionError> {
    if !config.world.is_finite() || config.world.width <= 0.0 || config.world.height <= 0.0 {
        return Err(MotionError::InvalidWorld);
    }
    if !config.body_length.is_finite() || config.body_length <= 0.0 {
        return Err(MotionError::InvalidBodyLength);
    }
    if !valid_collider_fraction(config.collider_half_width)
        || !valid_collider_fraction(config.collider_half_length)
    {
        return Err(MotionError::InvalidCollider);
    }
    Ok(())
}

fn valid_collider_fraction(value: f32) -> bool {
    value.is_finite() && value > 0.0 && value <= 1.0
}

fn finite_nonnegative(value: f32) -> f32 {
    if value.is_finite() {
        value.max(0.0)
    } else {
        0.0
    }
}

fn obstacle_padding(obstacle: ScreenObstacle) -> f32 {
    if obstacle.moving {
        MOVING_PADDING
    } else {
        STATIC_PADDING
    }
}

fn valid_obstacle(obstacle: ScreenObstacle) -> bool {
    obstacle.x.is_finite()
        && obstacle.y.is_finite()
        && obstacle.width.is_finite()
        && obstacle.height.is_finite()
        && obstacle.width > 0.0
        && obstacle.height > 0.0
}

#[derive(Clone, Copy, Debug)]
struct Bounds {
    left: f32,
    top: f32,
    right: f32,
    bottom: f32,
}

impl Bounds {
    fn contains(self, point: Vec2) -> bool {
        point.x >= self.left
            && point.x <= self.right
            && point.y >= self.top
            && point.y <= self.bottom
    }

    fn closest_point(self, point: Vec2) -> Vec2 {
        Vec2::new(
            clamp(point.x, self.left, self.right),
            clamp(point.y, self.top, self.bottom),
        )
    }

    fn center(self) -> Vec2 {
        Vec2::new(
            (self.left + self.right) * 0.5,
            (self.top + self.bottom) * 0.5,
        )
    }
}

fn expanded_bounds(obstacle: ScreenObstacle, extents: Vec2, padding: f32) -> Bounds {
    Bounds {
        left: obstacle.x - extents.x - padding,
        top: obstacle.y - extents.y - padding,
        right: obstacle.x + obstacle.width + extents.x + padding,
        bottom: obstacle.y + obstacle.height + extents.y + padding,
    }
}

fn legal_center_bounds(config: MotionSolverConfig, heading: f32) -> Bounds {
    let extents = collision_extents(config, heading);
    let mut left = config.world.x + extents.x + WORK_AREA_GAP;
    let mut right = config.world.right() - extents.x - WORK_AREA_GAP;
    let mut top = config.world.y + extents.y + WORK_AREA_GAP;
    let mut bottom = config.world.bottom() - extents.y - WORK_AREA_GAP;
    if left > right {
        left = config.world.x + config.world.width * 0.5;
        right = left;
    }
    if top > bottom {
        top = config.world.y + config.world.height * 0.5;
        bottom = top;
    }
    Bounds {
        left,
        top,
        right,
        bottom,
    }
}

fn clamp_to_world(config: MotionSolverConfig, point: Vec2, heading: f32) -> Vec2 {
    legal_center_bounds(config, heading).closest_point(point)
}

fn collision_extents(config: MotionSolverConfig, heading: f32) -> Vec2 {
    let half_length = config.body_length * config.collider_half_length;
    let half_width = config.body_length * config.collider_half_width;
    let sine = heading.sin().abs();
    let cosine = heading.cos().abs();
    Vec2::new(
        sine * half_length + cosine * half_width,
        cosine * half_length + sine * half_width,
    )
}

fn body_axes(heading: f32) -> (Vec2, Vec2) {
    (
        Vec2::new(heading.cos(), heading.sin()),
        forward_from_heading(heading),
    )
}

fn obstacle_projection(obstacle: ScreenObstacle, padding: f32, axis: Vec2) -> (f32, f32) {
    let center = Vec2::new(
        obstacle.x + obstacle.width * 0.5,
        obstacle.y + obstacle.height * 0.5,
    );
    let radius = (obstacle.width * 0.5 + padding) * axis.x.abs()
        + (obstacle.height * 0.5 + padding) * axis.y.abs();
    (center.dot(axis), radius)
}

fn body_radius(config: MotionSolverConfig, heading: f32, axis: Vec2) -> f32 {
    let (right, forward) = body_axes(heading);
    config.body_length * config.collider_half_width * right.dot(axis).abs()
        + config.body_length * config.collider_half_length * forward.dot(axis).abs()
}

fn collision_axes(heading: f32) -> [Vec2; 4] {
    let (right, forward) = body_axes(heading);
    [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), right, forward]
}

fn body_overlaps_obstacle(
    config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    obstacle: ScreenObstacle,
    padding: f32,
) -> bool {
    collision_axes(heading).into_iter().all(|axis| {
        let (obstacle_center, obstacle_radius) = obstacle_projection(obstacle, padding, axis);
        let radius = body_radius(config, heading, axis) + obstacle_radius;
        (position.dot(axis) - obstacle_center).abs() < radius - COLLISION_EPSILON
    })
}

fn penetration_depth(
    config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    obstacle: ScreenObstacle,
    padding: f32,
) -> f32 {
    let mut minimum = f32::MAX;
    for axis in collision_axes(heading) {
        let (obstacle_center, obstacle_radius) = obstacle_projection(obstacle, padding, axis);
        let overlap = body_radius(config, heading, axis) + obstacle_radius
            - (position.dot(axis) - obstacle_center).abs();
        if overlap <= COLLISION_EPSILON {
            return 0.0;
        }
        minimum = minimum.min(overlap);
    }
    minimum
}

fn total_penetration(
    config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    obstacles: &[ScreenObstacle],
) -> f32 {
    obstacles
        .iter()
        .copied()
        .filter(|obstacle| valid_obstacle(*obstacle))
        .map(|obstacle| {
            penetration_depth(
                config,
                position,
                heading,
                obstacle,
                obstacle_padding(obstacle),
            )
        })
        .sum()
}

fn separation_directions(
    _config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    obstacle: ScreenObstacle,
) -> [Vec2; 8] {
    let center = Vec2::new(
        obstacle.x + obstacle.width * 0.5,
        obstacle.y + obstacle.height * 0.5,
    );
    let (right, forward) = body_axes(heading);
    let axes = [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), right, forward];
    let mut result = [Vec2::ZERO; 8];
    for (index, axis) in axes.into_iter().enumerate() {
        let away = if (position - center).dot(axis) >= 0.0 {
            axis
        } else {
            -axis
        };
        result[index * 2] = away;
        // The opposite route can be the only legal exit when the nearest side
        // is pinned against the work-area edge.
        result[index * 2 + 1] = -away;
    }
    result
}

fn enters_unrelated_static(
    config: MotionSolverConfig,
    from: Vec2,
    to: Vec2,
    heading: f32,
    obstacles: &[ScreenObstacle],
) -> bool {
    obstacles.iter().copied().any(|obstacle| {
        valid_obstacle(obstacle)
            && !obstacle.moving
            && !body_overlaps_obstacle(config, from, heading, obstacle, STATIC_PADDING)
            && swept_collision_entry(config, from, to - from, heading, obstacle, STATIC_PADDING)
                .is_some()
    })
}

fn earliest_static_collision_fraction(
    config: MotionSolverConfig,
    start: Vec2,
    displacement: Vec2,
    heading: f32,
    obstacles: &[ScreenObstacle],
) -> f32 {
    if displacement.length_squared() < 0.000_001 {
        return 1.0;
    }
    if obstacles.iter().copied().any(|obstacle| {
        valid_obstacle(obstacle)
            && !obstacle.moving
            && body_overlaps_obstacle(config, start, heading, obstacle, STATIC_PADDING)
    }) {
        // A newly refreshed static icon can already cover the body.  Freeze
        // ordinary translation for this frame and let the bounded separation
        // pass resolve it; never tunnel through the whole icon in one step.
        return 0.0;
    }
    let mut earliest = 1.0_f32;
    let mut collision_found = false;
    for obstacle in obstacles.iter().copied().filter(|obstacle| {
        valid_obstacle(*obstacle)
            && !obstacle.moving
            && !body_overlaps_obstacle(config, start, heading, *obstacle, STATIC_PADDING)
    }) {
        if let Some(entry) = swept_collision_entry(
            config,
            start,
            displacement,
            heading,
            obstacle,
            STATIC_PADDING,
        ) {
            earliest = earliest.min(entry);
            collision_found = true;
        }
    }
    if collision_found {
        let safety_fraction = COLLISION_EPSILON / displacement.length().max(COLLISION_EPSILON);
        (earliest - safety_fraction).clamp(0.0, 1.0)
    } else {
        1.0
    }
}

fn swept_collision_entry(
    config: MotionSolverConfig,
    start: Vec2,
    displacement: Vec2,
    heading: f32,
    obstacle: ScreenObstacle,
    padding: f32,
) -> Option<f32> {
    let mut entry = f32::NEG_INFINITY;
    let mut exit = f32::INFINITY;
    for axis in collision_axes(heading) {
        let (obstacle_center, obstacle_radius) = obstacle_projection(obstacle, padding, axis);
        let body_radius = body_radius(config, heading, axis);
        let interval_low = obstacle_center - obstacle_radius - body_radius;
        let interval_high = obstacle_center + obstacle_radius + body_radius;
        let start_projection = start.dot(axis);
        let velocity = displacement.dot(axis);

        if velocity.abs() < 0.000_001 {
            if start_projection <= interval_low || start_projection >= interval_high {
                return None;
            }
            continue;
        }
        let first = (interval_low - start_projection) / velocity;
        let second = (interval_high - start_projection) / velocity;
        entry = entry.max(first.min(second));
        exit = exit.min(first.max(second));
        if entry > exit {
            return None;
        }
    }

    if exit <= 0.0 || entry > 1.0 || entry > exit {
        None
    } else {
        Some(entry.max(0.0))
    }
}

fn collides_with_any(
    config: MotionSolverConfig,
    position: Vec2,
    heading: f32,
    obstacles: &[ScreenObstacle],
) -> bool {
    obstacles.iter().copied().any(|obstacle| {
        valid_obstacle(obstacle)
            && body_overlaps_obstacle(
                config,
                position,
                heading,
                obstacle,
                obstacle_padding(obstacle),
            )
    })
}
