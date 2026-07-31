//! Sandboxed Lua 5.4 behavior host.
//!
//! One [`LuaHost`] owns one VM plus validated FSM and behavior sources. Every
//! [`LuaController`] evaluates both modules in fresh sandbox environments and
//! owns an independent registry-backed controller table and RNG callback.

mod budget;
mod controller;
mod error;
mod module;
mod sandbox;
mod value;

use crate::contract::MotionLimits;
use crate::species::MAX_LUA_FILE_BYTES;

pub use controller::LuaController;
pub use error::{ScriptError, ScriptErrorKind};
pub use module::{BehaviorDescriptor, BehaviorModule, LuaHost};

pub const DEFAULT_MEMORY_LIMIT_BYTES: usize = 32 * 1024 * 1024;
pub const DEFAULT_INSTRUCTION_LIMIT: u32 = 100_000;
pub const DEFAULT_MAXIMUM_TABLE_ENTRIES: usize = 8_192;
pub const DEFAULT_MAXIMUM_STRING_BYTES: usize = 1024 * 1024;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub struct LuaHostOptions {
    pub memory_limit_bytes: usize,
    pub instruction_limit: u32,
    pub maximum_table_entries: usize,
    pub maximum_string_bytes: usize,
    pub maximum_lua_file_bytes: usize,
}

impl Default for LuaHostOptions {
    fn default() -> Self {
        Self {
            memory_limit_bytes: DEFAULT_MEMORY_LIMIT_BYTES,
            instruction_limit: DEFAULT_INSTRUCTION_LIMIT,
            maximum_table_entries: DEFAULT_MAXIMUM_TABLE_ENTRIES,
            maximum_string_bytes: DEFAULT_MAXIMUM_STRING_BYTES,
            maximum_lua_file_bytes: MAX_LUA_FILE_BYTES,
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ControllerConfig {
    /// Zero selects the manifest's default body length.
    pub body_length: f32,
    pub speed_multiplier: f32,
    pub enable_extended_behaviors: bool,
    pub motion_limits: MotionLimits,
}

impl Default for ControllerConfig {
    fn default() -> Self {
        Self {
            body_length: 0.0,
            speed_multiplier: 1.0,
            enable_extended_behaviors: false,
            motion_limits: MotionLimits::default(),
        }
    }
}
