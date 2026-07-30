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
void renderFood(SDL_Renderer* renderer) {
    const float center =
        WindowsInteractionController::baitOverlaySize * 0.5f;
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_BLEND);

    for (int y = -17; y <= 17; ++y) {
        const float normalizedY =
            static_cast<float>(y) / 17.0f;
        const float width =
            std::sqrt(std::max(
                0.0f, 1.0f - normalizedY * normalizedY)) *
            (22.0f +
             std::sin(static_cast<float>(y) * 0.7f) * 2.0f);
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
    bool enabled, SDL_Rect workArea)
    : enabled_(enabled), workArea_(workArea) {}

WindowsInteractionEvents
WindowsInteractionController::update(
    float dt, Vec2 cursorPosition) {
    WindowsInteractionEvents events;
    (void)dt;
#if defined(_WIN32)
    if (!enabled_) return events;

    const bool cursorInside =
        cursorPosition.x >= workArea_.x &&
        cursorPosition.x < workArea_.x + workArea_.w &&
        cursorPosition.y >= workArea_.y &&
        cursorPosition.y < workArea_.y + workArea_.h;
    const bool control =
        (GetAsyncKeyState(VK_CONTROL) & 0x8000) != 0;
    const bool alt =
        (GetAsyncKeyState(VK_MENU) & 0x8000) != 0;
    const bool baitKeyDown =
        (GetAsyncKeyState('F') & 0x8000) != 0;
    if (baitKeyDown && !baitWasDown_ &&
        control && alt && cursorInside) {
        events.baitPlacementRequested = true;
        events.baitPosition = cursorPosition;
    }
    baitWasDown_ = baitKeyDown;
#else
    (void)cursorPosition;
#endif
    return events;
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
