use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::path::{Path, PathBuf};

use mlua::Error as MluaError;

use super::budget::INSTRUCTION_LIMIT_MARKER;

pub(crate) const HOST_CALLBACK_MARKER: &str = "__bug_host_callback__";

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum ScriptErrorKind {
    Initialization,
    File,
    Syntax,
    Runtime,
    InstructionLimit,
    MemoryLimit,
    Contract,
    HostCallback,
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct ScriptError {
    pub kind: ScriptErrorKind,
    pub operation: Box<str>,
    pub species: Option<Box<str>>,
    pub instance: Option<u64>,
    pub path: Option<Box<Path>>,
    pub message: Box<str>,
    pub traceback: Box<str>,
}

impl ScriptError {
    #[must_use]
    pub fn new(
        kind: ScriptErrorKind,
        operation: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            kind,
            operation: operation.into().into_boxed_str(),
            species: None,
            instance: None,
            path: None,
            message: message.into().into_boxed_str(),
            traceback: Box::from(""),
        }
    }

    #[must_use]
    pub fn contract(operation: impl Into<String>, message: impl Into<String>) -> Self {
        Self::new(ScriptErrorKind::Contract, operation, message)
    }

    #[must_use]
    pub fn file(
        operation: impl Into<String>,
        path: impl Into<PathBuf>,
        message: impl Into<String>,
    ) -> Self {
        let mut error = Self::new(ScriptErrorKind::File, operation, message);
        error.path = Some(path.into().into_boxed_path());
        error
    }

    #[must_use]
    pub fn from_mlua(error: MluaError, operation: impl Into<String>) -> Self {
        let rendered = error.to_string();
        let kind = classify_mlua_error(&error, &rendered);
        let message = concise_message(&rendered);
        Self {
            kind,
            operation: operation.into().into_boxed_str(),
            species: None,
            instance: None,
            path: None,
            message: message.into_boxed_str(),
            traceback: rendered.into_boxed_str(),
        }
    }

    #[must_use]
    pub fn with_species(mut self, species: impl Into<String>) -> Self {
        self.species = Some(species.into().into_boxed_str());
        self
    }

    #[must_use]
    pub fn with_instance(mut self, instance: u64) -> Self {
        self.instance = Some(instance);
        self
    }

    #[must_use]
    pub fn with_path(mut self, path: impl AsRef<Path>) -> Self {
        self.path = Some(path.as_ref().into());
        self
    }
}

impl Display for ScriptError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(
            formatter,
            "Lua {:?} error while {}",
            self.kind, self.operation
        )?;
        if let Some(species) = &self.species {
            write!(formatter, " [species {species}]")?;
        }
        if let Some(instance) = self.instance {
            write!(formatter, " [instance {instance}]")?;
        }
        if let Some(path) = &self.path {
            write!(formatter, " [{}]", path.display())?;
        }
        if !self.message.is_empty() {
            write!(formatter, ": {}", self.message)?;
        }
        Ok(())
    }
}

impl Error for ScriptError {}

fn classify_mlua_error(error: &MluaError, rendered: &str) -> ScriptErrorKind {
    if rendered.contains(INSTRUCTION_LIMIT_MARKER) {
        return ScriptErrorKind::InstructionLimit;
    }
    if rendered.contains(HOST_CALLBACK_MARKER) {
        return ScriptErrorKind::HostCallback;
    }

    match error {
        MluaError::SyntaxError { .. } => ScriptErrorKind::Syntax,
        MluaError::MemoryError(_) => ScriptErrorKind::MemoryLimit,
        MluaError::CallbackError { cause, .. }
        | MluaError::BadArgument { cause, .. }
        | MluaError::WithContext { cause, .. } => classify_mlua_error(cause, rendered),
        MluaError::FromLuaConversionError { .. } => ScriptErrorKind::Contract,
        MluaError::MemoryControlNotAvailable => ScriptErrorKind::Initialization,
        _ => ScriptErrorKind::Runtime,
    }
}

fn concise_message(rendered: &str) -> String {
    let first_line = rendered.lines().next().unwrap_or(rendered);
    first_line
        .replace(INSTRUCTION_LIMIT_MARKER, "instruction budget exceeded")
        .replace(HOST_CALLBACK_MARKER, "host callback failed")
}
