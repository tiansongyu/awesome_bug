//! Small, deterministic command-line parser shared by both Windows binaries.

use std::collections::BTreeSet;
use std::ffi::{OsStr, OsString};
use std::fmt::{self, Display, Formatter};
use std::path::PathBuf;

use bug_runtime::contract::is_valid_identifier;

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
pub enum DefaultMode {
    Single,
    Swarm20,
}

impl DefaultMode {
    const fn count(self) -> usize {
        match self {
            Self::Single => 1,
            Self::Swarm20 => 20,
        }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub struct Options {
    pub species: String,
    pub species_path: Option<PathBuf>,
    pub asset: Option<PathBuf>,
    pub body_size: Option<f32>,
    pub speed_multiplier: f32,
    pub display: usize,
    pub count: usize,
    pub seed: Option<u64>,
    pub click_through: bool,
    pub maximum_frames: Option<u64>,
    pub trace: Option<PathBuf>,
    pub show_help: bool,
}

impl Options {
    #[must_use]
    pub fn defaults(mode: DefaultMode) -> Self {
        Self {
            species: "cockroach".to_owned(),
            species_path: None,
            asset: None,
            body_size: None,
            speed_multiplier: 3.0,
            display: 0,
            count: mode.count(),
            seed: None,
            click_through: true,
            maximum_frames: None,
            trace: None,
            show_help: false,
        }
    }
}

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct CliError(pub String);

impl Display for CliError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.0)
    }
}

impl std::error::Error for CliError {}

pub fn parse(
    arguments: impl IntoIterator<Item = OsString>,
    mode: DefaultMode,
) -> Result<Options, CliError> {
    let mut arguments = arguments.into_iter();
    let _program = arguments.next();
    let mut options = Options::defaults(mode);
    let mut seen = BTreeSet::new();

    while let Some(argument) = arguments.next() {
        let Some(flag) = argument.to_str() else {
            return Err(CliError("option names must be valid Unicode".to_owned()));
        };
        match flag {
            "--help" | "-h" => {
                mark_once(&mut seen, "help")?;
                options.show_help = true;
            }
            "--no-click-through" => {
                mark_once(&mut seen, "no-click-through")?;
                options.click_through = false;
            }
            "--species" => {
                mark_once(&mut seen, "species")?;
                options.species = parse_text(take_value(&mut arguments, flag)?, flag)?;
            }
            "--species-path" => {
                mark_once(&mut seen, "species-path")?;
                options.species_path = Some(PathBuf::from(take_value(&mut arguments, flag)?));
            }
            "--asset" => {
                mark_once(&mut seen, "asset")?;
                options.asset = Some(PathBuf::from(take_value(&mut arguments, flag)?));
            }
            "--size" => {
                mark_once(&mut seen, "size")?;
                options.body_size = Some(parse_f32(
                    take_value(&mut arguments, flag)?,
                    flag,
                    100.0,
                    520.0,
                )?);
            }
            "--speed" => {
                mark_once(&mut seen, "speed")?;
                options.speed_multiplier =
                    parse_f32(take_value(&mut arguments, flag)?, flag, 0.25, 3.0)?;
            }
            "--display" => {
                mark_once(&mut seen, "display")?;
                options.display = parse_usize(take_value(&mut arguments, flag)?, flag, 0, 63)?;
            }
            "--count" => {
                mark_once(&mut seen, "count")?;
                options.count = parse_usize(take_value(&mut arguments, flag)?, flag, 1, 50)?;
            }
            "--seed" => {
                mark_once(&mut seen, "seed")?;
                options.seed = Some(parse_u64(take_value(&mut arguments, flag)?, flag)?);
            }
            "--frames" => {
                mark_once(&mut seen, "frames")?;
                options.maximum_frames = Some(parse_u64(take_value(&mut arguments, flag)?, flag)?);
            }
            "--trace" => {
                mark_once(&mut seen, "trace")?;
                options.trace = Some(PathBuf::from(take_value(&mut arguments, flag)?));
            }
            _ => return Err(CliError(format!("unknown option: {flag}"))),
        }
    }

    if !is_valid_identifier(&options.species) {
        return Err(CliError(
            "--species must contain 1..64 ASCII letters, digits, '-' or '_'".to_owned(),
        ));
    }
    if options
        .species_path
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(CliError("--species-path must not be empty".to_owned()));
    }
    if options
        .asset
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(CliError("--asset must not be empty".to_owned()));
    }
    if options
        .trace
        .as_ref()
        .is_some_and(|path| path.as_os_str().is_empty())
    {
        return Err(CliError("--trace must not be empty".to_owned()));
    }
    Ok(options)
}

fn mark_once(seen: &mut BTreeSet<&'static str>, flag: &'static str) -> Result<(), CliError> {
    if seen.insert(flag) {
        Ok(())
    } else {
        Err(CliError(format!(
            "option --{flag} was specified more than once"
        )))
    }
}

fn take_value(
    arguments: &mut impl Iterator<Item = OsString>,
    flag: &str,
) -> Result<OsString, CliError> {
    arguments
        .next()
        .ok_or_else(|| CliError(format!("missing value after {flag}")))
}

fn parse_text(value: OsString, flag: &str) -> Result<String, CliError> {
    value
        .into_string()
        .map_err(|_| CliError(format!("{flag} must be valid Unicode")))
}

fn parse_f32(value: OsString, flag: &str, minimum: f32, maximum: f32) -> Result<f32, CliError> {
    let text = numeric_text(&value, flag)?;
    let parsed = text
        .parse::<f32>()
        .map_err(|_| CliError(format!("invalid {flag} value")))?;
    if !parsed.is_finite() || !(minimum..=maximum).contains(&parsed) {
        return Err(CliError(format!(
            "{flag} must be a finite number in [{minimum}, {maximum}]"
        )));
    }
    Ok(parsed)
}

fn parse_usize(
    value: OsString,
    flag: &str,
    minimum: usize,
    maximum: usize,
) -> Result<usize, CliError> {
    let text = numeric_text(&value, flag)?;
    let parsed = text
        .parse::<usize>()
        .map_err(|_| CliError(format!("invalid {flag} value")))?;
    if !(minimum..=maximum).contains(&parsed) {
        return Err(CliError(format!(
            "{flag} must be an integer in [{minimum}, {maximum}]"
        )));
    }
    Ok(parsed)
}

fn parse_u64(value: OsString, flag: &str) -> Result<u64, CliError> {
    numeric_text(&value, flag)?
        .parse::<u64>()
        .map_err(|_| CliError(format!("invalid {flag} value")))
}

fn numeric_text<'value>(value: &'value OsStr, flag: &str) -> Result<&'value str, CliError> {
    value
        .to_str()
        .ok_or_else(|| CliError(format!("{flag} value must be valid Unicode")))
}

#[must_use]
pub fn usage(executable: &str, mode: DefaultMode) -> String {
    format!(
        "\
Scriptable Bug Overlay (Rust + Lua)\n\n\
Usage: {executable} [options]\n\
  --species ID          species package under bugs/ (default cockroach)\n\
  --species-path DIR    explicit species package directory\n\
  --asset PATH          alternate compatible atlas PNG\n\
  --size N              fixed body length in pixels (100..520; default auto)\n\
  --speed N             speed multiplier (0.25..3; default 3)\n\
  --display N           display index (default 0)\n\
  --count N             bug count (1..50; default {})\n\
  --seed N              deterministic master seed\n\
  --no-click-through    let overlay windows receive mouse input\n\
  --frames N            exit after N frames (test mode)\n\
  --trace PATH          write a deterministic frame trace\n\
  --help                show this help\n\n\
Windows single-pet hotkeys:\n\
  Ctrl+Alt+F            place or move food bait\n\
  Ctrl+Alt+Q            quit\n",
        mode.count()
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(values: &[&str]) -> Vec<OsString> {
        values.iter().map(OsString::from).collect()
    }

    #[test]
    fn modes_only_change_default_count() {
        assert_eq!(
            parse(args(&["one.exe"]), DefaultMode::Single)
                .expect("defaults")
                .count,
            1
        );
        assert_eq!(
            parse(args(&["many.exe"]), DefaultMode::Swarm20)
                .expect("defaults")
                .count,
            20
        );
    }

    #[test]
    fn parses_complete_explicit_configuration() {
        let parsed = parse(
            args(&[
                "bug.exe",
                "--species",
                "beetle_2",
                "--species-path",
                "D:/bugs/beetle",
                "--asset",
                "D:/atlas.png",
                "--size",
                "200.5",
                "--speed",
                "1.75",
                "--display",
                "2",
                "--count",
                "7",
                "--seed",
                "42",
                "--frames",
                "120",
                "--trace",
                "trace.tsv",
                "--no-click-through",
            ]),
            DefaultMode::Single,
        )
        .expect("valid arguments");
        assert_eq!(parsed.species, "beetle_2");
        assert_eq!(parsed.body_size, Some(200.5));
        assert_eq!(parsed.speed_multiplier, 1.75);
        assert_eq!(parsed.display, 2);
        assert_eq!(parsed.count, 7);
        assert_eq!(parsed.seed, Some(42));
        assert_eq!(parsed.maximum_frames, Some(120));
        assert!(!parsed.click_through);
    }

    #[test]
    fn rejects_duplicates_non_finite_and_paths_as_species() {
        assert!(
            parse(
                args(&["bug.exe", "--count", "1", "--count", "2"]),
                DefaultMode::Single
            )
            .is_err()
        );
        assert!(parse(args(&["bug.exe", "--speed", "NaN"]), DefaultMode::Single).is_err());
        assert!(
            parse(
                args(&["bug.exe", "--species", "../roach"]),
                DefaultMode::Single
            )
            .is_err()
        );
    }
}
