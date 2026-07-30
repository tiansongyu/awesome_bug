#pragma once

#include "math2d.h"

#include <SDL.h>

class OverlayWindow;

struct SlipperInteractionEvents {
    bool strikeStarted = false;
    bool strikeImpact = false;
    bool baitPlacementRequested = false;
    Vec2 strikePosition;
    Vec2 baitPosition;
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
    bool renderBait(OverlayWindow& overlay);

    bool slipperMode() const { return slipperMode_; }
    bool capturesMouse() const { return slipperMode_; }
    bool swingActive() const { return swingClock_ >= 0.0f; }
    void setStrikeHitBody(bool hit) { strikeHitBody_ = hit; }
    bool strikeHitBody() const { return strikeHitBody_; }
    bool baitActive() const { return baitActive_; }
    Vec2 baitPosition() const { return baitPosition_; }
    void placeBait(Vec2 position);
    void clearBait();
    void cancel();

    static constexpr int overlaySize = 240;
    static constexpr int baitOverlaySize = 84;

private:
    bool enabled_ = false;
    bool slipperMode_ = false;
    bool toggleWasDown_ = false;
    bool escapeWasDown_ = false;
    bool baitWasDown_ = false;
    bool leftWasDown_ = false;
    float swingClock_ = -1.0f;
    bool impactEmitted_ = false;
    bool strikeHitBody_ = false;
    Vec2 strikePosition_;
    bool baitActive_ = false;
    Vec2 baitPosition_;
    SDL_Cursor* hiddenCursor_ = nullptr;
    SDL_Cursor* arrowCursor_ = nullptr;

    void setSlipperMode(bool enabled);
};
