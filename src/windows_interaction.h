#pragma once

#include "math2d.h"

#include <SDL.h>

class OverlayWindow;

struct WindowsInteractionEvents {
    bool baitPlacementRequested = false;
    Vec2 baitPosition;
};

class WindowsInteractionController {
public:
    WindowsInteractionController(bool enabled, SDL_Rect workArea);

    WindowsInteractionEvents update(
        float dt, Vec2 cursorPosition);
    bool renderBait(OverlayWindow& overlay);

    bool baitActive() const { return baitActive_; }
    Vec2 baitPosition() const { return baitPosition_; }
    void placeBait(Vec2 position);
    void clearBait();

    static constexpr int baitOverlaySize = 84;

private:
    bool enabled_ = false;
    bool baitWasDown_ = false;
    bool baitActive_ = false;
    Vec2 baitPosition_;
    SDL_Rect workArea_{};
};
