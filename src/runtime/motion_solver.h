#pragma once

#include "desktop_icons.h"
#include "runtime/bug_types.h"

#include <functional>
#include <string_view>
#include <vector>

namespace bug {

struct MotionSolverConfig {
    Rect world;
    float bodyLength = 0.0f;
    float colliderHalfWidth = 0.0f;
    float colliderHalfLength = 0.0f;
    float speedScale = 1.0f;
};

class MotionSolver {
public:
    using RandomRange =
        std::function<float(std::string_view, float, float)>;

    MotionSolver(MotionSolverConfig config, Vec2 initialPosition,
                 float initialHeading, RandomRange randomRange = {});

    MotionFeedback step(
        float dt, const MotionIntent& intent,
        const std::vector<ScreenObstacle>& obstacles);

    BodyState body() const;
    CornerSensor corner(std::size_t index,
                        const std::vector<ScreenObstacle>& obstacles) const;
    ObstacleSensor sensors(
        const std::vector<ScreenObstacle>& obstacles,
        const BaitInput& bait);

    Vec2 obstacleEscapeDirection() const {
        return obstacleEscapeDirection_;
    }
    float obstacleEscapeTime() const { return obstacleEscapeTime_; }
    const MotionFeedback& feedback() const { return feedback_; }

private:
    MotionSolverConfig config_;
    RandomRange randomRange_;
    Vec2 position_;
    Vec2 obstacleEscapeDirection_;
    Vec2 recoveryDirection_;
    float heading_ = 0.0f;
    float speed_ = 0.0f;
    float desiredHeading_ = 0.0f;
    float obstacleEscapeTime_ = 0.0f;
    float recoveryTime_ = 0.0f;
    float blockedMotionTime_ = 0.0f;
    float edgeDwellTime_ = 0.0f;
    std::uint64_t stepCount_ = 0;
    MotionFeedback feedback_;

    Vec2 collisionExtents(float heading) const;
    Vec2 clampToWorld(Vec2 point, float heading) const;
    float random(std::string_view tag, float low, float high);
};

} // namespace bug
