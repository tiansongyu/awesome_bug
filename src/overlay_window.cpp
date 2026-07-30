#include "overlay_window.h"

#include <SDL_syswm.h>

#include <algorithm>
#include <cstring>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#elif defined(__linux__)
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <X11/Xutil.h>
#include <X11/extensions/Xfixes.h>
#include <X11/extensions/Xrender.h>
#include <X11/extensions/shape.h>
#endif

struct OverlayWindow::NativeState {
#if defined(_WIN32)
    HWND hwnd = nullptr;
    HDC memoryDc = nullptr;
    HBITMAP dib = nullptr;
    HGDIOBJ oldBitmap = nullptr;
    void* dibPixels = nullptr;
#elif defined(__linux__)
    Display* display = nullptr;
    ::Window window = 0;
    bool xfixesAvailable = false;
#endif
};

void OverlayWindow::prepareVideoDriver() {
#if defined(_WIN32)
    using SetDpiAwarenessContextFn = BOOL(WINAPI*)(HANDLE);
    if (HMODULE user32 = GetModuleHandleW(L"user32.dll")) {
        const FARPROC address =
            GetProcAddress(user32, "SetProcessDpiAwarenessContext");
        SetDpiAwarenessContextFn setDpiAwareness = nullptr;
        static_assert(sizeof(setDpiAwareness) == sizeof(address),
                      "Windows function pointer size mismatch");
        std::memcpy(&setDpiAwareness, &address, sizeof(address));
        if (setDpiAwareness) {
            setDpiAwareness(reinterpret_cast<HANDLE>(-4)); // Per-monitor aware v2.
        }
    }
#elif defined(__linux__)
    // Desktop overlays need absolute positioning. Prefer X11/XWayland when the
    // caller has not explicitly selected another SDL video driver.
    if (!SDL_getenv("SDL_VIDEODRIVER") && SDL_getenv("DISPLAY")) {
        SDL_setenv("SDL_VIDEODRIVER", "x11", 0);
    }

    Display* display = XOpenDisplay(nullptr);
    if (!display) return;

    const int screen = DefaultScreen(display);
    XVisualInfo request{};
    request.screen = screen;
    request.depth = 32;
    request.c_class = TrueColor;
    int count = 0;
    XVisualInfo* visuals = XGetVisualInfo(
        display, VisualScreenMask | VisualDepthMask | VisualClassMask, &request, &count);
    for (int i = 0; visuals && i < count; ++i) {
        XRenderPictFormat* format = XRenderFindVisualFormat(display, visuals[i].visual);
        if (format && format->type == PictTypeDirect && format->direct.alphaMask) {
            const std::string visualId = std::to_string(visuals[i].visualid);
            SDL_SetHint(SDL_HINT_VIDEO_X11_WINDOW_VISUALID, visualId.c_str());
            break;
        }
    }
    if (visuals) XFree(visuals);
    XCloseDisplay(display);
    SDL_SetHint(SDL_HINT_VIDEO_X11_NET_WM_BYPASS_COMPOSITOR, "0");
#endif
}

OverlayWindow::OverlayWindow(int size, bool clickThrough)
    : OverlayWindow(size, size, clickThrough, true) {}

OverlayWindow::OverlayWindow(int width, int height, bool clickThrough,
                             bool useBoundingShape)
    : native_(std::make_unique<NativeState>()),
      width_(width),
      height_(height),
      clickThrough_(clickThrough),
      useBoundingShape_(useBoundingShape) {
    Uint32 flags = SDL_WINDOW_BORDERLESS | SDL_WINDOW_ALWAYS_ON_TOP |
                   SDL_WINDOW_SKIP_TASKBAR | SDL_WINDOW_HIDDEN;
#ifdef SDL_WINDOW_UTILITY
    flags |= SDL_WINDOW_UTILITY;
#endif
    window_ = SDL_CreateWindow("Cockroach Overlay", SDL_WINDOWPOS_UNDEFINED,
                               SDL_WINDOWPOS_UNDEFINED, width_, height_, flags);
    if (!window_) {
        error_ = SDL_GetError();
        return;
    }

    canvas_ = SDL_CreateRGBSurfaceWithFormat(
        0, width_, height_, 32, SDL_PIXELFORMAT_ARGB8888);
    if (!canvas_) {
        error_ = SDL_GetError();
        return;
    }

    renderer_ = SDL_CreateSoftwareRenderer(canvas_);
    directRenderer_ = false;
    if (!renderer_) {
        error_ = SDL_GetError();
        return;
    }
    SDL_SetRenderDrawBlendMode(renderer_, SDL_BLENDMODE_BLEND);
    SDL_SetSurfaceBlendMode(canvas_, SDL_BLENDMODE_NONE);

#if defined(__linux__)
    // Some X11 window managers do not create/map the native X window until
    // SDL_ShowWindow. XFixes calls made before this point can target a stale
    // window id and terminate the process with BadWindow.
    if (!directRenderer_) {
        SDL_Surface* initialSurface = SDL_GetWindowSurface(window_);
        if (initialSurface) {
            SDL_FillRect(initialSurface, nullptr, 0);
            SDL_UpdateWindowSurface(window_);
        }
    }
    SDL_ShowWindow(window_);
    SDL_PumpEvents();
#endif

    if (!configureNative()) {
        return;
    }
#if defined(__linux__)
    shown_ = true;
#endif
}

OverlayWindow::~OverlayWindow() {
#if defined(_WIN32)
    if (native_) {
        if (native_->memoryDc && native_->oldBitmap) {
            SelectObject(native_->memoryDc, native_->oldBitmap);
        }
        if (native_->dib) DeleteObject(native_->dib);
        if (native_->memoryDc) DeleteDC(native_->memoryDc);
    }
#endif
    if (renderer_) SDL_DestroyRenderer(renderer_);
    if (canvas_) SDL_FreeSurface(canvas_);
    if (window_) SDL_DestroyWindow(window_);
}

bool OverlayWindow::valid() const {
    return window_ && canvas_ && renderer_ && error_.empty();
}

const std::string& OverlayWindow::error() const {
    return error_;
}

SDL_Renderer* OverlayWindow::renderer() const {
    return renderer_;
}

SDL_Surface* OverlayWindow::canvas() const {
    return canvas_;
}

int OverlayWindow::size() const {
    return width_;
}

int OverlayWindow::width() const {
    return width_;
}

int OverlayWindow::height() const {
    return height_;
}

bool OverlayWindow::configureNative() {
    SDL_SysWMinfo wmInfo{};
    SDL_VERSION(&wmInfo.version);
    if (!SDL_GetWindowWMInfo(window_, &wmInfo)) {
        // The dummy driver used by automated/headless checks has no native WM.
        const char* driver = SDL_GetCurrentVideoDriver();
        if (driver && std::strcmp(driver, "dummy") == 0) return true;
        error_ = std::string("Cannot access native window: ") + SDL_GetError();
        return false;
    }

#if defined(_WIN32)
    native_->hwnd = wmInfo.info.win.window;
    LONG_PTR exStyle = GetWindowLongPtrW(native_->hwnd, GWL_EXSTYLE);
    exStyle |= WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE;
    if (clickThrough_) exStyle |= WS_EX_TRANSPARENT;
    SetWindowLongPtrW(native_->hwnd, GWL_EXSTYLE, exStyle);

    HDC desktopDc = GetDC(nullptr);
    native_->memoryDc = CreateCompatibleDC(desktopDc);
    BITMAPINFO bitmapInfo{};
    bitmapInfo.bmiHeader.biSize = sizeof(BITMAPINFOHEADER);
    bitmapInfo.bmiHeader.biWidth = width_;
    bitmapInfo.bmiHeader.biHeight = -height_; // top-down BGRA
    bitmapInfo.bmiHeader.biPlanes = 1;
    bitmapInfo.bmiHeader.biBitCount = 32;
    bitmapInfo.bmiHeader.biCompression = BI_RGB;
    native_->dib = CreateDIBSection(desktopDc, &bitmapInfo, DIB_RGB_COLORS,
                                    &native_->dibPixels, nullptr, 0);
    ReleaseDC(nullptr, desktopDc);
    if (!native_->memoryDc || !native_->dib || !native_->dibPixels) {
        error_ = "Could not create the Windows layered-window buffer";
        return false;
    }
    native_->oldBitmap = SelectObject(native_->memoryDc, native_->dib);
    SetWindowPos(native_->hwnd, HWND_TOPMOST, 0, 0, width_, height_,
                 SWP_NOMOVE | SWP_NOACTIVATE);
#elif defined(__linux__)
    if (wmInfo.subsystem != SDL_SYSWM_X11) {
        error_ = "Linux transparent overlay requires X11 or XWayland";
        return false;
    }
    native_->display = wmInfo.info.x11.display;
    native_->window = wmInfo.info.x11.window;
    int eventBase = 0;
    int errorBase = 0;
    native_->xfixesAvailable =
        XFixesQueryExtension(native_->display, &eventBase, &errorBase);

    if (clickThrough_ && native_->xfixesAvailable) {
        XserverRegion empty = XFixesCreateRegion(native_->display, nullptr, 0);
        XFixesSetWindowShapeRegion(native_->display, native_->window,
                                   ShapeInput, 0, 0, empty);
        XFixesDestroyRegion(native_->display, empty);
    }

    Atom windowType = XInternAtom(native_->display, "_NET_WM_WINDOW_TYPE", False);
    Atom notification = XInternAtom(
        native_->display, "_NET_WM_WINDOW_TYPE_NOTIFICATION", False);
    XChangeProperty(native_->display, native_->window, windowType, XA_ATOM, 32,
                    PropModeReplace,
                    reinterpret_cast<unsigned char*>(&notification), 1);
    XFlush(native_->display);
#else
    (void)wmInfo;
#endif
    return true;
}

bool OverlayWindow::presentAt(int screenX, int screenY) {
    if (!valid()) return false;
    SDL_RenderPresent(renderer_);

#if defined(_WIN32)
    if (!native_->hwnd) return false;
    const auto* source = static_cast<const unsigned char*>(canvas_->pixels);
    auto* destination = static_cast<unsigned char*>(native_->dibPixels);
    const int rowBytes = width_ * 4;
    for (int y = 0; y < height_; ++y) {
        std::memcpy(destination + y * rowBytes,
                    source + y * canvas_->pitch,
                    static_cast<std::size_t>(rowBytes));
    }

    POINT destinationPoint{screenX, screenY};
    SIZE windowSize{width_, height_};
    POINT sourcePoint{0, 0};
    BLENDFUNCTION blend{AC_SRC_OVER, 0, 255, AC_SRC_ALPHA};
    HDC desktopDc = GetDC(nullptr);
    const BOOL ok = UpdateLayeredWindow(
        native_->hwnd, desktopDc, &destinationPoint, &windowSize,
        native_->memoryDc, &sourcePoint, 0, &blend, ULW_ALPHA);
    ReleaseDC(nullptr, desktopDc);
    if (ok && !shown_) {
        ShowWindow(native_->hwnd, SW_SHOWNOACTIVATE);
        SetWindowPos(native_->hwnd, HWND_TOPMOST, 0, 0, 0, 0,
                     SWP_NOMOVE | SWP_NOSIZE | SWP_NOACTIVATE);
        shown_ = true;
    }
    return ok != FALSE;
#elif defined(__linux__)
    SDL_SetWindowPosition(window_, screenX, screenY);
    SDL_Surface* windowSurface = SDL_GetWindowSurface(window_);
    if (!windowSurface) return false;
    SDL_SetSurfaceBlendMode(canvas_, SDL_BLENDMODE_NONE);
    if (SDL_BlitSurface(canvas_, nullptr, windowSurface, nullptr) != 0 ||
        SDL_UpdateWindowSurface(window_) != 0) {
        return false;
    }

    // A per-frame bounding shape is a robust fallback on compositors that do
    // not preserve the ARGB visual chosen above.
    if (!directRenderer_ && useBoundingShape_ && native_->display &&
        native_->xfixesAvailable) {
        std::vector<XRectangle> spans;
        spans.reserve(static_cast<std::size_t>(height_ * 2));
        const auto* pixels = static_cast<const Uint32*>(canvas_->pixels);
        const int stride = canvas_->pitch / 4;
        const Uint32 alphaMask = canvas_->format->Amask;
        const Uint8 alphaShift = canvas_->format->Ashift;
        for (int y = 0; y < height_; ++y) {
            int start = -1;
            for (int x = 0; x <= width_; ++x) {
                Uint8 alpha = 0;
                if (x < width_) {
                    alpha = static_cast<Uint8>(
                        (pixels[y * stride + x] & alphaMask) >> alphaShift);
                }
                if (alpha > 3 && start < 0) start = x;
                if ((alpha <= 3 || x == width_) && start >= 0) {
                    XRectangle rectangle{};
                    rectangle.x = static_cast<short>(start);
                    rectangle.y = static_cast<short>(y);
                    rectangle.width = static_cast<unsigned short>(x - start);
                    rectangle.height = 1;
                    spans.push_back(rectangle);
                    start = -1;
                }
            }
        }
        XserverRegion region = XFixesCreateRegion(
            native_->display, spans.empty() ? nullptr : spans.data(),
            static_cast<int>(spans.size()));
        XFixesSetWindowShapeRegion(native_->display, native_->window,
                                   ShapeBounding, 0, 0, region);
        XFixesDestroyRegion(native_->display, region);
        XFlush(native_->display);
    }
    if (!shown_) {
        SDL_ShowWindow(window_);
        SDL_RaiseWindow(window_);
        shown_ = true;
    }
    return true;
#else
    SDL_SetWindowPosition(window_, screenX, screenY);
    SDL_Surface* windowSurface = SDL_GetWindowSurface(window_);
    if (!windowSurface) return false;
    SDL_SetSurfaceBlendMode(canvas_, SDL_BLENDMODE_NONE);
    const bool ok = SDL_BlitSurface(canvas_, nullptr, windowSurface, nullptr) == 0 &&
                    SDL_UpdateWindowSurface(window_) == 0;
    if (ok && !shown_) {
        SDL_ShowWindow(window_);
        SDL_RaiseWindow(window_);
        shown_ = true;
    }
    return ok;
#endif
}

bool OverlayWindow::placeBehind(
    const OverlayWindow& foreground) {
#if defined(_WIN32)
    if (!native_ || !native_->hwnd ||
        !foreground.native_ || !foreground.native_->hwnd) {
        return false;
    }
    return SetWindowPos(
               native_->hwnd, foreground.native_->hwnd,
               0, 0, 0, 0,
               SWP_NOMOVE | SWP_NOSIZE |
                   SWP_NOACTIVATE) != FALSE;
#else
    (void)foreground;
    return true;
#endif
}

void OverlayWindow::hide() {
    if (!window_) return;
#if defined(_WIN32)
    if (native_ && native_->hwnd) {
        ShowWindow(native_->hwnd, SW_HIDE);
    } else {
        SDL_HideWindow(window_);
    }
#else
    SDL_HideWindow(window_);
#endif
    shown_ = false;
}

void OverlayWindow::finishFrame() {
}

bool OverlayWindow::quitHotkeyPressed() const {
#if defined(_WIN32)
    const bool control = (GetAsyncKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool alt = (GetAsyncKeyState(VK_MENU) & 0x8000) != 0;
    const bool q = (GetAsyncKeyState('Q') & 0x8000) != 0;
    return control && alt && q;
#elif defined(__linux__)
    if (!native_ || !native_->display) return false;
    char keys[32]{};
    XQueryKeymap(native_->display, keys);
    const auto pressed = [&](KeySym symbol) {
        const KeyCode code = XKeysymToKeycode(native_->display, symbol);
        return code != 0 && (keys[code / 8] & (1 << (code % 8))) != 0;
    };
    const bool control = pressed(XK_Control_L) || pressed(XK_Control_R);
    const bool alt = pressed(XK_Alt_L) || pressed(XK_Alt_R);
    return control && alt && pressed(XK_q);
#else
    return false;
#endif
}
