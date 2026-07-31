//! Application-level orchestration of generic Lua bug instances.
//!
//! No species state name or behavior table appears here.  Lua decides intent
//! and pose; this module only assembles the stable frame contract, applies hard
//! geometry, and turns the validated pose into renderer-neutral draw commands.

use std::cell::RefCell;
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::rc::Rc;

use bug_runtime::contract::{
    BaitInput, CursorInput, Decision, FeatureFlags, FrameInput, MotionLimits, Rect, ScreenObstacle,
};
use bug_runtime::lua::{BehaviorModule, ControllerConfig, LuaController, LuaHost, ScriptError};
use bug_runtime::math::Vec2;
use bug_runtime::motion::{MotionError, MotionSolver, MotionSolverConfig};
use bug_runtime::rig::{RigError, RigPlan, RigPlanner};
use bug_runtime::rng::TaggedRng;
use bug_runtime::species::Species;

use crate::spawn::{SpawnPlan, SpawnSpec};

#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct WorldFrameInput<'obstacles> {
    pub dt: f32,
    pub clock: f64,
    pub cursor: CursorInput,
    pub bait: BaitInput,
    pub request_corner_rest: bool,
    pub obstacles: &'obstacles [ScreenObstacle],
}

#[derive(Clone, Debug, PartialEq)]
pub struct InstanceFrameOutput {
    pub instance_id: u64,
    pub decision: Decision,
    pub body: bug_runtime::contract::BodyState,
    pub feedback: bug_runtime::contract::MotionFeedback,
    pub rig: RigPlan,
    pub rng_draws: u64,
    pub quarantined: bool,
    /// Remaining intersection with a static hard obstacle after this step.
    /// The host may keep a newly appearing desktop snapshot visually strict
    /// while bounded separation proceeds without teleporting.
    pub overlaps_static: bool,
    /// Present only on the frame where this instance first becomes
    /// quarantined, so callers log one diagnostic instead of flooding.
    pub quarantine_diagnostic: Option<String>,
}

#[derive(Clone, Debug, Default, PartialEq)]
pub struct WorldFrameOutput {
    pub instances: Vec<InstanceFrameOutput>,
    pub consume_bait: bool,
}

#[derive(Debug)]
pub enum WorldError {
    InvalidConfiguration(&'static str),
    Script(ScriptError),
    Motion(MotionError),
    Rig(RigError),
}

impl Display for WorldError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        match self {
            Self::InvalidConfiguration(message) => formatter.write_str(message),
            Self::Script(error) => write!(formatter, "Lua runtime failed: {error}"),
            Self::Motion(error) => write!(formatter, "motion runtime failed: {error}"),
            Self::Rig(error) => write!(formatter, "sprite rig failed: {error}"),
        }
    }
}

impl Error for WorldError {
    fn source(&self) -> Option<&(dyn Error + 'static)> {
        match self {
            Self::Script(error) => Some(error),
            Self::Motion(error) => Some(error),
            Self::Rig(error) => Some(error),
            Self::InvalidConfiguration(_) => None,
        }
    }
}

impl From<ScriptError> for WorldError {
    fn from(error: ScriptError) -> Self {
        Self::Script(error)
    }
}

impl From<MotionError> for WorldError {
    fn from(error: MotionError) -> Self {
        Self::Motion(error)
    }
}

impl From<RigError> for WorldError {
    fn from(error: RigError) -> Self {
        Self::Rig(error)
    }
}

struct BugInstance {
    id: u64,
    controller: LuaController,
    random: Rc<RefCell<TaggedRng>>,
    solver: MotionSolver,
    rig: RigPlanner,
    body_scale: f32,
    quarantine_reported: bool,
}

/// Owns one Lua VM, one behavior module, and independent per-instance state.
///
/// Field order is intentional: controller registry keys are dropped before
/// the behavior module and `LuaHost`.
pub struct RuntimeWorld {
    instances: Vec<BugInstance>,
    _behavior: BehaviorModule,
    species: Species,
    _host: LuaHost,
    features: FeatureFlags,
    base_body_length: f32,
    world: Rect,
}

impl RuntimeWorld {
    pub fn new(
        host: LuaHost,
        behavior: BehaviorModule,
        species: Species,
        spawn_plan: SpawnPlan,
        world: Rect,
        base_body_length: f32,
        base_speed_multiplier: f32,
    ) -> Result<Self, WorldError> {
        validate_configuration(
            &species,
            &behavior,
            &spawn_plan,
            world,
            base_body_length,
            base_speed_multiplier,
        )?;

        let single_instance = spawn_plan.instances.len() == 1;
        let features = FeatureFlags {
            single_instance,
            extended_behaviors: single_instance,
            bait: single_instance && species.capabilities.bait,
        };
        let mut instances = Vec::with_capacity(spawn_plan.instances.len());

        for (index, spawn) in spawn_plan.instances.into_iter().enumerate() {
            let id = u64::try_from(index).unwrap_or(u64::MAX);
            instances.push(create_instance(
                id,
                &behavior,
                &species,
                spawn,
                world,
                base_body_length,
                base_speed_multiplier,
                features.extended_behaviors,
            )?);
        }

        Ok(Self {
            instances,
            _behavior: behavior,
            species,
            _host: host,
            features,
            base_body_length,
            world,
        })
    }

    #[must_use]
    pub fn species(&self) -> &Species {
        &self.species
    }

    #[must_use]
    pub fn instance_count(&self) -> usize {
        self.instances.len()
    }

    #[must_use]
    pub const fn features(&self) -> FeatureFlags {
        self.features
    }

    #[must_use]
    pub fn primary_body_length(&self) -> f32 {
        self.instances
            .first()
            .map_or(self.base_body_length, |instance| {
                instance.solver.body().length
            })
    }

    #[must_use]
    pub fn overlay_sizes(&self) -> Vec<u32> {
        self.instances
            .iter()
            .map(|instance| {
                overlay_size(
                    instance.solver.body().length,
                    self.species.body.overlay_scale,
                )
            })
            .collect()
    }

    pub fn ensure_atlas_dimensions(
        &self,
        actual_width: i32,
        actual_height: i32,
    ) -> Result<(), WorldError> {
        let Some(instance) = self.instances.first() else {
            return Err(WorldError::InvalidConfiguration(
                "runtime world has no instances",
            ));
        };
        instance
            .rig
            .ensure_atlas_dimensions(actual_width, actual_height)
            .map_err(WorldError::from)
    }

    /// Applies a display/work-area update without resetting Lua controllers or
    /// behavior RNG streams.  Automatic body scale changes are reflected in
    /// the next frame's `body.length`.
    pub fn reconfigure(&mut self, world: Rect, base_body_length: f32) -> Result<(), WorldError> {
        if !valid_world(world) || !finite_positive(base_body_length) {
            return Err(WorldError::InvalidConfiguration(
                "display reconfiguration must use a finite positive work area and body length",
            ));
        }
        for instance in &mut self.instances {
            let body_length = base_body_length * instance.body_scale;
            instance.controller.reconfigure_body_length(body_length)?;
            instance.solver.reconfigure(MotionSolverConfig {
                world,
                body_length,
                collider_half_width: self.species.body.collider_half_width,
                collider_half_length: self.species.body.collider_half_length,
            })?;
        }
        self.world = world;
        self.base_body_length = base_body_length;
        Ok(())
    }

    pub fn step(&mut self, input: WorldFrameInput<'_>) -> Result<WorldFrameOutput, WorldError> {
        if !input.dt.is_finite() || input.dt < 0.0 || !input.clock.is_finite() {
            return Err(WorldError::InvalidConfiguration(
                "frame time must be finite and non-negative",
            ));
        }

        let bait = if self.features.bait {
            input.bait
        } else {
            BaitInput::default()
        };
        let mut output = WorldFrameOutput {
            instances: Vec::with_capacity(self.instances.len()),
            consume_bait: false,
        };

        for instance in &mut self.instances {
            let mut frame = build_frame(
                instance,
                input.dt,
                input.clock,
                input.cursor,
                bait,
                self.world,
                self.features,
                input.request_corner_rest,
                input.obstacles,
            );
            let decision = instance.controller.step(&frame)?;
            let feedback = instance
                .solver
                .step(input.dt, decision.motion, input.obstacles);

            // Pose observes the actual hard-constrained result from this same
            // frame rather than the requested displacement.
            frame.body = instance.solver.body();
            frame.feedback = feedback;
            frame.sensors = instance.solver.sensors(input.obstacles, bait);
            frame.corners =
                std::array::from_fn(|index| instance.solver.corner(index, input.obstacles));
            let pose = instance.controller.pose(&frame)?;
            let canvas_size =
                overlay_size(frame.body.length, self.species.body.overlay_scale) as f32;
            let rig = instance.rig.plan(
                &pose,
                frame.body,
                Vec2::new(canvas_size * 0.5, canvas_size * 0.5),
            )?;

            let quarantined = instance.controller.quarantined();
            let quarantine_diagnostic = if quarantined && !instance.quarantine_reported {
                instance.quarantine_reported = true;
                Some(instance.controller.error().map_or_else(
                    || "Lua controller quarantined".to_owned(),
                    ToString::to_string,
                ))
            } else {
                None
            };
            let rng_draws = instance.random.borrow().draw_count();
            let overlaps_static = instance.solver.overlaps_static(input.obstacles);
            output.consume_bait |= decision.consume_bait;
            output.instances.push(InstanceFrameOutput {
                instance_id: instance.id,
                decision,
                body: frame.body,
                feedback,
                rig,
                rng_draws,
                quarantined,
                overlaps_static,
                quarantine_diagnostic,
            });
        }
        Ok(output)
    }
}

#[allow(clippy::too_many_arguments)]
fn create_instance(
    id: u64,
    behavior: &BehaviorModule,
    species: &Species,
    spawn: SpawnSpec,
    world: Rect,
    base_body_length: f32,
    base_speed_multiplier: f32,
    enable_extended_behaviors: bool,
) -> Result<BugInstance, WorldError> {
    let body_length = base_body_length * spawn.body_scale;
    let speed_multiplier = base_speed_multiplier * spawn.speed_scale;
    let random = Rc::new(RefCell::new(TaggedRng::generate(spawn.behavior_seed)));
    let weak_random = Rc::downgrade(&random);
    let controller = behavior.create_controller(
        id,
        ControllerConfig {
            body_length,
            speed_multiplier,
            enable_extended_behaviors,
            motion_limits: MotionLimits::default(),
        },
        move |tag, low, high| {
            let random = weak_random
                .upgrade()
                .ok_or_else(|| "instance random stream is no longer available".to_owned())?;
            let mut stream = random.borrow_mut();
            stream
                .draw(tag, low, high)
                .map_err(|error| error.to_string())
        },
    )?;
    let solver = MotionSolver::new(
        MotionSolverConfig {
            world,
            body_length,
            collider_half_width: species.body.collider_half_width,
            collider_half_length: species.body.collider_half_length,
        },
        spawn.position,
        0.0,
    )?;

    Ok(BugInstance {
        id,
        controller,
        random,
        solver,
        rig: RigPlanner::new(species),
        body_scale: spawn.body_scale,
        quarantine_reported: false,
    })
}

#[allow(clippy::too_many_arguments)]
fn build_frame(
    instance: &BugInstance,
    dt: f32,
    clock: f64,
    cursor: CursorInput,
    bait: BaitInput,
    world: Rect,
    features: FeatureFlags,
    request_corner_rest: bool,
    obstacles: &[ScreenObstacle],
) -> FrameInput {
    FrameInput {
        dt,
        clock,
        body: instance.solver.body(),
        world,
        cursor,
        bait,
        corners: std::array::from_fn(|index| instance.solver.corner(index, obstacles)),
        sensors: instance.solver.sensors(obstacles, bait),
        feedback: instance.solver.feedback(),
        features,
        request_corner_rest,
    }
}

fn validate_configuration(
    species: &Species,
    behavior: &BehaviorModule,
    spawn_plan: &SpawnPlan,
    world: Rect,
    base_body_length: f32,
    base_speed_multiplier: f32,
) -> Result<(), WorldError> {
    if spawn_plan.instances.is_empty() {
        return Err(WorldError::InvalidConfiguration(
            "runtime world needs at least one instance",
        ));
    }
    if behavior.species_id() != species.id {
        return Err(WorldError::InvalidConfiguration(
            "behavior module does not match the loaded species",
        ));
    }
    if behavior.part_count() != species.parts.len() {
        return Err(WorldError::InvalidConfiguration(
            "behavior module part table does not match the species manifest",
        ));
    }
    if !valid_world(world)
        || !finite_positive(base_body_length)
        || !finite_positive(base_speed_multiplier)
    {
        return Err(WorldError::InvalidConfiguration(
            "runtime world geometry and scale must be finite and positive",
        ));
    }
    if spawn_plan.instances.iter().any(|spawn| {
        !spawn.position.is_finite()
            || !finite_positive(spawn.body_scale)
            || !finite_positive(spawn.speed_scale)
    }) {
        return Err(WorldError::InvalidConfiguration(
            "spawn plan contains an invalid instance",
        ));
    }
    Ok(())
}

#[must_use]
fn overlay_size(body_length: f32, overlay_scale: f32) -> u32 {
    (body_length * overlay_scale).max(210.0).ceil() as u32
}

#[must_use]
fn valid_world(world: Rect) -> bool {
    world.is_finite() && world.width > 0.0 && world.height > 0.0
}

#[must_use]
fn finite_positive(value: f32) -> bool {
    value.is_finite() && value > 0.0
}

#[cfg(test)]
mod tests {
    use std::path::PathBuf;

    use crate::spawn::make_spawn_plan;

    use super::*;

    const INITIAL_WORLD: Rect = Rect {
        x: -1920.0,
        y: 0.0,
        width: 1920.0,
        height: 1040.0,
    };

    fn cockroach_world(count: usize) -> RuntimeWorld {
        let bugs = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../bugs");
        let host = LuaHost::new(bugs.join("runtime/fsm.lua")).expect("FSM");
        let species = host.load_species(bugs.join("cockroach")).expect("species");
        let behavior = host.load_behavior(&species).expect("behavior");
        let spawn_plan = make_spawn_plan(INITIAL_WORLD, count, 0x1234_5678).expect("spawn plan");
        RuntimeWorld::new(
            host,
            behavior,
            species,
            spawn_plan,
            INITIAL_WORLD,
            165.0,
            3.0,
        )
        .expect("runtime world")
    }

    #[test]
    fn single_and_swarm_modes_share_the_generic_runtime_contract() {
        let mut single = cockroach_world(1);
        assert_eq!(single.instance_count(), 1);
        assert_eq!(
            single.features(),
            FeatureFlags {
                single_instance: true,
                extended_behaviors: true,
                bait: true,
            }
        );

        let single_frame = single
            .step(WorldFrameInput {
                dt: 1.0 / 60.0,
                clock: 1.0 / 60.0,
                ..WorldFrameInput::default()
            })
            .expect("single frame");
        assert_eq!(single_frame.instances.len(), 1);
        assert!(!single_frame.instances[0].rig.commands.is_empty());

        let mut swarm = cockroach_world(20);
        assert_eq!(swarm.instance_count(), 20);
        assert_eq!(
            swarm.features(),
            FeatureFlags {
                single_instance: false,
                extended_behaviors: false,
                bait: false,
            }
        );
        let swarm_frame = swarm
            .step(WorldFrameInput {
                dt: 1.0 / 60.0,
                clock: 1.0 / 60.0,
                bait: BaitInput {
                    active: true,
                    position: Vec2::new(-1000.0, 400.0),
                },
                ..WorldFrameInput::default()
            })
            .expect("swarm frame");
        assert_eq!(swarm_frame.instances.len(), 20);
        assert!(swarm_frame.instances.iter().all(|instance| {
            !instance.decision.consume_bait && !instance.rig.commands.is_empty()
        }));
    }

    #[test]
    fn display_reconfiguration_preserves_controllers_and_rng_streams() {
        let mut world = cockroach_world(1);
        let first = world
            .step(WorldFrameInput {
                dt: 1.0 / 60.0,
                clock: 1.0 / 60.0,
                ..WorldFrameInput::default()
            })
            .expect("initial frame");
        let draws_before = first.instances[0].rng_draws;

        let replacement = Rect {
            x: 0.0,
            y: -120.0,
            width: 2560.0,
            height: 1400.0,
        };
        world.reconfigure(replacement, 220.0).expect("reconfigure");
        assert_eq!(world.primary_body_length(), 220.0);
        let next = world
            .step(WorldFrameInput {
                dt: 1.0 / 60.0,
                clock: 2.0 / 60.0,
                ..WorldFrameInput::default()
            })
            .expect("post-reconfigure frame");
        assert_eq!(next.instances[0].body.length, 220.0);
        assert!(next.instances[0].rng_draws >= draws_before);
        assert!(!next.instances[0].quarantined);
    }
}
