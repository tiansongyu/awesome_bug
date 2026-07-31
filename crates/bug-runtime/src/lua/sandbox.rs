use mlua::chunk::ChunkMode;
use mlua::{Function, Lua, StdLib, Table, Value};

use super::budget::run_with_budget;

const BASE_FUNCTIONS: &[&str] = &[
    "assert", "error", "ipairs", "next", "pairs", "select", "tonumber", "tostring", "type",
];
const LIBRARIES: &[&str] = &["table", "string", "math", "utf8"];

pub(crate) struct Sandbox {
    safe_globals: Table,
}

impl Sandbox {
    pub(crate) fn create(lua: &Lua) -> mlua::Result<Self> {
        let globals = lua.globals();
        let safe_values = lua.create_table()?;

        for name in BASE_FUNCTIONS {
            let value: Value = globals.raw_get(*name)?;
            if !matches!(value, Value::Function(_)) {
                return Err(mlua::Error::runtime(format!(
                    "required Lua base function '{name}' is unavailable"
                )));
            }
            safe_values.raw_set(*name, value)?;
        }
        safe_values.raw_set("_VERSION", globals.raw_get::<Value>("_VERSION")?)?;

        for library_name in LIBRARIES {
            let original: Table = globals.raw_get(*library_name)?;
            let filtered = copy_library(lua, &original, library_name)?;
            safe_values.raw_set(*library_name, readonly_proxy(lua, filtered, library_name)?)?;
        }

        Ok(Self {
            safe_globals: readonly_proxy(lua, safe_values, "safe globals")?,
        })
    }

    pub(crate) fn environment(&self, lua: &Lua) -> mlua::Result<Table> {
        let environment = lua.create_table()?;
        let metatable = lua.create_table()?;
        metatable.raw_set("__index", self.safe_globals.clone())?;
        metatable.raw_set("__metatable", "protected sandbox environment")?;
        environment.set_metatable(Some(metatable))?;
        Ok(environment)
    }

    pub(crate) fn load_module(
        &self,
        lua: &Lua,
        source: &[u8],
        source_name: &str,
        instruction_limit: u32,
    ) -> mlua::Result<Table> {
        let environment = self.environment(lua)?;
        run_with_budget(lua, instruction_limit, || {
            lua.load(source)
                .set_name(source_name)
                .set_mode(ChunkMode::Text)
                .set_environment(environment)
                .eval::<Table>()
        })
    }
}

pub(crate) fn readonly_proxy(lua: &Lua, target: Table, label: &str) -> mlua::Result<Table> {
    let proxy = lua.create_table()?;
    let metatable = lua.create_table()?;
    metatable.raw_set("__index", target)?;
    let description = label.to_owned();
    let reject_write: Function =
        lua.create_function(move |_, (_table, key, _value): (Table, Value, Value)| {
            let key = display_key(&key);
            Err::<(), _>(mlua::Error::runtime(format!(
                "{description} is read-only (attempted field {key})"
            )))
        })?;
    metatable.raw_set("__newindex", reject_write)?;
    metatable.raw_set("__metatable", format!("protected read-only {label}"))?;
    proxy.set_metatable(Some(metatable))?;
    Ok(proxy)
}

pub(crate) fn require_function(table: &Table, field: &str) -> mlua::Result<Function> {
    match table.get::<Value>(field)? {
        Value::Function(function) => Ok(function),
        other => Err(mlua::Error::runtime(format!(
            "{field} must be a function, got {}",
            other.type_name()
        ))),
    }
}

fn copy_library(lua: &Lua, original: &Table, name: &str) -> mlua::Result<Table> {
    let copy = lua.create_table()?;
    for pair in original.clone().pairs::<Value, Value>() {
        let (key, value) = pair?;
        if name == "math"
            && matches!(
                &key,
                Value::String(value)
                    if matches!(value.to_str().ok().as_deref(), Some("random" | "randomseed"))
            )
        {
            continue;
        }
        if name == "string"
            && matches!(
                &key,
                Value::String(value)
                    if matches!(value.to_str().ok().as_deref(), Some("dump"))
            )
        {
            continue;
        }
        copy.raw_set(key, value)?;
    }
    Ok(copy)
}

fn display_key(value: &Value) -> String {
    match value {
        Value::String(value) => value
            .to_str()
            .map_or_else(|_| "<non-UTF-8>".to_owned(), |value| format!("'{value}'")),
        Value::Integer(value) => value.to_string(),
        Value::Number(value) => value.to_string(),
        _ => format!("<{}>", value.type_name()),
    }
}

pub(crate) fn create_lua() -> mlua::Result<Lua> {
    Lua::new_with(
        StdLib::TABLE | StdLib::STRING | StdLib::MATH | StdLib::UTF8,
        mlua::LuaOptions::default(),
    )
}
