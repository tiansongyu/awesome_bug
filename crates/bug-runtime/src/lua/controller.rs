use std::cell::RefCell;
use std::f64::consts::{PI, TAU};
use std::fmt::{self, Debug, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::rc::{Rc, Weak};

use mlua::{Function, LuaString, RegistryKey, Table, Value};

use crate::contract::{Decision, FrameInput, Pose};
use crate::math::{Vec2, forward_from_heading};

use super::budget::run_with_budget;
use super::error::HOST_CALLBACK_MARKER;
use super::module::{BehaviorInner, evaluate_behavior_module, evaluate_fsm_module};
use super::sandbox::{readonly_proxy, require_function};
use super::value::{
    config_table, frame_table, parse_decision, parse_pose, validate_controller_config,
    validate_frame,
};
use super::{ControllerConfig, ScriptError, ScriptErrorKind};

pub(crate) type RandomCallback = Box<dyn FnMut(&str, f32, f32) -> Result<f32, String>>;
type SharedRandom = Rc<RefCell<RandomCallback>>;

pub struct LuaController {
    behavior: Rc<BehaviorInner>,
    instance: u64,
    config: ControllerConfig,
    controller_key: Option<RegistryKey>,
    random: Option<SharedRandom>,
    quarantined: bool,
    error: Option<ScriptError>,
    has_successful_step: bool,
    last_decision: Decision,
    last_pose: Pose,
}

impl Debug for LuaController {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("LuaController")
            .field("species", &self.behavior.descriptor.species_id)
            .field("instance", &self.instance)
            .field("quarantined", &self.quarantined)
            .field("error", &self.error)
            .finish_non_exhaustive()
    }
}

pub(crate) fn create_controller(
    behavior: Rc<BehaviorInner>,
    instance: u64,
    mut config: ControllerConfig,
    random: RandomCallback,
) -> Result<LuaController, ScriptError> {
    validate_controller_config(&mut config, behavior.descriptor.default_body_length)
        .map_err(|error| decorate(error, &behavior, instance))?;
    let lua = &behavior.host.lua;
    let random = Rc::new(RefCell::new(random));
    let module = evaluate_behavior_module(&behavior.host, &behavior.descriptor)
        .map_err(|error| decorate(error, &behavior, instance))?;
    let fsm = evaluate_fsm_module(&behavior.host)
        .map_err(|error| decorate(error, &behavior, instance))?;
    let host_api = create_host_api(&behavior, Rc::downgrade(&random), fsm).map_err(|error| {
        decorate(
            ScriptError::from_mlua(error, "creating controller host API"),
            &behavior,
            instance,
        )
    })?;
    let config_table = config_table(
        lua,
        &behavior.descriptor.species_id,
        behavior.descriptor.default_body_length,
        behavior.descriptor.supports_bait,
        &config,
    )
    .map_err(|error| {
        decorate(
            ScriptError::from_mlua(error, "building controller configuration"),
            &behavior,
            instance,
        )
    })?;
    let controller: Table = run_with_budget(lua, behavior.host.options.instruction_limit, || {
        let factory = require_function(&module, "new")?;
        factory.call((config_table, host_api))
    })
    .map_err(|error| {
        decorate(
            ScriptError::from_mlua(error, "creating Lua behavior controller"),
            &behavior,
            instance,
        )
    })?;

    run_with_budget(lua, behavior.host.options.instruction_limit, || {
        require_function(&controller, "step")?;
        require_function(&controller, "pose")?;
        Ok(())
    })
    .map_err(|error| {
        decorate(
            ScriptError::from_mlua(error, "validating Lua behavior controller"),
            &behavior,
            instance,
        )
    })?;
    let controller_key = lua.create_registry_value(controller).map_err(|error| {
        decorate(
            ScriptError::from_mlua(error, "registering Lua behavior controller"),
            &behavior,
            instance,
        )
    })?;
    let part_count = behavior.descriptor.part_indices.len();

    Ok(LuaController {
        behavior,
        instance,
        config,
        controller_key: Some(controller_key),
        random: Some(random),
        quarantined: false,
        error: None,
        has_successful_step: false,
        last_decision: Decision::default(),
        last_pose: Pose {
            parts: vec![Default::default(); part_count],
            ..Pose::default()
        },
    })
}

impl LuaController {
    pub fn step(&mut self, frame: &FrameInput) -> Result<Decision, ScriptError> {
        validate_frame(frame, &self.config).map_err(|error| self.decorate(error))?;
        if self.quarantined {
            return Ok(self.safe_stop(frame));
        }

        let result = self.call_method("step", frame);
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.quarantine(error);
                return Ok(self.safe_stop(frame));
            }
        };
        let decision = match parse_decision(
            value,
            frame,
            &self.config,
            self.behavior.descriptor.supports_bait,
            self.has_successful_step,
            self.behavior.host.reader_limits(),
        ) {
            Ok(decision) => decision,
            Err(error) => {
                let error = self.decorate(error);
                self.quarantine(error);
                return Ok(self.safe_stop(frame));
            }
        };
        self.has_successful_step = true;
        self.last_decision = decision.clone();
        Ok(decision)
    }

    pub fn pose(&mut self, frame: &FrameInput) -> Result<Pose, ScriptError> {
        validate_frame(frame, &self.config).map_err(|error| self.decorate(error))?;
        if self.quarantined {
            return Ok(self.last_pose.clone());
        }
        if !self.has_successful_step {
            return Err(self.decorate(ScriptError::contract(
                "running Lua behavior pose",
                "controller.pose cannot run before the first successful step",
            )));
        }

        let result = self.call_method("pose", frame);
        let value = match result {
            Ok(value) => value,
            Err(error) => {
                self.quarantine(error);
                return Ok(self.last_pose.clone());
            }
        };
        let pose = match parse_pose(
            value,
            frame,
            &self.behavior.descriptor.part_indices,
            self.behavior.descriptor.part_indices.len(),
            self.behavior.host.reader_limits(),
        ) {
            Ok(pose) => pose,
            Err(error) => {
                let error = self.decorate(error);
                self.quarantine(error);
                return Ok(self.last_pose.clone());
            }
        };
        self.last_pose = pose.clone();
        Ok(pose)
    }

    #[must_use]
    pub fn quarantined(&self) -> bool {
        self.quarantined
    }

    #[must_use]
    pub fn error(&self) -> Option<&ScriptError> {
        self.error.as_ref()
    }

    #[must_use]
    pub fn instance(&self) -> u64 {
        self.instance
    }

    #[must_use]
    pub fn config(&self) -> ControllerConfig {
        self.config
    }

    /// Updates display-derived geometry without recreating the Lua
    /// controller or disturbing its RNG stream.
    ///
    /// Scripts receive the current length on every frame through
    /// `frame.body.length`; this method keeps the host-side frame contract in
    /// sync after a monitor/DPI reconfiguration.
    pub fn reconfigure_body_length(&mut self, body_length: f32) -> Result<(), ScriptError> {
        let mut updated = self.config;
        updated.body_length = body_length;
        validate_controller_config(&mut updated, self.behavior.descriptor.default_body_length)
            .map_err(|error| self.decorate(error))?;
        self.config = updated;
        Ok(())
    }

    fn call_method(&self, method_name: &str, frame: &FrameInput) -> Result<Value, ScriptError> {
        let Some(controller_key) = &self.controller_key else {
            return Err(self.decorate(ScriptError::new(
                ScriptErrorKind::Runtime,
                format!("running Lua behavior {method_name}"),
                "controller registry reference is unavailable",
            )));
        };
        let lua = &self.behavior.host.lua;
        let controller: Table = lua.registry_value(controller_key).map_err(|error| {
            self.decorate(ScriptError::from_mlua(
                error,
                format!("reading controller for {method_name}"),
            ))
        })?;
        let frame =
            frame_table(lua, frame, self.behavior.descriptor.supports_bait).map_err(|error| {
                self.decorate(ScriptError::from_mlua(
                    error,
                    format!("building Lua {method_name} frame"),
                ))
            })?;
        run_with_budget(lua, self.behavior.host.options.instruction_limit, || {
            let method: Function = require_function(&controller, method_name)?;
            method.call((controller, frame))
        })
        .map_err(|error| {
            self.decorate(ScriptError::from_mlua(
                error,
                format!("running Lua behavior {method_name}"),
            ))
        })
    }

    fn safe_stop(&self, frame: &FrameInput) -> Decision {
        let mut stopped = self.last_decision.clone();
        if !self.has_successful_step {
            stopped.state = "quarantined".to_owned();
            stopped.target = if frame.body.position.is_finite() {
                frame.body.position
            } else {
                Vec2::ZERO
            };
            let heading = if frame.body.heading.is_finite() {
                frame.body.heading
            } else {
                0.0
            };
            stopped.motion.direction = forward_from_heading(heading);
        }
        stopped.motion.speed = 0.0;
        stopped.motion.turn_rate = 0.0;
        stopped.motion.acceleration = 0.0;
        stopped.motion.lateral_speed = 0.0;
        stopped.motion.recovery_probe_phase = 0.0;
        stopped.motion.intentionally_still = true;
        stopped.motion.stop_immediately = true;
        stopped.motion.cancel_recovery = true;
        stopped.motion.allow_edge_rest = true;
        stopped.motion.initial_heading = None;
        stopped.consume_bait = false;
        stopped
    }

    fn quarantine(&mut self, error: ScriptError) {
        if self.quarantined {
            return;
        }
        self.quarantined = true;
        self.error = Some(error);
        self.random = None;
        if let Some(controller_key) = self.controller_key.take() {
            let _ = self.behavior.host.lua.remove_registry_value(controller_key);
        }
        let _ = self.behavior.host.lua.gc_collect();
        let _ = self.behavior.host.lua.gc_collect();
    }

    fn decorate(&self, error: ScriptError) -> ScriptError {
        decorate(error, &self.behavior, self.instance)
    }
}

impl Drop for LuaController {
    fn drop(&mut self) {
        self.random = None;
        if let Some(controller_key) = self.controller_key.take() {
            let _ = self.behavior.host.lua.remove_registry_value(controller_key);
        }
    }
}

fn create_host_api(
    behavior: &BehaviorInner,
    weak_random: Weak<RefCell<RandomCallback>>,
    fsm: Table,
) -> mlua::Result<Table> {
    let lua = &behavior.host.lua;
    let methods = lua.create_table()?;

    let random = lua.create_function(move |_, (tag, low, high): (LuaString, f64, f64)| {
        let tag_bytes = tag.as_bytes();
        if tag_bytes.is_empty() || tag_bytes.len() > 256 {
            return Err(mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: random tag must contain 1..256 UTF-8 bytes"
            )));
        }
        let tag = tag.to_str().map_err(|_| {
            mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: random tag must be valid UTF-8"
            ))
        })?;
        let low = checked_random_bound(low, "low")?;
        let high = checked_random_bound(high, "high")?;
        if low > high {
            return Err(mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: random range must be ordered"
            )));
        }
        let Some(random) = weak_random.upgrade() else {
            return Err(mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: controller RNG no longer exists"
            )));
        };
        let Ok(mut callback) = random.try_borrow_mut() else {
            return Err(mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: recursive RNG callback is forbidden"
            )));
        };
        let outcome = catch_unwind(AssertUnwindSafe(|| callback(tag.as_ref(), low, high)));
        let value = match outcome {
            Ok(Ok(value)) => value,
            Ok(Err(message)) => {
                return Err(mlua::Error::runtime(format!(
                    "{HOST_CALLBACK_MARKER}: {message}"
                )));
            }
            Err(_) => {
                return Err(mlua::Error::runtime(format!(
                    "{HOST_CALLBACK_MARKER}: RNG callback panicked"
                )));
            }
        };
        if !value.is_finite() || value < low || value > high {
            return Err(mlua::Error::runtime(format!(
                "{HOST_CALLBACK_MARKER}: RNG callback returned a non-finite or out-of-range value"
            )));
        }
        Ok(f64::from(canonical_f32_zero(value)))
    })?;
    methods.raw_set("random", random)?;
    methods.raw_set(
        "f32",
        lua.create_function(|_, value: f64| {
            let converted = checked_random_bound(value, "value")?;
            Ok(f64::from(converted))
        })?,
    )?;
    methods.raw_set(
        "clamp",
        lua.create_function(|_, (value, low, high): (f64, f64, f64)| {
            if !value.is_finite() || !low.is_finite() || !high.is_finite() || low > high {
                return Err(mlua::Error::runtime(
                    "host.clamp requires finite values and low <= high",
                ));
            }
            Ok(value.clamp(low, high))
        })?,
    )?;
    methods.raw_set(
        "wrap_angle",
        lua.create_function(|_, value: f64| {
            if !value.is_finite() {
                return Err(mlua::Error::runtime(
                    "host.wrap_angle requires a finite number",
                ));
            }
            let mut wrapped = (value + PI) % TAU;
            if wrapped < 0.0 {
                wrapped += TAU;
            }
            Ok(wrapped - PI)
        })?,
    )?;
    methods.raw_set("fsm", fsm)?;
    readonly_proxy(lua, methods, "host API")
}

fn checked_random_bound(value: f64, label: &str) -> mlua::Result<f32> {
    if !value.is_finite() || value < -f64::from(f32::MAX) || value > f64::from(f32::MAX) {
        return Err(mlua::Error::runtime(format!(
            "{HOST_CALLBACK_MARKER}: random {label} must fit a finite f32"
        )));
    }
    let converted = value as f32;
    if !converted.is_finite() {
        return Err(mlua::Error::runtime(format!(
            "{HOST_CALLBACK_MARKER}: random {label} must fit a finite f32"
        )));
    }
    Ok(canonical_f32_zero(converted))
}

fn canonical_f32_zero(value: f32) -> f32 {
    if value == 0.0 { 0.0 } else { value }
}

fn decorate(error: ScriptError, behavior: &BehaviorInner, instance: u64) -> ScriptError {
    let error = error
        .with_species(&behavior.descriptor.species_id)
        .with_instance(instance);
    if error.path.is_some() {
        error
    } else {
        error.with_path(&behavior.descriptor.behavior_path)
    }
}
