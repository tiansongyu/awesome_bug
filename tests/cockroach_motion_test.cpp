#define SDL_MAIN_HANDLED
#include "cockroach.h"
#include "display_scale.h"

#include <algorithm>
#include <cmath>
#include <iostream>
#include <vector>

namespace {
bool outsideExpanded(Vec2 point, const ScreenObstacle& obstacle,
                     float extent) {
    return point.x < obstacle.x - extent ||
           point.x > obstacle.x + obstacle.width + extent ||
           point.y < obstacle.y - extent ||
           point.y > obstacle.y + obstacle.height + extent;
}
} // namespace

int main() {
    struct ScaleCase {
        int width;
        int height;
        float expectedBodyLength;
    };
    const ScaleCase scaleCases[]{
        {800, 600, 99.0f},
        {1280, 720, 110.0f},
        {1366, 768, 117.0f},
        {1920, 1080, 165.0f},
        {2560, 1440, 220.0f},
        {3440, 1440, 220.0f},
        {3840, 2160, 330.0f},
        {7680, 4320, 330.0f}};
    for (const ScaleCase& scaleCase : scaleCases) {
        const float actual = resolutionScaledBodyLength(
            165.0f, scaleCase.width, scaleCase.height);
        if (actual != scaleCase.expectedBodyLength) {
            std::cerr
                << "resolution scale failed: "
                << scaleCase.width << 'x' << scaleCase.height
                << " expected=" << scaleCase.expectedBodyLength
                << " actual=" << actual << '\n';
            return 1;
        }
    }

    const SDL_Rect desktop{0, 0, 1280, 752};
    const RoachSettings settings{165.0f, 3.0f, 0.67f};
    const std::vector<Vec2> initialPositions{
        {72.0f, 72.0f},
        {1208.0f, 72.0f},
        {72.0f, 680.0f},
        {1208.0f, 680.0f},
        {640.0f, 376.0f}};
    bool failed = false;

    // Place one static or moving icon directly over the torso at every screen
    // region. The pet must leave it promptly, keep moving and never teleport.
    for (std::size_t trial = 0; trial < initialPositions.size(); ++trial) {
        Cockroach roach(desktop, 290, settings, initialPositions[trial]);
        const Vec2 start = roach.screenCenter();
        ScreenObstacle covering{
            start.x - 46.0f, start.y - 38.0f,
            92.0f, 76.0f, trial == initialPositions.size() - 1};
        const std::vector<ScreenObstacle> obstacles{covering};
        const float conservativeExtent =
            settings.bodyLength *
                std::sqrt(0.43f * 0.43f + 0.20f * 0.20f) +
            (covering.moving ? 8.0f : 2.0f);

        Vec2 previous = start;
        float pathLength = 0.0f;
        float maximumStep = 0.0f;
        int stationaryRun = 0;
        int longestStationaryRun = 0;
        int firstClearFrame = -1;
        for (int frame = 0; frame < 900; ++frame) {
            roach.update(
                1.0f / 60.0f, {-10000.0f, -10000.0f}, obstacles);
            const Vec2 current = roach.screenCenter();
            const float step = length(current - previous);
            pathLength += step;
            maximumStep = std::max(maximumStep, step);
            if (step < 0.02f) {
                ++stationaryRun;
                longestStationaryRun =
                    std::max(longestStationaryRun, stationaryRun);
            } else {
                stationaryRun = 0;
            }
            if (firstClearFrame < 0 &&
                outsideExpanded(current, covering, conservativeExtent)) {
                firstClearFrame = frame;
            }
            previous = current;
        }

        if (firstClearFrame < 0 || firstClearFrame > 240 ||
            pathLength < 800.0f || maximumStep > 55.0f ||
            longestStationaryRun > 90) {
            std::cerr
                << "covering obstacle trial failed: trial=" << trial
                << " clearFrame=" << firstClearFrame
                << " path=" << pathLength
                << " maxStep=" << maximumStep
                << " stationary=" << longestStationaryRun << '\n';
            failed = true;
        }
    }

    // Starting at the left edge is allowed, but prolonged edge-hugging is not.
    // Multiple randomized headings guard against a direction-specific trap.
    for (int trial = 0; trial < 20; ++trial) {
        Cockroach roach(
            desktop, 290, settings,
            {70.0f, 100.0f + static_cast<float>(trial) * 26.0f});
        int edgeRun = 0;
        int longestEdgeRun = 0;
        for (int frame = 0; frame < 600; ++frame) {
            roach.update(
                1.0f / 60.0f, {-10000.0f, -10000.0f}, {});
            if (roach.screenCenter().x < 110.0f) {
                ++edgeRun;
                longestEdgeRun =
                    std::max(longestEdgeRun, edgeRun);
            } else {
                edgeRun = 0;
            }
        }
        if (longestEdgeRun > 150) {
            std::cerr
                << "edge dwell trial failed: trial=" << trial
                << " longestEdgeRun=" << longestEdgeRun << '\n';
            failed = true;
        }
    }

    // Long runs should contain both sustained fast travel and occasional
    // visible low-speed crawling, not one nearly constant velocity.
    {
        Cockroach roach(
            desktop, 290, settings, {640.0f, 376.0f});
        Vec2 previous = roach.screenCenter();
        int slowMovingFrames = 0;
        int fastMovingFrames = 0;
        for (int frame = 0; frame < 10800; ++frame) {
            roach.update(
                1.0f / 60.0f, {-10000.0f, -10000.0f}, {});
            const Vec2 current = roach.screenCenter();
            const float step = length(current - previous);
            if (step >= 0.35f && step < 3.2f) {
                ++slowMovingFrames;
            }
            if (step > 4.0f) {
                ++fastMovingFrames;
            }
            previous = current;
        }
        if (slowMovingFrames < 100 || fastMovingFrames < 2000) {
            std::cerr
                << "speed variation failed: slowFrames="
                << slowMovingFrames
                << " fastFrames=" << fastMovingFrames << '\n';
            failed = true;
        }
    }

    return failed ? 1 : 0;
}
