//! SDL software surfaces presented through a non-activating Win32 layered window.
//!
//! SDL owns the event/window shell and the software renderer.  GDI owns only a
//! top-down 32-bit DIB used as the transport buffer for `UpdateLayeredWindow`.
//! No native handle escapes this module.

use std::error::Error;
use std::ffi::c_void;
use std::fmt::{self, Display, Formatter};
use std::mem::size_of;
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::ptr::NonNull;
use std::sync::atomic::{AtomicU64, Ordering};

use raw_window_handle::{HasWindowHandle, RawWindowHandle};
use sdl2::VideoSubsystem;
use sdl2::pixels::PixelFormatEnum;
use sdl2::render::{BlendMode, SurfaceCanvas, TextureCreator};
use sdl2::surface::{Surface, SurfaceContext};
use sdl2::video::Window;
use windows::Win32::Foundation::{COLORREF, HWND, POINT, SIZE};
use windows::Win32::Graphics::Gdi::{
    AC_SRC_ALPHA, AC_SRC_OVER, BI_RGB, BITMAPINFO, BITMAPINFOHEADER, BLENDFUNCTION,
    CreateCompatibleDC, CreateDIBSection, DIB_RGB_COLORS, DeleteDC, DeleteObject, GetDC, HBITMAP,
    HDC, HGDIOBJ, ReleaseDC, SelectObject,
};
use windows::Win32::UI::WindowsAndMessaging::{
    GWL_EXSTYLE, GetWindowLongPtrW, HWND_TOPMOST, SW_HIDE, SW_SHOWNOACTIVATE, SWP_NOACTIVATE,
    SWP_NOMOVE, SWP_NOSIZE, SetWindowLongPtrW, SetWindowPos, ShowWindow, ULW_ALPHA,
    UpdateLayeredWindow, WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TRANSPARENT,
};

const MAX_OVERLAY_DIMENSION: u32 = 8_192;
const MAX_OVERLAY_BYTES: usize = 256 * 1024 * 1024;

static NEXT_RENDERER_KEY: AtomicU64 = AtomicU64::new(1);

#[derive(Clone, Debug, Eq, PartialEq)]
pub struct LayeredWindowError {
    operation: &'static str,
    message: String,
}

impl LayeredWindowError {
    fn new(operation: &'static str, message: impl Into<String>) -> Self {
        Self {
            operation,
            message: message.into(),
        }
    }

    fn windows(operation: &'static str, error: windows::core::Error) -> Self {
        Self::new(operation, error.to_string())
    }
}

impl Display for LayeredWindowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl Error for LayeredWindowError {}

/// Opaque identity tying an uploaded texture to its original SDL renderer.
#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RendererKey(u64);

/// Stable texture-creation context for one layered window.
///
/// Applications first create all windows, then collect one of these values per
/// window into a stable `Vec`, and finally create textures that borrow that
/// slice.  Textures therefore drop before their creators without
/// `unsafe_textures`, self-references, leaked allocations, or raw SDL texture
/// pointers.
pub struct RendererResources {
    key: RendererKey,
    creator: TextureCreator<SurfaceContext<'static>>,
}

impl RendererResources {
    pub(crate) const fn key(&self) -> RendererKey {
        self.key
    }

    pub(crate) fn creator(&self) -> &TextureCreator<SurfaceContext<'static>> {
        &self.creator
    }
}

/// One click-through, borderless, taskbar-free Windows overlay.
pub struct LayeredWindow {
    // Field order is deliberate: the selected DIB is restored and destroyed
    // before SDL tears down the renderer and native window.
    native: LayeredBuffer,
    canvas: SurfaceCanvas<'static>,
    _window: Window,
    key: RendererKey,
    width: u32,
    height: u32,
    shown: bool,
    click_through: bool,
}

impl LayeredWindow {
    pub fn new(
        video: &VideoSubsystem,
        title: &str,
        width: u32,
        height: u32,
        click_through: bool,
    ) -> Result<Self, LayeredWindowError> {
        validate_dimensions(width, height)?;
        let key = next_renderer_key()?;

        let mut builder = video.window(title, width, height);
        builder.hidden().borderless().always_on_top();
        let extra_flags = sdl2::sys::SDL_WindowFlags::SDL_WINDOW_SKIP_TASKBAR as u32
            | sdl2::sys::SDL_WindowFlags::SDL_WINDOW_UTILITY as u32;
        builder.set_window_flags(builder.window_flags() | extra_flags);
        let window = builder
            .build()
            .map_err(|error| LayeredWindowError::new("create SDL window", error.to_string()))?;

        let hwnd = hwnd_from_sdl_window(&window)?;
        apply_extended_style(hwnd, click_through)?;

        let surface = Surface::new(width, height, PixelFormatEnum::ARGB8888)
            .map_err(|error| LayeredWindowError::new("create ARGB surface", error))?;
        let mut canvas = surface
            .into_canvas()
            .map_err(|error| LayeredWindowError::new("create software renderer", error))?;
        canvas.set_blend_mode(BlendMode::Blend);
        canvas.set_draw_color(sdl2::pixels::Color::RGBA(0, 0, 0, 0));
        canvas.clear();

        let native = LayeredBuffer::new(hwnd, width, height)?;

        // SAFETY: `hwnd` is the live HWND owned by `window`.  The call neither
        // activates nor moves it; it only establishes the topmost band and
        // confirms the already-created dimensions.
        unsafe {
            SetWindowPos(
                hwnd,
                Some(HWND_TOPMOST),
                0,
                0,
                width as i32,
                height as i32,
                SWP_NOMOVE | SWP_NOACTIVATE,
            )
            .map_err(|error| LayeredWindowError::windows("configure overlay Z-order", error))?;
        }

        Ok(Self {
            native,
            canvas,
            _window: window,
            key,
            width,
            height,
            shown: false,
            click_through,
        })
    }

    #[must_use]
    pub fn renderer_resources(&self) -> RendererResources {
        RendererResources {
            key: self.key,
            creator: self.canvas.texture_creator(),
        }
    }

    #[must_use]
    pub const fn dimensions(&self) -> (u32, u32) {
        (self.width, self.height)
    }

    #[must_use]
    pub const fn is_shown(&self) -> bool {
        self.shown
    }

    #[must_use]
    pub const fn click_through(&self) -> bool {
        self.click_through
    }

    pub fn set_click_through(&mut self, enabled: bool) -> Result<(), LayeredWindowError> {
        apply_extended_style(self.native.hwnd, enabled)?;
        self.click_through = enabled;
        Ok(())
    }

    /// Copies the completed ARGB8888 software surface to the DIB and submits
    /// it at an absolute physical-screen position.
    pub fn present_at(&mut self, screen_x: i32, screen_y: i32) -> Result<(), LayeredWindowError> {
        self.canvas.present();

        let pitch = self.canvas.surface().pitch() as usize;
        let source =
            self.canvas.surface().without_lock().ok_or_else(|| {
                LayeredWindowError::new("read software surface", "surface is locked")
            })?;
        self.native.copy_surface(source, pitch)?;
        self.native.present(screen_x, screen_y)?;

        if !self.shown {
            // SAFETY: The HWND remains owned by the live SDL Window.  Showing
            // with SW_SHOWNOACTIVATE and SWP_NOACTIVATE cannot steal focus.
            unsafe {
                let _ = ShowWindow(self.native.hwnd, SW_SHOWNOACTIVATE);
                SetWindowPos(
                    self.native.hwnd,
                    Some(HWND_TOPMOST),
                    0,
                    0,
                    0,
                    0,
                    SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
                )
                .map_err(|error| {
                    LayeredWindowError::windows("show overlay without activation", error)
                })?;
            }
            self.shown = true;
        }
        Ok(())
    }

    /// Places this window immediately below `foreground` without moving,
    /// resizing, or activating either window.
    pub fn place_behind(&self, foreground: &Self) -> Result<(), LayeredWindowError> {
        if self.native.hwnd == foreground.native.hwnd {
            return Err(LayeredWindowError::new(
                "place overlay behind foreground",
                "a window cannot be placed behind itself",
            ));
        }
        // SAFETY: Both HWNDs are borrowed from live LayeredWindow values.  No
        // ownership transfers and SWP_NOACTIVATE preserves the current focus.
        unsafe {
            SetWindowPos(
                self.native.hwnd,
                Some(foreground.native.hwnd),
                0,
                0,
                0,
                0,
                SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE,
            )
            .map_err(|error| LayeredWindowError::windows("place food below bug overlay", error))
        }
    }

    pub fn hide(&mut self) {
        // SAFETY: The HWND is valid while the SDL Window field is alive.
        unsafe {
            let _ = ShowWindow(self.native.hwnd, SW_HIDE);
        }
        self.shown = false;
    }

    pub(crate) const fn renderer_key(&self) -> RendererKey {
        self.key
    }

    pub(crate) fn canvas_mut(&mut self) -> &mut SurfaceCanvas<'static> {
        &mut self.canvas
    }
}

impl Drop for LayeredWindow {
    fn drop(&mut self) {
        self.hide();
    }
}

fn validate_dimensions(width: u32, height: u32) -> Result<(), LayeredWindowError> {
    if width == 0 || height == 0 {
        return Err(LayeredWindowError::new(
            "validate overlay dimensions",
            "width and height must be positive",
        ));
    }
    if width > MAX_OVERLAY_DIMENSION || height > MAX_OVERLAY_DIMENSION {
        return Err(LayeredWindowError::new(
            "validate overlay dimensions",
            format!("{width}x{height} exceeds the {MAX_OVERLAY_DIMENSION}-pixel dimension limit"),
        ));
    }
    let bytes = (width as usize)
        .checked_mul(height as usize)
        .and_then(|pixels| pixels.checked_mul(4))
        .ok_or_else(|| {
            LayeredWindowError::new("validate overlay dimensions", "pixel byte size overflows")
        })?;
    if bytes > MAX_OVERLAY_BYTES {
        return Err(LayeredWindowError::new(
            "validate overlay dimensions",
            format!("pixel buffer exceeds the {MAX_OVERLAY_BYTES}-byte limit"),
        ));
    }
    Ok(())
}

fn next_renderer_key() -> Result<RendererKey, LayeredWindowError> {
    NEXT_RENDERER_KEY
        .fetch_update(Ordering::Relaxed, Ordering::Relaxed, |current| {
            current.checked_add(1)
        })
        .map(RendererKey)
        .map_err(|_| {
            LayeredWindowError::new(
                "allocate renderer identity",
                "renderer identity space is exhausted",
            )
        })
}

fn hwnd_from_sdl_window(window: &Window) -> Result<HWND, LayeredWindowError> {
    // rust-sdl2's raw-window-handle implementation currently panics if
    // SDL_GetWindowWMInfo itself fails.  Convert that backend failure into the
    // startup error path instead of allowing a process panic.
    catch_unwind(AssertUnwindSafe(|| {
        let handle = window.window_handle().map_err(|error| {
            LayeredWindowError::new("obtain native window handle", error.to_string())
        })?;
        match handle.as_raw() {
            RawWindowHandle::Win32(handle) => Ok(HWND(handle.hwnd.get() as *mut c_void)),
            other => Err(LayeredWindowError::new(
                "obtain native window handle",
                format!("SDL returned a non-Win32 backend: {other:?}"),
            )),
        }
    }))
    .map_err(|_| {
        LayeredWindowError::new(
            "obtain native window handle",
            "SDL could not query its native window backend",
        )
    })?
}

fn apply_extended_style(hwnd: HWND, click_through: bool) -> Result<(), LayeredWindowError> {
    let required = WS_EX_LAYERED.0 | WS_EX_TOOLWINDOW.0 | WS_EX_NOACTIVATE.0;
    let transparent = WS_EX_TRANSPARENT.0;

    // SAFETY: `hwnd` is borrowed from a live SDL Window on the current thread.
    // GWL_EXSTYLE reads/writes one integer style value and retains no pointer.
    unsafe {
        let current = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let desired = if click_through {
            current | required | transparent
        } else {
            (current | required) & !transparent
        };
        let _ = SetWindowLongPtrW(hwnd, GWL_EXSTYLE, desired as isize);

        let applied = GetWindowLongPtrW(hwnd, GWL_EXSTYLE) as u32;
        let required_applied = applied & required == required;
        let transparent_applied = (applied & transparent != 0) == click_through;
        if !required_applied || !transparent_applied {
            return Err(LayeredWindowError::new(
                "configure layered-window styles",
                "Windows did not apply the required extended styles",
            ));
        }
    }
    Ok(())
}

struct ScreenDc(HDC);

impl ScreenDc {
    fn acquire() -> Result<Self, LayeredWindowError> {
        // SAFETY: Passing None requests the desktop DC.  A successful handle
        // is released exactly once by ScreenDc::drop.
        let dc = unsafe { GetDC(None) };
        if dc.is_invalid() {
            Err(LayeredWindowError::new(
                "acquire desktop device context",
                windows::core::Error::from_thread().to_string(),
            ))
        } else {
            Ok(Self(dc))
        }
    }
}

impl Drop for ScreenDc {
    fn drop(&mut self) {
        // SAFETY: self.0 came from GetDC(None) and is released exactly once.
        unsafe {
            let _ = ReleaseDC(None, self.0);
        }
    }
}

struct LayeredBuffer {
    hwnd: HWND,
    memory_dc: HDC,
    bitmap: HBITMAP,
    previous_bitmap: HGDIOBJ,
    pixels: NonNull<u8>,
    byte_len: usize,
    width: u32,
    height: u32,
}

impl LayeredBuffer {
    fn new(hwnd: HWND, width: u32, height: u32) -> Result<Self, LayeredWindowError> {
        let byte_len = (width as usize)
            .checked_mul(height as usize)
            .and_then(|pixels| pixels.checked_mul(4))
            .ok_or_else(|| {
                LayeredWindowError::new("create layered-window DIB", "pixel byte size overflows")
            })?;
        let screen_dc = ScreenDc::acquire()?;

        // SAFETY: screen_dc is a live desktop DC.  The returned memory DC is
        // owned by this function and either cleaned on an error path or moved
        // into LayeredBuffer.
        let memory_dc = unsafe { CreateCompatibleDC(Some(screen_dc.0)) };
        if memory_dc.is_invalid() {
            return Err(LayeredWindowError::new(
                "create compatible memory DC",
                windows::core::Error::from_thread().to_string(),
            ));
        }

        let bitmap_info = BITMAPINFO {
            bmiHeader: BITMAPINFOHEADER {
                biSize: size_of::<BITMAPINFOHEADER>() as u32,
                biWidth: width as i32,
                biHeight: -(height as i32),
                biPlanes: 1,
                biBitCount: 32,
                biCompression: BI_RGB.0,
                biSizeImage: byte_len as u32,
                ..BITMAPINFOHEADER::default()
            },
            ..BITMAPINFO::default()
        };
        let mut raw_pixels = std::ptr::null_mut();
        // SAFETY: bitmap_info is fully initialized and remains live for the
        // synchronous call.  The returned bitmap/pixel allocation is owned by
        // the caller and deleted by LayeredBuffer::drop.
        let bitmap = match unsafe {
            CreateDIBSection(
                Some(screen_dc.0),
                &raw const bitmap_info,
                DIB_RGB_COLORS,
                &raw mut raw_pixels,
                None,
                0,
            )
        } {
            Ok(bitmap) => bitmap,
            Err(error) => {
                // SAFETY: memory_dc was successfully created above and has not
                // been transferred or deleted.
                unsafe {
                    let _ = DeleteDC(memory_dc);
                }
                return Err(LayeredWindowError::windows(
                    "create top-down layered-window DIB",
                    error,
                ));
            }
        };
        let Some(pixels) = NonNull::new(raw_pixels.cast::<u8>()) else {
            // SAFETY: both handles were created above and the bitmap has not
            // yet been selected into the memory DC.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(memory_dc);
            }
            return Err(LayeredWindowError::new(
                "create top-down layered-window DIB",
                "Windows returned a null pixel buffer",
            ));
        };

        // SAFETY: memory_dc and bitmap are valid and owned by this function.
        // The previous object is restored before bitmap/DC destruction.
        let previous_bitmap = unsafe { SelectObject(memory_dc, HGDIOBJ(bitmap.0)) };
        if previous_bitmap.is_invalid() {
            // SAFETY: SelectObject failed, so bitmap is not selected; both
            // owned handles may be destroyed directly.
            unsafe {
                let _ = DeleteObject(HGDIOBJ(bitmap.0));
                let _ = DeleteDC(memory_dc);
            }
            return Err(LayeredWindowError::new(
                "select layered-window DIB",
                windows::core::Error::from_thread().to_string(),
            ));
        }

        Ok(Self {
            hwnd,
            memory_dc,
            bitmap,
            previous_bitmap,
            pixels,
            byte_len,
            width,
            height,
        })
    }

    fn copy_surface(
        &mut self,
        source: &[u8],
        source_pitch: usize,
    ) -> Result<(), LayeredWindowError> {
        let row_bytes = self.width as usize * 4;
        let required_source = source_pitch
            .checked_mul(self.height as usize)
            .ok_or_else(|| {
                LayeredWindowError::new("copy ARGB surface", "source byte size overflows")
            })?;
        if source_pitch < row_bytes || source.len() < required_source {
            return Err(LayeredWindowError::new(
                "copy ARGB surface",
                "SDL surface pitch or storage is smaller than expected",
            ));
        }

        // SAFETY: pixels points to the DIB allocation created with exactly
        // byte_len bytes and remains valid until LayeredBuffer::drop.  &mut
        // self guarantees no overlapping mutable slice exists.
        let destination =
            unsafe { std::slice::from_raw_parts_mut(self.pixels.as_ptr(), self.byte_len) };
        for row in 0..self.height as usize {
            let source_start = row * source_pitch;
            let destination_start = row * row_bytes;
            destination[destination_start..destination_start + row_bytes]
                .copy_from_slice(&source[source_start..source_start + row_bytes]);
        }
        Ok(())
    }

    fn present(&self, screen_x: i32, screen_y: i32) -> Result<(), LayeredWindowError> {
        let destination = POINT {
            x: screen_x,
            y: screen_y,
        };
        let size = SIZE {
            cx: self.width as i32,
            cy: self.height as i32,
        };
        let source = POINT::default();
        let blend = BLENDFUNCTION {
            BlendOp: AC_SRC_OVER as u8,
            BlendFlags: 0,
            SourceConstantAlpha: 255,
            AlphaFormat: AC_SRC_ALPHA as u8,
        };
        let screen_dc = ScreenDc::acquire()?;

        // SAFETY: HWND, desktop DC and memory DC are live.  The POINT, SIZE and
        // BLENDFUNCTION values remain valid for the synchronous call.  The DIB
        // selected into memory_dc is top-down BGRA/ARGB8888 with premultiplied
        // pixels produced by SDL's software blend.
        unsafe {
            UpdateLayeredWindow(
                self.hwnd,
                Some(screen_dc.0),
                Some(&raw const destination),
                Some(&raw const size),
                Some(self.memory_dc),
                Some(&raw const source),
                COLORREF(0),
                Some(&raw const blend),
                ULW_ALPHA,
            )
            .map_err(|error| LayeredWindowError::windows("present layered window", error))
        }
    }
}

impl Drop for LayeredBuffer {
    fn drop(&mut self) {
        // SAFETY: These objects were created and selected exactly once by
        // LayeredBuffer::new.  Restoration happens before deleting the bitmap,
        // and the memory DC is deleted last.
        unsafe {
            let _ = SelectObject(self.memory_dc, self.previous_bitmap);
            let _ = DeleteObject(HGDIOBJ(self.bitmap.0));
            let _ = DeleteDC(self.memory_dc);
        }
    }
}
