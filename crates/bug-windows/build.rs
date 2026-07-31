use std::env;
use std::fs;
use std::path::{Path, PathBuf};

const WINDOWS_BINARIES: [&str; 2] = ["cockroach_overlay", "cockroach_swarm_20"];

fn main() {
    println!("cargo:rerun-if-changed=resources/app.rc");
    println!("cargo:rerun-if-changed=../../packaging/cockroach.ico");
    println!("cargo:rerun-if-env-changed=CARGO_PKG_VERSION");
    println!("cargo:rerun-if-env-changed=SDL2_LIB_DIR");
    println!("cargo:rerun-if-env-changed=SDL2_INCLUDE_PATH");

    let target = required_environment("TARGET");
    if !target.contains("-windows-") {
        return;
    }

    configure_sdl_link_search(&target);
    configure_gui_subsystem(&target);
    compile_windows_resources();
}

fn configure_sdl_link_search(target: &str) {
    let directory = PathBuf::from(required_environment("SDL2_LIB_DIR"));
    if !directory.is_dir() {
        panic!("SDL2_LIB_DIR is not a directory: {}", directory.display());
    }

    let candidates: &[&str] = if target.ends_with("-msvc") {
        &["SDL2.lib"]
    } else {
        &["libSDL2.dll.a", "libSDL2.a"]
    };
    if !candidates.iter().any(|name| directory.join(name).is_file()) {
        panic!(
            "SDL2_LIB_DIR {} does not contain one of: {}",
            directory.display(),
            candidates.join(", ")
        );
    }

    println!("cargo:rustc-link-search=native={}", directory.display());
}

fn configure_gui_subsystem(target: &str) {
    let linker_argument = if target.ends_with("-msvc") {
        "/SUBSYSTEM:WINDOWS"
    } else {
        "-Wl,--subsystem,windows"
    };
    for binary in WINDOWS_BINARIES {
        println!("cargo:rustc-link-arg-bin={binary}={linker_argument}");
    }
}

fn compile_windows_resources() {
    let manifest_directory = PathBuf::from(required_environment("CARGO_MANIFEST_DIR"));
    let output_directory = PathBuf::from(required_environment("OUT_DIR"));
    let icon = manifest_directory.join("../../packaging/cockroach.ico");
    let icon = icon
        .canonicalize()
        .unwrap_or_else(|error| panic!("cannot resolve {}: {error}", icon.display()));

    let template_path = manifest_directory.join("resources/app.rc");
    let template = fs::read_to_string(&template_path)
        .unwrap_or_else(|error| panic!("cannot read {}: {error}", template_path.display()));
    let version = required_environment("CARGO_PKG_VERSION");
    let (version_quad, version_comma) = windows_version(&version);
    let rendered = template
        .replace("@COCKROACH_ICON_PATH@", &resource_path(&icon))
        .replace("@APP_VERSION_QUAD@", &version_quad)
        .replace("@APP_VERSION_COMMA@", &version_comma)
        .replace("@APP_VERSION@", &version);
    if rendered.contains("@COCKROACH_ICON_PATH@") || rendered.contains("@APP_VERSION") {
        panic!(
            "unresolved resource placeholder in {}",
            template_path.display()
        );
    }

    let generated_resource = output_directory.join("cockroach-app.rc");
    fs::write(&generated_resource, rendered).unwrap_or_else(|error| {
        panic!(
            "cannot write generated resource {}: {error}",
            generated_resource.display()
        )
    });

    embed_resource::compile(&generated_resource, embed_resource::NONE)
        .manifest_required()
        .unwrap_or_else(|error| panic!("{error}"));
}

fn windows_version(version: &str) -> (String, String) {
    let numeric_end = version.find(['-', '+']).unwrap_or(version.len());
    let numeric = &version[..numeric_end];
    let components = numeric
        .split('.')
        .map(|component| {
            component
                .parse::<u16>()
                .unwrap_or_else(|_| panic!("package version component is not a u16: {component}"))
        })
        .collect::<Vec<_>>();
    if components.len() != 3 {
        panic!("package version must contain major.minor.patch: {version}");
    }

    (
        format!("{}.{}.{}.0", components[0], components[1], components[2]),
        format!("{},{},{},0", components[0], components[1], components[2]),
    )
}

fn resource_path(path: &Path) -> String {
    path.to_string_lossy()
        .replace('\\', "\\\\")
        .replace('"', "\\\"")
}

fn required_environment(name: &str) -> String {
    env::var(name).unwrap_or_else(|_| panic!("Cargo did not provide {name}"))
}
