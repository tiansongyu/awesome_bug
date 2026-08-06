#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(any(windows, target_os = "macos")))]
compile_error!("cockroach_overlay supports Windows and macOS only");

#[cfg(any(windows, target_os = "macos"))]
fn main() {
    if let Err(error) = bug_windows::app::run(bug_windows::cli::DefaultMode::Single) {
        bug_windows::app::report_error(&error);
        std::process::exit(1);
    }
}
