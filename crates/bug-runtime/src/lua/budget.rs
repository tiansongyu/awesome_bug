use std::cell::Cell;
use std::rc::Rc;

use mlua::{HookTriggers, Lua, VmState};

pub(crate) const INSTRUCTION_LIMIT_MARKER: &str = "__bug_instruction_limit__";
const HOOK_INTERVAL: u32 = 100;

pub(crate) struct InstructionBudget<'lua> {
    lua: &'lua Lua,
}

impl<'lua> InstructionBudget<'lua> {
    pub(crate) fn install(lua: &'lua Lua, maximum_instructions: u32) -> mlua::Result<Self> {
        let consumed = Rc::new(Cell::new(0_u32));
        lua.set_hook(
            HookTriggers::new().every_nth_instruction(HOOK_INTERVAL),
            move |_, _| {
                let next = consumed.get().saturating_add(HOOK_INTERVAL);
                consumed.set(next);
                if next >= maximum_instructions {
                    return Err(mlua::Error::runtime(INSTRUCTION_LIMIT_MARKER));
                }
                Ok(VmState::Continue)
            },
        )?;
        Ok(Self { lua })
    }
}

impl Drop for InstructionBudget<'_> {
    fn drop(&mut self) {
        self.lua.remove_hook();
    }
}

pub(crate) fn run_with_budget<T>(
    lua: &Lua,
    maximum_instructions: u32,
    operation: impl FnOnce() -> mlua::Result<T>,
) -> mlua::Result<T> {
    let _budget = InstructionBudget::install(lua, maximum_instructions)?;
    operation()
}
