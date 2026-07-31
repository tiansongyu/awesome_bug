//! Deterministic instance spawning and safe bait placement.
//!
//! This module deliberately has no SDL or Win32 dependency.  The spawn stream
//! is separate from every behavior stream, so changing desktop layout or bug
//! count cannot silently perturb an existing controller's random sequence.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::time::{SystemTime, UNIX_EPOCH};

use bug_runtime::contract::{Rect, ScreenObstacle};
use bug_runtime::math::Vec2;
use bug_runtime::motion::{MotionError, MotionSolver, MotionSolverConfig};
use bug_runtime::rng::{RandomError, TaggedRng, derive_stream_seeds};

const SINGLE_SPAWN_MARGIN_FRACTION: f32 = 0.08;
const CELL_JITTER: f32 = 0.28;
const MINIMUM_SIZE_SCALE: f32 = 0.52;
const MAXIMUM_SIZE_SCALE: f32 = 1.02;
const MINIMUM_SPEED_SCALE: f32 = 0.82;
const MAXIMUM_SPEED_SCALE: f32 = 1.18;
const BAIT_RING_SAMPLES: usize = 24;
const BAIT_RING_COUNT: usize = 14;
const BAIT_GRID_COLUMNS: usize = 25;
const BAIT_GRID_ROWS: usize = 15;
const SPAWN_GRID_COLUMNS: usize = 80;
const SPAWN_GRID_ROWS: usize = 45;
const SPAWN_EDGE_CLEARANCE: f32 = 10.0;

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnSpec {
    pub position: Vec2,
    pub body_scale: f32,
    pub speed_scale: f32,
    pub behavior_seed: u32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct SpawnPlan {
    pub spawn_seed: u32,
    pub instances: Vec<SpawnSpec>,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SpawnGeometry {
    pub base_body_length: f32,
    pub collider_half_width: f32,
    pub collider_half_length: f32,
}

#[derive(Debug)]
pub enum SpawnError {
    InvalidWorld,
    InvalidCount,
    NoClearPosition(usize),
    Random(RandomError),
    Motion(MotionError),
}

impl Display for SpawnError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidWorld => {
                formatter.write_str("spawn work area must be a finite positive rectangle")
            }
            Self::InvalidCount => formatter.write_str("spawn count must be positive"),
            Self::NoClearPosition(index) => {
                write!(
                    formatter,
                    "no icon-free spawn point exists for instance {index}"
                )
            }
            Self::Random(error) => write!(formatter, "spawn random stream failed: {error}"),
            Self::Motion(error) => write!(formatter, "spawn geometry failed: {error}"),
        }
    }
}

impl Error for SpawnError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Random(error) => Some(error),
            Self::Motion(error) => Some(error),
            Self::InvalidWorld | Self::InvalidCount | Self::NoClearPosition(_) => None,
        }
    }
}

impl From<RandomError> for SpawnError {
    fn from(error: RandomError) -> Self {
        Self::Random(error)
    }
}

impl From<MotionError> for SpawnError {
    fn from(error: MotionError) -> Self {
        Self::Motion(error)
    }
}

/// Creates one spawn stream and one independent behavior stream per instance.
pub fn make_spawn_plan(
    world: Rect,
    count: usize,
    master_seed: u64,
) -> Result<SpawnPlan, SpawnError> {
    if !valid_world(world) {
        return Err(SpawnError::InvalidWorld);
    }
    if count == 0 {
        return Err(SpawnError::InvalidCount);
    }

    let seeds = derive_stream_seeds(master_seed, count);
    let mut random = TaggedRng::generate(seeds.spawn);
    let positions = make_spawn_points(world, count, &mut random)?;
    let mut instances = Vec::with_capacity(count);

    for (index, (position, behavior_seed)) in positions.into_iter().zip(seeds.instances).enumerate()
    {
        let (body_scale, speed_scale) = if count == 1 {
            (1.0, 1.0)
        } else {
            (
                draw_indexed(
                    &mut random,
                    "spawn.body_scale",
                    index,
                    MINIMUM_SIZE_SCALE,
                    MAXIMUM_SIZE_SCALE,
                )?,
                draw_indexed(
                    &mut random,
                    "spawn.speed_scale",
                    index,
                    MINIMUM_SPEED_SCALE,
                    MAXIMUM_SPEED_SCALE,
                )?,
            )
        };
        instances.push(SpawnSpec {
            position,
            body_scale,
            speed_scale,
            behavior_seed,
        });
    }

    Ok(SpawnPlan {
        spawn_seed: seeds.spawn,
        instances,
    })
}

/// Preserves the spawn/RNG contract and deterministically relocates only
/// bodies whose initial collider intersects an Explorer icon.
pub fn make_spawn_plan_avoiding_obstacles(
    world: Rect,
    count: usize,
    master_seed: u64,
    geometry: SpawnGeometry,
    obstacles: &[ScreenObstacle],
) -> Result<SpawnPlan, SpawnError> {
    let mut plan = make_spawn_plan(world, count, master_seed)?;
    for (index, spawn) in plan.instances.iter_mut().enumerate() {
        let config = MotionSolverConfig {
            world,
            body_length: geometry.base_body_length * spawn.body_scale,
            collider_half_width: geometry.collider_half_width,
            collider_half_length: geometry.collider_half_length,
        };
        let Some(position) = nearest_clear_spawn(config, spawn.position, obstacles)? else {
            return Err(SpawnError::NoClearPosition(index));
        };
        spawn.position = position;
    }
    Ok(plan)
}

fn nearest_clear_spawn(
    config: MotionSolverConfig,
    requested: Vec2,
    obstacles: &[ScreenObstacle],
) -> Result<Option<Vec2>, SpawnError> {
    if let Some(position) = checked_spawn_candidate(config, requested, obstacles)? {
        return Ok(Some(position));
    }

    let half_width = config.body_length * config.collider_half_width;
    let half_length = config.body_length * config.collider_half_length;
    let mut candidates = Vec::with_capacity(
        SPAWN_GRID_COLUMNS * SPAWN_GRID_ROWS + obstacles.len().saturating_mul(8) + 5,
    );
    candidates.extend([
        Vec2::new(config.world.x, config.world.y),
        Vec2::new(config.world.right(), config.world.y),
        Vec2::new(config.world.x, config.world.bottom()),
        Vec2::new(config.world.right(), config.world.bottom()),
        Vec2::new(
            config.world.x + config.world.width * 0.5,
            config.world.y + config.world.height * 0.5,
        ),
    ]);

    for obstacle in obstacles
        .iter()
        .copied()
        .filter(|item| valid_obstacle(*item))
    {
        let padding = if obstacle.moving { 8.0 } else { 2.0 } + SPAWN_EDGE_CLEARANCE;
        let left = obstacle.x - half_width - padding;
        let right = obstacle.x + obstacle.width + half_width + padding;
        let top = obstacle.y - half_length - padding;
        let bottom = obstacle.y + obstacle.height + half_length + padding;
        candidates.extend([
            Vec2::new(left, requested.y),
            Vec2::new(right, requested.y),
            Vec2::new(requested.x, top),
            Vec2::new(requested.x, bottom),
            Vec2::new(left, top),
            Vec2::new(right, top),
            Vec2::new(left, bottom),
            Vec2::new(right, bottom),
        ]);
    }

    for row in 0..SPAWN_GRID_ROWS {
        for column in 0..SPAWN_GRID_COLUMNS {
            candidates.push(Vec2::new(
                config.world.x
                    + config.world.width * (column as f32 + 0.5) / SPAWN_GRID_COLUMNS as f32,
                config.world.y + config.world.height * (row as f32 + 0.5) / SPAWN_GRID_ROWS as f32,
            ));
        }
    }
    candidates.sort_by(|left, right| {
        left.distance(requested)
            .total_cmp(&right.distance(requested))
            .then_with(|| left.x.total_cmp(&right.x))
            .then_with(|| left.y.total_cmp(&right.y))
    });

    for candidate in candidates {
        if let Some(position) = checked_spawn_candidate(config, candidate, obstacles)? {
            return Ok(Some(position));
        }
    }
    Ok(None)
}

fn checked_spawn_candidate(
    config: MotionSolverConfig,
    candidate: Vec2,
    obstacles: &[ScreenObstacle],
) -> Result<Option<Vec2>, SpawnError> {
    let solver = MotionSolver::new(config, candidate, 0.0)?;
    Ok((!solver.sensors(obstacles, Default::default()).overlapping)
        .then_some(solver.body().position))
}

fn make_spawn_points(
    world: Rect,
    count: usize,
    random: &mut TaggedRng,
) -> Result<Vec<Vec2>, SpawnError> {
    if count == 1 {
        let margin_x = world.width * SINGLE_SPAWN_MARGIN_FRACTION;
        let margin_y = world.height * SINGLE_SPAWN_MARGIN_FRACTION;
        return Ok(vec![Vec2::new(
            random.draw(
                "spawn.single.x",
                world.x + margin_x,
                world.right() - margin_x,
            )?,
            random.draw(
                "spawn.single.y",
                world.y + margin_y,
                world.bottom() - margin_y,
            )?,
        )]);
    }

    let aspect = world.width / world.height;
    let columns = ((count as f32 * aspect).sqrt().ceil() as usize).max(1);
    let rows = count.div_ceil(columns).max(1);
    let mut cells: Vec<usize> = (0..columns.saturating_mul(rows)).collect();
    fisher_yates_shuffle(&mut cells, random)?;

    let cell_width = world.width / columns as f32;
    let cell_height = world.height / rows as f32;
    let mut result = Vec::with_capacity(count);
    for (index, cell) in cells.into_iter().take(count).enumerate() {
        let column = cell % columns;
        let row = cell / columns;
        let jitter_x = draw_indexed(random, "spawn.jitter_x", index, -CELL_JITTER, CELL_JITTER)?;
        let jitter_y = draw_indexed(random, "spawn.jitter_y", index, -CELL_JITTER, CELL_JITTER)?;
        result.push(Vec2::new(
            world.x + (column as f32 + 0.5 + jitter_x) * cell_width,
            world.y + (row as f32 + 0.5 + jitter_y) * cell_height,
        ));
    }
    Ok(result)
}

fn fisher_yates_shuffle(values: &mut [usize], random: &mut TaggedRng) -> Result<(), SpawnError> {
    for upper in (1..values.len()).rev() {
        let tag = format!("spawn.shuffle.{upper}");
        let sample = random.draw(&tag, 0.0, (upper + 1) as f32)?;
        let selected = (sample.floor() as usize).min(upper);
        values.swap(upper, selected);
    }
    Ok(())
}

fn draw_indexed(
    random: &mut TaggedRng,
    prefix: &str,
    index: usize,
    low: f32,
    high: f32,
) -> Result<f32, SpawnError> {
    let tag = format!("{prefix}.{index}");
    random.draw(&tag, low, high).map_err(SpawnError::from)
}

/// Finds the nearest reachable bait point that is clear of desktop icons.
///
/// The local rings preserve the user's requested location when possible.  The
/// complete work-area grid is a final deterministic fallback for dense icon
/// layouts; `None` is preferable to placing unreachable food.
#[must_use]
pub fn find_safe_bait_position(
    requested: Vec2,
    world: Rect,
    body_length: f32,
    obstacles: &[ScreenObstacle],
) -> Option<Vec2> {
    if !valid_world(world) || !requested.is_finite() || !body_length.is_finite() {
        return None;
    }

    let bait_radius = (body_length.max(0.0) * 0.16).max(32.0);
    let minimum = Vec2::new(world.x + bait_radius, world.y + bait_radius);
    let maximum = Vec2::new(world.right() - bait_radius, world.bottom() - bait_radius);
    if minimum.x > maximum.x || minimum.y > maximum.y {
        return None;
    }

    let clamp_to_world = |point: Vec2| {
        Vec2::new(
            point.x.clamp(minimum.x, maximum.x),
            point.y.clamp(minimum.y, maximum.y),
        )
    };
    let is_clear = |point: Vec2| {
        obstacles.iter().all(|obstacle| {
            !valid_obstacle(*obstacle)
                || point.x < obstacle.x - bait_radius
                || point.x > obstacle.x + obstacle.width + bait_radius
                || point.y < obstacle.y - bait_radius
                || point.y > obstacle.y + obstacle.height + bait_radius
        })
    };

    let requested = clamp_to_world(requested);
    if is_clear(requested) {
        return Some(requested);
    }

    let ring_step = (bait_radius * 0.75).max(24.0);
    for ring in 1..=BAIT_RING_COUNT {
        let radius = ring as f32 * ring_step;
        for sample in 0..BAIT_RING_SAMPLES {
            let angle = std::f32::consts::TAU * sample as f32 / BAIT_RING_SAMPLES as f32;
            let candidate =
                clamp_to_world(requested + Vec2::new(angle.cos(), angle.sin()) * radius);
            if is_clear(candidate) {
                return Some(candidate);
            }
        }
    }

    let mut best = None;
    let mut best_distance = f32::MAX;
    for row in 0..BAIT_GRID_ROWS {
        for column in 0..BAIT_GRID_COLUMNS {
            let candidate = Vec2::new(
                minimum.x
                    + (maximum.x - minimum.x) * (column as f32 + 0.5) / BAIT_GRID_COLUMNS as f32,
                minimum.y + (maximum.y - minimum.y) * (row as f32 + 0.5) / BAIT_GRID_ROWS as f32,
            );
            if !is_clear(candidate) {
                continue;
            }
            let distance = candidate.distance(requested);
            if distance < best_distance {
                best = Some(candidate);
                best_distance = distance;
            }
        }
    }
    best
}

/// Mixes wall-clock time, the process id and a high-resolution platform
/// counter into a non-contractual default seed.  `--seed` remains the
/// reproducible path.
#[must_use]
pub fn default_master_seed(performance_counter: u64) -> u64 {
    let wall_clock = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_or(0, |duration| {
            duration.as_secs() ^ u64::from(duration.subsec_nanos()).rotate_left(21)
        });
    wall_clock ^ performance_counter.rotate_left(17) ^ u64::from(std::process::id()).rotate_left(43)
}

#[must_use]
fn valid_world(world: Rect) -> bool {
    world.is_finite() && world.width > 0.0 && world.height > 0.0
}

#[must_use]
fn valid_obstacle(obstacle: ScreenObstacle) -> bool {
    obstacle.x.is_finite()
        && obstacle.y.is_finite()
        && obstacle.width.is_finite()
        && obstacle.height.is_finite()
        && obstacle.width > 0.0
        && obstacle.height > 0.0
}

#[cfg(test)]
mod tests {
    use super::*;

    const WORLD: Rect = Rect {
        x: -1920.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };

    #[test]
    fn spawn_plan_is_deterministic_and_streams_are_independent() {
        let first = make_spawn_plan(WORLD, 20, 0x1234_5678).expect("spawn plan");
        let second = make_spawn_plan(WORLD, 20, 0x1234_5678).expect("spawn plan");
        assert_eq!(first, second);
        assert_eq!(first.instances.len(), 20);
        assert!(first.instances.iter().all(|item| {
            (MINIMUM_SIZE_SCALE..=MAXIMUM_SIZE_SCALE).contains(&item.body_scale)
                && (MINIMUM_SPEED_SCALE..=MAXIMUM_SPEED_SCALE).contains(&item.speed_scale)
        }));
        let mut seeds: Vec<u32> = first
            .instances
            .iter()
            .map(|item| item.behavior_seed)
            .collect();
        seeds.sort_unstable();
        seeds.dedup();
        assert_eq!(seeds.len(), 20);
    }

    #[test]
    fn single_instance_uses_inset_position_and_neutral_scales() {
        let plan = make_spawn_plan(WORLD, 1, 7).expect("spawn plan");
        let instance = plan.instances[0];
        assert_eq!(instance.body_scale, 1.0);
        assert_eq!(instance.speed_scale, 1.0);
        assert!(instance.position.x > WORLD.x);
        assert!(instance.position.x < WORLD.right());
        assert!(instance.position.y > WORLD.y);
        assert!(instance.position.y < WORLD.bottom());
    }

    #[test]
    fn bait_search_avoids_icons_and_can_scan_the_complete_work_area() {
        let icon = ScreenObstacle {
            x: 430.0,
            y: 230.0,
            width: 140.0,
            height: 140.0,
            moving: false,
        };
        let world = Rect {
            x: 0.0,
            y: 0.0,
            width: 1000.0,
            height: 600.0,
        };
        let result = find_safe_bait_position(Vec2::new(500.0, 300.0), world, 165.0, &[icon])
            .expect("a clear point exists");
        let radius = 32.0;
        assert!(
            result.x < icon.x - radius
                || result.x > icon.x + icon.width + radius
                || result.y < icon.y - radius
                || result.y > icon.y + icon.height + radius
        );
    }

    #[test]
    fn bait_search_declines_an_area_too_small_for_the_bait() {
        let tiny = Rect {
            x: 0.0,
            y: 0.0,
            width: 40.0,
            height: 40.0,
        };
        assert_eq!(
            find_safe_bait_position(Vec2::new(20.0, 20.0), tiny, 165.0, &[]),
            None
        );
    }

    #[test]
    fn bait_search_never_returns_a_requested_point_when_the_area_is_blocked() {
        let world = Rect {
            x: 0.0,
            y: 0.0,
            width: 1_000.0,
            height: 600.0,
        };
        let covering_icon = ScreenObstacle {
            x: -100.0,
            y: -100.0,
            width: 1_200.0,
            height: 800.0,
            moving: false,
        };
        assert_eq!(
            find_safe_bait_position(Vec2::new(500.0, 300.0), world, 165.0, &[covering_icon],),
            None
        );
    }

    #[test]
    fn spawn_relocation_is_deterministic_and_never_starts_inside_an_icon() {
        let geometry = SpawnGeometry {
            base_body_length: 165.0,
            collider_half_width: 0.20,
            collider_half_length: 0.43,
        };
        let original = make_spawn_plan(WORLD, 1, 99).expect("original plan");
        let center = original.instances[0].position;
        let icon = ScreenObstacle {
            x: center.x - 55.0,
            y: center.y - 55.0,
            width: 110.0,
            height: 110.0,
            moving: false,
        };
        let first =
            make_spawn_plan_avoiding_obstacles(WORLD, 1, 99, geometry, &[icon]).expect("safe plan");
        let second = make_spawn_plan_avoiding_obstacles(WORLD, 1, 99, geometry, &[icon])
            .expect("repeat safe plan");
        assert_eq!(first, second);
        assert_ne!(first.instances[0].position, center);

        let solver = MotionSolver::new(
            MotionSolverConfig {
                world: WORLD,
                body_length: geometry.base_body_length,
                collider_half_width: geometry.collider_half_width,
                collider_half_length: geometry.collider_half_length,
            },
            first.instances[0].position,
            0.0,
        )
        .expect("solver");
        assert!(!solver.sensors(&[icon], Default::default()).overlapping);
    }

    #[test]
    fn spawn_relocation_rejects_a_completely_blocked_work_area() {
        let geometry = SpawnGeometry {
            base_body_length: 165.0,
            collider_half_width: 0.20,
            collider_half_length: 0.43,
        };
        let covering_icon = ScreenObstacle {
            x: WORLD.x - 100.0,
            y: WORLD.y - 100.0,
            width: WORLD.width + 200.0,
            height: WORLD.height + 200.0,
            moving: false,
        };
        assert!(matches!(
            make_spawn_plan_avoiding_obstacles(WORLD, 1, 7, geometry, &[covering_icon]),
            Err(SpawnError::NoClearPosition(0))
        ));
    }
}
