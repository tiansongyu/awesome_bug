use std::collections::{BTreeMap, HashSet};
use std::f32::consts::PI;

use mlua::{Lua, Table, Value};

use crate::contract::{
    API_VERSION, ContractError, Decision, FrameInput, MotionIntent, PartPose, Pose, checked_f32,
    is_valid_identifier,
};
use crate::math::Vec2;

use super::{ControllerConfig, ScriptError};

const MAXIMUM_CLOCK: f64 = 1.0e12;
const MAXIMUM_COORDINATE: f64 = 10_000_000.0;
const MAXIMUM_BODY_LENGTH: f64 = 100_000.0;

#[derive(Clone, Copy, Debug)]
pub(crate) struct ReaderLimits {
    pub(crate) maximum_table_entries: usize,
    pub(crate) maximum_string_bytes: usize,
}

pub(crate) struct TableReader {
    limits: ReaderLimits,
    table_entries: usize,
    string_bytes: usize,
}

impl TableReader {
    pub(crate) fn new(limits: ReaderLimits) -> Self {
        Self {
            limits,
            table_entries: 0,
            string_bytes: 0,
        }
    }

    fn object(
        &mut self,
        value: Value,
        path: &str,
        allowed_fields: &[&str],
    ) -> Result<Table, ScriptError> {
        let Value::Table(table) = value else {
            return Err(contract(path, "must be a table"));
        };
        let allowed: HashSet<&str> = allowed_fields.iter().copied().collect();
        for pair in table.clone().pairs::<Value, Value>() {
            let (key, _) =
                pair.map_err(|error| ScriptError::from_mlua(error, "reading Lua result table"))?;
            self.add_entry(path)?;
            let key = self.string(key, path)?;
            if !allowed.contains(key.as_str()) {
                return Err(contract(path, format!("contains unknown field '{key}'")));
            }
        }
        Ok(table)
    }

    fn required_value(&self, table: &Table, field: &str) -> Result<Value, ScriptError> {
        table
            .raw_get(field)
            .map_err(|error| ScriptError::from_mlua(error, "reading Lua result field"))
    }

    fn required_table(
        &mut self,
        table: &Table,
        field: &str,
        path: &str,
        allowed_fields: &[&str],
    ) -> Result<Table, ScriptError> {
        let value = self.required_value(table, field)?;
        self.object(value, path, allowed_fields)
    }

    fn string(&mut self, value: Value, path: &str) -> Result<String, ScriptError> {
        let Value::String(string) = value else {
            return Err(contract(path, "must be a UTF-8 string"));
        };
        let bytes = string.as_bytes();
        self.string_bytes = self
            .string_bytes
            .checked_add(bytes.len())
            .ok_or_else(|| contract(path, "exceeds the aggregate string byte limit"))?;
        if self.string_bytes > self.limits.maximum_string_bytes {
            return Err(contract(path, "exceeds the aggregate string byte limit"));
        }
        string
            .to_str()
            .map(|text| text.to_owned())
            .map_err(|_| contract(path, "must be valid UTF-8"))
    }

    fn required_string(
        &mut self,
        table: &Table,
        field: &str,
        path: &str,
    ) -> Result<String, ScriptError> {
        let value = self.required_value(table, field)?;
        self.string(value, path)
    }

    fn required_number(
        &self,
        table: &Table,
        field: &str,
        path: &str,
        minimum: f64,
        maximum: f64,
    ) -> Result<f32, ScriptError> {
        let value = self.required_value(table, field)?;
        let number = lua_number(value, path)?;
        checked_f32(number, minimum, maximum, path).map_err(contract_error)
    }

    fn optional_number(
        &self,
        table: &Table,
        field: &str,
        path: &str,
        minimum: f64,
        maximum: f64,
    ) -> Result<Option<f32>, ScriptError> {
        let value = self.required_value(table, field)?;
        if matches!(value, Value::Nil) {
            return Ok(None);
        }
        let number = lua_number(value, path)?;
        checked_f32(number, minimum, maximum, path)
            .map(Some)
            .map_err(contract_error)
    }

    fn required_bool(&self, table: &Table, field: &str, path: &str) -> Result<bool, ScriptError> {
        match self.required_value(table, field)? {
            Value::Boolean(value) => Ok(value),
            _ => Err(contract(path, "must be a boolean")),
        }
    }

    fn optional_bool(
        &self,
        table: &Table,
        field: &str,
        path: &str,
        fallback: bool,
    ) -> Result<bool, ScriptError> {
        match self.required_value(table, field)? {
            Value::Nil => Ok(fallback),
            Value::Boolean(value) => Ok(value),
            _ => Err(contract(path, "must be a boolean when present")),
        }
    }

    fn vector(
        &mut self,
        value: Value,
        path: &str,
        minimum: f64,
        maximum: f64,
    ) -> Result<Vec2, ScriptError> {
        let table = self.object(value, path, &["x", "y"])?;
        Ok(Vec2::new(
            self.required_number(&table, "x", &format!("{path}.x"), minimum, maximum)?,
            self.required_number(&table, "y", &format!("{path}.y"), minimum, maximum)?,
        ))
    }

    fn add_entry(&mut self, path: &str) -> Result<(), ScriptError> {
        self.table_entries = self
            .table_entries
            .checked_add(1)
            .ok_or_else(|| contract(path, "exceeds the aggregate table entry limit"))?;
        if self.table_entries > self.limits.maximum_table_entries {
            return Err(contract(path, "exceeds the aggregate table entry limit"));
        }
        Ok(())
    }
}

pub(crate) fn validate_controller_config(
    config: &mut ControllerConfig,
    default_body_length: f32,
) -> Result<(), ScriptError> {
    if config.body_length == 0.0 {
        config.body_length = default_body_length;
    }
    checked_f32(
        f64::from(config.body_length),
        f64::MIN_POSITIVE,
        MAXIMUM_BODY_LENGTH,
        "controller.body_length",
    )
    .map_err(contract_error)?;
    checked_f32(
        f64::from(config.speed_multiplier),
        0.01,
        32.0,
        "controller.speed_multiplier",
    )
    .map_err(contract_error)?;

    let limits = config.motion_limits;
    checked_f32(
        f64::from(limits.maximum_speed),
        f64::MIN_POSITIVE,
        1_000_000.0,
        "controller.motion_limits.maximum_speed",
    )
    .map_err(contract_error)?;
    checked_f32(
        f64::from(limits.maximum_turn_rate),
        f64::MIN_POSITIVE,
        10_000.0,
        "controller.motion_limits.maximum_turn_rate",
    )
    .map_err(contract_error)?;
    checked_f32(
        f64::from(limits.maximum_acceleration),
        f64::MIN_POSITIVE,
        10_000_000.0,
        "controller.motion_limits.maximum_acceleration",
    )
    .map_err(contract_error)?;
    checked_f32(
        f64::from(limits.maximum_lateral_speed),
        f64::MIN_POSITIVE,
        1_000_000.0,
        "controller.motion_limits.maximum_lateral_speed",
    )
    .map_err(contract_error)?;
    Ok(())
}

pub(crate) fn validate_frame(
    frame: &FrameInput,
    config: &ControllerConfig,
) -> Result<(), ScriptError> {
    finite_range(frame.dt, 0.0, 0.25, "frame.dt")?;
    if !frame.clock.is_finite() || !(0.0..=MAXIMUM_CLOCK).contains(&frame.clock) {
        return Err(contract("frame.clock", "must be finite and in [0, 1e12]"));
    }
    vector_range(
        frame.body.position,
        -MAXIMUM_COORDINATE,
        MAXIMUM_COORDINATE,
        "frame.body.position",
    )?;
    finite_range(
        frame.body.heading,
        -1_000_000.0,
        1_000_000.0,
        "frame.body.heading",
    )?;
    finite_range(
        frame.body.speed,
        0.0,
        config.motion_limits.maximum_speed + 0.01,
        "frame.body.speed",
    )?;
    finite_range(
        frame.body.length,
        f32::MIN_POSITIVE,
        MAXIMUM_BODY_LENGTH as f32,
        "frame.body.length",
    )?;
    let length_tolerance = (config.body_length * 1.0e-5).max(0.01);
    if (frame.body.length - config.body_length).abs() > length_tolerance {
        return Err(contract(
            "frame.body.length",
            "does not match controller body_length",
        ));
    }

    if !frame.world.is_finite()
        || frame.world.width <= 0.0
        || frame.world.height <= 0.0
        || f64::from(frame.world.x).abs() > MAXIMUM_COORDINATE
        || f64::from(frame.world.y).abs() > MAXIMUM_COORDINATE
        || f64::from(frame.world.width) > MAXIMUM_COORDINATE
        || f64::from(frame.world.height) > MAXIMUM_COORDINATE
    {
        return Err(contract(
            "frame.world",
            "must be a finite, positive and bounded rectangle",
        ));
    }

    vector_range(
        frame.cursor.position,
        -MAXIMUM_COORDINATE,
        MAXIMUM_COORDINATE,
        "frame.cursor.position",
    )?;
    vector_range(
        frame.cursor.velocity,
        -1_000_000.0,
        1_000_000.0,
        "frame.cursor.velocity",
    )?;
    vector_range(
        frame.bait.position,
        -MAXIMUM_COORDINATE,
        MAXIMUM_COORDINATE,
        "frame.bait.position",
    )?;
    for (index, corner) in frame.corners.iter().enumerate() {
        vector_range(
            corner.position,
            -MAXIMUM_COORDINATE,
            MAXIMUM_COORDINATE,
            &format!("frame.corners[{}].position", index + 1),
        )?;
        finite_range(
            corner.distance,
            0.0,
            (MAXIMUM_COORDINATE * 3.0) as f32,
            &format!("frame.corners[{}].distance", index + 1),
        )?;
    }

    let sensors = frame.sensors;
    vector_range(
        sensors.avoidance_direction,
        -2.0,
        2.0,
        "frame.sensors.avoidance_direction",
    )?;
    finite_range(
        sensors.obstacle_urgency,
        0.0,
        1.0,
        "frame.sensors.obstacle_urgency",
    )?;
    finite_range(
        sensors.moving_obstacle_urgency,
        0.0,
        1.0,
        "frame.sensors.moving_obstacle_urgency",
    )?;
    vector_range(
        sensors.nearest_point,
        -MAXIMUM_COORDINATE,
        MAXIMUM_COORDINATE,
        "frame.sensors.nearest_point",
    )?;
    vector_range(
        sensors.nearest_away,
        -2.0,
        2.0,
        "frame.sensors.nearest_away",
    )?;
    finite_range(
        sensors.nearest_distance,
        0.0,
        (MAXIMUM_COORDINATE * 3.0) as f32,
        "frame.sensors.nearest_distance",
    )?;

    let feedback = frame.feedback;
    vector_range(
        feedback.actual_displacement,
        -1_000_000.0,
        1_000_000.0,
        "frame.feedback.actual_displacement",
    )?;
    finite_range(
        feedback.blocked_time,
        0.0,
        1_000_000.0,
        "frame.feedback.blocked_time",
    )?;
    finite_range(
        feedback.edge_dwell_time,
        0.0,
        1_000_000.0,
        "frame.feedback.edge_dwell_time",
    )?;
    vector_range(
        feedback.recovery_direction,
        -2.0,
        2.0,
        "frame.feedback.recovery_direction",
    )?;
    finite_range(
        feedback.recovery_time,
        0.0,
        1_000_000.0,
        "frame.feedback.recovery_time",
    )?;
    finite_range(
        feedback.recovery_clearance,
        0.0,
        (MAXIMUM_COORDINATE * 3.0) as f32,
        "frame.feedback.recovery_clearance",
    )?;
    Ok(())
}

pub(crate) fn frame_table(
    lua: &Lua,
    frame: &FrameInput,
    species_supports_bait: bool,
) -> mlua::Result<Table> {
    let output = lua.create_table()?;
    output.raw_set("dt", f64::from(frame.dt))?;
    output.raw_set("clock", frame.clock)?;
    output.raw_set(
        "body",
        object(
            lua,
            &[
                ("x", Value::Number(f64::from(frame.body.position.x))),
                ("y", Value::Number(f64::from(frame.body.position.y))),
                ("heading", Value::Number(f64::from(frame.body.heading))),
                ("speed", Value::Number(f64::from(frame.body.speed))),
                ("length", Value::Number(f64::from(frame.body.length))),
            ],
        )?,
    )?;
    output.raw_set(
        "world",
        object(
            lua,
            &[
                ("x", Value::Number(f64::from(frame.world.x))),
                ("y", Value::Number(f64::from(frame.world.y))),
                ("width", Value::Number(f64::from(frame.world.width))),
                ("height", Value::Number(f64::from(frame.world.height))),
            ],
        )?,
    )?;
    output.raw_set(
        "cursor",
        object(
            lua,
            &[
                ("valid", Value::Boolean(frame.cursor.valid)),
                ("x", Value::Number(f64::from(frame.cursor.position.x))),
                ("y", Value::Number(f64::from(frame.cursor.position.y))),
                ("vx", Value::Number(f64::from(frame.cursor.velocity.x))),
                ("vy", Value::Number(f64::from(frame.cursor.velocity.y))),
            ],
        )?,
    )?;
    let bait_enabled = species_supports_bait && frame.features.bait;
    output.raw_set(
        "bait",
        object(
            lua,
            &[
                ("active", Value::Boolean(bait_enabled && frame.bait.active)),
                ("x", Value::Number(f64::from(frame.bait.position.x))),
                ("y", Value::Number(f64::from(frame.bait.position.y))),
            ],
        )?,
    )?;
    let corners = lua.create_table_with_capacity(4, 0)?;
    for (index, corner) in frame.corners.iter().enumerate() {
        corners.raw_set(
            index + 1,
            object(
                lua,
                &[
                    ("x", Value::Number(f64::from(corner.position.x))),
                    ("y", Value::Number(f64::from(corner.position.y))),
                    ("distance", Value::Number(f64::from(corner.distance))),
                    ("blocked", Value::Boolean(corner.blocked)),
                ],
            )?,
        )?;
    }
    output.raw_set("corners", corners)?;
    output.raw_set(
        "sensors",
        object(
            lua,
            &[
                ("overlapping", Value::Boolean(frame.sensors.overlapping)),
                ("bait_blocked", Value::Boolean(frame.sensors.bait_blocked)),
                ("nearest_valid", Value::Boolean(frame.sensors.nearest_valid)),
                (
                    "nearest_moving",
                    Value::Boolean(frame.sensors.nearest_moving),
                ),
                (
                    "avoidance_direction",
                    Value::Table(vector_table(lua, frame.sensors.avoidance_direction)?),
                ),
                (
                    "obstacle_urgency",
                    Value::Number(f64::from(frame.sensors.obstacle_urgency)),
                ),
                (
                    "moving_obstacle_urgency",
                    Value::Number(f64::from(frame.sensors.moving_obstacle_urgency)),
                ),
                (
                    "nearest_point",
                    Value::Table(vector_table(lua, frame.sensors.nearest_point)?),
                ),
                (
                    "nearest_away",
                    Value::Table(vector_table(lua, frame.sensors.nearest_away)?),
                ),
                (
                    "nearest_distance",
                    Value::Number(f64::from(frame.sensors.nearest_distance)),
                ),
            ],
        )?,
    )?;
    output.raw_set(
        "feedback",
        object(
            lua,
            &[
                (
                    "actual_displacement",
                    Value::Table(vector_table(lua, frame.feedback.actual_displacement)?),
                ),
                ("overlapping", Value::Boolean(frame.feedback.overlapping)),
                (
                    "blocked_time",
                    Value::Number(f64::from(frame.feedback.blocked_time)),
                ),
                (
                    "edge_dwell_time",
                    Value::Number(f64::from(frame.feedback.edge_dwell_time)),
                ),
                (
                    "recovery_direction",
                    Value::Table(vector_table(lua, frame.feedback.recovery_direction)?),
                ),
                (
                    "recovery_time",
                    Value::Number(f64::from(frame.feedback.recovery_time)),
                ),
                (
                    "recovery_clearance",
                    Value::Number(f64::from(frame.feedback.recovery_clearance)),
                ),
            ],
        )?,
    )?;
    output.raw_set(
        "features",
        object(
            lua,
            &[
                (
                    "single_instance",
                    Value::Boolean(frame.features.single_instance),
                ),
                (
                    "extended_behaviors",
                    Value::Boolean(frame.features.extended_behaviors),
                ),
                ("bait", Value::Boolean(bait_enabled)),
            ],
        )?,
    )?;
    output.raw_set("request_corner_rest", frame.request_corner_rest)?;
    Ok(output)
}

pub(crate) fn config_table(
    lua: &Lua,
    species_id: &str,
    default_body_length: f32,
    species_supports_bait: bool,
    config: &ControllerConfig,
) -> mlua::Result<Table> {
    let output = lua.create_table()?;
    output.raw_set("api_version", API_VERSION)?;
    output.raw_set("species_id", species_id)?;
    output.raw_set("body_length", f64::from(config.body_length))?;
    output.raw_set("default_body_length", f64::from(default_body_length))?;
    output.raw_set("speed_multiplier", f64::from(config.speed_multiplier))?;
    output.raw_set(
        "enable_extended_behaviors",
        config.enable_extended_behaviors,
    )?;
    output.raw_set(
        "capabilities",
        object(lua, &[("bait", Value::Boolean(species_supports_bait))])?,
    )?;
    output.raw_set(
        "limits",
        object(
            lua,
            &[
                (
                    "speed",
                    Value::Number(f64::from(config.motion_limits.maximum_speed)),
                ),
                (
                    "turn_rate",
                    Value::Number(f64::from(config.motion_limits.maximum_turn_rate)),
                ),
                (
                    "acceleration",
                    Value::Number(f64::from(config.motion_limits.maximum_acceleration)),
                ),
                (
                    "lateral_speed",
                    Value::Number(f64::from(config.motion_limits.maximum_lateral_speed)),
                ),
            ],
        )?,
    )?;
    Ok(output)
}

pub(crate) fn parse_decision(
    value: Value,
    frame: &FrameInput,
    config: &ControllerConfig,
    species_supports_bait: bool,
    has_successful_step: bool,
    limits: ReaderLimits,
) -> Result<Decision, ScriptError> {
    let mut reader = TableReader::new(limits);
    let step = reader.object(value, "step", &["state", "target", "motion", "events"])?;
    let state = reader.required_string(&step, "state", "step.state")?;
    if !is_valid_identifier(&state) {
        return Err(contract(
            "step.state",
            "must contain 1..64 ASCII letters, digits, '_' or '-'",
        ));
    }

    let target_margin = (f64::from(frame.body.length) * 8.0)
        .max(f64::from(frame.world.width).max(f64::from(frame.world.height)) * 2.0);
    let target_low = f64::from(frame.world.x.min(frame.world.y)) - target_margin;
    let target_high =
        f64::from((frame.world.x + frame.world.width).max(frame.world.y + frame.world.height))
            + target_margin;
    let target_value = reader.required_value(&step, "target")?;
    let target = reader.vector(
        target_value,
        "step.target",
        target_low.max(-MAXIMUM_COORDINATE),
        target_high.min(MAXIMUM_COORDINATE),
    )?;

    let motion = reader.required_table(
        &step,
        "motion",
        "step.motion",
        &[
            "direction",
            "speed",
            "turn_rate",
            "acceleration",
            "lateral_speed",
            "recovery_probe_phase",
            "intentionally_still",
            "stop_immediately",
            "cancel_recovery",
            "allow_edge_rest",
            "initial_heading",
        ],
    )?;
    let direction_value = reader.required_value(&motion, "direction")?;
    let direction = reader.vector(direction_value, "step.motion.direction", -1.0, 1.0)?;
    let initial_heading = reader.optional_number(
        &motion,
        "initial_heading",
        "step.motion.initial_heading",
        -f64::from(PI),
        f64::from(PI),
    )?;
    if has_successful_step && initial_heading.is_some() {
        return Err(contract(
            "step.motion.initial_heading",
            "is only legal on the first successful step",
        ));
    }
    let motion_intent = MotionIntent {
        direction,
        speed: reader.required_number(
            &motion,
            "speed",
            "step.motion.speed",
            0.0,
            f64::from(config.motion_limits.maximum_speed),
        )?,
        turn_rate: reader.required_number(
            &motion,
            "turn_rate",
            "step.motion.turn_rate",
            0.0,
            f64::from(config.motion_limits.maximum_turn_rate),
        )?,
        acceleration: reader.required_number(
            &motion,
            "acceleration",
            "step.motion.acceleration",
            0.0,
            f64::from(config.motion_limits.maximum_acceleration),
        )?,
        lateral_speed: reader.required_number(
            &motion,
            "lateral_speed",
            "step.motion.lateral_speed",
            -f64::from(config.motion_limits.maximum_lateral_speed),
            f64::from(config.motion_limits.maximum_lateral_speed),
        )?,
        recovery_probe_phase: reader
            .optional_number(
                &motion,
                "recovery_probe_phase",
                "step.motion.recovery_probe_phase",
                -1_000_000.0,
                1_000_000.0,
            )?
            .unwrap_or(0.0),
        intentionally_still: reader.required_bool(
            &motion,
            "intentionally_still",
            "step.motion.intentionally_still",
        )?,
        stop_immediately: reader.optional_bool(
            &motion,
            "stop_immediately",
            "step.motion.stop_immediately",
            false,
        )?,
        cancel_recovery: reader.optional_bool(
            &motion,
            "cancel_recovery",
            "step.motion.cancel_recovery",
            false,
        )?,
        allow_edge_rest: reader.required_bool(
            &motion,
            "allow_edge_rest",
            "step.motion.allow_edge_rest",
        )?,
        initial_heading,
    };
    motion_intent
        .validate(config.motion_limits)
        .map_err(contract_error)?;

    let events = reader.required_table(&step, "events", "step.events", &["consume_bait"])?;
    let consume_bait = reader.required_bool(&events, "consume_bait", "step.events.consume_bait")?;
    let bait_enabled = species_supports_bait && frame.features.bait && frame.bait.active;
    if consume_bait && !bait_enabled {
        return Err(contract(
            "step.events.consume_bait",
            "requires an active, enabled bait capability",
        ));
    }

    Ok(Decision {
        state,
        target,
        motion: motion_intent,
        consume_bait,
    })
}

pub(crate) fn parse_pose(
    value: Value,
    frame: &FrameInput,
    part_indices: &BTreeMap<String, usize>,
    part_count: usize,
    limits: ReaderLimits,
) -> Result<Pose, ScriptError> {
    let mut reader = TableReader::new(limits);
    let pose = reader.object(value, "pose", &["body", "parts"])?;
    let body = reader.required_table(&pose, "body", "pose.body", &["x", "y", "rotation"])?;
    let body_limit = f64::from(frame.body.length);
    let body_offset = Vec2::new(
        reader.required_number(&body, "x", "pose.body.x", -body_limit, body_limit)?,
        reader.required_number(&body, "y", "pose.body.y", -body_limit, body_limit)?,
    );
    let body_rotation = reader.required_number(
        &body,
        "rotation",
        "pose.body.rotation",
        -2.0 * f64::from(PI),
        2.0 * f64::from(PI),
    )?;

    let parts_value = reader.required_value(&pose, "parts")?;
    let Value::Table(parts_table) = parts_value else {
        return Err(contract("pose.parts", "must be a table"));
    };
    let mut parts = vec![PartPose::default(); part_count];
    let mut seen = vec![false; part_count];
    let joint_limit = f64::from(frame.body.length) * 2.0;
    for pair in parts_table.pairs::<Value, Value>() {
        let (name, part_value) =
            pair.map_err(|error| ScriptError::from_mlua(error, "reading Lua pose parts"))?;
        reader.add_entry("pose.parts")?;
        let name = reader.string(name, "pose.parts")?;
        let Some(&index) = part_indices.get(&name) else {
            return Err(contract(
                "pose.parts",
                format!("contains unknown part '{name}'"),
            ));
        };
        if index >= part_count {
            return Err(contract(
                "pose.parts",
                format!("part '{name}' has an invalid manifest index"),
            ));
        }
        if seen[index] {
            return Err(contract(
                "pose.parts",
                format!("contains duplicate part '{name}'"),
            ));
        }
        seen[index] = true;
        let part_path = format!("pose.parts.{name}");
        let part = reader.object(part_value, &part_path, &["rotation", "joint_offset"])?;
        let joint_value = reader.required_value(&part, "joint_offset")?;
        parts[index] = PartPose {
            rotation: reader.required_number(
                &part,
                "rotation",
                &format!("{part_path}.rotation"),
                -8.0 * f64::from(PI),
                8.0 * f64::from(PI),
            )?,
            joint_offset: reader.vector(
                joint_value,
                &format!("{part_path}.joint_offset"),
                -joint_limit,
                joint_limit,
            )?,
        };
    }

    Ok(Pose {
        body_offset,
        body_rotation,
        parts,
    })
}

fn object(lua: &Lua, fields: &[(&str, Value)]) -> mlua::Result<Table> {
    let table = lua.create_table_with_capacity(0, fields.len())?;
    for (name, value) in fields {
        table.raw_set(*name, value.clone())?;
    }
    Ok(table)
}

fn vector_table(lua: &Lua, vector: Vec2) -> mlua::Result<Table> {
    object(
        lua,
        &[
            ("x", Value::Number(f64::from(vector.x))),
            ("y", Value::Number(f64::from(vector.y))),
        ],
    )
}

fn lua_number(value: Value, path: &str) -> Result<f64, ScriptError> {
    match value {
        Value::Number(value) => Ok(value),
        Value::Integer(value) => Ok(value as f64),
        _ => Err(contract(path, "must be a finite number")),
    }
}

fn finite_range(value: f32, minimum: f32, maximum: f32, path: &str) -> Result<(), ScriptError> {
    checked_f32(
        f64::from(value),
        f64::from(minimum),
        f64::from(maximum),
        path,
    )
    .map(|_| ())
    .map_err(contract_error)
}

fn vector_range(vector: Vec2, minimum: f64, maximum: f64, path: &str) -> Result<(), ScriptError> {
    checked_f32(vector.x.into(), minimum, maximum, format!("{path}.x")).map_err(contract_error)?;
    checked_f32(vector.y.into(), minimum, maximum, format!("{path}.y")).map_err(contract_error)?;
    Ok(())
}

fn contract(path: &str, message: impl Into<String>) -> ScriptError {
    ScriptError::contract(
        "validating Lua contract",
        format!("{path} {}", message.into()),
    )
}

fn contract_error(error: ContractError) -> ScriptError {
    ScriptError::contract("validating Lua contract", error.to_string())
}
