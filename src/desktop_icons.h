#pragma once

#include "math2d.h"

#include <memory>
#include <vector>

struct ScreenObstacle {
    float x = 0.0f;
    float y = 0.0f;
    float width = 0.0f;
    float height = 0.0f;
    bool moving = false;
};

class DesktopIconTracker {
public:
    DesktopIconTracker();
    ~DesktopIconTracker();

    DesktopIconTracker(const DesktopIconTracker&) = delete;
    DesktopIconTracker& operator=(const DesktopIconTracker&) = delete;

    void update(Vec2 cursorScreenPosition, bool allowDrag = true);
    const std::vector<ScreenObstacle>& obstacles() const;

private:
    struct Impl;
    std::unique_ptr<Impl> impl_;
};
