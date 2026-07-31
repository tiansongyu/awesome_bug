//! Resource discovery anchored exclusively to the executable directory.

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::fs;
use std::path::{Path, PathBuf};

use bug_runtime::species::{canonical_species_root, resolve_species_file};

use crate::cli::Options;

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourcePaths {
    pub executable_directory: PathBuf,
    pub bugs_root: PathBuf,
    pub fsm_path: PathBuf,
    pub species_root: PathBuf,
    pub asset_override: Option<PathBuf>,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ResourceError(pub String);

impl Display for ResourceError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl Error for ResourceError {}

pub fn discover(executable: &Path, options: &Options) -> Result<ResourcePaths, ResourceError> {
    let executable = fs::canonicalize(executable).map_err(|error| {
        ResourceError(format!(
            "cannot resolve executable {}: {error}",
            executable.display()
        ))
    })?;
    let executable_directory = executable
        .parent()
        .ok_or_else(|| ResourceError("the executable has no parent directory".to_owned()))?
        .to_path_buf();
    let bugs_root = executable_directory.join("bugs");
    let fsm_path = resolve_species_file(&bugs_root, "runtime/fsm.lua", "runtime FSM")
        .map_err(|error| ResourceError(error.to_string()))?;
    let requested_species = options
        .species_path
        .clone()
        .unwrap_or_else(|| bugs_root.join(&options.species));
    let species_root = canonical_species_root(&requested_species)
        .map_err(|error| ResourceError(error.to_string()))?;
    let asset_override = options
        .asset
        .as_deref()
        .map(canonical_regular_file)
        .transpose()?;

    Ok(ResourcePaths {
        executable_directory,
        bugs_root,
        fsm_path,
        species_root,
        asset_override,
    })
}

fn canonical_regular_file(path: &Path) -> Result<PathBuf, ResourceError> {
    let canonical = fs::canonicalize(path).map_err(|error| {
        ResourceError(format!("cannot resolve asset {}: {error}", path.display()))
    })?;
    let metadata = fs::metadata(&canonical).map_err(|error| {
        ResourceError(format!(
            "cannot inspect asset {}: {error}",
            canonical.display()
        ))
    })?;
    if !metadata.is_file() {
        return Err(ResourceError(format!(
            "asset is not a regular file: {}",
            canonical.display()
        )));
    }
    Ok(canonical)
}
