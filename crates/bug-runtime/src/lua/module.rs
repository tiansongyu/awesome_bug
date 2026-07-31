use std::cell::RefCell;
use std::collections::{BTreeMap, HashMap};
use std::fmt::{self, Debug, Formatter};
use std::fs;
use std::path::{Path, PathBuf};
use std::rc::{Rc, Weak};

use mlua::{Lua, RegistryKey, Table, Value};

use crate::contract::{API_VERSION, MAX_PARTS, is_valid_identifier};
use crate::species::{
    MAX_LUA_FILE_BYTES, Species, SpeciesError, load_manifest_source, parse_manifest,
    read_limited_file,
};

use super::controller::{LuaController, RandomCallback, create_controller};
use super::sandbox::{Sandbox, create_lua, readonly_proxy, require_function};
use super::value::ReaderLimits;
use super::{ControllerConfig, LuaHostOptions, ScriptError, ScriptErrorKind};

#[derive(Clone, Debug)]
pub struct BehaviorDescriptor {
    pub species_id: String,
    pub behavior_path: PathBuf,
    pub source: Vec<u8>,
    pub default_body_length: f32,
    pub supports_bait: bool,
    pub part_indices: BTreeMap<String, usize>,
}

impl BehaviorDescriptor {
    pub fn from_species(species: &Species) -> Result<Self, ScriptError> {
        let source =
            read_limited_file(&species.behavior_path, MAX_LUA_FILE_BYTES).map_err(species_error)?;
        Ok(Self {
            species_id: species.id.clone(),
            behavior_path: species.behavior_path.clone(),
            source,
            default_body_length: species.body.default_length,
            supports_bait: species.capabilities.bait,
            part_indices: species.part_indices.clone(),
        })
    }

    #[must_use]
    pub fn for_test(
        species_id: impl Into<String>,
        source_name: impl Into<PathBuf>,
        source: impl Into<Vec<u8>>,
        default_body_length: f32,
        supports_bait: bool,
        part_names: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        let part_indices = part_names
            .into_iter()
            .enumerate()
            .map(|(index, name)| (name.into(), index))
            .collect();
        Self {
            species_id: species_id.into(),
            behavior_path: source_name.into(),
            source: source.into(),
            default_body_length,
            supports_bait,
            part_indices,
        }
    }
}

pub(crate) struct HostInner {
    pub(crate) lua: Lua,
    pub(crate) sandbox: Sandbox,
    pub(crate) options: LuaHostOptions,
    pub(crate) fsm_key: RegistryKey,
    modules: RefCell<HashMap<String, Weak<BehaviorInner>>>,
}

pub struct LuaHost {
    pub(crate) inner: Rc<HostInner>,
}

impl LuaHost {
    pub fn new(fsm_path: impl AsRef<Path>) -> Result<Self, ScriptError> {
        Self::with_options(fsm_path, LuaHostOptions::default())
    }

    pub fn with_options(
        fsm_path: impl AsRef<Path>,
        options: LuaHostOptions,
    ) -> Result<Self, ScriptError> {
        let path = fsm_path.as_ref();
        let source = read_script(path, options.maximum_lua_file_bytes)?;
        Self::from_fsm_source(path, &source, options)
    }

    pub fn from_fsm_source(
        source_name: impl AsRef<Path>,
        source: &[u8],
        options: LuaHostOptions,
    ) -> Result<Self, ScriptError> {
        validate_options(&options)?;
        if source.len() > options.maximum_lua_file_bytes {
            return Err(ScriptError::file(
                "reading FSM source",
                source_name.as_ref(),
                format!(
                    "file exceeds the {}-byte limit",
                    options.maximum_lua_file_bytes
                ),
            ));
        }

        let lua =
            create_lua().map_err(|error| ScriptError::from_mlua(error, "creating Lua 5.4 VM"))?;
        lua.set_memory_limit(options.memory_limit_bytes)
            .map_err(|error| ScriptError::from_mlua(error, "applying Lua memory limit"))?;
        if lua.used_memory() >= options.memory_limit_bytes {
            return Err(ScriptError::new(
                ScriptErrorKind::MemoryLimit,
                "initializing Lua host",
                "the configured memory limit is smaller than the sandbox baseline",
            ));
        }

        let sandbox = Sandbox::create(&lua)
            .map_err(|error| ScriptError::from_mlua(error, "creating Lua sandbox"))?;
        let source_label = source_name.as_ref().to_string_lossy();
        let fsm = sandbox
            .load_module(&lua, source, &source_label, options.instruction_limit)
            .map_err(|error| {
                ScriptError::from_mlua(error, "loading shared FSM module")
                    .with_path(source_name.as_ref())
            })?;
        validate_module(&fsm, "FSM", &["api_version", "create"])?;
        require_function(&fsm, "create").map_err(|error| {
            ScriptError::from_mlua(error, "validating shared FSM module")
                .with_path(source_name.as_ref())
        })?;
        let fsm_proxy = readonly_proxy(&lua, fsm, "shared FSM module").map_err(|error| {
            ScriptError::from_mlua(error, "protecting shared FSM module")
                .with_path(source_name.as_ref())
        })?;
        let fsm_key = lua.create_registry_value(fsm_proxy).map_err(|error| {
            ScriptError::from_mlua(error, "registering shared FSM module")
                .with_path(source_name.as_ref())
        })?;

        Ok(Self {
            inner: Rc::new(HostInner {
                lua,
                sandbox,
                options,
                fsm_key,
                modules: RefCell::new(HashMap::new()),
            }),
        })
    }

    pub fn load_species(&self, species_root: impl AsRef<Path>) -> Result<Species, ScriptError> {
        let source = load_manifest_source(species_root).map_err(species_error)?;
        let source_name = source.path.to_string_lossy();
        let manifest = self
            .inner
            .sandbox
            .load_module(
                &self.inner.lua,
                &source.bytes,
                &source_name,
                self.inner.options.instruction_limit,
            )
            .map_err(|error| {
                ScriptError::from_mlua(error, "loading species manifest").with_path(&source.path)
            })?;
        parse_manifest(&manifest, &source.root).map_err(species_error)
    }

    pub fn load_behavior(&self, species: &Species) -> Result<BehaviorModule, ScriptError> {
        self.load_behavior_descriptor(BehaviorDescriptor::from_species(species)?)
    }

    pub fn load_behavior_descriptor(
        &self,
        descriptor: BehaviorDescriptor,
    ) -> Result<BehaviorModule, ScriptError> {
        validate_descriptor(&descriptor)?;
        if descriptor.source.len() > self.inner.options.maximum_lua_file_bytes {
            return Err(ScriptError::file(
                "reading behavior source",
                &descriptor.behavior_path,
                format!(
                    "file exceeds the {}-byte limit",
                    self.inner.options.maximum_lua_file_bytes
                ),
            )
            .with_species(descriptor.species_id));
        }

        if let Some(existing) = self
            .inner
            .modules
            .borrow()
            .get(&descriptor.species_id)
            .and_then(Weak::upgrade)
        {
            if existing.behavior_path != descriptor.behavior_path {
                return Err(ScriptError::contract(
                    "loading Lua behavior module",
                    format!(
                        "species '{}' was already loaded from {}",
                        descriptor.species_id,
                        existing.behavior_path.display()
                    ),
                )
                .with_species(descriptor.species_id)
                .with_path(descriptor.behavior_path));
            }
            return Ok(BehaviorModule { inner: existing });
        }

        let source_name = descriptor.behavior_path.to_string_lossy();
        let module = self
            .inner
            .sandbox
            .load_module(
                &self.inner.lua,
                &descriptor.source,
                &source_name,
                self.inner.options.instruction_limit,
            )
            .map_err(|error| {
                ScriptError::from_mlua(error, "loading Lua behavior module")
                    .with_species(&descriptor.species_id)
                    .with_path(&descriptor.behavior_path)
            })?;
        validate_module(&module, "behavior", &["api_version", "new"]).map_err(|error| {
            error
                .with_species(&descriptor.species_id)
                .with_path(&descriptor.behavior_path)
        })?;
        require_function(&module, "new").map_err(|error| {
            ScriptError::from_mlua(error, "validating Lua behavior module")
                .with_species(&descriptor.species_id)
                .with_path(&descriptor.behavior_path)
        })?;
        let module =
            readonly_proxy(&self.inner.lua, module, "behavior module").map_err(|error| {
                ScriptError::from_mlua(error, "protecting Lua behavior module")
                    .with_species(&descriptor.species_id)
                    .with_path(&descriptor.behavior_path)
            })?;
        let module_key = self
            .inner
            .lua
            .create_registry_value(module)
            .map_err(|error| {
                ScriptError::from_mlua(error, "registering Lua behavior module")
                    .with_species(&descriptor.species_id)
                    .with_path(&descriptor.behavior_path)
            })?;
        let inner = Rc::new(BehaviorInner {
            host: Rc::clone(&self.inner),
            module_key,
            species_id: descriptor.species_id.clone(),
            behavior_path: descriptor.behavior_path,
            default_body_length: descriptor.default_body_length,
            supports_bait: descriptor.supports_bait,
            part_indices: descriptor.part_indices,
        });
        self.inner
            .modules
            .borrow_mut()
            .insert(descriptor.species_id, Rc::downgrade(&inner));
        Ok(BehaviorModule { inner })
    }

    #[must_use]
    pub fn used_memory_bytes(&self) -> usize {
        self.inner.lua.used_memory()
    }

    #[must_use]
    pub fn memory_limit_bytes(&self) -> usize {
        self.inner.options.memory_limit_bytes
    }

    pub fn collect_garbage(&self) -> Result<(), ScriptError> {
        self.inner
            .lua
            .gc_collect()
            .and_then(|()| self.inner.lua.gc_collect())
            .map_err(|error| ScriptError::from_mlua(error, "collecting Lua garbage"))
    }
}

pub(crate) struct BehaviorInner {
    pub(crate) host: Rc<HostInner>,
    pub(crate) module_key: RegistryKey,
    pub(crate) species_id: String,
    pub(crate) behavior_path: PathBuf,
    pub(crate) default_body_length: f32,
    pub(crate) supports_bait: bool,
    pub(crate) part_indices: BTreeMap<String, usize>,
}

#[derive(Clone)]
pub struct BehaviorModule {
    pub(crate) inner: Rc<BehaviorInner>,
}

impl Debug for BehaviorModule {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter
            .debug_struct("BehaviorModule")
            .field("species_id", &self.inner.species_id)
            .field("behavior_path", &self.inner.behavior_path)
            .field("part_count", &self.inner.part_indices.len())
            .finish()
    }
}

impl BehaviorModule {
    pub fn create_controller<F>(
        &self,
        instance: u64,
        config: ControllerConfig,
        random: F,
    ) -> Result<LuaController, ScriptError>
    where
        F: FnMut(&str, f32, f32) -> Result<f32, String> + 'static,
    {
        create_controller(
            Rc::clone(&self.inner),
            instance,
            config,
            Box::new(random) as RandomCallback,
        )
    }

    #[must_use]
    pub fn species_id(&self) -> &str {
        &self.inner.species_id
    }

    #[must_use]
    pub fn behavior_path(&self) -> &Path {
        &self.inner.behavior_path
    }

    #[must_use]
    pub fn part_count(&self) -> usize {
        self.inner.part_indices.len()
    }
}

impl HostInner {
    pub(crate) fn reader_limits(&self) -> ReaderLimits {
        ReaderLimits {
            maximum_table_entries: self.options.maximum_table_entries,
            maximum_string_bytes: self.options.maximum_string_bytes,
        }
    }
}

fn validate_options(options: &LuaHostOptions) -> Result<(), ScriptError> {
    if options.memory_limit_bytes == 0
        || options.instruction_limit < 100
        || options.maximum_value_depth < 4
        || options.maximum_table_entries == 0
        || options.maximum_string_bytes == 0
        || options.maximum_lua_file_bytes == 0
    {
        return Err(ScriptError::new(
            ScriptErrorKind::Initialization,
            "validating Lua host options",
            "all limits must be positive; depth must be at least 4 and instructions at least 100",
        ));
    }
    Ok(())
}

fn validate_descriptor(descriptor: &BehaviorDescriptor) -> Result<(), ScriptError> {
    if !is_valid_identifier(&descriptor.species_id) {
        return Err(ScriptError::contract(
            "loading Lua behavior module",
            "species id must contain 1..64 ASCII letters, digits, '_' or '-'",
        ));
    }
    if descriptor.behavior_path.as_os_str().is_empty() {
        return Err(ScriptError::contract(
            "loading Lua behavior module",
            "behavior source path must not be empty",
        )
        .with_species(&descriptor.species_id));
    }
    if !descriptor.default_body_length.is_finite()
        || descriptor.default_body_length <= 0.0
        || descriptor.part_indices.is_empty()
        || descriptor.part_indices.len() > MAX_PARTS
    {
        return Err(ScriptError::contract(
            "loading Lua behavior module",
            "species runtime metadata is incomplete or outside limits",
        )
        .with_species(&descriptor.species_id)
        .with_path(&descriptor.behavior_path));
    }
    let mut seen_indices = vec![false; descriptor.part_indices.len()];
    for (name, &index) in &descriptor.part_indices {
        if !is_valid_identifier(name) || index >= seen_indices.len() || seen_indices[index] {
            return Err(ScriptError::contract(
                "loading Lua behavior module",
                "part names and stable indices must be unique and contiguous",
            )
            .with_species(&descriptor.species_id)
            .with_path(&descriptor.behavior_path));
        }
        seen_indices[index] = true;
    }
    Ok(())
}

fn validate_module(
    module: &Table,
    label: &str,
    allowed_fields: &[&str],
) -> Result<(), ScriptError> {
    let mut entry_count = 0_usize;
    for pair in module.clone().pairs::<Value, Value>() {
        let (key, _) = pair
            .map_err(|error| ScriptError::from_mlua(error, format!("validating {label} module")))?;
        entry_count += 1;
        if entry_count > 32 {
            return Err(ScriptError::contract(
                format!("validating {label} module"),
                "module contains too many fields",
            ));
        }
        let Value::String(key) = key else {
            return Err(ScriptError::contract(
                format!("validating {label} module"),
                "module must contain only named fields",
            ));
        };
        let key = key.to_str().map_err(|_| {
            ScriptError::contract(
                format!("validating {label} module"),
                "module field names must be valid UTF-8",
            )
        })?;
        if !allowed_fields.contains(&key.as_ref()) {
            return Err(ScriptError::contract(
                format!("validating {label} module"),
                format!("module contains unknown field '{key}'"),
            ));
        }
    }
    match module
        .raw_get::<Value>("api_version")
        .map_err(|error| ScriptError::from_mlua(error, format!("validating {label} module")))?
    {
        Value::Integer(version) if version == API_VERSION => Ok(()),
        Value::Number(version)
            if version.is_finite() && version.fract() == 0.0 && version == API_VERSION as f64 =>
        {
            Ok(())
        }
        _ => Err(ScriptError::contract(
            format!("validating {label} module"),
            format!("api_version must be exactly {API_VERSION}"),
        )),
    }
}

fn read_script(path: &Path, maximum_bytes: usize) -> Result<Vec<u8>, ScriptError> {
    let metadata = fs::metadata(path).map_err(|error| {
        ScriptError::file(
            "reading Lua script",
            path,
            format!("cannot inspect file: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(ScriptError::file(
            "reading Lua script",
            path,
            "path is not a regular file",
        ));
    }
    if metadata.len() > maximum_bytes as u64 {
        return Err(ScriptError::file(
            "reading Lua script",
            path,
            format!("file exceeds the {maximum_bytes}-byte limit"),
        ));
    }
    let bytes = fs::read(path).map_err(|error| {
        ScriptError::file(
            "reading Lua script",
            path,
            format!("cannot read file: {error}"),
        )
    })?;
    if bytes.len() > maximum_bytes {
        return Err(ScriptError::file(
            "reading Lua script",
            path,
            format!("file exceeds the {maximum_bytes}-byte limit"),
        ));
    }
    Ok(bytes)
}

fn species_error(error: SpeciesError) -> ScriptError {
    let kind = match error.kind {
        crate::species::SpeciesErrorKind::File => ScriptErrorKind::File,
        crate::species::SpeciesErrorKind::Contract | crate::species::SpeciesErrorKind::Limit => {
            ScriptErrorKind::Contract
        }
    };
    ScriptError::new(kind, "loading species package", error.message).with_path(error.path)
}
