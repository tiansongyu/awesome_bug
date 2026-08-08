#![cfg_attr(windows, windows_subsystem = "windows")]

#[cfg(not(windows))]
compile_error!("turtle_overlay is a Windows-only desktop application");

#[cfg(windows)]
fn main() {
    if let Err(error) = bug_windows::app::run(bug_windows::cli::DefaultMode::Turtle) {
        bug_windows::app::report_error(&error);
        std::process::exit(1);
    }
}
