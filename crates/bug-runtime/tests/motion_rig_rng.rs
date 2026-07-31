use std::collections::BTreeMap;
use std::f32::consts::{FRAC_PI_2, PI, TAU};
use std::path::PathBuf;

use bug_runtime::contract::{
    BaitInput, BodyState, MotionIntent, PartPose, Pose, Rect, ScreenObstacle, SourceRect,
};
use bug_runtime::math::{Vec2, forward_from_heading};
use bug_runtime::motion::{MotionSolver, MotionSolverConfig};
use bug_runtime::rig::{ColorMod, DrawPass, RigError, RigPlanner};
use bug_runtime::rng::{
    Mt19937, RandomError, RandomSample, SplitMix64, TaggedRng, derive_seed, derive_stream_seeds,
};
use bug_runtime::species::{
    AtlasDefinition, BodyDefinition, Capabilities, PartDefinition, Species, VisualDefinition,
};

fn motion_config() -> MotionSolverConfig {
    MotionSolverConfig {
        world: Rect {
            x: 0.0,
            y: 0.0,
            width: 1_280.0,
            height: 720.0,
        },
        body_length: 165.0,
        collider_half_width: 0.20,
        collider_half_length: 0.43,
    }
}

fn moving_intent(direction: Vec2) -> MotionIntent {
    MotionIntent {
        direction,
        speed: 540.0,
        turn_rate: 8.0,
        acceleration: 1_350.0,
        intentionally_still: false,
        ..MotionIntent::default()
    }
}

fn make_solver(position: Vec2, heading: f32) -> MotionSolver {
    MotionSolver::new(motion_config(), position, heading).unwrap()
}

fn body_overlaps(body: BodyState, obstacle: ScreenObstacle, padding: f32) -> bool {
    let right = Vec2::new(body.heading.cos(), body.heading.sin());
    let forward = forward_from_heading(body.heading);
    let obstacle_center = Vec2::new(
        obstacle.x + obstacle.width * 0.5,
        obstacle.y + obstacle.height * 0.5,
    );
    let difference = body.position - obstacle_center;
    [Vec2::new(1.0, 0.0), Vec2::new(0.0, 1.0), right, forward]
        .into_iter()
        .all(|axis| {
            let body_radius = body.length * 0.20 * right.dot(axis).abs()
                + body.length * 0.43 * forward.dot(axis).abs();
            let obstacle_radius = (obstacle.width * 0.5 + padding) * axis.x.abs()
                + (obstacle.height * 0.5 + padding) * axis.y.abs();
            difference.dot(axis).abs() < body_radius + obstacle_radius - 0.009
        })
}

#[test]
fn static_overlap_visibility_probe_ignores_a_dragged_icon() {
    let solver = make_solver(Vec2::new(500.0, 360.0), 0.0);
    let static_icon = ScreenObstacle {
        x: 480.0,
        y: 330.0,
        width: 40.0,
        height: 60.0,
        moving: false,
    };
    assert!(solver.overlaps_static(&[static_icon]));
    assert!(!solver.overlaps_static(&[ScreenObstacle {
        moving: true,
        ..static_icon
    }]));
}

fn fixture_species(shadow_alpha: u8) -> Species {
    let parts = vec![
        PartDefinition {
            name: "body".to_owned(),
            source: SourceRect {
                x: 0,
                y: 0,
                width: 20,
                height: 30,
            },
            pivot: Vec2::new(4.0, 6.0),
            attachment: Vec2::new(0.10, -0.20),
            layer: 2,
        },
        PartDefinition {
            name: "left_leg".to_owned(),
            source: SourceRect {
                x: 20,
                y: 0,
                width: 8,
                height: 9,
            },
            pivot: Vec2::new(2.0, 3.0),
            attachment: Vec2::new(-0.20, 0.05),
            layer: -1,
        },
        PartDefinition {
            name: "right_leg".to_owned(),
            source: SourceRect {
                x: 28,
                y: 0,
                width: 8,
                height: 9,
            },
            pivot: Vec2::new(2.0, 3.0),
            attachment: Vec2::new(0.20, 0.05),
            layer: 2,
        },
    ];
    let part_indices = parts
        .iter()
        .enumerate()
        .map(|(index, part)| (part.name.clone(), index))
        .collect::<BTreeMap<_, _>>();
    Species {
        api_version: 1,
        id: "fixture".to_owned(),
        name: "Fixture".to_owned(),
        manifest_path: PathBuf::from("manifest.lua"),
        root: PathBuf::from("."),
        behavior_path: PathBuf::from("behavior.lua"),
        atlas: AtlasDefinition {
            file: PathBuf::from("atlas.png"),
            width: 64,
            height: 32,
            reference_length: 100.0,
        },
        body: BodyDefinition {
            default_length: 200.0,
            overlay_scale: 2.0,
            collider_half_width: 0.20,
            collider_half_length: 0.43,
            root_part: "body".to_owned(),
        },
        visual: VisualDefinition {
            red: 190,
            green: 180,
            blue: 170,
            alpha: 255,
            shadow_alpha,
            shadow_offset: Vec2::new(5.0, 6.0),
        },
        capabilities: Capabilities { bait: false },
        parts,
        root_part_index: 0,
        part_indices,
    }
}

fn fixture_pose() -> Pose {
    Pose {
        body_offset: Vec2::new(3.0, 4.0),
        body_rotation: 0.0,
        parts: vec![
            PartPose {
                rotation: 0.4,
                joint_offset: Vec2::new(1.0, 2.0),
            },
            PartPose {
                rotation: -0.2,
                joint_offset: Vec2::ZERO,
            },
            PartPose {
                rotation: 0.1,
                joint_offset: Vec2::new(-1.0, 1.0),
            },
        ],
    }
}

#[test]
fn mt19937_matches_the_reference_known_vector() {
    let expected = [
        3_499_211_612,
        581_869_302,
        3_890_346_734,
        3_586_334_585,
        545_404_204,
        4_161_255_391,
        3_922_919_429,
        949_333_985,
        2_715_962_298,
        1_323_567_403,
    ];
    let mut generator = Mt19937::new(5_489);
    for expected_word in expected {
        assert_eq!(generator.next_u32(), expected_word);
    }
}

#[test]
fn splitmix_and_stream_derivation_have_frozen_vectors() {
    let mut splitmix = SplitMix64::new(0);
    assert_eq!(splitmix.next_u64(), 0xe220_a839_7b1d_cdaf);
    assert_eq!(splitmix.next_u64(), 0x6e78_9e6a_a1b9_65f4);
    assert_eq!(splitmix.next_u64(), 0x06c4_5d18_8009_454f);

    assert_eq!(derive_seed(0, 0), 0x993d_6596);
    assert_eq!(derive_seed(0, 1), 0xcfc1_fb9e);
    assert_eq!(derive_seed(0, 2), 0x86cd_1857);
    assert_eq!(
        derive_stream_seeds(0, 2),
        bug_runtime::rng::StreamSeeds {
            spawn: 0x993d_6596,
            instances: vec![0xcfc1_fb9e, 0x86cd_1857],
        }
    );
}

#[test]
fn tagged_rng_mapping_record_and_replay_are_bit_exact() {
    let mut recorder = TaggedRng::recording(5_489);
    let first = recorder.draw("speed.fast", -10.0, 10.0).unwrap();
    let second = recorder.draw("turn.bias", -10.0, 10.0).unwrap();
    assert_eq!(first.to_bits(), 0x40c9_6c54);
    assert_eq!(second.to_bits(), 0xc0e9_4b75);
    assert_eq!(recorder.draw_count(), 2);

    let tape = recorder.tape().to_vec();
    assert_eq!(tape[0].low_bits, (-10.0_f32).to_bits());
    assert_eq!(tape[0].value_bits, first.to_bits());

    let mut replay = TaggedRng::replay(tape);
    assert_eq!(
        replay.draw("speed.fast", -10.0, 10.0).unwrap().to_bits(),
        first.to_bits()
    );
    assert!(!replay.replay_complete());
    assert_eq!(
        replay.draw("turn.bias", -10.0, 10.0).unwrap().to_bits(),
        second.to_bits()
    );
    assert!(replay.replay_complete());
    replay.require_replay_complete().unwrap();
}

#[test]
fn tagged_rng_rejects_bad_contracts_and_tape_mismatches() {
    let mut generated = TaggedRng::generate(7);
    assert!(matches!(
        generated.draw("", 0.0, 1.0),
        Err(RandomError::InvalidTagLength { .. })
    ));
    assert!(matches!(
        generated.draw("bad", f32::NAN, 1.0),
        Err(RandomError::InvalidRange { .. })
    ));
    assert!(matches!(
        generated.draw("bad", 2.0, 1.0),
        Err(RandomError::InvalidRange { .. })
    ));

    let sample = RandomSample::new("exact", 0.0, 1.0, 0.5);
    let mut replay = TaggedRng::replay(vec![sample.clone()]);
    assert!(matches!(
        replay.draw("other", 0.0, 1.0),
        Err(RandomError::TapeMismatch { draw: 0, .. })
    ));
    assert_eq!(replay.draw_count(), 0);
    assert!(!replay.replay_complete());
    assert!(matches!(
        replay.require_replay_complete(),
        Err(RandomError::TapeRemaining { samples: 1 })
    ));

    let mut signed_zero = TaggedRng::replay(vec![sample]);
    assert!(matches!(
        signed_zero.draw("exact", -0.0, 1.0),
        Err(RandomError::TapeMismatch { .. })
    ));

    let mut invalid_value = TaggedRng::replay(vec![RandomSample {
        tag: "invalid".to_owned(),
        low_bits: 0.0_f32.to_bits(),
        high_bits: 1.0_f32.to_bits(),
        value_bits: f32::NAN.to_bits(),
    }]);
    assert!(matches!(
        invalid_value.draw("invalid", 0.0, 1.0),
        Err(RandomError::InvalidTapeValue { .. })
    ));
}

#[test]
fn static_sweep_cannot_tunnel_through_a_one_pixel_obstacle() {
    let mut config = motion_config();
    config.body_length = 100.0;
    let mut solver = MotionSolver::new(config, Vec2::new(300.0, 360.0), FRAC_PI_2).unwrap();
    let obstacle = ScreenObstacle {
        x: 400.0,
        y: 250.0,
        width: 1.0,
        height: 220.0,
        moving: false,
    };
    let intent = MotionIntent {
        direction: Vec2::new(1.0, 0.0),
        speed: 4_000.0,
        turn_rate: 0.0,
        acceleration: 100_000.0,
        intentionally_still: false,
        ..MotionIntent::default()
    };
    solver.step(0.05, intent, &[obstacle]);
    let body = solver.body();
    assert!(!body_overlaps(body, obstacle, 2.0));
    assert!(
        body.position.x <= 355.01,
        "body crossed sweep boundary: {body:?}"
    );
}

#[test]
fn static_obstacles_and_work_area_remain_hard_constraints() {
    let mut solver = make_solver(Vec2::new(130.0, 360.0), FRAC_PI_2);
    let obstacle = ScreenObstacle {
        x: 570.0,
        y: 285.0,
        width: 110.0,
        height: 150.0,
        moving: false,
    };
    let intent = moving_intent(Vec2::new(1.0, 0.0));

    for _ in 0..1_800 {
        solver.step(1.0 / 60.0, intent, &[obstacle]);
        let body = solver.body();
        let sine = body.heading.sin().abs();
        let cosine = body.heading.cos().abs();
        let extent_x = sine * body.length * 0.43 + cosine * body.length * 0.20;
        let extent_y = cosine * body.length * 0.43 + sine * body.length * 0.20;
        assert!(body.position.x >= extent_x + 10.0 - 0.001);
        assert!(body.position.x <= 1_280.0 - extent_x - 10.0 + 0.001);
        assert!(body.position.y >= extent_y + 10.0 - 0.001);
        assert!(body.position.y <= 720.0 - extent_y - 10.0 + 0.001);
        assert!(!body_overlaps(body, obstacle, 2.0));
    }
}

#[test]
fn only_the_manifest_body_collider_blocks_motion() {
    let mut solver = make_solver(Vec2::new(200.0, 360.0), FRAC_PI_2);
    // This icon is outside the 33 px body half-width, although a full sprite,
    // leg, antenna, or overlay-sized collider would overlap it.
    let obstacle = ScreenObstacle {
        x: 350.0,
        y: 398.0,
        width: 100.0,
        height: 20.0,
        moving: false,
    };
    let intent = moving_intent(Vec2::new(1.0, 0.0));
    for _ in 0..100 {
        solver.step(1.0 / 60.0, intent, &[obstacle]);
    }
    assert!(solver.body().position.x > obstacle.x + obstacle.width);
}

#[test]
fn dragged_overlap_separates_with_a_strict_per_frame_budget() {
    let mut solver = make_solver(Vec2::new(620.0, 360.0), 0.0);
    let obstacle = ScreenObstacle {
        x: 575.0,
        y: 320.0,
        width: 90.0,
        height: 80.0,
        moving: true,
    };
    let still = MotionIntent::default();
    let budget = 420.0 / 60.0 + 1.5;
    let mut became_clear = false;
    for _ in 0..360 {
        let before = solver.body().position;
        let feedback = solver.step(1.0 / 60.0, still, &[obstacle]);
        let movement = solver.body().position.distance(before);
        assert!(
            movement <= budget + 0.001,
            "separation teleported by {movement}"
        );
        became_clear |= !body_overlaps(solver.body(), obstacle, 8.0);
        if became_clear {
            assert!(feedback.overlapping || movement == 0.0);
            break;
        }
    }
    assert!(became_clear, "moving overlap did not clear");
}

#[test]
fn opposing_static_icons_allow_a_bounded_plateau_escape() {
    let config = MotionSolverConfig {
        world: Rect {
            x: 0.0,
            y: 0.0,
            width: 1_000.0,
            height: 600.0,
        },
        body_length: 165.0,
        collider_half_width: 0.20,
        collider_half_length: 0.43,
    };
    let mut solver = MotionSolver::new(config, Vec2::new(500.0, 300.0), 0.0).unwrap();
    let obstacles = [
        ScreenObstacle {
            x: 430.0,
            y: 250.0,
            width: 65.0,
            height: 100.0,
            moving: false,
        },
        ScreenObstacle {
            x: 505.0,
            y: 250.0,
            width: 65.0,
            height: 100.0,
            moving: false,
        },
        // Never overlapped at the start. Escaping the pinch must not trade it
        // for a collision with an unrelated icon.
        ScreenObstacle {
            x: 430.0,
            y: 410.0,
            width: 140.0,
            height: 60.0,
            moving: false,
        },
    ];
    let intent = MotionIntent {
        direction: Vec2::new(0.0, -1.0),
        speed: 300.0,
        turn_rate: 8.0,
        acceleration: 10_000.0,
        intentionally_still: false,
        ..MotionIntent::default()
    };
    assert!(body_overlaps(solver.body(), obstacles[0], 2.0));
    assert!(body_overlaps(solver.body(), obstacles[1], 2.0));
    assert!(!body_overlaps(solver.body(), obstacles[2], 2.0));

    let budget = 420.0 / 60.0 + 1.5;
    let mut cleared = false;
    for _ in 0..600 {
        let before = solver.body().position;
        solver.step(1.0 / 60.0, intent, &obstacles);
        let movement = solver.body().position.distance(before);
        assert!(
            movement <= budget + 0.001,
            "plateau escape teleported by {movement}"
        );
        assert!(
            !body_overlaps(solver.body(), obstacles[2], 2.0),
            "escape entered an unrelated icon"
        );
        if !solver.overlaps_static(&obstacles[..2]) {
            cleared = true;
            break;
        }
    }
    assert!(
        cleared,
        "opposing static icon pinch did not clear: {:?}",
        solver.body()
    );
}

#[test]
fn edge_clamping_and_reconfiguration_never_wrap_to_the_other_side() {
    let mut solver = make_solver(Vec2::new(1_250.0, 360.0), FRAC_PI_2);
    let old_x = solver.body().position.x;
    let mut smaller = motion_config();
    smaller.world.width = 900.0;
    solver.reconfigure(smaller).unwrap();
    let new_x = solver.body().position.x;
    assert!(new_x < old_x);
    assert!(new_x > smaller.world.width * 0.5);

    let intent = moving_intent(Vec2::new(1.0, 0.0));
    for _ in 0..120 {
        let before = solver.body().position;
        solver.step(1.0 / 60.0, intent, &[]);
        let after = solver.body().position;
        assert!(after.x >= smaller.world.x);
        assert!(after.distance(before) < 20.0);
    }
}

#[test]
fn blocked_solver_reports_a_deterministic_24_direction_probe() {
    let obstacle = ScreenObstacle {
        x: 560.0,
        y: 0.0,
        width: 180.0,
        height: 720.0,
        moving: false,
    };
    let intent = moving_intent(Vec2::new(1.0, 0.0));
    let mut first = make_solver(Vec2::new(450.0, 360.0), FRAC_PI_2);
    let mut second = make_solver(Vec2::new(450.0, 360.0), FRAC_PI_2);
    let mut feedback = Default::default();
    for _ in 0..120 {
        feedback = first.step(1.0 / 60.0, intent, &[obstacle]);
        let other = second.step(1.0 / 60.0, intent, &[obstacle]);
        assert_eq!(
            feedback.recovery_direction.x.to_bits(),
            other.recovery_direction.x.to_bits()
        );
        assert_eq!(
            feedback.recovery_direction.y.to_bits(),
            other.recovery_direction.y.to_bits()
        );
        assert_eq!(
            feedback.recovery_clearance.to_bits(),
            other.recovery_clearance.to_bits()
        );
    }
    assert!(feedback.blocked_time >= 0.16);
    assert!(feedback.recovery_direction.length() > 0.99);
    assert!(feedback.recovery_clearance > 0.0);

    let cancelled = MotionIntent {
        cancel_recovery: true,
        ..intent
    };
    let feedback = first.step(1.0 / 60.0, cancelled, &[obstacle]);
    assert!(feedback.blocked_time < 0.16);
    assert_eq!(feedback.recovery_direction, Vec2::ZERO);
    assert_eq!(feedback.recovery_clearance, 0.0);
}

#[test]
fn sensors_and_corners_are_geometry_only_summaries() {
    let solver = make_solver(Vec2::new(640.0, 360.0), 0.0);
    let obstacles = [
        ScreenObstacle {
            x: 0.0,
            y: 0.0,
            width: 260.0,
            height: 240.0,
            moving: false,
        },
        ScreenObstacle {
            x: 610.0,
            y: 330.0,
            width: 60.0,
            height: 60.0,
            moving: true,
        },
    ];
    assert!(solver.corner(0, &obstacles).blocked);
    assert!(!solver.corner(3, &obstacles).blocked);

    let sensors = solver.sensors(
        &obstacles,
        BaitInput {
            active: true,
            position: Vec2::new(630.0, 350.0),
        },
    );
    assert!(sensors.overlapping);
    assert!(sensors.bait_blocked);
    assert!(sensors.nearest_valid);
    assert!(sensors.nearest_moving);
    assert!(sensors.obstacle_urgency > 0.0);
    assert!(sensors.avoidance_direction.length() > 0.0);
}

#[test]
fn rig_plan_is_stable_layered_and_renderer_neutral() {
    let species = fixture_species(38);
    let planner = RigPlanner::new(&species);
    planner.ensure_atlas_dimensions(64, 32).unwrap();
    let body = BodyState {
        position: Vec2::new(800.0, 400.0),
        heading: 0.0,
        speed: 12.0,
        length: 200.0,
    };
    let plan = planner
        .plan(&fixture_pose(), body, Vec2::new(100.0, 100.0))
        .unwrap();

    assert_eq!(plan.body_center, Vec2::new(103.0, 104.0));
    assert_eq!(plan.sprite_scale, 2.0);
    assert_eq!(plan.commands.len(), 6);
    assert_eq!(
        plan.commands
            .iter()
            .map(|command| (command.pass, command.part_index))
            .collect::<Vec<_>>(),
        vec![
            (DrawPass::Shadow, 1),
            (DrawPass::Shadow, 0),
            (DrawPass::Shadow, 2),
            (DrawPass::Sprite, 1),
            (DrawPass::Sprite, 0),
            (DrawPass::Sprite, 2),
        ]
    );

    let shadow_body = plan
        .commands
        .iter()
        .find(|command| command.pass == DrawPass::Shadow && command.part_index == 0)
        .unwrap();
    assert_eq!(shadow_body.destination.x, 121.0);
    assert_eq!(shadow_body.destination.y, 60.0);
    assert_eq!(shadow_body.pivot, Vec2::new(8.0, 12.0));
    assert_eq!(
        shadow_body.color,
        ColorMod {
            red: 0,
            green: 0,
            blue: 0,
            alpha: 38,
        }
    );

    let sprite_body = plan
        .commands
        .iter()
        .find(|command| command.pass == DrawPass::Sprite && command.part_index == 0)
        .unwrap();
    assert_eq!(sprite_body.destination.x, 116.0);
    assert_eq!(sprite_body.destination.y, 54.0);
    assert_eq!(sprite_body.rotation, 0.4);
    assert_eq!(sprite_body.color.alpha, 255);
}

#[test]
fn rig_rotation_matches_the_legacy_heading_convention() {
    let species = fixture_species(0);
    let planner = RigPlanner::new(&species);
    let mut pose = fixture_pose();
    pose.body_rotation = FRAC_PI_2;
    pose.body_offset = Vec2::new(10.0, 0.0);
    let body = BodyState {
        position: Vec2::ZERO,
        heading: 0.0,
        speed: 0.0,
        length: 200.0,
    };
    let plan = planner.plan(&pose, body, Vec2::new(100.0, 100.0)).unwrap();
    assert!((plan.body_center.x - 100.0).abs() < 0.0001);
    assert!((plan.body_center.y - 110.0).abs() < 0.0001);
    assert_eq!(plan.commands.len(), 3);
    assert!(
        plan.commands
            .iter()
            .all(|command| command.pass == DrawPass::Sprite)
    );
    let body_command = plan
        .commands
        .iter()
        .find(|command| command.part_index == 0)
        .unwrap();
    assert!((body_command.rotation - (FRAC_PI_2 + 0.4)).abs() < 0.0001);
}

#[test]
fn rig_rejects_bad_atlas_pose_and_nonfinite_geometry() {
    let species = fixture_species(0);
    let planner = RigPlanner::new(&species);
    assert!(matches!(
        planner.ensure_atlas_dimensions(63, 32),
        Err(RigError::AtlasDimensions { .. })
    ));

    let body = BodyState {
        position: Vec2::ZERO,
        heading: 0.0,
        speed: 0.0,
        length: 200.0,
    };
    let mut pose = fixture_pose();
    pose.parts.pop();
    assert!(matches!(
        planner.plan(&pose, body, Vec2::ZERO),
        Err(RigError::PosePartCount { .. })
    ));

    let mut pose = fixture_pose();
    pose.parts[1].rotation = f32::NAN;
    assert!(matches!(
        planner.plan(&pose, body, Vec2::ZERO),
        Err(RigError::InvalidPose {
            part_index: Some(1)
        })
    ));

    let invalid_body = BodyState {
        heading: f32::INFINITY,
        ..body
    };
    assert!(matches!(
        planner.plan(&fixture_pose(), invalid_body, Vec2::ZERO),
        Err(RigError::InvalidBody)
    ));
}

#[test]
fn heading_sweep_property_holds_for_every_probe_direction() {
    for index in 0..24 {
        let heading = TAU * index as f32 / 24.0 - PI;
        let forward = forward_from_heading(heading);
        let start = Vec2::new(640.0, 360.0);
        let obstacle_center = start + forward * 180.0;
        let obstacle = ScreenObstacle {
            x: obstacle_center.x - 12.0,
            y: obstacle_center.y - 12.0,
            width: 24.0,
            height: 24.0,
            moving: false,
        };
        let mut solver = make_solver(start, heading);
        let intent = MotionIntent {
            direction: forward,
            speed: 4_000.0,
            turn_rate: 0.0,
            acceleration: 100_000.0,
            intentionally_still: false,
            ..MotionIntent::default()
        };
        solver.step(0.05, intent, &[obstacle]);
        assert!(
            !body_overlaps(solver.body(), obstacle, 2.0),
            "sweep failed at heading {heading}"
        );
    }
}
