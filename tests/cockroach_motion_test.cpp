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
    const RoachSettings settings{165.0f, 3.0f};
    const std::vector<Vec2> initialPositions{
        {72.0f, 72.0f},
        {1208.0f, 72.0f},
        {72.0f, 680.0f},
        {1208.0f, 680.0f},
        {640.0f, 376.0f}};
    bool failed = false;

    // A fixed seed makes behavior transitions reproducible without exposing
    // mutating test hooks in the production state machine.
    {
        Cockroach first(
            desktop, 290, settings, {640.0f, 376.0f}, 0xC0FFEEu);
        Cockroach second(
            desktop, 290, settings, {640.0f, 376.0f}, 0xC0FFEEu);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        for (int frame = 0; frame < 1200; ++frame) {
            first.updateWithInput(1.0f / 60.0f, input, {});
            second.updateWithInput(1.0f / 60.0f, input, {});
            const auto left = first.behaviorSnapshot();
            const auto right = second.behaviorSnapshot();
            if (left.state != right.state ||
                length(left.position - right.position) > 0.0001f ||
                std::abs(left.heading - right.heading) > 0.0001f ||
                std::abs(left.speed - right.speed) > 0.0001f) {
                std::cerr
                    << "fixed-seed behavior is not deterministic at frame "
                    << frame << '\n';
                failed = true;
                break;
            }
            if (std::string(cockroachBehaviorStateName(left.state)).empty()) {
                std::cerr << "behavior state has no readable name\n";
                failed = true;
                break;
            }
        }
    }

    // Cursor proximity enters the unified Startled -> Flee transition path.
    {
        Cockroach roach(
            desktop, 290, settings, {640.0f, 376.0f}, 12345u);
        CockroachBehaviorInput input;
        input.cursorScreenPosition = {650.0f, 376.0f};
        roach.updateWithInput(1.0f / 60.0f, input, {});
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::Startled) {
            std::cerr << "near cursor did not enter startled state\n";
            failed = true;
        }
        for (int frame = 0; frame < 12; ++frame) {
            roach.updateWithInput(1.0f / 60.0f, input, {});
        }
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::Flee) {
            std::cerr << "startled state did not transition to flee\n";
            failed = true;
        }
    }

    // Extended single-pet behavior approaches a corner, rests without body
    // drift, grooms, and finally resumes roaming.
    {
        const RoachSettings extendedSettings{165.0f, 3.0f, true};
        Cockroach roach(
            desktop, 290, extendedSettings,
            {640.0f, 376.0f}, 90210u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        input.requestCornerRest = true;
        roach.updateWithInput(1.0f / 60.0f, input, {});
        input.requestCornerRest = false;
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::SeekCorner) {
            std::cerr << "corner-rest request was not accepted\n";
            failed = true;
        }

        int reachedLurkFrame = -1;
        for (int frame = 0; frame < 900; ++frame) {
            roach.updateWithInput(1.0f / 60.0f, input, {});
            if (roach.behaviorSnapshot().state ==
                CockroachBehaviorState::Lurk) {
                reachedLurkFrame = frame;
                break;
            }
        }
        if (reachedLurkFrame < 0) {
            std::cerr << "roach did not reach a corner-rest state\n";
            failed = true;
        } else {
            const Vec2 restingPosition = roach.screenCenter();
            for (int frame = 0; frame < 180; ++frame) {
                roach.updateWithInput(1.0f / 60.0f, input, {});
            }
            if (roach.behaviorSnapshot().state !=
                    CockroachBehaviorState::Lurk ||
                length(roach.screenCenter() - restingPosition) > 0.1f) {
                std::cerr << "corner-rest body drifted or ended too early\n";
                failed = true;
            }
        }

        bool groomed = false;
        bool resumed = false;
        for (int frame = 0; frame < 1200; ++frame) {
            const Vec2 before = roach.screenCenter();
            roach.updateWithInput(1.0f / 60.0f, input, {});
            const auto snapshot = roach.behaviorSnapshot();
            if (snapshot.state == CockroachBehaviorState::Groom) {
                groomed = true;
                if (length(roach.screenCenter() - before) > 0.01f) {
                    std::cerr << "grooming moved the body\n";
                    failed = true;
                    break;
                }
            }
            if (groomed &&
                (snapshot.state == CockroachBehaviorState::Wander ||
                 snapshot.state == CockroachBehaviorState::Creep)) {
                resumed = true;
                break;
            }
        }
        if (!groomed || !resumed) {
            std::cerr << "corner-rest grooming cycle did not complete\n";
            failed = true;
        }
    }

    // Extended threat sensing ignores a slow distant cursor, but a fast
    // approach triggers a short freeze followed by a burst away from it.
    {
        const RoachSettings extendedSettings{165.0f, 3.0f, true};
        Cockroach roach(
            desktop, 290, extendedSettings,
            {640.0f, 376.0f}, 777u);
        CockroachBehaviorInput input;
        input.cursorScreenPosition = {940.0f, 376.0f};
        input.cursorVelocity = {-20.0f, 0.0f};
        roach.updateWithInput(1.0f / 60.0f, input, {});
        if (roach.behaviorSnapshot().state ==
            CockroachBehaviorState::Startled) {
            std::cerr << "slow distant cursor caused a false alarm\n";
            failed = true;
        }

        input.cursorVelocity = {-900.0f, 0.0f};
        roach.updateWithInput(1.0f / 60.0f, input, {});
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::Startled) {
            std::cerr << "fast approaching cursor did not startle\n";
            failed = true;
        }

        const Vec2 positionAtAlarm = roach.screenCenter();
        bool enteredFlee = false;
        for (int frame = 0; frame < 12; ++frame) {
            input.cursorVelocity = {};
            roach.updateWithInput(1.0f / 60.0f, input, {});
            if (roach.behaviorSnapshot().state ==
                CockroachBehaviorState::Flee) {
                enteredFlee = true;
            }
        }
        for (int frame = 0; frame < 18; ++frame) {
            roach.updateWithInput(1.0f / 60.0f, input, {});
        }
        const Vec2 awayFromCursor =
            normalized(positionAtAlarm - input.cursorScreenPosition);
        const Vec2 escapeDisplacement =
            roach.screenCenter() - positionAtAlarm;
        const float escapeProjection =
            escapeDisplacement.x * awayFromCursor.x +
            escapeDisplacement.y * awayFromCursor.y;
        if (!enteredFlee || escapeProjection < 20.0f) {
            std::cerr << "startled roach did not burst away from cursor\n";
            failed = true;
        }
    }

    // Keeping the cursor on top of the body must not repeatedly reset the
    // startled timer. It may alarm again only after leaving the hysteresis
    // radius and the cooldown has expired.
    {
        const RoachSettings extendedSettings{165.0f, 3.0f, true};
        Cockroach roach(
            desktop, 290, extendedSettings,
            {640.0f, 376.0f}, 778u);
        CockroachBehaviorInput input;
        input.cursorVelocity = {1000.0f, 0.0f};
        input.cursorScreenPosition = roach.screenCenter();
        int startledEntries = 0;
        CockroachBehaviorState previousState =
            roach.behaviorSnapshot().state;
        for (int frame = 0; frame < 240; ++frame) {
            input.cursorScreenPosition = roach.screenCenter();
            input.cursorVelocity = {};
            roach.updateWithInput(1.0f / 60.0f, input, {});
            const auto state = roach.behaviorSnapshot().state;
            if (state == CockroachBehaviorState::Startled &&
                previousState != CockroachBehaviorState::Startled) {
                ++startledEntries;
            }
            previousState = state;
        }
        if (startledEntries != 1) {
            std::cerr << "stationary nearby cursor retriggered alarm: "
                      << startledEntries << '\n';
            failed = true;
        }
    }

    // Slipper hit testing uses only the rotated torso ellipse. A locked hit
    // freezes the target, becomes a ten-second corpse at impact, and respawns
    // exactly once at a safe screen edge.
    {
        const RoachSettings extendedSettings{165.0f, 3.0f, true};
        Cockroach roach(
            desktop, 290, extendedSettings,
            {640.0f, 376.0f}, 424242u);
        const auto initial = roach.behaviorSnapshot();
        const Vec2 insideBody =
            initial.position +
            rotateLocal(
                {0.0f, extendedSettings.bodyLength * 0.30f},
                initial.heading);
        const Vec2 legOnlyArea =
            initial.position +
            rotateLocal(
                {extendedSettings.bodyLength * 0.31f, 0.0f},
                initial.heading);
        if (!roach.hitTestBody(initial.position) ||
            !roach.hitTestBody(insideBody) ||
            roach.hitTestBody(legOnlyArea)) {
            std::cerr << "body-only slipper hit test failed\n";
            failed = true;
        }

        CockroachBehaviorInput input;
        input.cursorValid = false;
        input.slipperStrikeStarted = true;
        input.slipperHitBody = true;
        input.slipperPosition = initial.position;
        roach.updateWithInput(1.0f / 60.0f, input, {});
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::SlapTargeted) {
            std::cerr << "locked slipper hit did not freeze target\n";
            failed = true;
        }

        input.slipperStrikeStarted = false;
        input.slipperImpact = true;
        roach.updateWithInput(1.0f / 60.0f, input, {});
        input.slipperImpact = false;
        if (roach.behaviorSnapshot().state !=
                CockroachBehaviorState::Dead ||
            roach.behaviorSnapshot().alive) {
            std::cerr << "slipper impact did not kill target\n";
            failed = true;
        }

        const Vec2 corpsePosition = roach.screenCenter();
        for (int halfSecond = 0; halfSecond < 19; ++halfSecond) {
            roach.updateWithInput(0.5f, input, {});
        }
        roach.updateWithInput(0.49f, input, {});
        if (roach.behaviorSnapshot().state !=
                CockroachBehaviorState::Dead ||
            length(roach.screenCenter() - corpsePosition) > 0.001f ||
            roach.behaviorSnapshot().respawnCount != 0) {
            std::cerr << "corpse moved or respawned before ten seconds\n";
            failed = true;
        }

        roach.updateWithInput(0.02f, input, {});
        const auto respawned = roach.behaviorSnapshot();
        if (!respawned.alive ||
            respawned.state == CockroachBehaviorState::Dead ||
            respawned.respawnCount != 1 ||
            length(respawned.position - corpsePosition) < 20.0f ||
            respawned.position.x < desktop.x ||
            respawned.position.x > desktop.x + desktop.w ||
            respawned.position.y < desktop.y ||
            respawned.position.y > desktop.y + desktop.h) {
            std::cerr << "ten-second respawn failed\n";
            failed = true;
        }
    }

    // A slipper miss does not kill the animal; the impact startles it away.
    {
        const RoachSettings extendedSettings{165.0f, 3.0f, true};
        Cockroach roach(
            desktop, 290, extendedSettings,
            {640.0f, 376.0f}, 424243u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        input.slipperImpact = true;
        input.slipperHitBody = false;
        input.slipperPosition = {520.0f, 376.0f};
        roach.updateWithInput(1.0f / 60.0f, input, {});
        if (roach.behaviorSnapshot().state !=
            CockroachBehaviorState::Startled) {
            std::cerr << "missed slipper did not startle target\n";
            failed = true;
        }
    }

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
