use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bug_runtime::contract::{
    BaitInput, BodyState, CornerSensor, CursorInput, Decision, FeatureFlags, FrameInput,
    MotionFeedback, MotionIntent, MotionLimits, ObstacleSensor, PartPose, Pose, Rect,
};
use bug_runtime::lua::{ControllerConfig, LuaHost};
use bug_runtime::math::Vec2;
use bug_runtime::rng::{RandomSample, TaggedRng};

const FIXTURE: &str = include_str!("fixtures/cockroach_cpp_oracle_v1.tsv");
const EXPECTED_FRAMES: usize = 2_400;

// These are the cross-toolchain numeric gates from the runtime design. State,
// events, optional values, booleans, and every RNG field remain exact.
const TARGET_EPSILON_PX: f32 = 0.01;
const SPEED_EPSILON_PX_PER_SECOND: f32 = 0.01;
const ANGLE_EPSILON_RAD: f32 = 1.0e-5;
const JOINT_EPSILON_PX: f32 = 0.01;
const MOTION_SCALAR_EPSILON: f32 = 0.01;
const DIRECTION_EPSILON: f32 = 1.0e-5;

#[derive(Debug)]
struct Oracle {
    frame_count: usize,
    random_count: usize,
    body_length: f32,
    speed_multiplier: f32,
    part_names: Vec<String>,
    inputs: Vec<FrameInput>,
    outputs: Vec<OracleOutput>,
    tape: Vec<RandomSample>,
}

#[derive(Debug)]
struct OracleOutput {
    decision: Decision,
    pose: Pose,
}

struct Fields<'a> {
    line: usize,
    values: std::str::Split<'a, char>,
}

impl<'a> Fields<'a> {
    fn new(line: usize, source: &'a str) -> Self {
        Self {
            line,
            values: source.split('\t'),
        }
    }

    fn text(&mut self, label: &str) -> &'a str {
        self.values
            .next()
            .unwrap_or_else(|| panic!("fixture line {} lacks {label}", self.line))
    }

    fn usize(&mut self, label: &str) -> usize {
        self.text(label)
            .parse()
            .unwrap_or_else(|error| panic!("fixture line {} invalid {label}: {error}", self.line))
    }

    fn boolean(&mut self, label: &str) -> bool {
        match self.text(label) {
            "0" => false,
            "1" => true,
            value => panic!(
                "fixture line {} invalid {label} boolean {value:?}",
                self.line
            ),
        }
    }

    fn f32(&mut self, label: &str) -> f32 {
        let value = self.text(label);
        let bits = u32::from_str_radix(value, 16).unwrap_or_else(|error| {
            panic!(
                "fixture line {} invalid {label} bits {value:?}: {error}",
                self.line
            )
        });
        f32::from_bits(bits)
    }

    fn f64(&mut self, label: &str) -> f64 {
        let value = self.text(label);
        let bits = u64::from_str_radix(value, 16).unwrap_or_else(|error| {
            panic!(
                "fixture line {} invalid {label} bits {value:?}: {error}",
                self.line
            )
        });
        f64::from_bits(bits)
    }

    fn vec2(&mut self, label: &str) -> Vec2 {
        Vec2::new(
            self.f32(&format!("{label}.x")),
            self.f32(&format!("{label}.y")),
        )
    }

    fn finish(mut self) {
        assert!(
            self.values.next().is_none(),
            "fixture line {} has trailing fields",
            self.line
        );
    }
}

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn parse_fixture() -> Oracle {
    let mut frame_count = None;
    let mut random_count = None;
    let mut body_length = None;
    let mut speed_multiplier = None;
    let mut part_names = Vec::new();
    let mut inputs = Vec::new();
    let mut outputs = Vec::new();
    let mut tape = Vec::new();

    for (line_index, line) in FIXTURE.lines().enumerate() {
        let line_number = line_index + 1;
        if line.is_empty() || line.starts_with('#') {
            continue;
        }
        let mut fields = Fields::new(line_number, line);
        match fields.text("record kind") {
            "M" => {
                let key = fields.text("metadata key");
                let value = fields.text("metadata value");
                match key {
                    "frame_count" => {
                        frame_count = Some(
                            value
                                .parse()
                                .expect("fixture frame_count must be an integer"),
                        );
                    }
                    "random_count" => {
                        random_count = Some(
                            value
                                .parse()
                                .expect("fixture random_count must be an integer"),
                        );
                    }
                    "body_length_bits" => {
                        body_length = Some(f32::from_bits(
                            u32::from_str_radix(value, 16)
                                .expect("fixture body length bits must be hex"),
                        ));
                    }
                    "speed_multiplier_bits" => {
                        speed_multiplier = Some(f32::from_bits(
                            u32::from_str_radix(value, 16)
                                .expect("fixture speed multiplier bits must be hex"),
                        ));
                    }
                    // The C++ seed documents provenance. Replay deliberately
                    // does not regenerate samples from either implementation.
                    "seed" => {
                        let _: u32 = value.parse().expect("fixture seed must be an integer");
                    }
                    other => panic!("fixture line {line_number} has unknown metadata {other:?}"),
                }
                fields.finish();
            }
            "P" => {
                let index = fields.usize("part index");
                assert_eq!(
                    index,
                    part_names.len(),
                    "fixture part indices must be contiguous"
                );
                part_names.push(fields.text("part name").to_owned());
                fields.finish();
            }
            "I" => {
                let index = fields.usize("frame index");
                assert_eq!(
                    index,
                    inputs.len(),
                    "fixture input frames must be contiguous"
                );
                inputs.push(parse_input(&mut fields));
                fields.finish();
            }
            "O" => {
                let index = fields.usize("frame index");
                assert_eq!(index, outputs.len(), "fixture outputs must be contiguous");
                outputs.push(parse_output(&mut fields, part_names.len()));
                fields.finish();
            }
            "R" => {
                let index = fields.usize("random index");
                assert_eq!(index, tape.len(), "fixture RNG rows must be contiguous");
                let tag = fields.text("random tag").to_owned();
                let low = fields.f32("random low");
                let high = fields.f32("random high");
                let value = fields.f32("random value");
                tape.push(RandomSample::new(tag, low, high, value));
                fields.finish();
            }
            kind => panic!("fixture line {line_number} has unknown record kind {kind:?}"),
        }
    }

    Oracle {
        frame_count: frame_count.expect("fixture lacks frame_count"),
        random_count: random_count.expect("fixture lacks random_count"),
        body_length: body_length.expect("fixture lacks body_length_bits"),
        speed_multiplier: speed_multiplier.expect("fixture lacks speed_multiplier_bits"),
        part_names,
        inputs,
        outputs,
        tape,
    }
}

fn parse_input(fields: &mut Fields<'_>) -> FrameInput {
    let dt = fields.f32("dt");
    let clock = fields.f64("clock");
    let body = BodyState {
        position: fields.vec2("body.position"),
        heading: fields.f32("body.heading"),
        speed: fields.f32("body.speed"),
        length: fields.f32("body.length"),
    };
    let world = Rect {
        x: fields.f32("world.x"),
        y: fields.f32("world.y"),
        width: fields.f32("world.width"),
        height: fields.f32("world.height"),
    };
    let cursor = CursorInput {
        valid: fields.boolean("cursor.valid"),
        position: fields.vec2("cursor.position"),
        velocity: fields.vec2("cursor.velocity"),
    };
    let bait = BaitInput {
        active: fields.boolean("bait.active"),
        position: fields.vec2("bait.position"),
    };
    let mut corners = [CornerSensor::default(); 4];
    for (index, corner) in corners.iter_mut().enumerate() {
        *corner = CornerSensor {
            position: fields.vec2(&format!("corners[{index}].position")),
            distance: fields.f32(&format!("corners[{index}].distance")),
            blocked: fields.boolean(&format!("corners[{index}].blocked")),
        };
    }
    let sensors = ObstacleSensor {
        overlapping: fields.boolean("sensors.overlapping"),
        bait_blocked: fields.boolean("sensors.bait_blocked"),
        nearest_valid: fields.boolean("sensors.nearest_valid"),
        nearest_moving: fields.boolean("sensors.nearest_moving"),
        avoidance_direction: fields.vec2("sensors.avoidance_direction"),
        obstacle_urgency: fields.f32("sensors.obstacle_urgency"),
        moving_obstacle_urgency: fields.f32("sensors.moving_obstacle_urgency"),
        nearest_point: fields.vec2("sensors.nearest_point"),
        nearest_away: fields.vec2("sensors.nearest_away"),
        nearest_distance: fields.f32("sensors.nearest_distance"),
    };
    let actual_displacement = fields.vec2("feedback.actual_displacement");
    let overlapping = fields.boolean("feedback.overlapping");
    let blocked_time = fields.f32("feedback.blocked_time");
    let edge_dwell_time = fields.f32("feedback.edge_dwell_time");
    let recovery_direction = fields.vec2("feedback.recovery_direction");
    // The frozen C++ v1 row contained a behavior-owned recovery timer. The
    // final contract keeps duration policy in Lua, so consume the provenance
    // field without reintroducing it into `MotionFeedback`.
    let _recorded_recovery_time = fields.f32("feedback.recovery_time");
    let feedback = MotionFeedback {
        actual_displacement,
        overlapping,
        blocked_time,
        edge_dwell_time,
        recovery_direction,
        recovery_clearance: 0.0,
    };
    let features = FeatureFlags {
        single_instance: fields.boolean("features.single_instance"),
        extended_behaviors: fields.boolean("features.extended_behaviors"),
        bait: fields.boolean("features.bait"),
    };
    let request_corner_rest = fields.boolean("request_corner_rest");
    FrameInput {
        dt,
        clock,
        body,
        world,
        cursor,
        bait,
        corners,
        sensors,
        feedback,
        features,
        request_corner_rest,
    }
}

fn parse_output(fields: &mut Fields<'_>, part_count: usize) -> OracleOutput {
    let state = fields.text("decision.state").to_owned();
    let consume_bait = fields.boolean("decision.consume_bait");
    let target = fields.vec2("decision.target");
    let direction = fields.vec2("decision.motion.direction");
    let speed = fields.f32("decision.motion.speed");
    let turn_rate = fields.f32("decision.motion.turn_rate");
    let acceleration = fields.f32("decision.motion.acceleration");
    let lateral_speed = fields.f32("decision.motion.lateral_speed");
    let recovery_probe_phase = fields.f32("decision.motion.recovery_probe_phase");
    let intentionally_still = fields.boolean("decision.motion.intentionally_still");
    let stop_immediately = fields.boolean("decision.motion.stop_immediately");
    let cancel_recovery = fields.boolean("decision.motion.cancel_recovery");
    let allow_edge_rest = fields.boolean("decision.motion.allow_edge_rest");
    let initial_heading_valid = fields.boolean("decision.motion.initial_heading_valid");
    let initial_heading = fields.f32("decision.motion.initial_heading");
    let decision = Decision {
        state,
        target,
        motion: MotionIntent {
            direction,
            speed,
            turn_rate,
            acceleration,
            lateral_speed,
            recovery_probe_phase,
            intentionally_still,
            stop_immediately,
            cancel_recovery,
            allow_edge_rest,
            initial_heading: initial_heading_valid.then_some(initial_heading),
        },
        consume_bait,
    };
    let body_offset = fields.vec2("pose.body_offset");
    let body_rotation = fields.f32("pose.body_rotation");
    let mut parts = Vec::with_capacity(part_count);
    for index in 0..part_count {
        parts.push(PartPose {
            rotation: fields.f32(&format!("pose.parts[{index}].rotation")),
            joint_offset: fields.vec2(&format!("pose.parts[{index}].joint_offset")),
        });
    }
    OracleOutput {
        decision,
        pose: Pose {
            body_offset,
            body_rotation,
            parts,
        },
    }
}

fn assert_near(actual: f32, expected: f32, epsilon: f32, frame: usize, field: &str) {
    let difference = (actual - expected).abs();
    assert!(
        difference <= epsilon,
        "frame {frame} {field}: actual={actual:?} expected={expected:?} \
         difference={difference:?} epsilon={epsilon:?}"
    );
}

fn assert_vector_near(actual: Vec2, expected: Vec2, epsilon: f32, frame: usize, field: &str) {
    assert_near(actual.x, expected.x, epsilon, frame, &format!("{field}.x"));
    assert_near(actual.y, expected.y, epsilon, frame, &format!("{field}.y"));
}

fn assert_output(frame: usize, actual: &Decision, pose: &Pose, expected: &OracleOutput) {
    let expected_decision = &expected.decision;
    assert_eq!(
        actual.state, expected_decision.state,
        "frame {frame} decision state"
    );
    assert_eq!(
        actual.consume_bait, expected_decision.consume_bait,
        "frame {frame} consume_bait event"
    );
    assert_eq!(
        actual.motion.intentionally_still, expected_decision.motion.intentionally_still,
        "frame {frame} intentionally_still"
    );
    assert_eq!(
        actual.motion.stop_immediately, expected_decision.motion.stop_immediately,
        "frame {frame} stop_immediately"
    );
    assert_eq!(
        actual.motion.cancel_recovery, expected_decision.motion.cancel_recovery,
        "frame {frame} cancel_recovery"
    );
    assert_eq!(
        actual.motion.allow_edge_rest, expected_decision.motion.allow_edge_rest,
        "frame {frame} allow_edge_rest"
    );
    assert_eq!(
        actual.motion.initial_heading.is_some(),
        expected_decision.motion.initial_heading.is_some(),
        "frame {frame} initial_heading presence"
    );

    assert_vector_near(
        actual.target,
        expected_decision.target,
        TARGET_EPSILON_PX,
        frame,
        "target",
    );
    assert_vector_near(
        actual.motion.direction,
        expected_decision.motion.direction,
        DIRECTION_EPSILON,
        frame,
        "motion.direction",
    );
    assert_near(
        actual.motion.speed,
        expected_decision.motion.speed,
        SPEED_EPSILON_PX_PER_SECOND,
        frame,
        "motion.speed",
    );
    assert_near(
        actual.motion.turn_rate,
        expected_decision.motion.turn_rate,
        MOTION_SCALAR_EPSILON,
        frame,
        "motion.turn_rate",
    );
    assert_near(
        actual.motion.acceleration,
        expected_decision.motion.acceleration,
        MOTION_SCALAR_EPSILON,
        frame,
        "motion.acceleration",
    );
    assert_near(
        actual.motion.lateral_speed,
        expected_decision.motion.lateral_speed,
        SPEED_EPSILON_PX_PER_SECOND,
        frame,
        "motion.lateral_speed",
    );
    assert_near(
        actual.motion.recovery_probe_phase,
        expected_decision.motion.recovery_probe_phase,
        ANGLE_EPSILON_RAD,
        frame,
        "motion.recovery_probe_phase",
    );
    if let (Some(actual), Some(expected)) = (
        actual.motion.initial_heading,
        expected_decision.motion.initial_heading,
    ) {
        assert_near(
            actual,
            expected,
            ANGLE_EPSILON_RAD,
            frame,
            "motion.initial_heading",
        );
    }

    assert_vector_near(
        pose.body_offset,
        expected.pose.body_offset,
        JOINT_EPSILON_PX,
        frame,
        "pose.body_offset",
    );
    assert_near(
        pose.body_rotation,
        expected.pose.body_rotation,
        ANGLE_EPSILON_RAD,
        frame,
        "pose.body_rotation",
    );
    assert_eq!(
        pose.parts.len(),
        expected.pose.parts.len(),
        "frame {frame} pose part count"
    );
    for (index, (actual, expected)) in pose
        .parts
        .iter()
        .zip(expected.pose.parts.iter())
        .enumerate()
    {
        assert_near(
            actual.rotation,
            expected.rotation,
            ANGLE_EPSILON_RAD,
            frame,
            &format!("pose.parts[{index}].rotation"),
        );
        assert_vector_near(
            actual.joint_offset,
            expected.joint_offset,
            JOINT_EPSILON_PX,
            frame,
            &format!("pose.parts[{index}].joint_offset"),
        );
    }
}

#[test]
fn official_lua_replays_the_cpp_migration_oracle_in_lockstep() {
    let oracle = parse_fixture();
    assert_eq!(oracle.frame_count, EXPECTED_FRAMES);
    assert_eq!(oracle.inputs.len(), oracle.frame_count);
    assert_eq!(oracle.outputs.len(), oracle.frame_count);
    assert_eq!(oracle.tape.len(), oracle.random_count);

    let root = source_root();
    let host = LuaHost::new(root.join("bugs/runtime/fsm.lua")).expect("checked-in FSM must load");
    let species = host
        .load_species(root.join("bugs/cockroach"))
        .expect("cockroach manifest must load");
    assert_eq!(
        species
            .parts
            .iter()
            .map(|part| part.name.as_str())
            .collect::<Vec<_>>(),
        oracle
            .part_names
            .iter()
            .map(String::as_str)
            .collect::<Vec<_>>(),
        "manifest part order changed relative to the C++ oracle"
    );
    assert_eq!(
        species.body.default_length.to_bits(),
        oracle.body_length.to_bits(),
        "manifest body length changed relative to the C++ oracle"
    );
    let behavior = host
        .load_behavior(&species)
        .expect("cockroach behavior must load");

    let random = Rc::new(RefCell::new(TaggedRng::replay(oracle.tape.clone())));
    let callback_random = Rc::clone(&random);
    let mut controller = behavior
        .create_controller(
            1,
            ControllerConfig {
                body_length: oracle.body_length,
                speed_multiplier: oracle.speed_multiplier,
                enable_extended_behaviors: true,
                motion_limits: MotionLimits::default(),
            },
            move |tag, low, high| {
                callback_random
                    .borrow_mut()
                    .draw(tag, low, high)
                    .map_err(|error| error.to_string())
            },
        )
        .expect("controller must start against the recorded C++ tape");

    let mut observed_states = BTreeSet::new();
    let mut consumed_bait = 0_usize;
    for (frame_index, (input, expected)) in
        oracle.inputs.iter().zip(oracle.outputs.iter()).enumerate()
    {
        let decision = controller
            .step(input)
            .unwrap_or_else(|error| panic!("frame {frame_index} step failed: {error}"));
        assert!(
            !controller.quarantined(),
            "frame {frame_index} controller quarantined: {:?}",
            controller.error()
        );
        let pose = controller
            .pose(input)
            .unwrap_or_else(|error| panic!("frame {frame_index} pose failed: {error}"));
        assert!(
            !controller.quarantined(),
            "frame {frame_index} pose quarantined controller: {:?}",
            controller.error()
        );
        observed_states.insert(decision.state.clone());
        consumed_bait += usize::from(decision.consume_bait);
        assert_output(frame_index, &decision, &pose, expected);
    }

    for required in [
        "wander",
        "creep",
        "pause",
        "startled",
        "flee",
        "seek-food",
        "feeding",
        "seek-corner",
        "lurk",
        "groom",
    ] {
        assert!(
            observed_states.contains(required),
            "lockstep fixture does not cover required behavior {required:?}: {observed_states:?}"
        );
    }
    assert_eq!(
        consumed_bait, 1,
        "the migration oracle must retain its one-shot bait event"
    );
    let random = random.borrow();
    assert_eq!(
        random.draw_count() as usize,
        oracle.random_count,
        "RNG call count changed"
    );
    random
        .require_replay_complete()
        .expect("RNG tag/range/value tape must be consumed exactly");
}
