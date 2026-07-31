//! Explorer desktop-icon discovery and collision snapshots.
//!
//! Explorer owns the `SysListView32` item rectangles.  `LVM_GETITEMRECT`
//! therefore writes through a small buffer allocated in Explorer's process.
//! All ownership of that process handle and allocation is contained here.

use std::ffi::c_void;
use std::mem::size_of;
use std::ptr::NonNull;
use std::time::{Duration, Instant};

use bug_runtime::contract::ScreenObstacle;
use bug_runtime::math::Vec2;
use windows::Win32::Foundation::{
    ERROR_SUCCESS, GetLastError, HANDLE, HWND, LPARAM, POINT, RECT, SetLastError, WPARAM,
};
use windows::Win32::Graphics::Gdi::MapWindowPoints;
use windows::Win32::System::Diagnostics::Debug::{ReadProcessMemory, WriteProcessMemory};
use windows::Win32::System::Memory::{
    MEM_COMMIT, MEM_RELEASE, MEM_RESERVE, PAGE_READWRITE, VirtualAllocEx, VirtualFreeEx,
};
use windows::Win32::System::Threading::{
    GetCurrentProcessId, OpenProcess, PROCESS_QUERY_LIMITED_INFORMATION, PROCESS_VM_OPERATION,
    PROCESS_VM_READ, PROCESS_VM_WRITE,
};
use windows::Win32::UI::Controls::{LVIR_BOUNDS, LVM_GETITEMCOUNT, LVM_GETITEMRECT};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, FindWindowExW, FindWindowW, GA_ROOT, GetAncestor, GetClassNameW,
    GetForegroundWindow, GetShellWindow, GetWindowThreadProcessId, IsWindow, IsWindowVisible,
    SMTO_ABORTIFHUNG, SMTO_BLOCK, SendMessageTimeoutW,
};
use windows::core::{BOOL, Owned, PCWSTR, w};

use super::interaction::{
    IconDragTracker, PointerSample, compose_icon_obstacles, left_mouse_button_down,
};

pub const MAX_DESKTOP_ICONS: usize = 2048;
pub const ICON_PADDING: f32 = 9.0;
pub const REFRESH_INTERVAL: Duration = Duration::from_millis(120);
const EXPLORER_MESSAGE_TIMEOUT_MS: u32 = 100;
const CONNECTION_OPEN_RETRY_DELAY: Duration = Duration::from_millis(500);
const AMBIGUOUS_RETRY_MAX_DELAY: Duration = Duration::from_secs(30);

/// An atomic, cached view of Explorer's desktop icons.
///
/// Failed refreshes never replace `icon_cache` or `obstacles`; a temporarily
/// hung or restarting Explorer cannot create a collision-free frame.
pub struct DesktopIconTracker {
    list_view: Option<HWND>,
    connection: Option<ExplorerConnection>,
    retry: ExplorerRetryState,
    icon_cache: Vec<ScreenObstacle>,
    obstacles: Vec<ScreenObstacle>,
    last_refresh_attempt: Option<Instant>,
    empty_snapshot_streak: u8,
    was_desktop_active: bool,
    drag: IconDragTracker,
}

impl Default for DesktopIconTracker {
    fn default() -> Self {
        Self {
            list_view: None,
            connection: None,
            retry: ExplorerRetryState::default(),
            icon_cache: Vec::new(),
            obstacles: Vec::new(),
            last_refresh_attempt: None,
            empty_snapshot_streak: 0,
            was_desktop_active: false,
            drag: IconDragTracker::new(),
        }
    }
}

impl DesktopIconTracker {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Reads one complete Explorer snapshot without publishing collisions.
    ///
    /// Startup uses this before choosing spawn points even when another
    /// application is foreground. Runtime publication still follows the
    /// desktop-foreground rule in `update`.
    pub fn preload(&mut self) {
        let now = Instant::now();
        if self.connect_to_explorer()
            && let Some(snapshot) = self.read_icon_snapshot_safely(now)
        {
            self.icon_cache = snapshot;
            self.last_refresh_attempt = Some(now);
            self.empty_snapshot_streak = 0;
            self.drag.acknowledge_snapshot();
        }
    }

    #[must_use]
    pub fn cached_icons(&self) -> &[ScreenObstacle] {
        &self.icon_cache
    }

    /// Refreshes and publishes the current desktop obstacles.
    ///
    /// When another application is foreground, the published slice is empty
    /// but the last complete Explorer cache is retained for an immediate,
    /// gap-free return to the desktop.
    pub fn update(&mut self, cursor: Vec2, allow_drag: bool) {
        let now = Instant::now();
        let connected = self.connect_to_explorer();
        if !self.desktop_is_foreground() {
            self.obstacles.clear();
            self.was_desktop_active = false;
            self.drag.reset();
            return;
        }

        let refresh_due = (!self.was_desktop_active && self.icon_cache.is_empty())
            || self
                .last_refresh_attempt
                .is_none_or(|attempt| now.saturating_duration_since(attempt) >= REFRESH_INTERVAL);
        if refresh_due {
            // Throttle retries as well as successful reads.  A hung Explorer
            // must not stall every animation frame for the 100 ms timeout.
            self.last_refresh_attempt = Some(now);
            if connected {
                match self.read_icon_snapshot_safely(now) {
                    Some(snapshot) => {
                        if snapshot.is_empty() && !self.icon_cache.is_empty() {
                            self.empty_snapshot_streak =
                                self.empty_snapshot_streak.saturating_add(1);
                            if self.empty_snapshot_streak >= 2 {
                                self.icon_cache = snapshot;
                                self.drag.acknowledge_snapshot();
                            }
                        } else {
                            self.empty_snapshot_streak = 0;
                            self.icon_cache = snapshot;
                            self.drag.acknowledge_snapshot();
                        }
                    }
                    None => {
                        // Preserve the last complete snapshot while a safe
                        // reconnect or timeout backoff is pending.
                    }
                }
            }
        }

        let drag_update = self.drag.update(
            PointerSample {
                position: cursor,
                left_button_down: allow_drag && left_mouse_button_down(),
            },
            &self.icon_cache,
            allow_drag,
        );
        if drag_update.refresh_requested {
            self.last_refresh_attempt = None;
        }
        self.obstacles = compose_icon_obstacles(&self.icon_cache, drag_update.active);
        self.was_desktop_active = true;
    }

    #[must_use]
    pub fn obstacles(&self) -> &[ScreenObstacle] {
        &self.obstacles
    }

    #[must_use]
    pub fn cached_icon_count(&self) -> usize {
        self.icon_cache.len()
    }

    /// Requests a fresh monitor/Explorer snapshot at the next update boundary.
    pub fn invalidate(&mut self) {
        self.last_refresh_attempt = None;
    }

    fn connect_to_explorer(&mut self) -> bool {
        let now = Instant::now();
        let Some(discovered) = find_desktop_list_view() else {
            self.list_view = None;
            self.connection = None;
            return false;
        };
        // SAFETY: `discovered` was returned by FindWindow/EnumWindows.  IsWindow
        // performs validation without transferring ownership.
        if !unsafe { IsWindow(Some(discovered)).as_bool() } {
            self.list_view = None;
            self.connection = None;
            return false;
        }

        let mut process_id = 0_u32;
        // SAFETY: process_id is valid writable storage and `discovered` is
        // validated immediately above.
        unsafe {
            GetWindowThreadProcessId(discovered, Some(&mut process_id));
        }
        if process_id == 0 {
            self.list_view = Some(discovered);
            self.connection = None;
            return false;
        }

        let connection_matches = self.list_view == Some(discovered)
            && self
                .connection
                .as_ref()
                .is_some_and(|connection| connection.process_id == process_id);
        if connection_matches {
            return true;
        }

        let identity = ExplorerIdentity {
            list_view: discovered,
            process_id,
        };
        if !self.retry.connection_attempt_allowed(identity, now) {
            // A timed-out Explorer WndProc may still hold an abandoned remote
            // RECT, or the last OpenProcess/VirtualAllocEx attempt failed.
            // Preserve the cache and wait before allocating another buffer.
            self.list_view = Some(discovered);
            self.connection = None;
            return false;
        }

        // Dropping the old connection frees its remote rectangle before
        // closing the old Explorer process handle.
        self.connection = None;
        self.list_view = Some(discovered);
        self.empty_snapshot_streak = 0;
        match ExplorerConnection::open(process_id) {
            Ok(connection) => {
                self.retry.record_connection_opened(identity);
                self.connection = Some(connection);
                true
            }
            Err(_) => {
                self.retry.record_connection_failure(identity, now);
                false
            }
        }
    }

    fn desktop_is_foreground(&self) -> bool {
        // SAFETY: These calls only query window metadata.  All returned HWNDs
        // are treated as borrowed identifiers and are never closed.
        unsafe {
            let foreground = GetForegroundWindow();
            if foreground.is_invalid() {
                return false;
            }

            if let Some(list_view) = self.list_view.filter(|window| {
                IsWindow(Some(*window)).as_bool() && IsWindowVisible(*window).as_bool()
            }) {
                let desktop_root = GetAncestor(list_view, GA_ROOT);
                let foreground_root = GetAncestor(foreground, GA_ROOT);
                if foreground == list_view
                    || (!desktop_root.is_invalid() && foreground_root == desktop_root)
                {
                    return true;
                }
            }

            let shell = GetShellWindow();
            if !shell.is_invalid() && foreground == shell {
                return true;
            }

            let mut foreground_process_id = 0_u32;
            GetWindowThreadProcessId(foreground, Some(&mut foreground_process_id));
            if foreground_process_id == GetCurrentProcessId() {
                return true;
            }

            let mut class_name = [0_u16; 64];
            let length = GetClassNameW(foreground, &mut class_name);
            length > 0 && is_desktop_class(&class_name[..length as usize])
        }
    }

    fn read_icon_snapshot_safely(&mut self, now: Instant) -> Option<Vec<ScreenObstacle>> {
        let list_view = self.list_view?;
        let snapshot = {
            let connection = self.connection.as_ref()?;
            read_icon_snapshot(list_view, connection)
        };
        match snapshot {
            Ok(snapshot) => {
                let identity = ExplorerIdentity {
                    list_view,
                    process_id: self.connection.as_ref()?.process_id,
                };
                self.retry.record_snapshot_success(identity);
                Some(snapshot)
            }
            Err(SnapshotFailure::Retryable) => {
                // No remote write can still be running. Dropping the
                // connection safely releases its RECT. Use the short
                // connection backoff as well as the normal refresh throttle.
                let connection = self.connection.take()?;
                self.retry.record_connection_failure(
                    ExplorerIdentity {
                        list_view,
                        process_id: connection.process_id,
                    },
                    now,
                );
                None
            }
            Err(SnapshotFailure::RemoteRectangleInFlight) => {
                self.retire_connection(now);
                None
            }
        }
    }

    fn retire_connection(&mut self, now: Instant) {
        let Some(mut connection) = self.connection.take() else {
            return;
        };
        let Some(list_view) = self.list_view else {
            connection.abandon_remote_rectangle();
            return;
        };
        self.retry.record_ambiguous_timeout(
            ExplorerIdentity {
                list_view,
                process_id: connection.process_id,
            },
            now,
        );
        connection.abandon_remote_rectangle();
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
struct ExplorerIdentity {
    list_view: HWND,
    process_id: u32,
}

/// Pure retry policy for one Explorer `(HWND, PID)` generation.
///
/// The two deadlines deliberately serve different failure classes:
/// connection failures receive a short fixed delay, while a possibly in-flight
/// remote write receives an exponentially increasing delay. A new Explorer
/// generation starts immediately, and only a complete snapshot clears an
/// ambiguous-timeout streak for the current generation.
#[derive(Default)]
struct ExplorerRetryState {
    identity: Option<ExplorerIdentity>,
    connection_retry_not_before: Option<Instant>,
    ambiguous_retry_not_before: Option<Instant>,
    ambiguous_timeout_streak: u8,
}

impl ExplorerRetryState {
    fn connection_attempt_allowed(&mut self, identity: ExplorerIdentity, now: Instant) -> bool {
        self.observe_identity(identity);
        self.connection_retry_not_before
            .is_none_or(|retry_at| now >= retry_at)
            && self
                .ambiguous_retry_not_before
                .is_none_or(|retry_at| now >= retry_at)
    }

    fn record_connection_opened(&mut self, identity: ExplorerIdentity) {
        self.observe_identity(identity);
        self.connection_retry_not_before = None;
    }

    fn record_connection_failure(&mut self, identity: ExplorerIdentity, now: Instant) {
        self.observe_identity(identity);
        self.connection_retry_not_before = Some(now + CONNECTION_OPEN_RETRY_DELAY);
    }

    fn record_ambiguous_timeout(&mut self, identity: ExplorerIdentity, now: Instant) -> Duration {
        self.observe_identity(identity);
        self.connection_retry_not_before = None;
        self.ambiguous_timeout_streak = self.ambiguous_timeout_streak.saturating_add(1);
        let delay = ambiguous_retry_delay(self.ambiguous_timeout_streak);
        self.ambiguous_retry_not_before = Some(now + delay);
        delay
    }

    fn record_snapshot_success(&mut self, identity: ExplorerIdentity) {
        self.observe_identity(identity);
        self.connection_retry_not_before = None;
        self.ambiguous_retry_not_before = None;
        self.ambiguous_timeout_streak = 0;
    }

    fn observe_identity(&mut self, identity: ExplorerIdentity) {
        if self.identity == Some(identity) {
            return;
        }
        self.identity = Some(identity);
        self.connection_retry_not_before = None;
        self.ambiguous_retry_not_before = None;
        self.ambiguous_timeout_streak = 0;
    }
}

#[derive(Clone, Copy, Debug, Eq, PartialEq)]
enum SnapshotFailure {
    /// No Explorer window procedure can still reference the remote RECT.
    Retryable,
    /// A timed-out `LVM_GETITEMRECT` may still write through the remote
    /// pointer. That exact allocation must never be reused or freed.
    RemoteRectangleInFlight,
}

/// Reads one all-or-nothing snapshot through a live Explorer connection.
fn read_icon_snapshot(
    list_view: HWND,
    connection: &ExplorerConnection,
) -> Result<Vec<ScreenObstacle>, SnapshotFailure> {
    let mut count_result = 0_usize;
    if !send_list_view_message(
        list_view,
        LVM_GETITEMCOUNT,
        WPARAM(0),
        LPARAM(0),
        &mut count_result,
    ) {
        return Err(SnapshotFailure::Retryable);
    }
    let count = count_result.min(MAX_DESKTOP_ICONS);
    let mut snapshot = Vec::with_capacity(count);

    for index in 0..count {
        // Abort immediately: retrying all 2048 bounded sends against a hung
        // Explorer would turn a 100 ms guard into a long freeze.
        let rectangle = read_item_rectangle(list_view, connection, index)?;
        let width = rectangle.right - rectangle.left;
        let height = rectangle.bottom - rectangle.top;
        if !(8..=360).contains(&width) || !(8..=300).contains(&height) {
            continue;
        }
        snapshot.push(ScreenObstacle {
            x: rectangle.left as f32 - ICON_PADDING,
            y: rectangle.top as f32 - ICON_PADDING,
            width: width as f32 + ICON_PADDING * 2.0,
            height: height as f32 + ICON_PADDING * 2.0,
            moving: false,
        });
    }

    Ok(snapshot)
}

struct ExplorerConnection {
    process: Owned<HANDLE>,
    remote_rectangle: Option<NonNull<c_void>>,
    process_id: u32,
}

impl ExplorerConnection {
    fn open(process_id: u32) -> windows::core::Result<Self> {
        let access = PROCESS_QUERY_LIMITED_INFORMATION
            | PROCESS_VM_OPERATION
            | PROCESS_VM_READ
            | PROCESS_VM_WRITE;
        // SAFETY: OpenProcess returns a newly owned HANDLE on success.
        let raw_process = unsafe { OpenProcess(access, false, process_id)? };
        // SAFETY: ownership of the successful OpenProcess handle is transferred
        // exactly once into Owned and is not closed elsewhere.
        let process = unsafe { Owned::new(raw_process) };
        // SAFETY: The owned process handle is valid.  The requested allocation
        // is one writable RECT and is released in Drop before the handle closes.
        let allocation = unsafe {
            VirtualAllocEx(
                *process,
                None,
                size_of::<RECT>(),
                MEM_COMMIT | MEM_RESERVE,
                PAGE_READWRITE,
            )
        };
        let Some(remote_rectangle) = NonNull::new(allocation) else {
            return Err(windows::core::Error::from_thread());
        };
        Ok(Self {
            process,
            remote_rectangle: Some(remote_rectangle),
            process_id,
        })
    }

    #[must_use]
    fn handle(&self) -> HANDLE {
        *self.process
    }

    fn remote_rectangle(&self) -> Option<NonNull<c_void>> {
        self.remote_rectangle
    }

    /// Forgets the cross-process allocation without freeing it.
    ///
    /// A timed-out `SendMessageTimeoutW` may already be executing inside
    /// Explorer and may still write this RECT. Explorer owns the address space,
    /// so abandoning 16 bytes is the only safe local action; Windows reclaims
    /// it when that Explorer process exits.
    fn abandon_remote_rectangle(&mut self) {
        self.remote_rectangle = None;
    }
}

impl Drop for ExplorerConnection {
    fn drop(&mut self) {
        let Some(remote_rectangle) = self.remote_rectangle else {
            return;
        };
        // SAFETY: remote_rectangle was allocated exactly once by VirtualAllocEx
        // in this same process.  Drop runs before the Owned<HANDLE> field drops.
        let _ = unsafe { VirtualFreeEx(*self.process, remote_rectangle.as_ptr(), 0, MEM_RELEASE) };
    }
}

#[must_use]
fn find_desktop_list_view() -> Option<HWND> {
    // SAFETY: Static UTF-16 strings are NUL terminated.  Returned HWND values
    // are borrowed and only queried.
    unsafe {
        if let Ok(program_manager) = FindWindowW(w!("Progman"), PCWSTR::null())
            && let Some(list_view) = list_view_in_host(program_manager)
        {
            return Some(list_view);
        }

        let mut result = None;
        let parameter = LPARAM((&raw mut result).cast::<c_void>() as isize);
        // EnumWindows reports failure when our callback deliberately returns
        // FALSE after finding the window, so the Result itself is not useful.
        let _ = EnumWindows(Some(find_worker_list_view), parameter);
        result
    }
}

#[must_use]
fn list_view_in_host(host: HWND) -> Option<HWND> {
    if host.is_invalid() {
        return None;
    }
    // SAFETY: host is a borrowed top-level HWND. Static strings are valid for
    // the duration of each synchronous call.
    unsafe {
        let shell_view =
            FindWindowExW(Some(host), None, w!("SHELLDLL_DefView"), PCWSTR::null()).ok()?;
        FindWindowExW(Some(shell_view), None, w!("SysListView32"), PCWSTR::null()).ok()
    }
}

unsafe extern "system" fn find_worker_list_view(window: HWND, parameter: LPARAM) -> BOOL {
    if let Some(list_view) = list_view_in_host(window) {
        let output = parameter.0 as *mut Option<HWND>;
        if !output.is_null() {
            // SAFETY: find_desktop_list_view passes a live stack pointer to
            // synchronous EnumWindows, and this callback writes it at most once.
            unsafe {
                output.write(Some(list_view));
            }
            return false.into();
        }
    }
    true.into()
}

#[must_use]
fn send_list_view_message(
    list_view: HWND,
    message: u32,
    wparam: WPARAM,
    lparam: LPARAM,
    result: &mut usize,
) -> bool {
    // SAFETY: The HWND belongs to Explorer.  SendMessageTimeoutW is synchronous,
    // aborts a hung target after 100 ms, and `result` remains writable for the
    // entire call.
    unsafe {
        SendMessageTimeoutW(
            list_view,
            message,
            wparam,
            lparam,
            SMTO_ABORTIFHUNG | SMTO_BLOCK,
            EXPLORER_MESSAGE_TIMEOUT_MS,
            Some(result),
        )
        .0 != 0
    }
}

fn read_item_rectangle(
    list_view: HWND,
    connection: &ExplorerConnection,
    index: usize,
) -> Result<RECT, SnapshotFailure> {
    let request = RECT {
        left: LVIR_BOUNDS as i32,
        ..RECT::default()
    };
    let remote_rectangle = connection
        .remote_rectangle()
        .ok_or(SnapshotFailure::Retryable)?;
    let mut transferred = 0_usize;
    // SAFETY: remote_rectangle points to one writable RECT in `handle`.
    // request is initialized local storage and both buffers are exactly one
    // RECT long.
    if unsafe {
        WriteProcessMemory(
            connection.handle(),
            remote_rectangle.as_ptr(),
            (&raw const request).cast::<c_void>(),
            size_of::<RECT>(),
            Some(&mut transferred),
        )
    }
    .is_err()
        || transferred != size_of::<RECT>()
    {
        return Err(SnapshotFailure::Retryable);
    }

    let mut message_result = 0_usize;
    if !send_list_view_message(
        list_view,
        LVM_GETITEMRECT,
        WPARAM(index),
        LPARAM(remote_rectangle.as_ptr() as isize),
        &mut message_result,
    ) {
        return Err(SnapshotFailure::RemoteRectangleInFlight);
    }
    if message_result == 0 {
        return Err(SnapshotFailure::Retryable);
    }

    let mut rectangle = RECT::default();
    transferred = 0;
    // SAFETY: remote_rectangle still points to one readable RECT in the same
    // process, and rectangle is valid writable local storage.
    if unsafe {
        ReadProcessMemory(
            connection.handle(),
            remote_rectangle.as_ptr(),
            (&raw mut rectangle).cast::<c_void>(),
            size_of::<RECT>(),
            Some(&mut transferred),
        )
    }
    .is_err()
        || transferred != size_of::<RECT>()
    {
        return Err(SnapshotFailure::Retryable);
    }

    let mut corners = [
        POINT {
            x: rectangle.left,
            y: rectangle.top,
        },
        POINT {
            x: rectangle.right,
            y: rectangle.bottom,
        },
    ];
    // SAFETY: corners contains two valid POINTs. list_view remains borrowed and
    // alive for this synchronous coordinate conversion. Clearing last-error is
    // required because a successful zero-offset MapWindowPoints also returns 0.
    unsafe {
        SetLastError(ERROR_SUCCESS);
        let mapped = MapWindowPoints(Some(list_view), None, &mut corners);
        if mapped == 0 && GetLastError() != ERROR_SUCCESS {
            return Err(SnapshotFailure::Retryable);
        }
    }

    Ok(RECT {
        left: corners[0].x,
        top: corners[0].y,
        right: corners[1].x,
        bottom: corners[1].y,
    })
}

#[must_use]
fn ambiguous_retry_delay(streak: u8) -> Duration {
    let exponent = streak.saturating_sub(1).min(5);
    Duration::from_secs((1_u64 << exponent).min(AMBIGUOUS_RETRY_MAX_DELAY.as_secs()))
}

#[must_use]
fn is_desktop_class(class_name: &[u16]) -> bool {
    [
        "Progman",
        "WorkerW",
        "Shell_TrayWnd",
        "Shell_SecondaryTrayWnd",
    ]
    .iter()
    .any(|candidate| class_name.iter().copied().eq(candidate.encode_utf16()))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn explorer(process_id: u32) -> ExplorerIdentity {
        ExplorerIdentity {
            list_view: HWND::default(),
            process_id,
        }
    }

    #[test]
    fn recognizes_only_desktop_shell_classes() {
        assert!(is_desktop_class(
            &"Shell_SecondaryTrayWnd".encode_utf16().collect::<Vec<_>>()
        ));
        assert!(is_desktop_class(
            &"WorkerW".encode_utf16().collect::<Vec<_>>()
        ));
        assert!(!is_desktop_class(
            &"CabinetWClass".encode_utf16().collect::<Vec<_>>()
        ));
    }

    #[test]
    fn ambiguous_timeout_backoff_is_exponential_and_bounded() {
        let seconds = (0_u8..=10)
            .map(|streak| ambiguous_retry_delay(streak).as_secs())
            .collect::<Vec<_>>();
        assert_eq!(seconds, vec![1, 1, 2, 4, 8, 16, 30, 30, 30, 30, 30]);
    }

    #[test]
    fn same_generation_waits_until_ambiguous_deadline() {
        let now = Instant::now();
        let identity = explorer(10);
        let mut retry = ExplorerRetryState::default();

        let delay = retry.record_ambiguous_timeout(identity, now);

        assert!(
            !retry.connection_attempt_allowed(identity, now + delay - Duration::from_millis(1))
        );
        assert!(retry.connection_attempt_allowed(identity, now + delay));
        assert_eq!(retry.ambiguous_timeout_streak, 1);
    }

    #[test]
    fn connection_open_failure_has_a_short_generation_scoped_backoff() {
        let now = Instant::now();
        let first_generation = explorer(10);
        let next_generation = explorer(11);
        let mut retry = ExplorerRetryState::default();

        retry.record_connection_failure(first_generation, now);

        assert!(!retry.connection_attempt_allowed(
            first_generation,
            now + CONNECTION_OPEN_RETRY_DELAY - Duration::from_millis(1)
        ));
        assert!(
            retry.connection_attempt_allowed(first_generation, now + CONNECTION_OPEN_RETRY_DELAY)
        );
        assert!(retry.connection_attempt_allowed(next_generation, now));
        assert_eq!(retry.identity, Some(next_generation));
        assert_eq!(retry.ambiguous_timeout_streak, 0);
    }

    #[test]
    fn new_explorer_generation_clears_an_old_ambiguous_timeout() {
        let now = Instant::now();
        let first_generation = explorer(20);
        let next_generation = explorer(21);
        let mut retry = ExplorerRetryState::default();
        retry.record_ambiguous_timeout(first_generation, now);

        assert!(retry.connection_attempt_allowed(next_generation, now));
        assert_eq!(retry.identity, Some(next_generation));
        assert_eq!(retry.ambiguous_timeout_streak, 0);
        assert!(retry.ambiguous_retry_not_before.is_none());
    }

    #[test]
    fn complete_snapshot_clears_retry_state() {
        let now = Instant::now();
        let identity = explorer(30);
        let mut retry = ExplorerRetryState::default();
        retry.record_ambiguous_timeout(identity, now);
        retry.record_ambiguous_timeout(identity, now + Duration::from_secs(1));
        retry.record_connection_failure(identity, now + Duration::from_secs(2));

        retry.record_snapshot_success(identity);

        assert_eq!(retry.ambiguous_timeout_streak, 0);
        assert!(retry.ambiguous_retry_not_before.is_none());
        assert!(retry.connection_retry_not_before.is_none());
        assert!(retry.connection_attempt_allowed(identity, now));
    }

    #[test]
    fn ordinary_failure_does_not_increment_ambiguous_streak() {
        let now = Instant::now();
        let identity = explorer(40);
        let mut retry = ExplorerRetryState::default();
        let ambiguous_deadline = now + retry.record_ambiguous_timeout(identity, now);

        retry.record_connection_failure(identity, now);

        assert_eq!(retry.ambiguous_timeout_streak, 1);
        assert_eq!(retry.ambiguous_retry_not_before, Some(ambiguous_deadline));
        assert_eq!(
            retry.connection_retry_not_before,
            Some(now + CONNECTION_OPEN_RETRY_DELAY)
        );
    }

    #[test]
    fn ambiguous_timeout_events_increment_the_exponential_streak() {
        let now = Instant::now();
        let identity = explorer(50);
        let mut retry = ExplorerRetryState::default();
        let delays = (0..8)
            .map(|offset| {
                retry
                    .record_ambiguous_timeout(identity, now + Duration::from_secs(offset))
                    .as_secs()
            })
            .collect::<Vec<_>>();

        assert_eq!(delays, vec![1, 2, 4, 8, 16, 30, 30, 30]);
        assert_eq!(retry.ambiguous_timeout_streak, 8);
    }
}
