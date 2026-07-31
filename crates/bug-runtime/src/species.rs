//! Strict manifest data model, parser, and species-root file confinement.

use std::collections::{BTreeMap, BTreeSet};
use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File};
use std::io::Read;
use std::path::{Path, PathBuf};

use mlua::{Table, Value};

use crate::contract::{
    API_VERSION, ContractError, MAX_COORDINATE, MAX_PARTS, SourceRect, checked_f32, checked_i32,
    is_valid_identifier,
};
use crate::math::Vec2;

pub const MAX_PATH_BYTES: usize = 1_024;
pub const MAX_MANIFEST_BYTES: usize = 256 * 1_024;
pub const MAX_LUA_FILE_BYTES: usize = 1_024 * 1_024;

const MAX_TABLE_ENTRIES: usize = 8_192;
const MAX_ATLAS_DIMENSION: i32 = 16_384;
const MAX_REFERENCE_LENGTH: f64 = 100_000.0;
const MAX_OVERLAY_SCALE: f64 = 32.0;
const MAX_COLLIDER_HALF_EXTENT: f64 = 1.0;
const MAX_ATTACHMENT_COORDINATE: f64 = 32.0;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum SpeciesErrorKind {
    File,
    Contract,
    Limit,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct SpeciesError {
    pub kind: SpeciesErrorKind,
    pub path: PathBuf,
    pub message: String,
}

impl SpeciesError {
    #[must_use]
    pub fn new(
        kind: SpeciesErrorKind,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            path: path.into(),
            message: message.into(),
        }
    }

    fn file(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::new(SpeciesErrorKind::File, path, message)
    }

    fn contract(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::new(SpeciesErrorKind::Contract, path, message)
    }

    fn limit(path: impl Into<PathBuf>, message: impl Into<String>) -> Self {
        Self::new(SpeciesErrorKind::Limit, path, message)
    }
}

impl Display for SpeciesError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "{:?} error for {}: {}",
            self.kind,
            self.path.display(),
            self.message
        )
    }
}

impl Error for SpeciesError {}

pub type SpeciesResult<T> = Result<T, SpeciesError>;

#[derive(Clone, Debug, PartialEq)]
pub struct PartDefinition {
    pub name: String,
    pub source: SourceRect,
    pub pivot: Vec2,
    pub attachment: Vec2,
    pub layer: i32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct AtlasDefinition {
    pub file: PathBuf,
    pub width: i32,
    pub height: i32,
    pub reference_length: f32,
}

#[derive(Clone, Debug, PartialEq)]
pub struct BodyDefinition {
    pub default_length: f32,
    pub overlay_scale: f32,
    pub collider_half_width: f32,
    pub collider_half_length: f32,
    pub root_part: String,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct VisualDefinition {
    pub red: u8,
    pub green: u8,
    pub blue: u8,
    pub alpha: u8,
    pub shadow_alpha: u8,
    pub shadow_offset: Vec2,
}

impl Default for VisualDefinition {
    fn default() -> Self {
        Self {
            red: 255,
            green: 255,
            blue: 255,
            alpha: 255,
            shadow_alpha: 0,
            shadow_offset: Vec2::ZERO,
        }
    }
}

#[derive(Clone, Copy, Debug, Default, Eq, PartialEq)]
pub struct Capabilities {
    pub bait: bool,
}

#[derive(Clone, Debug, PartialEq)]
pub struct Species {
    pub api_version: i64,
    pub id: String,
    pub name: String,
    pub manifest_path: PathBuf,
    pub root: PathBuf,
    pub behavior_path: PathBuf,
    pub atlas: AtlasDefinition,
    pub body: BodyDefinition,
    pub visual: VisualDefinition,
    pub capabilities: Capabilities,
    pub parts: Vec<PartDefinition>,
    pub root_part_index: usize,
    /// Stable manifest part-name to manifest-index mapping.
    pub part_indices: BTreeMap<String, usize>,
}

impl Species {
    #[must_use]
    pub fn part_index(&self, name: &str) -> Option<usize> {
        self.part_indices.get(name).copied()
    }

    #[must_use]
    pub fn root_part(&self) -> Option<&PartDefinition> {
        self.parts.get(self.root_part_index)
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ManifestSource {
    pub root: PathBuf,
    pub path: PathBuf,
    pub bytes: Vec<u8>,
}

/// Canonicalizes and verifies a caller-selected species directory.
pub fn canonical_species_root(species_root: impl AsRef<Path>) -> SpeciesResult<PathBuf> {
    let species_root = species_root.as_ref();
    let canonical = fs::canonicalize(species_root).map_err(|error| {
        SpeciesError::file(
            species_root,
            format!("cannot canonicalize species directory: {error}"),
        )
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        SpeciesError::file(
            &canonical,
            format!("cannot inspect species directory: {error}"),
        )
    })?;
    if !metadata.is_dir() {
        return Err(SpeciesError::file(
            canonical,
            "species path is not a directory",
        ));
    }
    Ok(canonical)
}

/// Resolves one manifest-owned file after enforcing lexical and symlink
/// containment within the canonical species directory.
pub fn resolve_species_file(
    species_root: impl AsRef<Path>,
    raw_path: &str,
    field_path: &str,
) -> SpeciesResult<PathBuf> {
    let canonical_root = canonical_species_root(species_root)?;
    let relative = strict_relative_path(raw_path).map_err(|message| {
        SpeciesError::contract(&canonical_root, format!("{field_path} {message}"))
    })?;
    let unresolved = canonical_root.join(relative);
    let canonical_candidate = fs::canonicalize(&unresolved).map_err(|error| {
        SpeciesError::file(
            &unresolved,
            format!("{field_path} does not name a readable regular file: {error}"),
        )
    })?;
    if !canonical_candidate.starts_with(&canonical_root) {
        return Err(SpeciesError::contract(
            canonical_candidate,
            format!("{field_path} escapes the species directory"),
        ));
    }
    let metadata = fs::metadata(&canonical_candidate).map_err(|error| {
        SpeciesError::file(
            &canonical_candidate,
            format!("cannot inspect {field_path}: {error}"),
        )
    })?;
    if !metadata.is_file() {
        return Err(SpeciesError::file(
            canonical_candidate,
            format!("{field_path} does not name a regular file"),
        ));
    }
    Ok(canonical_candidate)
}

/// Reads a regular file while enforcing a hard byte limit both before and
/// during the read.
pub fn read_limited_file(path: impl AsRef<Path>, maximum_bytes: usize) -> SpeciesResult<Vec<u8>> {
    let path = path.as_ref();
    let file = File::open(path)
        .map_err(|error| SpeciesError::file(path, format!("cannot open file: {error}")))?;
    let metadata = file
        .metadata()
        .map_err(|error| SpeciesError::file(path, format!("cannot inspect file: {error}")))?;
    if !metadata.is_file() {
        return Err(SpeciesError::file(path, "path is not a regular file"));
    }
    if metadata.len() > maximum_bytes as u64 {
        return Err(SpeciesError::limit(
            path,
            format!("file exceeds the {maximum_bytes}-byte limit"),
        ));
    }

    let read_limit = (maximum_bytes as u64).saturating_add(1);
    let mut bytes = Vec::with_capacity((metadata.len() as usize).min(maximum_bytes));
    file.take(read_limit)
        .read_to_end(&mut bytes)
        .map_err(|error| SpeciesError::file(path, format!("cannot read file: {error}")))?;
    if bytes.len() > maximum_bytes {
        return Err(SpeciesError::limit(
            path,
            format!("file exceeds the {maximum_bytes}-byte limit"),
        ));
    }
    Ok(bytes)
}

/// Resolves and reads one file owned by a species manifest.
pub fn read_species_file(
    species_root: impl AsRef<Path>,
    raw_path: &str,
    field_path: &str,
    maximum_bytes: usize,
) -> SpeciesResult<(PathBuf, Vec<u8>)> {
    let path = resolve_species_file(species_root, raw_path, field_path)?;
    let bytes = read_limited_file(&path, maximum_bytes)?;
    Ok((path, bytes))
}

/// Loads the bytes required for the sandboxed manifest chunk.
pub fn load_manifest_source(species_root: impl AsRef<Path>) -> SpeciesResult<ManifestSource> {
    let root = canonical_species_root(species_root)?;
    let path = resolve_species_file(&root, "manifest.lua", "manifest")?;
    let bytes = read_limited_file(&path, MAX_MANIFEST_BYTES)?;
    Ok(ManifestSource { root, path, bytes })
}

/// Strictly parses a manifest table already evaluated by the sandboxed
/// `LuaHost`.
///
/// All table access is raw, so manifest metatables cannot execute code during
/// parsing. File paths are canonicalized and confined before the result is
/// returned.
pub fn parse_manifest(manifest: &Table, species_root: impl AsRef<Path>) -> SpeciesResult<Species> {
    let root = canonical_species_root(species_root)?;
    let manifest_path = resolve_species_file(&root, "manifest.lua", "manifest")?;
    let reader = ManifestReader::new(&manifest_path);
    reader.ensure_fields(
        manifest,
        "manifest",
        &[
            "api_version",
            "id",
            "name",
            "behavior",
            "atlas",
            "body",
            "capabilities",
            "render",
            "parts",
        ],
    )?;

    let api_version = reader.required_integer(manifest, "api_version", "api_version")?;
    if api_version != API_VERSION {
        return Err(reader.error(format!(
            "api_version must be {API_VERSION}, got {api_version}"
        )));
    }

    let id = reader.required_string(manifest, "id", "id")?;
    if !is_valid_identifier(&id) {
        return Err(reader.error("id must contain 1..64 ASCII letters, digits, '-' or '_'"));
    }
    let name = reader.required_string(manifest, "name", "name")?;
    if name.len() > 128 {
        return Err(reader.error("name must contain at most 128 UTF-8 bytes"));
    }

    let behavior = reader.required_string(manifest, "behavior", "behavior")?;
    let behavior_path = resolve_species_file(&root, &behavior, "behavior")?;

    let atlas_table = reader.required_table(manifest, "atlas", "atlas")?;
    reader.ensure_fields(
        &atlas_table,
        "atlas",
        &["file", "width", "height", "reference_length"],
    )?;
    let atlas_file = reader.required_string(&atlas_table, "file", "atlas.file")?;
    let atlas_path = resolve_species_file(&root, &atlas_file, "atlas.file")?;
    let atlas_width =
        reader.required_i32(&atlas_table, "width", 1, MAX_ATLAS_DIMENSION, "atlas.width")?;
    let atlas_height = reader.required_i32(
        &atlas_table,
        "height",
        1,
        MAX_ATLAS_DIMENSION,
        "atlas.height",
    )?;
    let reference_length = reader.required_positive_f32(
        &atlas_table,
        "reference_length",
        MAX_REFERENCE_LENGTH,
        "atlas.reference_length",
    )?;
    let atlas = AtlasDefinition {
        file: atlas_path,
        width: atlas_width,
        height: atlas_height,
        reference_length,
    };

    let body_table = reader.required_table(manifest, "body", "body")?;
    reader.ensure_fields(
        &body_table,
        "body",
        &[
            "default_length",
            "overlay_scale",
            "collider_half_width",
            "collider_half_length",
            "root_part",
        ],
    )?;
    let root_part = reader.required_string(&body_table, "root_part", "body.root_part")?;
    if !is_valid_identifier(&root_part) {
        return Err(reader.error("body.root_part is not a valid part identifier"));
    }
    let body = BodyDefinition {
        default_length: reader.required_positive_f32(
            &body_table,
            "default_length",
            MAX_REFERENCE_LENGTH,
            "body.default_length",
        )?,
        overlay_scale: reader.required_positive_f32(
            &body_table,
            "overlay_scale",
            MAX_OVERLAY_SCALE,
            "body.overlay_scale",
        )?,
        collider_half_width: reader.required_positive_f32(
            &body_table,
            "collider_half_width",
            MAX_COLLIDER_HALF_EXTENT,
            "body.collider_half_width",
        )?,
        collider_half_length: reader.required_positive_f32(
            &body_table,
            "collider_half_length",
            MAX_COLLIDER_HALF_EXTENT,
            "body.collider_half_length",
        )?,
        root_part,
    };

    let capabilities = parse_capabilities(&reader, manifest)?;
    let visual = parse_visual(&reader, manifest)?;
    let parts = parse_parts(&reader, manifest, &atlas)?;

    let part_indices = parts
        .iter()
        .enumerate()
        .map(|(index, part)| (part.name.clone(), index))
        .collect::<BTreeMap<_, _>>();
    let Some(root_part_index) = part_indices.get(&body.root_part).copied() else {
        return Err(reader.error("body.root_part does not name exactly one part"));
    };

    Ok(Species {
        api_version,
        id,
        name,
        manifest_path,
        root,
        behavior_path,
        atlas,
        body,
        visual,
        capabilities,
        parts,
        root_part_index,
        part_indices,
    })
}

fn strict_relative_path(raw_path: &str) -> Result<PathBuf, &'static str> {
    if raw_path.is_empty() {
        return Err("must be a non-empty relative file path");
    }
    if raw_path.len() > MAX_PATH_BYTES {
        return Err("must contain at most 1024 UTF-8 bytes");
    }
    if raw_path.contains('\0') {
        return Err("must not contain NUL");
    }
    if raw_path.starts_with('/') || raw_path.starts_with('\\') {
        return Err("must be a relative file path");
    }

    let mut path = PathBuf::new();
    for component in raw_path.split(['/', '\\']) {
        if component.is_empty() || matches!(component, "." | "..") {
            return Err("must contain only non-empty normal components");
        }
        if component.contains(':') {
            return Err("must not contain a Windows path prefix or alternate stream");
        }
        path.push(component);
    }
    if path.as_os_str().is_empty() {
        return Err("must be a non-empty relative file path");
    }
    Ok(path)
}

fn parse_capabilities(
    reader: &ManifestReader<'_>,
    manifest: &Table,
) -> SpeciesResult<Capabilities> {
    let Some(table) = reader.optional_table(manifest, "capabilities", "capabilities")? else {
        return Ok(Capabilities::default());
    };
    reader.ensure_fields(&table, "capabilities", &["bait"])?;
    Ok(Capabilities {
        bait: reader.optional_boolean(&table, "bait", false, "capabilities.bait")?,
    })
}

fn parse_visual(reader: &ManifestReader<'_>, manifest: &Table) -> SpeciesResult<VisualDefinition> {
    let Some(render) = reader.optional_table(manifest, "render", "render")? else {
        return Ok(VisualDefinition::default());
    };
    reader.ensure_fields(&render, "render", &["color", "shadow"])?;
    let mut visual = VisualDefinition::default();
    if let Some(value) = reader.optional_value(&render, "color", "render.color")? {
        let color = reader.color(value, "render.color")?;
        if color[3] != 255 {
            return Err(reader.error("render.color alpha must be 255"));
        }
        [visual.red, visual.green, visual.blue, visual.alpha] = color;
    }
    if let Some(shadow) = reader.optional_table(&render, "shadow", "render.shadow")? {
        reader.ensure_fields(&shadow, "render.shadow", &["color", "offset"])?;
        if let Some(value) = reader.optional_value(&shadow, "color", "render.shadow.color")? {
            let color = reader.color(value, "render.shadow.color")?;
            if color[0..3] != [0, 0, 0] {
                return Err(reader.error("render.shadow.color RGB must be black"));
            }
            visual.shadow_alpha = color[3];
        }
        if let Some(value) = reader.optional_value(&shadow, "offset", "render.shadow.offset")? {
            visual.shadow_offset = reader.point(value, MAX_COORDINATE, "render.shadow.offset")?;
        }
    }
    Ok(visual)
}

fn parse_parts(
    reader: &ManifestReader<'_>,
    manifest: &Table,
    atlas: &AtlasDefinition,
) -> SpeciesResult<Vec<PartDefinition>> {
    let parts_value = reader.required_value(manifest, "parts", "parts")?;
    let part_values = reader.indexed_values(parts_value, "parts", MAX_PARTS)?;
    if part_values.is_empty() {
        return Err(reader.error("parts must contain 1..64 entries"));
    }

    let mut names = BTreeSet::new();
    let mut parts = Vec::with_capacity(part_values.len());
    for (index, value) in part_values.into_iter().enumerate() {
        let path = format!("parts[{}]", index + 1);
        let table = reader.table(value, &path)?;
        reader.ensure_fields(
            &table,
            &path,
            &["name", "source", "pivot", "attachment", "layer"],
        )?;
        let name = reader.required_string(&table, "name", &format!("{path}.name"))?;
        if !is_valid_identifier(&name) {
            return Err(reader.error(format!("{path}.name is not a valid identifier")));
        }
        if !names.insert(name.clone()) {
            return Err(reader.error(format!("duplicate part name: {name}")));
        }

        let source_value = reader.required_value(&table, "source", &format!("{path}.source"))?;
        let source_numbers = reader.number_array(source_value, &format!("{path}.source"), 4)?;
        let source = SourceRect {
            x: reader.i32_from_number(
                source_numbers[0],
                0,
                i32::MAX,
                &format!("{path}.source[1]"),
            )?,
            y: reader.i32_from_number(
                source_numbers[1],
                0,
                i32::MAX,
                &format!("{path}.source[2]"),
            )?,
            width: reader.i32_from_number(
                source_numbers[2],
                0,
                i32::MAX,
                &format!("{path}.source[3]"),
            )?,
            height: reader.i32_from_number(
                source_numbers[3],
                0,
                i32::MAX,
                &format!("{path}.source[4]"),
            )?,
        };
        let source_right = i64::from(source.x) + i64::from(source.width);
        let source_bottom = i64::from(source.y) + i64::from(source.height);
        if source.width <= 0
            || source.height <= 0
            || source_right > i64::from(atlas.width)
            || source_bottom > i64::from(atlas.height)
        {
            return Err(reader.error(format!("{path}.source lies outside the atlas")));
        }

        let pivot_value = reader.required_value(&table, "pivot", &format!("{path}.pivot"))?;
        let attachment_value =
            reader.required_value(&table, "attachment", &format!("{path}.attachment"))?;
        let layer = reader.required_i32(
            &table,
            "layer",
            i32::MIN,
            i32::MAX,
            &format!("{path}.layer"),
        )?;
        parts.push(PartDefinition {
            name,
            source,
            pivot: reader.point(pivot_value, MAX_COORDINATE, &format!("{path}.pivot"))?,
            attachment: reader.point(
                attachment_value,
                MAX_ATTACHMENT_COORDINATE,
                &format!("{path}.attachment"),
            )?,
            layer,
        });
    }
    Ok(parts)
}

struct ManifestReader<'a> {
    subject: &'a Path,
}

impl<'a> ManifestReader<'a> {
    const fn new(subject: &'a Path) -> Self {
        Self { subject }
    }

    fn error(&self, message: impl Into<String>) -> SpeciesError {
        SpeciesError::contract(self.subject, message)
    }

    fn lua_error(&self, path: &str, error: mlua::Error) -> SpeciesError {
        self.error(format!("{path} cannot be read: {error}"))
    }

    fn ensure_fields(&self, table: &Table, path: &str, allowed: &[&str]) -> SpeciesResult<()> {
        let mut entries = 0_usize;
        for pair in table.pairs::<Value, Value>() {
            entries += 1;
            if entries > MAX_TABLE_ENTRIES {
                return Err(self.error(format!("{path} exceeds the table entry limit")));
            }
            let (key, _) = pair.map_err(|error| self.lua_error(path, error))?;
            let Value::String(key) = key else {
                return Err(self.error(format!("{path} must use string field names")));
            };
            let key = key.to_str().map_err(|error| self.lua_error(path, error))?;
            if !allowed.contains(&key.as_ref()) {
                return Err(self.error(format!("{path} contains unknown field '{key}'")));
            }
        }
        Ok(())
    }

    fn value(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<Value> {
        table
            .raw_get(field)
            .map_err(|error| self.lua_error(path, error))
    }

    fn required_value(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<Value> {
        let value = self.value(table, field, path)?;
        if matches!(value, Value::Nil) {
            return Err(self.error(format!("{path} is required")));
        }
        Ok(value)
    }

    fn optional_value(
        &self,
        table: &Table,
        field: &str,
        path: &str,
    ) -> SpeciesResult<Option<Value>> {
        let value = self.value(table, field, path)?;
        if matches!(value, Value::Nil) {
            Ok(None)
        } else {
            Ok(Some(value))
        }
    }

    fn table(&self, value: Value, path: &str) -> SpeciesResult<Table> {
        let Value::Table(table) = value else {
            return Err(self.error(format!("{path} must be a table")));
        };
        Ok(table)
    }

    fn required_table(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<Table> {
        let value = self.required_value(table, field, path)?;
        self.table(value, path)
    }

    fn optional_table(
        &self,
        table: &Table,
        field: &str,
        path: &str,
    ) -> SpeciesResult<Option<Table>> {
        self.optional_value(table, field, path)?
            .map(|value| self.table(value, path))
            .transpose()
    }

    fn required_string(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<String> {
        let value = self.required_value(table, field, path)?;
        let Value::String(string) = value else {
            return Err(self.error(format!("{path} must be a non-empty UTF-8 string")));
        };
        let string = string
            .to_str()
            .map_err(|error| self.lua_error(path, error))?;
        if string.is_empty() {
            return Err(self.error(format!("{path} must be a non-empty UTF-8 string")));
        }
        Ok(string.as_ref().to_owned())
    }

    fn required_number(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<f64> {
        let value = self.required_value(table, field, path)?;
        self.number(value, path)
    }

    fn number(&self, value: Value, path: &str) -> SpeciesResult<f64> {
        let number = match value {
            Value::Integer(integer) => integer as f64,
            Value::Number(number) => number,
            _ => return Err(self.error(format!("{path} must be a finite number"))),
        };
        if !number.is_finite() {
            return Err(self.error(format!(
                "{path} must be a finite number (not NaN or infinity)"
            )));
        }
        Ok(number)
    }

    fn required_integer(&self, table: &Table, field: &str, path: &str) -> SpeciesResult<i64> {
        let value = self.required_value(table, field, path)?;
        match value {
            Value::Integer(integer) => Ok(integer),
            Value::Number(number)
                if number.is_finite()
                    && number.fract() == 0.0
                    && number >= i64::MIN as f64
                    && number <= i64::MAX as f64 =>
            {
                Ok(number as i64)
            }
            _ => Err(self.error(format!("{path} must be an integer in the host range"))),
        }
    }

    fn required_i32(
        &self,
        table: &Table,
        field: &str,
        minimum: i32,
        maximum: i32,
        path: &str,
    ) -> SpeciesResult<i32> {
        let number = self.required_number(table, field, path)?;
        self.i32_from_number(number, minimum, maximum, path)
    }

    fn i32_from_number(
        &self,
        number: f64,
        minimum: i32,
        maximum: i32,
        path: &str,
    ) -> SpeciesResult<i32> {
        checked_i32(number, minimum, maximum, path).map_err(|error| self.contract_error(error))
    }

    fn required_positive_f32(
        &self,
        table: &Table,
        field: &str,
        maximum: f64,
        path: &str,
    ) -> SpeciesResult<f32> {
        let number = self.required_number(table, field, path)?;
        if number <= 0.0 {
            return Err(self.error(format!("{path} must be in (0, {maximum}]")));
        }
        checked_f32(number, 0.0, maximum, path).map_err(|error| self.contract_error(error))
    }

    fn optional_boolean(
        &self,
        table: &Table,
        field: &str,
        fallback: bool,
        path: &str,
    ) -> SpeciesResult<bool> {
        let value = self.value(table, field, path)?;
        match value {
            Value::Nil => Ok(fallback),
            Value::Boolean(boolean) => Ok(boolean),
            _ => Err(self.error(format!("{path} must be a boolean"))),
        }
    }

    fn indexed_values(
        &self,
        value: Value,
        path: &str,
        maximum_entries: usize,
    ) -> SpeciesResult<Vec<Value>> {
        let table = self.table(value, path)?;
        let mut indexed = BTreeMap::new();
        for pair in table.pairs::<Value, Value>() {
            if indexed.len() >= maximum_entries {
                return Err(self.error(format!(
                    "{path} must contain at most {maximum_entries} entries"
                )));
            }
            let (key, value) = pair.map_err(|error| self.lua_error(path, error))?;
            let index = match key {
                Value::Integer(integer) if integer >= 1 => usize::try_from(integer).ok(),
                Value::Number(number)
                    if number.is_finite() && number.fract() == 0.0 && number >= 1.0 =>
                {
                    Some(number as usize)
                }
                _ => None,
            };
            let Some(index) = index.filter(|index| *index <= maximum_entries) else {
                return Err(
                    self.error(format!("{path} must contain only consecutive numeric keys"))
                );
            };
            if indexed.insert(index, value).is_some() {
                return Err(self.error(format!("{path} contains a duplicate array index")));
            }
        }
        if indexed.keys().copied().ne(1..=indexed.len()) {
            return Err(self.error(format!("{path} must not contain array holes")));
        }
        Ok(indexed.into_values().collect())
    }

    fn number_array(
        &self,
        value: Value,
        path: &str,
        expected_size: usize,
    ) -> SpeciesResult<Vec<f64>> {
        let values = self.indexed_values(value, path, expected_size)?;
        if values.len() != expected_size {
            return Err(self.error(format!(
                "{path} must contain exactly {expected_size} numbers"
            )));
        }
        values
            .into_iter()
            .enumerate()
            .map(|(index, value)| self.number(value, &format!("{path}[{}]", index + 1)))
            .collect()
    }

    fn point(&self, value: Value, maximum: f64, path: &str) -> SpeciesResult<Vec2> {
        let values = self.number_array(value, path, 2)?;
        Ok(Vec2 {
            x: checked_f32(values[0], -maximum, maximum, format!("{path}[1]"))
                .map_err(|error| self.contract_error(error))?,
            y: checked_f32(values[1], -maximum, maximum, format!("{path}[2]"))
                .map_err(|error| self.contract_error(error))?,
        })
    }

    fn color(&self, value: Value, path: &str) -> SpeciesResult<[u8; 4]> {
        let values = self.number_array(value, path, 4)?;
        let mut output = [0_u8; 4];
        for (index, value) in values.into_iter().enumerate() {
            let component = checked_i32(value, 0, 255, format!("{path}[{}]", index + 1))
                .map_err(|error| self.contract_error(error))?;
            output[index] = component as u8;
        }
        Ok(output)
    }

    fn contract_error(&self, error: ContractError) -> SpeciesError {
        self.error(error.to_string())
    }
}
