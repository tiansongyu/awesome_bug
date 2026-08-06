//! SDL software surfaces presented through a non-activating Cocoa shaped window.

// objc 0.2 checks its historical `cargo-clippy` feature inside macros. Modern
// rustc's check-cfg sees that expansion in this crate, where the feature is not
// declared, so contain the compatibility warning in this FFI-only module.
#![allow(unexpected_cfgs)]

use std::error::Error;
use std::fmt::{self, Display, Formatter};
use std::panic::{AssertUnwindSafe, catch_unwind};
use std::sync::atomic::{AtomicU64, Ordering};

use objc::runtime::{NO, Object, YES};
use objc::{class, msg_send, sel, sel_impl};
use sdl2::VideoSubsystem;
use sdl2::pixels::{Color, PixelFormatEnum};
use sdl2::render::{BlendMode, SurfaceCanvas, TextureCreator};
use sdl2::surface::{Surface, SurfaceContext, SurfaceRef};
use sdl2::video::{Window, WindowPos};

const MAX_OVERLAY_DIMENSION: u32 = 8_192;
const MAX_OVERLAY_BYTES: usize = 256 * 1024 * 1024;
const NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES: usize = 1 << 0;
const NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY: usize = 1 << 4;
const NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE: usize = 1 << 6;
const NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY: usize = 1 << 8;
const NS_WINDOW_BELOW: isize = -1;
const SDL_SYSWM_COCOA: u32 = 4;

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
}

impl Display for LayeredWindowError {
    fn fmt(&self, formatter: &mut Formatter<'_>) -> fmt::Result {
        write!(formatter, "{}: {}", self.operation, self.message)
    }
}

impl Error for LayeredWindowError {}

#[derive(Clone, Copy, Debug, Eq, Hash, PartialEq)]
pub(crate) struct RendererKey(u64);

pub struct RendererResources {
    key: RendererKey,
    creator: TextureCreator<SurfaceContext<'static>>,
}

impl RendererResources {
    pub(crate) const fn key(&self) -> RendererKey {
        self.key
    }

    pub(crate) const fn creator(&self) -> &TextureCreator<SurfaceContext<'static>> {
        &self.creator
    }
}

/// One transparent, click-through, Dock-free Cocoa overlay.
pub struct LayeredWindow {
    canvas: SurfaceCanvas<'static>,
    window: Window,
    native_window: *mut Object,
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
        builder.hidden().borderless().always_on_top().set_shaped();
        let extra_flags = sdl2::sys::SDL_WindowFlags::SDL_WINDOW_SKIP_TASKBAR as u32
            | sdl2::sys::SDL_WindowFlags::SDL_WINDOW_UTILITY as u32;
        builder.set_window_flags(builder.window_flags() | extra_flags);
        let mut window = builder
            .build()
            .map_err(|error| LayeredWindowError::new("create SDL window", error.to_string()))?;
        let native_window = native_window_from_sdl(&window)?;
        configure_native_window(native_window, click_through);

        let mut surface = Surface::new(width, height, PixelFormatEnum::ARGB8888)
            .map_err(|error| LayeredWindowError::new("create ARGB surface", error))?;
        surface
            .set_blend_mode(BlendMode::None)
            .map_err(|error| LayeredWindowError::new("configure ARGB surface", error))?;
        let mut canvas = surface
            .into_canvas()
            .map_err(|error| LayeredWindowError::new("create software renderer", error))?;
        canvas.set_blend_mode(BlendMode::Blend);
        canvas.set_draw_color(Color::RGBA(0, 0, 0, 0));
        canvas.clear();
        canvas.present();

        // A fully open Cocoa shape establishes a non-opaque compositing area
        // once. Per-pixel alpha then comes from the replaced ARGB framebuffer;
        // rebuilding SDL's quadtree/Bezier clip every frame is prohibitively
        // expensive for the 20-window swarm.
        let mut shape = Surface::new(width, height, PixelFormatEnum::ARGB8888)
            .map_err(|error| LayeredWindowError::new("create Cocoa shape surface", error))?;
        shape
            .fill_rect(None, Color::RGBA(255, 255, 255, 255))
            .map_err(|error| LayeredWindowError::new("fill Cocoa shape surface", error))?;
        window.set_window_shape_alpha(&shape, 1).map_err(|code| {
            LayeredWindowError::new(
                "initialize transparent window shape",
                format!("SDL error code {code}: {}", sdl2::get_error()),
            )
        })?;

        Ok(Self {
            canvas,
            window,
            native_window,
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
        set_ignores_mouse_events(self.native_window, enabled);
        self.click_through = enabled;
        Ok(())
    }

    /// Replaces the ARGB framebuffer and places it in global desktop
    /// coordinates.
    pub fn present_at(&mut self, screen_x: i32, screen_y: i32) -> Result<(), LayeredWindowError> {
        self.canvas.present();

        let destination_raw = unsafe { sdl2::sys::SDL_GetWindowSurface(self.window.raw()) };
        if destination_raw.is_null() {
            return Err(LayeredWindowError::new(
                "obtain Cocoa window surface",
                sdl2::get_error(),
            ));
        }
        // SAFETY: SDL owns destination_raw for the lifetime of the live
        // window. It is borrowed only for this synchronous blit/update.
        let destination = unsafe { SurfaceRef::from_ll_mut(destination_raw) };
        self.canvas
            .surface()
            .blit(None, destination, None)
            .map_err(|error| LayeredWindowError::new("copy ARGB window surface", error))?;
        // SAFETY: The window and its framebuffer are live and SDL retains all
        // native ownership.
        if unsafe { sdl2::sys::SDL_UpdateWindowSurface(self.window.raw()) } != 0 {
            return Err(LayeredWindowError::new(
                "present Cocoa window surface",
                sdl2::get_error(),
            ));
        }

        self.window.set_position(
            WindowPos::Positioned(screen_x),
            WindowPos::Positioned(screen_y),
        );
        order_front_without_activation(self.native_window);
        self.shown = true;
        Ok(())
    }

    pub fn place_behind(&self, foreground: &Self) -> Result<(), LayeredWindowError> {
        if self.native_window == foreground.native_window {
            return Err(LayeredWindowError::new(
                "place overlay behind foreground",
                "a window cannot be placed behind itself",
            ));
        }
        // SAFETY: Both Objective-C pointers are borrowed from live SDL
        // windows. The messages retain neither pointer.
        unsafe {
            let number: isize = msg_send![foreground.native_window, windowNumber];
            let _: () =
                msg_send![self.native_window, orderWindow: NS_WINDOW_BELOW relativeTo: number];
        }
        Ok(())
    }

    pub fn hide(&mut self) {
        self.window.hide();
        self.shown = false;
    }

    pub(crate) const fn renderer_key(&self) -> RendererKey {
        self.key
    }

    pub(crate) const fn canvas_mut(&mut self) -> &mut SurfaceCanvas<'static> {
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

#[repr(C)]
struct SdlSysWmInfo {
    version: sdl2::sys::SDL_version,
    subsystem: u32,
    info: [usize; 8],
}

unsafe extern "C" {
    fn SDL_GetWindowWMInfo(
        window: *mut sdl2::sys::SDL_Window,
        information: *mut SdlSysWmInfo,
    ) -> sdl2::sys::SDL_bool;
}

fn native_window_from_sdl(window: &Window) -> Result<*mut Object, LayeredWindowError> {
    catch_unwind(AssertUnwindSafe(|| {
        let mut information = SdlSysWmInfo {
            version: sdl2::sys::SDL_version {
                major: 0,
                minor: 0,
                patch: 0,
            },
            subsystem: 0,
            info: [0; 8],
        };
        // SAFETY: information is correctly sized writable storage and window
        // remains live for the synchronous SDL query.
        unsafe {
            sdl2::sys::SDL_GetVersion(&raw mut information.version);
            if SDL_GetWindowWMInfo(window.raw(), &raw mut information)
                == sdl2::sys::SDL_bool::SDL_FALSE
            {
                return Err(LayeredWindowError::new(
                    "obtain native Cocoa window",
                    sdl2::get_error(),
                ));
            }
        }
        if information.subsystem != SDL_SYSWM_COCOA {
            return Err(LayeredWindowError::new(
                "obtain native Cocoa window",
                format!("SDL returned window subsystem {}", information.subsystem),
            ));
        }
        let native = information.info[0] as *mut Object;
        if native.is_null() {
            Err(LayeredWindowError::new(
                "obtain native Cocoa window",
                "SDL returned a null NSWindow",
            ))
        } else {
            Ok(native)
        }
    }))
    .map_err(|_| {
        LayeredWindowError::new(
            "obtain native Cocoa window",
            "SDL could not query its native window backend",
        )
    })?
}

fn configure_native_window(window: *mut Object, click_through: bool) {
    // SAFETY: window is the borrowed NSWindow returned by SDL. All values use
    // stable AppKit property setter ABIs and no ownership is transferred.
    unsafe {
        let clear_color: *mut Object = msg_send![class!(NSColor), clearColor];
        let _: () = msg_send![window, setOpaque: NO];
        let _: () = msg_send![window, setBackgroundColor: clear_color];
        let _: () = msg_send![window, setHasShadow: NO];
        let behavior = NS_WINDOW_COLLECTION_BEHAVIOR_CAN_JOIN_ALL_SPACES
            | NS_WINDOW_COLLECTION_BEHAVIOR_STATIONARY
            | NS_WINDOW_COLLECTION_BEHAVIOR_IGNORES_CYCLE
            | NS_WINDOW_COLLECTION_BEHAVIOR_FULL_SCREEN_AUXILIARY;
        let _: () = msg_send![window, setCollectionBehavior: behavior];
        let _: () = msg_send![window, setReleasedWhenClosed: NO];
    }
    set_ignores_mouse_events(window, click_through);
}

fn set_ignores_mouse_events(window: *mut Object, enabled: bool) {
    // SAFETY: window is borrowed from a live SDL window and the setter retains
    // no pointer.
    unsafe {
        let value = if enabled { YES } else { NO };
        let _: () = msg_send![window, setIgnoresMouseEvents: value];
    }
}

fn order_front_without_activation(window: *mut Object) {
    // SAFETY: orderFrontRegardless changes Z-order without making this
    // click-through utility window the key window.
    unsafe {
        let _: () = msg_send![window, orderFrontRegardless];
    }
}
