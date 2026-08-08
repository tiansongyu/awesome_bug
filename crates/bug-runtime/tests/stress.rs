use std::cell::RefCell;
use std::collections::BTreeSet;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bug_runtime::contract::{BaitInput, FeatureFlags, FrameInput, MotionLimits, Rect};
use bug_runtime::lua::{ControllerConfig, LuaHost};
use bug_runtime::math::Vec2;
use bug_runtime::motion::{MotionSolver, MotionSolverConfig};
use bug_runtime::rig::RigPlanner;
use bug_runtime::rng::TaggedRng;

const FRAME_COUNT: usize = 100_000;
const DT: f32 = 1.0 / 60.0;

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

#[test]
fn cockroach_runtime_survives_one_hundred_thousand_integrated_frames() {
    run_integrated_stress(
        "cockroach",
        3.0,
        0x51a7_2e19,
        100_000,
        &["wander", "seek-corner", "lurk", "groom", "startled", "flee"],
    );
}

#[test]
fn turtle_runtime_survives_one_hundred_thousand_integrated_frames() {
    run_integrated_stress(
        "turtle",
        1.0,
        0x7a27_1e19,
        100_001,
        &["wander", "retreat", "seek-corner", "corner-rest"],
    );
}

fn run_integrated_stress(
    species_id: &str,
    speed_multiplier: f32,
    seed: u32,
    instance_id: u64,
    expected_states: &[&str],
) {
    let root = source_root();
    let host = LuaHost::new(root.join("bugs/runtime/fsm.lua")).expect("checked-in FSM must load");
    let species = host
        .load_species(root.join("bugs").join(species_id))
        .expect("species manifest must load");
    let behavior = host
        .load_behavior(&species)
        .expect("species behavior must load");
    let planner = RigPlanner::new(&species);
    let random = Rc::new(RefCell::new(TaggedRng::generate(seed)));
    let callback_random = Rc::clone(&random);
    let mut controller = behavior
        .create_controller(
            instance_id,
            ControllerConfig {
                body_length: species.body.default_length,
                speed_multiplier,
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
        .expect("controller must start");

    let world = Rect {
        x: -320.0,
        y: 40.0,
        width: 2_240.0,
        height: 1_120.0,
    };
    let mut solver = MotionSolver::new(
        MotionSolverConfig {
            world,
            body_length: species.body.default_length,
            collider_half_width: species.body.collider_half_width,
            collider_half_length: species.body.collider_half_length,
        },
        Vec2::new(800.0, 560.0),
        0.0,
    )
    .expect("valid solver configuration");
    let baseline_memory = host.used_memory_bytes();
    let mut states = BTreeSet::new();
    let mut bait_consumed = 0_u32;
    let mut bait_active = false;
    let mut clock = 0.0_f64;

    for index in 0..FRAME_COUNT {
        let cycle = index % 20_000;
        if cycle == 1_500 {
            bait_active = true;
        } else if cycle == 8_000 {
            bait_active = false;
        }

        let body = solver.body();
        let threat_frame = matches!(cycle, 11_000..=11_002);
        let bait = BaitInput {
            active: bait_active,
            position: Vec2::new(world.x + world.width * 0.72, world.y + world.height * 0.67),
        };
        let mut frame = FrameInput {
            dt: DT,
            clock,
            body,
            world,
            bait,
            sensors: solver.sensors(&[], bait),
            feedback: solver.feedback(),
            features: FeatureFlags {
                single_instance: true,
                extended_behaviors: true,
                bait: true,
            },
            request_corner_rest: cycle == 14_500,
            ..FrameInput::default()
        };
        frame.cursor.valid = threat_frame;
        frame.cursor.position = if threat_frame {
            body.position + Vec2::new(3.0, 3.0)
        } else {
            Vec2::new(world.x - 500.0, world.y - 500.0)
        };
        frame.cursor.velocity = if threat_frame {
            Vec2::new(420.0, 380.0)
        } else {
            Vec2::ZERO
        };
        for corner_index in 0..frame.corners.len() {
            frame.corners[corner_index] = solver.corner(corner_index, &[]);
        }

        let decision = controller.step(&frame).expect("step contract");
        states.insert(decision.state.clone());
        if decision.consume_bait {
            bait_active = false;
            bait_consumed = bait_consumed.saturating_add(1);
        }

        let feedback = solver.step(DT, decision.motion, &[]);
        frame.body = solver.body();
        frame.feedback = feedback;
        frame.sensors = solver.sensors(&[], frame.bait);
        let pose = controller.pose(&frame).expect("pose contract");
        let plan = planner
            .plan(&pose, frame.body, Vec2::new(180.0, 180.0))
            .expect("renderer-neutral rig plan");
        assert!(!controller.quarantined(), "{:?}", controller.error());
        assert!(frame.body.position.is_finite());
        assert!(frame.body.heading.is_finite());
        assert!(frame.body.speed.is_finite());
        assert_eq!(pose.parts.len(), species.parts.len());
        assert!(!plan.commands.is_empty());
        clock += f64::from(DT);
    }

    for expected in expected_states {
        assert!(
            states.contains(*expected),
            "long run never reached {expected}; observed {states:?}"
        );
    }
    assert!(
        states.contains("seek-food") || states.contains("feeding"),
        "bait behavior was never reached: {states:?}"
    );
    assert!(random.borrow().draw_count() > 100);
    assert!(bait_consumed <= 5);

    drop(controller);
    drop(behavior);
    host.collect_garbage().expect("Lua full collection");
    let retained = host.used_memory_bytes().saturating_sub(baseline_memory);
    assert!(
        retained <= 256 * 1024,
        "controller leaked {retained} bytes after full collection"
    );
}
