#pragma once

#include "math2d.h"

#include <SDL.h>

class OverlayWindow;

struct SlipperInteractionEvents {
    bool strikeStarted = false;
    bool strikeImpact = false;
    Vec2 strikePosition;
};

class WindowsInteractionController {
public:
    explicit WindowsInteractionController(bool enabled);
    ~WindowsInteractionController();

    WindowsInteractionController(
        const WindowsInteractionController&) = delete;
    WindowsInteractionController& operator=(
        const WindowsInteractionController&) = delete;

    SlipperInteractionEvents update(float dt, Vec2 cursorPosition);
    bool render(OverlayWindow& overlay, Vec2 cursorPosition);

    bool slipperMode() const { return slipperMode_; }
    bool capturesMouse() const { return slipperMode_; }
    bool swingActive() const { return swingClock_ >= 0.0f; }
    void setStrikeHitBody(bool hit) { strikeHitBody_ = hit; }
    bool strikeHitBody() const { return strikeHitBody_; }
    void cancel();

    static constexpr int overlaySize = 240;

private:
    bool enabled_ = false;
    bool slipperMode_ = false;
    bool toggleWasDown_ = false;
    bool escapeWasDown_ = false;
    bool leftWasDown_ = false;
    float swingClock_ = -1.0f;
    bool impactEmitted_ = false;
    bool strikeHitBody_ = false;
    Vec2 strikePosition_;
    SDL_Cursor* hiddenCursor_ = nullptr;
    SDL_Cursor* arrowCursor_ = nullptr;

    void setSlipperMode(bool enabled);
};
