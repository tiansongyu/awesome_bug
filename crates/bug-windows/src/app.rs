//! Windows application host shared by the single-pet and swarm executables.
//!
//! This is deliberately an orchestration layer: species policy stays in Lua,
//! collision geometry stays in `bug-runtime`, and Win32 details stay in
//! `platform`.  The two executables differ only in their default instance
//! count.

use std::env;
use std::error::Error;
use std::ffi::OsStr;
use std::fmt::{self, Display, Formatter};
use std::fs::{self, File};
use std::io::{BufWriter, Write};
use std::path::PathBuf;
use std::time::Duration;

use bug_runtime::contract::{BaitInput, CursorInput};
use bug_runtime::lua::LuaHost;
use bug_runtime::math::Vec2;
use sdl2::event::Event;
use sdl2::keyboard::Keycode;
use windows::Win32::UI::WindowsAndMessaging::{MB_ICONERROR, MB_OK, MessageBoxW};
use windows::core::PCWSTR;

use crate::cli::{DefaultMode, Options, parse, usage};
use crate::platform::desktop_icons::DesktopIconTracker;
use crate::platform::dpi::{
    BodySizePolicy, DisplayGeometry, PixelRect, enable_per_monitor_v2, query_display_geometry,
};
use crate::platform::interaction::{InteractionController, cursor_position};
use crate::platform::layered_window::{LayeredWindow, RendererResources};
use crate::render::{BAIT_OVERLAY_SIZE, RenderSession, decode_png_path, render_bait};
use crate::resource::discover;
use crate::spawn::{
    SpawnGeometry, default_master_seed, find_safe_bait_position, make_spawn_plan_avoiding_obstacles,
};
use crate::trace::{TraceRow, TraceWriter};
use crate::world::{RuntimeWorld, WorldFrameInput};

const TARGET_FRAME_SECONDS: f64 = 1.0 / 60.0;
const MAXIMUM_FRAME_SECONDS: f32 = 0.05;
const MAXIMUM_CURSOR_SPEED: f32 = 6_000.0;
const GEOMETRY_PROBE_INTERVAL: Duration = Duration::from_secs(1);
const APPLICATION_TITLE: &str = "Scriptable Bug Overlay";

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct AppError {
    message: String,
}

impl AppError {
    fn new(message: impl Into<String>) -> Self {
        Self {
            message: message.into(),
        }
    }

    fn operation(operation: &str, error: impl Display) -> Self {
        Self::new(format!("{operation}: {error}"))
    }
}

impl Display for AppError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl Error for AppError {}

#[derive(Clone, Copy, Debug, PartialEq)]
enum GenerationOutcome {
    Exit,
    Rebuild(DisplayGeometry),
}

#[derive(Clone, Copy, Debug, Default)]
struct CursorHistory {
    previous: Option<Vec2>,
}

impl CursorHistory {
    fn sample(&mut self, dt: f32) -> CursorInput {
        let Ok(position) = cursor_position() else {
            self.previous = None;
            return CursorInput::default();
        };
        let velocity = self.previous.map_or(Vec2::ZERO, |previous| {
            if dt <= f32::EPSILON {
                return Vec2::ZERO;
            }
            limit_vector((position - previous) * (1.0 / dt), MAXIMUM_CURSOR_SPEED)
        });
        self.previous = Some(position);
        CursorInput {
            valid: position.is_finite() && velocity.is_finite(),
            position,
            velocity,
        }
    }
}

#[derive(Debug, Default)]
struct SessionLog {
    writer: Option<BufWriter<File>>,
}

impl SessionLog {
    fn open() -> Self {
        let Some(local_app_data) = env::var_os("LOCALAPPDATA") else {
            return Self::default();
        };
        let directory = PathBuf::from(local_app_data)
            .join("ScriptableBugOverlay")
            .join("logs");
        if fs::create_dir_all(&directory).is_err() {
            return Self::default();
        }
        let writer = File::create(directory.join("latest.log"))
            .ok()
            .map(BufWriter::new);
        Self { writer }
    }

    fn line(&mut self, message: impl Display) {
        if let Some(writer) = &mut self.writer {
            let _ = writeln!(writer, "{message}");
            let _ = writer.flush();
        }
    }
}

/// Runs one configured Windows overlay application.
///
/// Startup validation completes before the first overlay is shown.  A display
/// topology change rebuilds only the native windows and renderer resources;
/// Lua controllers, RNG streams and motion state remain live.
pub fn run(mode: DefaultMode) -> Result<(), AppError> {
    let options =
        parse(env::args_os(), mode).map_err(|error| AppError::operation("command line", error))?;
    let executable =
        env::current_exe().map_err(|error| AppError::operation("locate executable", error))?;
    let executable_name = executable
        .file_name()
        .and_then(OsStr::to_str)
        .unwrap_or("cockroach_overlay.exe");
    if options.show_help {
        println!("{}", usage(executable_name, mode));
        return Ok(());
    }

    let mut log = SessionLog::open();
    log.line(format_args!(
        "starting {} with {} instance(s)",
        executable.display(),
        options.count
    ));

    enable_per_monitor_v2()
        .map_err(|error| AppError::operation("enable per-monitor DPI awareness", error))?;
    let sdl = sdl2::init().map_err(|error| AppError::operation("initialize SDL", error))?;
    let video = sdl
        .video()
        .map_err(|error| AppError::operation("initialize SDL video", error))?;
    let timer = sdl
        .timer()
        .map_err(|error| AppError::operation("initialize SDL timer", error))?;
    let mut events = sdl
        .event_pump()
        .map_err(|error| AppError::operation("create SDL event pump", error))?;
    let _ = sdl2::hint::set("SDL_RENDER_SCALE_QUALITY", "2");

    let resources =
        discover(&executable, &options).map_err(|error| AppError::operation("resources", error))?;
    let host = LuaHost::new(&resources.fsm_path)
        .map_err(|error| AppError::operation("load Lua FSM", error))?;
    let species = host
        .load_species(&resources.species_root)
        .map_err(|error| AppError::operation("load species manifest", error))?;
    let behavior = host
        .load_behavior(&species)
        .map_err(|error| AppError::operation("load species behavior", error))?;
    let atlas_path = resources
        .asset_override
        .clone()
        .unwrap_or_else(|| species.atlas.file.clone());
    let atlas = decode_png_path(&atlas_path)
        .map_err(|error| AppError::operation("load sprite atlas", error))?;

    let mut geometry = read_display_geometry(&video, &options, species.body.default_length)?;
    let world_rect = geometry.work_area.to_runtime_rect();
    let mut desktop_icons = DesktopIconTracker::new();
    desktop_icons.preload();
    let master_seed = options
        .seed
        .unwrap_or_else(|| default_master_seed(timer.performance_counter()));
    let spawn_plan = make_spawn_plan_avoiding_obstacles(
        world_rect,
        options.count,
        master_seed,
        SpawnGeometry {
            base_body_length: geometry.body_length,
            collider_half_width: species.body.collider_half_width,
            collider_half_length: species.body.collider_half_length,
        },
        desktop_icons.cached_icons(),
    )
    .map_err(|error| AppError::operation("create icon-safe spawn plan", error))?;
    let mut world = RuntimeWorld::new(
        host,
        behavior,
        species,
        spawn_plan,
        world_rect,
        geometry.body_length,
        options.speed_multiplier,
    )
    .map_err(|error| AppError::operation("create runtime world", error))?;
    world
        .ensure_atlas_dimensions(atlas.width() as i32, atlas.height() as i32)
        .map_err(|error| AppError::operation("validate sprite atlas", error))?;
    log.line(format_args!(
        "display {}: {}x{} at ({}, {}), work area {}x{} at ({}, {}), body {}, seed {master_seed}",
        options.display,
        geometry.display_bounds.width,
        geometry.display_bounds.height,
        geometry.display_bounds.x,
        geometry.display_bounds.y,
        geometry.work_area.width,
        geometry.work_area.height,
        geometry.work_area.x,
        geometry.work_area.y,
        geometry.body_length,
    ));
    log.line(format_args!(
        "species {}, atlas {}",
        world.species().id,
        atlas_path.display()
    ));

    let mut trace = options
        .trace
        .as_deref()
        .map(TraceWriter::create)
        .transpose()
        .map_err(|error| AppError::operation("create frame trace", error))?;
    let mut interaction = InteractionController::new(world.features().bait, world_rect);
    let mut cursor_history = CursorHistory::default();
    let mut previous_counter = timer.performance_counter();
    let counter_frequency = timer.performance_frequency() as f64;
    if counter_frequency <= 0.0 {
        return Err(AppError::new("SDL performance counter has zero frequency"));
    }
    let mut simulation_clock = 0.0_f64;
    let mut frame_index = 0_u64;

    loop {
        let outcome = run_overlay_generation(
            &options,
            &video,
            &timer,
            &mut events,
            &atlas,
            &mut geometry,
            &mut world,
            &mut desktop_icons,
            &mut interaction,
            &mut cursor_history,
            &mut previous_counter,
            counter_frequency,
            &mut simulation_clock,
            &mut frame_index,
            trace.as_mut(),
            &mut log,
        )?;
        match outcome {
            GenerationOutcome::Exit => break,
            GenerationOutcome::Rebuild(new_geometry) => {
                geometry = new_geometry;
                let new_world = geometry.work_area.to_runtime_rect();
                world
                    .reconfigure(new_world, geometry.body_length)
                    .map_err(|error| AppError::operation("reconfigure runtime world", error))?;
                interaction.set_work_area(new_world);
                desktop_icons.invalidate();
                log.line(format_args!(
                    "display reconfigured: {}x{} work area at ({}, {}), body {}",
                    geometry.work_area.width,
                    geometry.work_area.height,
                    geometry.work_area.x,
                    geometry.work_area.y,
                    geometry.body_length
                ));
            }
        }
    }

    if let Some(trace) = &mut trace {
        trace
            .flush()
            .map_err(|error| AppError::operation("flush frame trace", error))?;
    }
    log.line("clean shutdown");
    Ok(())
}

#[allow(clippy::too_many_arguments)]
fn run_overlay_generation(
    options: &Options,
    video: &sdl2::VideoSubsystem,
    timer: &sdl2::TimerSubsystem,
    events: &mut sdl2::EventPump,
    atlas: &crate::render::RgbaImage,
    geometry: &mut DisplayGeometry,
    world: &mut RuntimeWorld,
    desktop_icons: &mut DesktopIconTracker,
    interaction: &mut InteractionController,
    cursor_history: &mut CursorHistory,
    previous_counter: &mut u64,
    counter_frequency: f64,
    simulation_clock: &mut f64,
    frame_index: &mut u64,
    mut trace: Option<&mut TraceWriter>,
    log: &mut SessionLog,
) -> Result<GenerationOutcome, AppError> {
    let overlay_sizes = world.overlay_sizes();
    let mut windows = create_bug_windows(video, &overlay_sizes, options.click_through)?;
    let renderer_resources: Vec<RendererResources> = windows
        .iter()
        .map(LayeredWindow::renderer_resources)
        .collect();
    let mut renderer = RenderSession::new(&renderer_resources, atlas)
        .map_err(|error| AppError::operation("create renderer session", error))?;
    if renderer.renderer_count() != world.instance_count() {
        return Err(AppError::new(
            "renderer count does not match runtime instance count",
        ));
    }
    let mut bait_window = world
        .features()
        .bait
        .then(|| {
            LayeredWindow::new(
                video,
                "Bug Food",
                BAIT_OVERLAY_SIZE,
                BAIT_OVERLAY_SIZE,
                true,
            )
        })
        .transpose()
        .map_err(|error| AppError::operation("create food overlay", error))?;
    let mut next_geometry_probe = timer.performance_counter();
    let geometry_probe_ticks =
        duration_to_counter_ticks(GEOMETRY_PROBE_INTERVAL, counter_frequency);
    let mut overlap_suppressed = vec![false; windows.len()];

    loop {
        let mut quit_requested = false;
        let mut geometry_refresh_requested = false;
        for event in events.poll_iter() {
            match event {
                Event::Quit { .. } | Event::AppTerminating { .. } => quit_requested = true,
                Event::KeyDown {
                    keycode: Some(Keycode::ESCAPE | Keycode::Q),
                    repeat: false,
                    ..
                } => quit_requested = true,
                Event::Display { .. } => geometry_refresh_requested = true,
                _ => {}
            }
        }

        let now = timer.performance_counter();
        if now.wrapping_sub(next_geometry_probe) >= geometry_probe_ticks {
            geometry_refresh_requested = true;
            next_geometry_probe = now;
        }
        if geometry_refresh_requested {
            let observed =
                read_display_geometry(video, options, world.species().body.default_length)?;
            if observed != *geometry {
                return Ok(GenerationOutcome::Rebuild(observed));
            }
        }

        let dt = frame_delta(
            now,
            *previous_counter,
            counter_frequency,
            options.maximum_frames.is_some(),
        );
        *previous_counter = now;
        *simulation_clock += f64::from(dt);

        let cursor = cursor_history.sample(dt);
        let cursor_position = if cursor.valid {
            cursor.position
        } else {
            Vec2::new(
                geometry.work_area.x as f32 - 10_000.0,
                geometry.work_area.y as f32 - 10_000.0,
            )
        };
        let interaction_events = interaction.poll(cursor_position);
        quit_requested |= interaction_events.quit_requested;
        desktop_icons.update(cursor_position, cursor.valid);

        if let Some(requested) = interaction_events.bait_placement {
            interaction.clear_bait();
            let bait_obstacles = if desktop_icons.obstacles().is_empty() {
                desktop_icons.cached_icons()
            } else {
                desktop_icons.obstacles()
            };
            if let Some(safe) = find_safe_bait_position(
                requested,
                geometry.work_area.to_runtime_rect(),
                world.primary_body_length(),
                bait_obstacles,
            ) {
                interaction.place_bait(safe);
            }
        }

        let bait = interaction
            .bait_position()
            .map_or_else(BaitInput::default, |position| BaitInput {
                active: true,
                position,
            });
        let output = world
            .step(WorldFrameInput {
                dt,
                clock: *simulation_clock,
                cursor,
                bait,
                request_corner_rest: false,
                obstacles: desktop_icons.obstacles(),
            })
            .map_err(|error| AppError::operation("advance runtime world", error))?;

        if output.instances.len() != windows.len() {
            return Err(AppError::new(
                "runtime output count does not match overlay count",
            ));
        }
        for (index, instance) in output.instances.iter().enumerate() {
            if let Some(diagnostic) = &instance.quarantine_diagnostic {
                log.line(format_args!(
                    "instance {} quarantined: {diagnostic}",
                    instance.instance_id
                ));
            }
            renderer
                .render(&mut windows[index], &instance.rig)
                .map_err(|error| AppError::operation("render bug overlay", error))?;
            if instance.overlaps_static {
                if !overlap_suppressed[index] {
                    log.line(format_args!(
                        "instance {} hidden while bounded static-overlap separation runs",
                        instance.instance_id
                    ));
                }
                overlap_suppressed[index] = true;
                windows[index].hide();
            } else {
                if overlap_suppressed[index] {
                    log.line(format_args!(
                        "instance {} cleared static overlap",
                        instance.instance_id
                    ));
                }
                overlap_suppressed[index] = false;
                let (width, height) = windows[index].dimensions();
                let x = rounded_screen_coordinate(instance.body.position.x - width as f32 * 0.5)?;
                let y = rounded_screen_coordinate(instance.body.position.y - height as f32 * 0.5)?;
                windows[index]
                    .present_at(x, y)
                    .map_err(|error| AppError::operation("present bug overlay", error))?;
            }

            if let Some(trace) = trace.as_deref_mut() {
                trace
                    .write_row(TraceRow {
                        frame_index: *frame_index,
                        instance_id: instance.instance_id,
                        decision: &instance.decision,
                        body: instance.body,
                        feedback: instance.feedback,
                        rng_draws: instance.rng_draws,
                        quarantined: instance.quarantined,
                    })
                    .map_err(|error| AppError::operation("write frame trace", error))?;
            }
        }

        if output.consume_bait {
            interaction.clear_bait();
        }
        update_bait_window(bait_window.as_mut(), interaction.bait_position(), &windows)?;

        *frame_index = frame_index.saturating_add(1);
        if quit_requested
            || options
                .maximum_frames
                .is_some_and(|maximum| *frame_index >= maximum)
        {
            return Ok(GenerationOutcome::Exit);
        }
        pace_frame(timer, now, counter_frequency);
    }
}

fn create_bug_windows(
    video: &sdl2::VideoSubsystem,
    sizes: &[u32],
    click_through: bool,
) -> Result<Vec<LayeredWindow>, AppError> {
    let mut windows = Vec::with_capacity(sizes.len());
    for (index, size) in sizes.iter().copied().enumerate() {
        let title = format!("Bug Overlay {}", index + 1);
        windows.push(
            LayeredWindow::new(video, &title, size, size, click_through)
                .map_err(|error| AppError::operation("create bug overlay", error))?,
        );
    }
    Ok(windows)
}

fn update_bait_window(
    window: Option<&mut LayeredWindow>,
    position: Option<Vec2>,
    bug_windows: &[LayeredWindow],
) -> Result<(), AppError> {
    let Some(window) = window else {
        return Ok(());
    };
    let Some(position) = position else {
        window.hide();
        return Ok(());
    };
    let Some(bug_window) = bug_windows.first() else {
        return Err(AppError::new("food overlay has no owning bug overlay"));
    };
    render_bait(window).map_err(|error| AppError::operation("render food overlay", error))?;
    let half = BAIT_OVERLAY_SIZE as f32 * 0.5;
    window
        .present_at(
            rounded_screen_coordinate(position.x - half)?,
            rounded_screen_coordinate(position.y - half)?,
        )
        .map_err(|error| AppError::operation("present food overlay", error))?;
    window
        .place_behind(bug_window)
        .map_err(|error| AppError::operation("place food below bug", error))
}

fn read_display_geometry(
    video: &sdl2::VideoSubsystem,
    options: &Options,
    reference_body_length: f32,
) -> Result<DisplayGeometry, AppError> {
    let display_count = video
        .num_video_displays()
        .map_err(|error| AppError::operation("enumerate displays", error))?;
    let display_index = i32::try_from(options.display)
        .map_err(|_| AppError::new("display index does not fit SDL's coordinate space"))?;
    if display_index < 0 || display_index >= display_count {
        return Err(AppError::new(format!(
            "display {} does not exist ({} display(s) available)",
            options.display, display_count
        )));
    }
    let bounds = video
        .display_bounds(display_index)
        .map_err(|error| AppError::operation("read display bounds", error))?;
    let width = i32::try_from(bounds.width())
        .map_err(|_| AppError::new("display width exceeds Windows coordinates"))?;
    let height = i32::try_from(bounds.height())
        .map_err(|_| AppError::new("display height exceeds Windows coordinates"))?;
    let size_policy = options.body_size.map_or(
        BodySizePolicy::Automatic {
            reference_length: reference_body_length,
        },
        BodySizePolicy::Fixed,
    );
    Ok(query_display_geometry(
        PixelRect::new(bounds.x(), bounds.y(), width, height),
        size_policy,
    ))
}

fn frame_delta(now: u64, previous: u64, frequency: f64, fixed_step: bool) -> f32 {
    if fixed_step {
        return TARGET_FRAME_SECONDS as f32;
    }
    ((now.wrapping_sub(previous) as f64 / frequency) as f32).clamp(0.0, MAXIMUM_FRAME_SECONDS)
}

fn pace_frame(timer: &sdl2::TimerSubsystem, frame_start: u64, frequency: f64) {
    let elapsed = timer.performance_counter().wrapping_sub(frame_start) as f64 / frequency;
    if elapsed < TARGET_FRAME_SECONDS {
        let remaining_ms = ((TARGET_FRAME_SECONDS - elapsed) * 1_000.0).floor() as u32;
        if remaining_ms > 0 {
            timer.delay(remaining_ms);
        }
    }
}

fn duration_to_counter_ticks(duration: Duration, frequency: f64) -> u64 {
    (duration.as_secs_f64() * frequency).round().max(1.0) as u64
}

fn rounded_screen_coordinate(value: f32) -> Result<i32, AppError> {
    if !value.is_finite() || value < i32::MIN as f32 || value > i32::MAX as f32 {
        return Err(AppError::new(
            "overlay position exceeds Windows coordinates",
        ));
    }
    Ok(value.round() as i32)
}

fn limit_vector(value: Vec2, maximum_length: f32) -> Vec2 {
    let length = value.length();
    if length.is_finite() && length > maximum_length {
        value * (maximum_length / length)
    } else {
        value
    }
}

/// Displays a startup/runtime error without relying on a console subsystem.
pub fn report_error(error: &AppError) {
    show_message(&format!("{error}"), MB_OK | MB_ICONERROR);
}

fn show_message(message: &str, style: windows::Win32::UI::WindowsAndMessaging::MESSAGEBOX_STYLE) {
    let title = wide_string(APPLICATION_TITLE);
    let message = wide_string(message);
    // SAFETY: Both UTF-16 buffers are NUL terminated and remain live for the
    // duration of this synchronous call.  No HWND ownership is involved.
    unsafe {
        let _ = MessageBoxW(
            None,
            PCWSTR::from_raw(message.as_ptr()),
            PCWSTR::from_raw(title.as_ptr()),
            style,
        );
    }
}

fn wide_string(value: &str) -> Vec<u16> {
    value
        .encode_utf16()
        .map(|unit| if unit == 0 { u16::from(b'?') } else { unit })
        .chain(std::iter::once(0))
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn fixed_frame_mode_and_cursor_limit_are_bounded() {
        assert_eq!(frame_delta(90, 10, 1_000.0, true), 1.0 / 60.0);
        assert_eq!(frame_delta(90, 10, 1_000.0, false), 0.05);
        let limited = limit_vector(Vec2::new(6_000.0, 8_000.0), 5_000.0);
        assert!((limited.length() - 5_000.0).abs() < 0.01);
    }

    #[test]
    fn screen_coordinate_rejects_non_finite_values() {
        assert!(rounded_screen_coordinate(f32::NAN).is_err());
        assert_eq!(rounded_screen_coordinate(-42.6).expect("coordinate"), -43);
    }
}
