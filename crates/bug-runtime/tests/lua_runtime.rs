use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use bug_runtime::contract::{FrameInput, MotionLimits};
use bug_runtime::lua::{BehaviorDescriptor, ControllerConfig, LuaHost, ScriptErrorKind};
use bug_runtime::math::Vec2;

const VALID_HELPERS: &str = r#"
local function decision(frame, initial_heading)
    return {
        state = "moving",
        target = { x = frame.body.x + 10.0, y = frame.body.y },
        motion = {
            direction = { x = 0.0, y = -1.0 },
            speed = 80.0,
            turn_rate = 2.0,
            acceleration = 240.0,
            lateral_speed = 0.0,
            recovery_probe_phase = 0.0,
            intentionally_still = false,
            stop_immediately = false,
            cancel_recovery = false,
            allow_edge_rest = false,
            initial_heading = initial_heading,
        },
        events = { consume_bait = false },
    }
end

local function valid_pose()
    return {
        body = { x = 3.0, y = -2.0, rotation = 0.1 },
        parts = {
            body = {
                rotation = 0.2,
                joint_offset = { x = 1.0, y = -1.0 },
            },
        },
    }
end
"#;

fn source_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn host() -> LuaHost {
    LuaHost::new(source_root().join("bugs/runtime/fsm.lua")).expect("the checked-in FSM must load")
}

fn descriptor(id: &str, source: &str) -> BehaviorDescriptor {
    BehaviorDescriptor::for_test(
        id,
        format!("{id}.lua"),
        source.as_bytes().to_vec(),
        120.0,
        false,
        ["body"],
    )
}

fn config() -> ControllerConfig {
    ControllerConfig {
        body_length: 120.0,
        speed_multiplier: 1.0,
        enable_extended_behaviors: false,
        motion_limits: MotionLimits::default(),
    }
}

fn frame() -> FrameInput {
    let mut frame = FrameInput {
        dt: 1.0 / 60.0,
        clock: 1.0,
        ..FrameInput::default()
    };
    frame.body.position = Vec2::new(960.0, 500.0);
    frame.body.heading = 0.25;
    frame.body.length = 120.0;
    frame.world.x = 0.0;
    frame.world.y = 0.0;
    frame.world.width = 1920.0;
    frame.world.height = 1040.0;
    frame.cursor.position = Vec2::new(200.0, 200.0);
    frame.bait.position = Vec2::new(400.0, 300.0);
    let positions = [
        Vec2::new(90.0, 90.0),
        Vec2::new(1830.0, 90.0),
        Vec2::new(90.0, 950.0),
        Vec2::new(1830.0, 950.0),
    ];
    for (corner, position) in frame.corners.iter_mut().zip(positions) {
        corner.position = position;
        corner.distance = position.distance(frame.body.position);
    }
    frame.features.single_instance = true;
    frame
}

fn constant_random(value: f32) -> impl FnMut(&str, f32, f32) -> Result<f32, String> {
    move |_tag, low, high| Ok(low + (high - low) * value)
}

#[test]
fn sandbox_exposes_only_the_narrow_safe_environment() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
local forbidden = {{
    "dofile", "load", "loadfile", "require", "collectgarbage",
    "pcall", "xpcall", "rawset", "setmetatable", "getmetatable",
    "package", "io", "os", "debug", "coroutine",
}}
for index = 1, #forbidden do
    if _ENV[forbidden[index]] ~= nil then
        error("forbidden global is visible: " .. forbidden[index])
    end
end
if type(table) ~= "table" or type(string) ~= "table"
    or type(math) ~= "table" or type(utf8) ~= "table" then
    error("required safe library is missing")
end
if math.random ~= nil or math.randomseed ~= nil or string.dump ~= nil
    or ("").dump ~= nil then
    error("dynamic code or untagged randomness is visible")
end
return {{
    api_version = 1,
    new = function(config, host)
        if type(host) ~= "table" or type(host.fsm) ~= "table"
            or type(host.fsm.create) ~= "function" then
            error("host API is incomplete")
        end
        local self = {{}}
        function self:step(frame) return decision(frame, 0.0) end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("sandbox_a", &source))
        .expect("safe script must load");
    let mut controller = module
        .create_controller(1, config(), constant_random(0.5))
        .expect("safe controller must start");
    assert_eq!(
        controller.step(&frame()).expect("step must run").state,
        "moving"
    );

    let global_write_source = format!(
        r#"{VALID_HELPERS}
shared_controller_state = 42
return {{
    api_version = 1,
    new = function()
        local self = {{}}
        function self:step(frame) return decision(frame, 0.0) end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let error = host
        .load_behavior_descriptor(descriptor("sandbox_b", &global_write_source))
        .expect_err("module-level mutable globals must be rejected");
    assert_eq!(error.kind, ScriptErrorKind::Runtime);
    assert!(error.message.contains("local bindings"));
}

#[test]
fn shared_fsm_and_host_api_are_read_only() {
    let host = host();
    let mutation_source = r#"
return {
    api_version = 1,
    new = function(config, host)
        host.fsm.create = nil
        return {}
    end,
}
"#;
    let module = host
        .load_behavior_descriptor(descriptor("mutate_fsm", mutation_source))
        .expect("module itself is valid");
    let error = module
        .create_controller(2, config(), constant_random(0.5))
        .expect_err("FSM mutation must fail");
    assert_eq!(error.kind, ScriptErrorKind::Runtime);
    assert!(error.message.contains("read-only"));

    let host_mutation_source = r#"
return {
    api_version = 1,
    new = function(config, host)
        host.random = nil
        return {}
    end,
}
"#;
    let module = host
        .load_behavior_descriptor(descriptor("mutate_host", host_mutation_source))
        .expect("module itself is valid");
    let error = module
        .create_controller(3, config(), constant_random(0.5))
        .expect_err("host mutation must fail");
    assert_eq!(error.kind, ScriptErrorKind::Runtime);
    assert!(error.message.contains("read-only"));
}

#[test]
fn controllers_share_code_but_not_mutable_instance_state() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function()
        local self = {{ calls = 0 }}
        function self:step(frame)
            self.calls = self.calls + 1
            local result = decision(frame, self.calls == 1 and 0.25 or nil)
            result.target.x = result.target.x + self.calls
            return result
        end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("isolated", &source))
        .expect("module must load");
    let mut first = module
        .create_controller(10, config(), constant_random(0.5))
        .expect("first controller must start");
    let mut second = module
        .create_controller(11, config(), constant_random(0.5))
        .expect("second controller must start");
    let frame = frame();

    let first_one = first.step(&frame).expect("first step");
    let first_two = first.step(&frame).expect("second step");
    let second_one = second.step(&frame).expect("independent first step");
    assert_eq!(first_one.target, second_one.target);
    assert_ne!(first_two.target, second_one.target);
    assert_eq!(first_one.motion.initial_heading, Some(0.25));
    assert_eq!(second_one.motion.initial_heading, Some(0.25));
}

#[test]
fn tagged_random_and_f32_boundary_are_the_only_entropy_path() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function(config, host)
        local self = {{
            sample = host.random("test.sample", 10.0, 20.0),
            rounded = host.f32(0.1),
            first = true,
        }}
        function self:step(frame)
            local result = decision(frame, self.first and self.rounded or nil)
            self.first = false
            result.motion.speed = self.sample
            return result
        end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let tags = Rc::new(RefCell::new(Vec::new()));
    let captured = Rc::clone(&tags);
    let module = host
        .load_behavior_descriptor(descriptor("tagged_rng", &source))
        .expect("module must load");
    let mut controller = module
        .create_controller(12, config(), move |tag, low, high| {
            captured.borrow_mut().push(tag.to_owned());
            Ok(low + (high - low) * 0.25)
        })
        .expect("controller must start");
    let decision = controller.step(&frame()).expect("step must run");
    assert_eq!(tags.borrow().as_slice(), ["test.sample"]);
    assert_eq!(decision.motion.speed, 12.5);
    assert_eq!(
        decision
            .motion
            .initial_heading
            .expect("first heading")
            .to_bits(),
        0.1_f32.to_bits()
    );
}

#[test]
fn instruction_budget_quarantines_only_the_failing_instance() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function(config, host)
        local self = {{ spin = host.random("spin", 0.0, 1.0) < 0.5 }}
        function self:step(frame)
            if self.spin then while true do end end
            return decision(frame, 0.0)
        end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("budget", &source))
        .expect("module must load");
    let mut broken = module
        .create_controller(20, config(), constant_random(0.0))
        .expect("broken controller starts before step");
    let mut healthy = module
        .create_controller(21, config(), constant_random(1.0))
        .expect("healthy controller starts");
    let frame = frame();

    let stopped = broken
        .step(&frame)
        .expect("script failures return safe output");
    assert!(broken.quarantined());
    assert_eq!(
        broken.error().expect("sticky error").kind,
        ScriptErrorKind::InstructionLimit
    );
    assert_eq!(stopped.state, "quarantined");
    assert!(stopped.motion.stop_immediately);
    assert_eq!(stopped.motion.speed, 0.0);

    let stopped_again = broken.step(&frame).expect("quarantine is sticky");
    assert_eq!(stopped, stopped_again);
    assert_eq!(
        healthy
            .step(&frame)
            .expect("other instance remains live")
            .state,
        "moving"
    );
    assert!(!healthy.quarantined());
}

#[test]
fn runtime_failure_retains_last_decision_and_pose() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function()
        local self = {{ calls = 0 }}
        function self:step(frame)
            self.calls = self.calls + 1
            if self.calls == 2 then error("instance crash") end
            return decision(frame, 0.4)
        end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("runtime_crash", &source))
        .expect("module must load");
    let mut controller = module
        .create_controller(30, config(), constant_random(0.5))
        .expect("controller must start");
    let frame = frame();
    let before = controller.step(&frame).expect("first step succeeds");
    let before_pose = controller.pose(&frame).expect("first pose succeeds");
    let stopped = controller
        .step(&frame)
        .expect("second script call is safely quarantined");

    assert!(controller.quarantined());
    assert_eq!(
        controller.error().expect("sticky error").kind,
        ScriptErrorKind::Runtime
    );
    assert!(
        controller
            .error()
            .expect("sticky error")
            .message
            .contains("instance crash")
    );
    assert_eq!(stopped.state, before.state);
    assert_eq!(stopped.target, before.target);
    assert_eq!(stopped.motion.speed, 0.0);
    assert!(stopped.motion.stop_immediately);
    assert_eq!(
        controller.pose(&frame).expect("last pose is retained"),
        before_pose
    );
}

#[test]
fn non_finite_output_and_unknown_parts_are_contract_failures() {
    let host = host();
    let nan_source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function()
        local self = {{}}
        function self:step(frame)
            local result = decision(frame, 0.0)
            result.motion.speed = 0.0 / 0.0
            return result
        end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("nan_output", &nan_source))
        .expect("module must load");
    let mut controller = module
        .create_controller(40, config(), constant_random(0.5))
        .expect("controller must start");
    let stopped = controller
        .step(&frame())
        .expect("contract failure returns safe stop");
    assert!(stopped.motion.stop_immediately);
    assert_eq!(
        controller.error().expect("contract error").kind,
        ScriptErrorKind::Contract
    );

    let unknown_part_source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function()
        local self = {{}}
        function self:step(frame) return decision(frame, 0.0) end
        function self:pose()
            return {{
                body = {{ x = 0.0, y = 0.0, rotation = 0.0 }},
                parts = {{
                    ghost_leg = {{
                        rotation = 0.0,
                        joint_offset = {{ x = 0.0, y = 0.0 }},
                    }},
                }},
            }}
        end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("unknown_part", &unknown_part_source))
        .expect("module must load");
    let mut controller = module
        .create_controller(41, config(), constant_random(0.5))
        .expect("controller must start");
    let frame = frame();
    controller.step(&frame).expect("step succeeds");
    let retained = controller
        .pose(&frame)
        .expect("bad pose returns the initialized safe pose");
    assert!(controller.quarantined());
    assert_eq!(
        controller.error().expect("contract error").kind,
        ScriptErrorKind::Contract
    );
    assert!(
        controller
            .error()
            .expect("contract error")
            .message
            .contains("ghost_leg")
    );
    assert_eq!(retained.parts.len(), 1);
}

#[test]
fn invalid_host_frame_is_reported_without_poisoning_the_controller() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function()
        local self = {{}}
        function self:step(frame) return decision(frame, 0.0) end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("host_frame", &source))
        .expect("module must load");
    let mut controller = module
        .create_controller(50, config(), constant_random(0.5))
        .expect("controller must start");
    let mut invalid = frame();
    invalid.world.width = f32::INFINITY;
    let error = controller
        .step(&invalid)
        .expect_err("invalid host data must not cross into Lua");
    assert_eq!(error.kind, ScriptErrorKind::Contract);
    assert!(!controller.quarantined());
    assert_eq!(
        controller
            .step(&frame())
            .expect("valid input still works")
            .state,
        "moving"
    );
}

#[test]
fn callback_errors_panics_and_binary_chunks_do_not_escape() {
    let host = host();
    let source = format!(
        r#"{VALID_HELPERS}
return {{
    api_version = 1,
    new = function(config, host)
        host.random("startup", 0.0, 1.0)
        local self = {{}}
        function self:step(frame) return decision(frame, 0.0) end
        function self:pose() return valid_pose() end
        return self
    end,
}}
"#
    );
    let module = host
        .load_behavior_descriptor(descriptor("callback_error", &source))
        .expect("module must load");
    let error = module
        .create_controller(60, config(), |_tag, _low, _high| {
            Err("tape exhausted".to_owned())
        })
        .expect_err("callback error must abort controller startup");
    assert_eq!(error.kind, ScriptErrorKind::HostCallback);
    assert!(error.message.contains("tape exhausted"));

    let panic_module = host
        .load_behavior_descriptor(descriptor("callback_panic", &source))
        .expect("same source under a distinct species loads");
    let error = panic_module
        .create_controller(61, config(), |_tag, _low, _high| {
            panic!("callback panic must be contained")
        })
        .expect_err("callback panic must become a script error");
    assert_eq!(error.kind, ScriptErrorKind::HostCallback);
    assert!(error.message.contains("panicked"));

    let binary = [0x1b, b'L', b'u', b'a', 0x54, 0, 0, 0];
    let error = host
        .load_behavior_descriptor(BehaviorDescriptor::for_test(
            "binary_chunk",
            "binary.lua",
            binary.to_vec(),
            120.0,
            false,
            ["body"],
        ))
        .expect_err("binary Lua chunks must be rejected");
    assert!(matches!(
        error.kind,
        ScriptErrorKind::Syntax | ScriptErrorKind::Runtime
    ));
}

#[test]
fn real_cockroach_package_runs_through_the_generic_host() {
    let host = host();
    let species = host
        .load_species(source_root().join("bugs/cockroach"))
        .expect("cockroach manifest must pass the sandboxed loader");
    let module = host
        .load_behavior(&species)
        .expect("cockroach behavior must load");
    let mut controller = module
        .create_controller(
            70,
            ControllerConfig {
                body_length: species.body.default_length,
                speed_multiplier: 3.0,
                enable_extended_behaviors: true,
                motion_limits: MotionLimits::default(),
            },
            constant_random(0.5),
        )
        .expect("cockroach controller must start");
    let mut frame = frame();
    frame.body.length = species.body.default_length;
    frame.features.extended_behaviors = true;
    frame.features.bait = true;

    for _ in 0..600 {
        let decision = controller.step(&frame).expect("cockroach step");
        let pose = controller.pose(&frame).expect("cockroach pose");
        assert!(!controller.quarantined(), "{:?}", controller.error());
        assert!(!decision.state.is_empty());
        assert!(decision.target.is_finite());
        assert!(decision.motion.direction.is_finite());
        assert_eq!(pose.parts.len(), species.parts.len());
        assert!(pose.body_offset.is_finite());
        assert!(pose.body_rotation.is_finite());
        frame.clock += f64::from(frame.dt);
    }
}
