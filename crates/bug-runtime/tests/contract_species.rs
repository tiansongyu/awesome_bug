use std::fs;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use bug_runtime::contract::{
    ContractError, MotionIntent, MotionLimits, checked_f32, checked_i32, is_valid_identifier,
};
use bug_runtime::math::{Vec2, forward_from_heading, rotate_local, wrap_angle};
use bug_runtime::species::{
    MAX_MANIFEST_BYTES, SpeciesErrorKind, load_manifest_source, parse_manifest, read_limited_file,
    resolve_species_file,
};
use mlua::{Lua, Table};

static NEXT_TREE: AtomicU64 = AtomicU64::new(0);

struct TemporaryTree {
    root: PathBuf,
}

impl TemporaryTree {
    fn new() -> Self {
        let nonce = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("the system clock must be after the Unix epoch")
            .as_nanos();
        let serial = NEXT_TREE.fetch_add(1, Ordering::Relaxed);
        let root = std::env::temp_dir().join(format!(
            "desktop-display-rust-species-{}-{nonce}-{serial}",
            std::process::id()
        ));
        fs::create_dir_all(&root).expect("temporary tree should be creatable");
        Self { root }
    }

    fn write(&self, relative: impl AsRef<Path>, contents: impl AsRef<[u8]>) {
        let path = self.root.join(relative);
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).expect("fixture parent should be creatable");
        }
        fs::write(path, contents).expect("fixture should be writable");
    }
}

impl Drop for TemporaryTree {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.root);
    }
}

fn valid_manifest(extra: &str) -> String {
    format!(
        r#"
return {{
    api_version = 1,
    id = "fixture",
    name = "Fixture Bug",
    behavior = "behavior.lua",
    atlas = {{
        file = "atlas.png",
        width = 16,
        height = 16,
        reference_length = 16,
    }},
    body = {{
        default_length = 12,
        overlay_scale = 2,
        collider_half_width = 0.25,
        collider_half_length = 0.40,
        root_part = "body",
    }},
    parts = {{
        {{
            name = "body",
            source = {{ 0, 0, 8, 8 }},
            pivot = {{ 4, 4 }},
            attachment = {{ 0, 0 }},
            layer = 7,
        }},
    }},
    {extra}
}}
"#
    )
}

fn fixture(script: &str) -> TemporaryTree {
    let tree = TemporaryTree::new();
    tree.write("manifest.lua", script);
    tree.write("behavior.lua", "return {}\n");
    tree.write("atlas.png", b"not decoded by the manifest reader");
    tree
}

fn evaluate_and_parse(
    root: &Path,
    script: &str,
) -> bug_runtime::species::SpeciesResult<bug_runtime::species::Species> {
    let lua = Lua::new();
    let table: Table = lua
        .load(script)
        .set_name("manifest.lua")
        .eval()
        .expect("fixture Lua should evaluate");
    parse_manifest(&table, root)
}

#[test]
fn math_preserves_screen_heading_convention() {
    let forward = forward_from_heading(0.0);
    assert_eq!(forward, Vec2::new(0.0, -1.0));
    let right = rotate_local(forward, std::f32::consts::FRAC_PI_2);
    assert!((right.x - 1.0).abs() < 1.0e-6);
    assert!(right.y.abs() < 1.0e-6);

    assert_eq!(wrap_angle(std::f32::consts::PI), std::f32::consts::PI);
    assert_eq!(wrap_angle(-std::f32::consts::PI), -std::f32::consts::PI);
    assert_eq!(wrap_angle(f32::INFINITY), 0.0);
    assert!((Vec2::new(3.0, 4.0).normalized().length() - 1.0).abs() < 1.0e-6);
    assert_eq!(Vec2::new(0.000_01, 0.0).normalized(), Vec2::ZERO);
}

#[test]
fn numeric_boundary_rejects_non_finite_and_canonicalizes_zero() {
    let negative_zero = checked_f32(-0.0, -1.0, 1.0, "value").expect("zero is in range");
    assert_eq!(negative_zero.to_bits(), 0.0_f32.to_bits());

    for value in [f64::NAN, f64::INFINITY, f64::NEG_INFINITY] {
        let error =
            checked_f32(value, -1.0, 1.0, "value").expect_err("non-finite values must be rejected");
        assert_eq!(error.path, "value");
        assert!(error.message.contains("finite"));
    }
    assert!(checked_i32(1.5, 0, 10, "integer").is_err());
    assert!(checked_i32(11.0, 0, 10, "integer").is_err());
    assert!(is_valid_identifier("roach_2-large"));
    assert!(!is_valid_identifier("../roach"));
}

#[test]
fn motion_intent_validation_is_strict() {
    let valid = MotionIntent {
        direction: Vec2::new(0.0, -1.0),
        speed: 180.0,
        turn_rate: 4.5,
        acceleration: 680.0,
        intentionally_still: false,
        ..MotionIntent::default()
    };
    valid
        .validate(MotionLimits::default())
        .expect("normal motion should validate");

    let invalid = MotionIntent {
        speed: f32::NAN,
        ..valid
    };
    let error: ContractError = invalid
        .validate(MotionLimits::default())
        .expect_err("NaN speed must fail");
    assert_eq!(error.path, "step.motion.speed");
}

#[test]
fn valid_manifest_is_fully_compiled() {
    let script = valid_manifest(
        r#"
capabilities = { bait = true },
render = {
    color = { 190, 191, 192, 255 },
    shadow = { color = { 0, 0, 0, 38 }, offset = { 3, -5 } },
},
"#,
    );
    let tree = fixture(&script);
    let species = evaluate_and_parse(&tree.root, &script).expect("valid manifest should parse");

    assert_eq!(species.id, "fixture");
    assert_eq!(species.api_version, 1);
    assert!(species.root.is_absolute());
    assert_eq!(species.root_part_index, 0);
    assert_eq!(species.part_index("body"), Some(0));
    assert_eq!(
        species.root_part().map(|part| part.name.as_str()),
        Some("body")
    );
    assert_eq!(species.atlas.width, 16);
    assert_eq!(species.atlas.height, 16);
    assert_eq!(species.body.default_length, 12.0);
    assert!(species.capabilities.bait);
    assert_eq!(
        (
            species.visual.red,
            species.visual.green,
            species.visual.blue,
            species.visual.alpha,
            species.visual.shadow_alpha,
        ),
        (190, 191, 192, 255, 38)
    );
    assert_eq!(species.visual.shadow_offset, Vec2::new(3.0, -5.0));
    assert!(species.behavior_path.is_file());
    assert!(species.atlas.file.is_file());

    let source = load_manifest_source(&tree.root).expect("manifest source should load");
    assert_eq!(source.bytes, script.as_bytes());
    assert_eq!(source.root, species.root);
}

#[test]
fn optional_manifest_sections_have_safe_defaults() {
    let script = valid_manifest("");
    let tree = fixture(&script);
    let species = evaluate_and_parse(&tree.root, &script).expect("valid manifest should parse");
    assert!(!species.capabilities.bait);
    assert_eq!(
        (
            species.visual.red,
            species.visual.green,
            species.visual.blue,
            species.visual.alpha,
            species.visual.shadow_alpha,
        ),
        (255, 255, 255, 255, 0)
    );
    assert_eq!(species.visual.shadow_offset, Vec2::ZERO);
}

#[test]
fn manifest_rejects_unknown_fields_and_bad_numbers() {
    let unknown_script = valid_manifest("typo_field = true,");
    let unknown_tree = fixture(&unknown_script);
    let error = evaluate_and_parse(&unknown_tree.root, &unknown_script)
        .expect_err("unknown manifest fields must fail");
    assert_eq!(error.kind, SpeciesErrorKind::Contract);
    assert!(error.message.contains("unknown field 'typo_field'"));

    let nan_script =
        valid_manifest("").replace("reference_length = 16", "reference_length = 0 / 0");
    let nan_tree = fixture(&nan_script);
    let error = evaluate_and_parse(&nan_tree.root, &nan_script).expect_err("NaN must be rejected");
    assert!(error.message.contains("NaN or infinity"));

    let fractional_script = valid_manifest("").replace("width = 16", "width = 15.5");
    let fractional_tree = fixture(&fractional_script);
    let error = evaluate_and_parse(&fractional_tree.root, &fractional_script)
        .expect_err("fractional dimensions must fail");
    assert!(error.message.contains("integer"));

    let collider_script =
        valid_manifest("").replace("collider_half_width = 0.25", "collider_half_width = 1.01");
    let collider_tree = fixture(&collider_script);
    let error = evaluate_and_parse(&collider_tree.root, &collider_script)
        .expect_err("a collider unsupported by the motion solver must fail");
    assert!(error.message.contains("body.collider_half_width"));

    let translucent_script = valid_manifest("render = { color = { 255, 255, 255, 254 } },");
    let translucent_tree = fixture(&translucent_script);
    let error = evaluate_and_parse(&translucent_tree.root, &translucent_script)
        .expect_err("whole-sprite translucency must fail at manifest load");
    assert!(error.message.contains("render.color alpha must be 255"));
}

#[test]
fn manifest_rejects_array_holes_duplicates_and_atlas_overflow() {
    let hole_script =
        valid_manifest("").replace("parts = {\n        {", "parts = {\n        [2] = {");
    let hole_tree = fixture(&hole_script);
    let error =
        evaluate_and_parse(&hole_tree.root, &hole_script).expect_err("array holes must fail");
    assert!(error.message.contains("holes"));

    let duplicate_script = valid_manifest("").replacen(
        "        },\n    },\n",
        r#"        },
        {
            name = "body",
            source = { 8, 0, 8, 8 },
            pivot = { 4, 4 },
            attachment = { 0, 0 },
            layer = 8,
        },
    },
"#,
        1,
    );
    let duplicate_tree = fixture(&duplicate_script);
    let error = evaluate_and_parse(&duplicate_tree.root, &duplicate_script)
        .expect_err("duplicate part names must fail");
    assert!(error.message.contains("duplicate part name"));

    let overflow_script =
        valid_manifest("").replace("source = { 0, 0, 8, 8 }", "source = { 9, 9, 8, 8 }");
    let overflow_tree = fixture(&overflow_script);
    let error = evaluate_and_parse(&overflow_tree.root, &overflow_script)
        .expect_err("source outside atlas must fail");
    assert!(error.message.contains("outside the atlas"));
}

#[test]
fn species_paths_reject_ambiguous_or_escaping_names() {
    let script = valid_manifest("");
    let tree = fixture(&script);
    for invalid in [
        "",
        "../outside.lua",
        "./behavior.lua",
        "nested//behavior.lua",
        "/absolute.lua",
        r"C:\absolute.lua",
        "behavior.lua:stream",
        "behavior.lua\0suffix",
    ] {
        let error = resolve_species_file(&tree.root, invalid, "behavior")
            .expect_err("unsafe path should fail");
        assert_eq!(error.kind, SpeciesErrorKind::Contract, "{invalid:?}");
    }
}

#[cfg(unix)]
#[test]
fn canonical_path_check_rejects_symlink_escape() {
    use std::os::unix::fs::symlink;

    let script = valid_manifest("");
    let tree = fixture(&script);
    let outside = TemporaryTree::new();
    outside.write("outside.lua", "return {}\n");
    symlink(
        outside.root.join("outside.lua"),
        tree.root.join("linked.lua"),
    )
    .expect("symlink fixture should be creatable");

    let error = resolve_species_file(&tree.root, "linked.lua", "behavior")
        .expect_err("symlink escape must fail");
    assert_eq!(error.kind, SpeciesErrorKind::Contract);
    assert!(error.message.contains("escapes"));
}

#[test]
fn limited_reader_checks_manifest_size() {
    let tree = TemporaryTree::new();
    tree.write("large.lua", vec![b'x'; MAX_MANIFEST_BYTES + 1]);
    let error = read_limited_file(tree.root.join("large.lua"), MAX_MANIFEST_BYTES)
        .expect_err("oversized files must fail");
    assert_eq!(error.kind, SpeciesErrorKind::Limit);
}
