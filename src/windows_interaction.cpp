#include "windows_interaction.h"

#include "overlay_window.h"

#include <algorithm>
#include <array>
#include <cmath>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

namespace {
#if defined(_WIN32)
constexpr float pi = 3.14159265358979323846f;
constexpr float raiseEnd = 0.10f;
constexpr float impactTime = 0.18f;
constexpr float impactEnd = 0.27f;
constexpr float swingEnd = 0.43f;

float smoothStep(float value) {
    value = clampf(value, 0.0f, 1.0f);
    return value * value * (3.0f - 2.0f * value);
}

Vec2 transformPoint(
    Vec2 point, Vec2 center, float angle, float scale) {
    return center + rotateLocal(point * scale, angle);
}

void renderSlipper(
    SDL_Renderer* renderer, float swingClock) {
    Vec2 center{
        WindowsInteractionController::overlaySize * 0.5f,
        WindowsInteractionController::overlaySize * 0.5f};
    float angle = -18.0f * pi / 180.0f;
    float scale = 1.0f;

    if (swingClock >= 0.0f && swingClock < raiseEnd) {
        const float progress =
            smoothStep(swingClock / raiseEnd);
        center += Vec2{-30.0f * progress, -34.0f * progress};
        angle += -34.0f * pi / 180.0f * progress;
        scale += 0.08f * progress;
    } else if (swingClock >= raiseEnd &&
               swingClock < impactTime) {
        const float progress = smoothStep(
            (swingClock - raiseEnd) /
            (impactTime - raiseEnd));
        center += Vec2{
            -30.0f * (1.0f - progress),
            -34.0f * (1.0f - progress)};
        angle =
            (-52.0f + 57.0f * progress) * pi / 180.0f;
        scale = 1.08f - 0.08f * progress;
    } else if (swingClock >= impactTime &&
               swingClock < impactEnd) {
        const float progress =
            (swingClock - impactTime) /
            (impactEnd - impactTime);
        angle = (5.0f - 4.0f * progress) * pi / 180.0f;
        scale = 1.0f - std::sin(progress * pi) * 0.10f;
    } else if (swingClock >= impactEnd) {
        const float progress = smoothStep(
            (swingClock - impactEnd) /
            (swingEnd - impactEnd));
        center += Vec2{18.0f * progress, -14.0f * progress};
        angle =
            (1.0f - 19.0f * progress) * pi / 180.0f;
    }

    constexpr std::array<Vec2, 8> outline{{
        {-35.0f, -82.0f}, {23.0f, -84.0f},
        {42.0f, -61.0f},  {45.0f, 35.0f},
        {25.0f, 79.0f},   {-18.0f, 82.0f},
        {-42.0f, 47.0f},  {-44.0f, -57.0f},
    }};
    constexpr std::array<int, 18> indices{{
        0, 1, 7, 1, 2, 7, 2, 3, 7,
        3, 6, 7, 3, 4, 6, 4, 5, 6,
    }};
    std::array<SDL_Vertex, outline.size()> vertices{};
    for (std::size_t index = 0; index < outline.size(); ++index) {
        const Vec2 transformed =
            transformPoint(outline[index], center, angle, scale);
        vertices[index].position = {
            transformed.x, transformed.y};
        vertices[index].color =
            index < 3
                ? SDL_Color{62, 91, 99, 255}
                : SDL_Color{42, 66, 73, 255};
        vertices[index].tex_coord = {};
    }
    SDL_RenderGeometry(
        renderer, nullptr, vertices.data(),
        static_cast<int>(vertices.size()), indices.data(),
        static_cast<int>(indices.size()));

    // Raised rim and the two V-shaped straps make the silhouette read as a
    // rubber household slipper even at small cursor sizes.
    const std::array<Vec2, 4> rim{{
        {-27.0f, -67.0f}, {16.0f, -69.0f},
        {29.0f, 51.0f}, {-27.0f, 56.0f},
    }};
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_BLEND);
    SDL_SetRenderDrawColor(renderer, 115, 142, 145, 230);
    for (std::size_t index = 0; index < rim.size(); ++index) {
        const Vec2 start = transformPoint(
            rim[index], center, angle, scale);
        const Vec2 end = transformPoint(
            rim[(index + 1) % rim.size()],
            center, angle, scale);
        SDL_RenderDrawLineF(
            renderer, start.x, start.y, end.x, end.y);
    }

    const Vec2 strapCenter =
        transformPoint({0.0f, -4.0f}, center, angle, scale);
    const Vec2 strapLeft =
        transformPoint({-31.0f, -43.0f}, center, angle, scale);
    const Vec2 strapRight =
        transformPoint({31.0f, -43.0f}, center, angle, scale);
    SDL_SetRenderDrawColor(renderer, 26, 43, 48, 255);
    for (int offset = -3; offset <= 3; ++offset) {
        SDL_RenderDrawLineF(
            renderer,
            strapLeft.x + static_cast<float>(offset),
            strapLeft.y,
            strapCenter.x + static_cast<float>(offset),
            strapCenter.y);
        SDL_RenderDrawLineF(
            renderer,
            strapRight.x + static_cast<float>(offset),
            strapRight.y,
            strapCenter.x + static_cast<float>(offset),
            strapCenter.y);
    }
}

void renderFood(SDL_Renderer* renderer) {
    const float center =
        WindowsInteractionController::baitOverlaySize * 0.5f;
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_BLEND);

    // A small bread crumb with an irregular dark crust and three loose
    // granules remains readable on both light and dark wallpapers.
    for (int y = -17; y <= 17; ++y) {
        const float normalizedY =
            static_cast<float>(y) / 17.0f;
        const float width =
            std::sqrt(std::max(
                0.0f, 1.0f - normalizedY * normalizedY)) *
            (22.0f + std::sin(static_cast<float>(y) * 0.7f) * 2.0f);
        SDL_SetRenderDrawColor(
            renderer,
            y < -12 || y > 13 ? 116 : 194,
            y < -12 || y > 13 ? 72 : 132,
            y < -12 || y > 13 ? 34 : 65,
            255);
        SDL_RenderDrawLineF(
            renderer, center - width, center + y,
            center + width, center + y);
    }
    const std::array<SDL_FRect, 3> crumbs{{
        {center - 30.0f, center + 18.0f, 6.0f, 5.0f},
        {center + 25.0f, center + 11.0f, 5.0f, 5.0f},
        {center + 18.0f, center - 27.0f, 4.0f, 4.0f},
    }};
    SDL_SetRenderDrawColor(renderer, 171, 106, 47, 255);
    for (const SDL_FRect& crumb : crumbs) {
        SDL_RenderFillRectF(renderer, &crumb);
    }
}
#endif
} // namespace

WindowsInteractionController::WindowsInteractionController(
    bool enabled)
    : enabled_(enabled) {
#if defined(_WIN32)
    if (!enabled_) return;
    Uint8 cursorData[8]{};
    Uint8 cursorMask[8]{};
    hiddenCursor_ = SDL_CreateCursor(
        cursorData, cursorMask, 8, 8, 0, 0);
    arrowCursor_ =
        SDL_CreateSystemCursor(SDL_SYSTEM_CURSOR_ARROW);
#endif
}

WindowsInteractionController::~WindowsInteractionController() {
    cancel();
    if (hiddenCursor_) SDL_FreeCursor(hiddenCursor_);
    if (arrowCursor_) SDL_FreeCursor(arrowCursor_);
}

void WindowsInteractionController::setSlipperMode(bool enabled) {
    if (!enabled_) return;
    slipperMode_ = enabled;
    swingClock_ = -1.0f;
    impactEmitted_ = false;
    strikeHitBody_ = false;
    leftWasDown_ = false;
    SDL_ShowCursor(SDL_ENABLE);
    if (slipperMode_ && hiddenCursor_) {
        SDL_SetCursor(hiddenCursor_);
    } else if (arrowCursor_) {
        SDL_SetCursor(arrowCursor_);
    }
}

void WindowsInteractionController::cancel() {
    setSlipperMode(false);
}

SlipperInteractionEvents WindowsInteractionController::update(
    float dt, Vec2 cursorPosition) {
    SlipperInteractionEvents events;
#if defined(_WIN32)
    if (!enabled_) return events;

    const bool control =
        (GetAsyncKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool alt =
        (GetAsyncKeyState(VK_MENU) & 0x8000) != 0;
    const bool toggleDown =
        control && alt &&
        (GetAsyncKeyState('S') & 0x8000) != 0;
    if (toggleDown && !toggleWasDown_) {
        setSlipperMode(!slipperMode_);
    }
    toggleWasDown_ = toggleDown;

    const bool baitDown =
        control && alt &&
        (GetAsyncKeyState('F') & 0x8000) != 0;
    if (baitDown && !baitWasDown_) {
        setSlipperMode(false);
        events.baitPlacementRequested = true;
        events.baitPosition = cursorPosition;
    }
    baitWasDown_ = baitDown;

    const bool escapeDown =
        (GetAsyncKeyState(VK_ESCAPE) & 0x8000) != 0;
    if (escapeDown && !escapeWasDown_ && slipperMode_) {
        setSlipperMode(false);
    }
    escapeWasDown_ = escapeDown;

    const bool leftDown =
        (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;
    if (slipperMode_ && leftDown && !leftWasDown_ &&
        swingClock_ < 0.0f) {
        swingClock_ = 0.0f;
        impactEmitted_ = false;
        strikeHitBody_ = false;
        strikePosition_ = cursorPosition;
        events.strikeStarted = true;
        events.strikePosition = strikePosition_;
    }
    leftWasDown_ = leftDown;

    if (swingClock_ >= 0.0f) {
        const float previousClock = swingClock_;
        swingClock_ += std::max(0.0f, dt);
        if (!impactEmitted_ &&
            previousClock < impactTime &&
            swingClock_ >= impactTime) {
            impactEmitted_ = true;
            events.strikeImpact = true;
            events.strikePosition = strikePosition_;
        }
        if (swingClock_ >= swingEnd) {
            swingClock_ = -1.0f;
            impactEmitted_ = false;
            strikeHitBody_ = false;
        }
    }
#else
    (void)dt;
    (void)cursorPosition;
#endif
    return events;
}

bool WindowsInteractionController::render(
    OverlayWindow& overlay, Vec2 cursorPosition) {
#if defined(_WIN32)
    if (!slipperMode_) {
        overlay.hide();
        return true;
    }
    SDL_Renderer* renderer = overlay.renderer();
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_NONE);
    // Alpha 1 is visually transparent but keeps the small cursor-following
    // window in the hit-test region so clicks do not reach desktop icons.
    SDL_SetRenderDrawColor(renderer, 0, 0, 0, 1);
    SDL_RenderClear(renderer);
    renderSlipper(renderer, swingClock_);
    return overlay.presentAt(
        static_cast<int>(std::round(
            cursorPosition.x - overlaySize * 0.5f)),
        static_cast<int>(std::round(
            cursorPosition.y - overlaySize * 0.5f)));
#else
    (void)overlay;
    (void)cursorPosition;
    return true;
#endif
}

void WindowsInteractionController::placeBait(Vec2 position) {
    if (!enabled_) return;
    baitPosition_ = position;
    baitActive_ = true;
}

void WindowsInteractionController::clearBait() {
    baitActive_ = false;
}

bool WindowsInteractionController::renderBait(
    OverlayWindow& overlay) {
#if defined(_WIN32)
    if (!baitActive_) {
        overlay.hide();
        return true;
    }
    SDL_Renderer* renderer = overlay.renderer();
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_NONE);
    SDL_SetRenderDrawColor(renderer, 0, 0, 0, 0);
    SDL_RenderClear(renderer);
    renderFood(renderer);
    return overlay.presentAt(
        static_cast<int>(std::round(
            baitPosition_.x - baitOverlaySize * 0.5f)),
        static_cast<int>(std::round(
            baitPosition_.y - baitOverlaySize * 0.5f)));
#else
    (void)overlay;
    return true;
#endif
}
