#[cfg(not(windows))]
compile_error!("cockroach_overlay is a Windows-only desktop application");

#[cfg(windows)]
fn main() {
    if let Err(error) = bug_windows::app::run(bug_windows::cli::DefaultMode::Single) {
        bug_windows::app::report_error(&error);
        std::process::exit(1);
    }
}
