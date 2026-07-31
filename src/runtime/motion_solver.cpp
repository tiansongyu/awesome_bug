#include "runtime/motion_solver.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <limits>
#include <utility>

namespace bug {
namespace {
constexpr float pi = 3.14159265358979323846f;
constexpr float screenEdgeGap = 10.0f;

float dot(Vec2 left, Vec2 right) {
    return left.x * right.x + left.y * right.y;
}

Vec2 rotate(Vec2 value, float angle) {
    const float cosine = std::cos(angle);
    const float sine = std::sin(angle);
    return {
        value.x * cosine - value.y * sine,
        value.x * sine + value.y * cosine};
}

struct ExpandedObstacle {
    float left = 0.0f;
    float top = 0.0f;
    float right = 0.0f;
    float bottom = 0.0f;
};

ExpandedObstacle expand(const ScreenObstacle& obstacle, Vec2 extents,
                        float padding) {
    return {
        obstacle.x - extents.x - padding,
        obstacle.y - extents.y - padding,
        obstacle.x + obstacle.width + extents.x + padding,
        obstacle.y + obstacle.height + extents.y + padding};
}

bool contains(const ExpandedObstacle& obstacle, Vec2 point) {
    return point.x >= obstacle.left && point.x <= obstacle.right &&
           point.y >= obstacle.top && point.y <= obstacle.bottom;
}

Vec2 closestPoint(const ExpandedObstacle& obstacle, Vec2 point) {
    return {
        clampf(point.x, obstacle.left, obstacle.right),
        clampf(point.y, obstacle.top, obstacle.bottom)};
}

} // namespace

MotionSolver::MotionSolver(MotionSolverConfig config, Vec2 initialPosition,
                           float initialHeading, RandomRange randomRange)
    : config_(std::move(config)),
      randomRange_(std::move(randomRange)),
      position_(initialPosition),
      heading_(wrapAngle(initialHeading)),
      desiredHeading_(heading_) {
    config_.speedScale = std::max(0.01f, config_.speedScale);
    position_ = clampToWorld(position_, heading_);
}

float MotionSolver::random(std::string_view tag, float low, float high) {
    if (randomRange_) {
        return clampf(randomRange_(tag, low, high), low, high);
    }
    return (low + high) * 0.5f;
}

Vec2 MotionSolver::collisionExtents(float heading) const {
    const float halfLength =
        config_.bodyLength * config_.colliderHalfLength;
    const float halfWidth =
        config_.bodyLength * config_.colliderHalfWidth;
    const float headingSin = std::abs(std::sin(heading));
    const float headingCos = std::abs(std::cos(heading));
    return {
        headingSin * halfLength + headingCos * halfWidth,
        headingCos * halfLength + headingSin * halfWidth};
}

Vec2 MotionSolver::clampToWorld(Vec2 point, float heading) const {
    const Vec2 extents = collisionExtents(heading);
    float minimumX = config_.world.x + extents.x + screenEdgeGap;
    float maximumX = config_.world.x + config_.world.width -
                     extents.x - screenEdgeGap;
    float minimumY = config_.world.y + extents.y + screenEdgeGap;
    float maximumY = config_.world.y + config_.world.height -
                     extents.y - screenEdgeGap;
    if (minimumX > maximumX) {
        minimumX = maximumX =
            config_.world.x + config_.world.width * 0.5f;
    }
    if (minimumY > maximumY) {
        minimumY = maximumY =
            config_.world.y + config_.world.height * 0.5f;
    }
    return {
        clampf(point.x, minimumX, maximumX),
        clampf(point.y, minimumY, maximumY)};
}

BodyState MotionSolver::body() const {
    return {position_, heading_, speed_, config_.bodyLength};
}

CornerSensor MotionSolver::corner(
    std::size_t index,
    const std::vector<ScreenObstacle>& obstacles) const {
    const float halfLength =
        config_.bodyLength * config_.colliderHalfLength;
    const float halfWidth =
        config_.bodyLength * config_.colliderHalfWidth;
    const float safeExtent =
        std::sqrt(halfLength * halfLength + halfWidth * halfWidth);
    const float margin = safeExtent + 12.0f;
    const std::array<Vec2, 4> corners{{
        {config_.world.x + margin, config_.world.y + margin},
        {config_.world.x + config_.world.width - margin,
         config_.world.y + margin},
        {config_.world.x + margin,
         config_.world.y + config_.world.height - margin},
        {config_.world.x + config_.world.width - margin,
         config_.world.y + config_.world.height - margin},
    }};

    CornerSensor result;
    result.position = corners[index % corners.size()];
    result.distance = length(result.position - position_);
    const Vec2 safeExtents{safeExtent, safeExtent};
    for (const ScreenObstacle& obstacle : obstacles) {
        if (contains(expand(obstacle, safeExtents, 8.0f),
                     result.position)) {
            result.blocked = true;
            break;
        }
    }
    return result;
}

ObstacleSensor MotionSolver::sensors(
    const std::vector<ScreenObstacle>& obstacles,
    const BaitInput& bait) {
    ObstacleSensor result;
    const Vec2 extents = collisionExtents(heading_);
    const Vec2 currentForward{
        std::sin(heading_), -std::cos(heading_)};
    const float lookAheadDistance = clampf(
        speed_ * 0.12f + config_.bodyLength * 0.18f,
        config_.bodyLength * 0.25f,
        config_.bodyLength * 0.90f);
    const Vec2 lookAhead =
        position_ + currentForward * lookAheadDistance;
    Vec2 steering;
    float nearest = std::numeric_limits<float>::max();
    for (const ScreenObstacle& obstacle : obstacles) {
        const ExpandedObstacle collisionArea = expand(
            obstacle, extents, obstacle.moving ? 8.0f : 2.0f);
        if (contains(collisionArea, position_)) {
            result.overlapping = true;
        }
        if (bait.active &&
            contains(collisionArea, bait.position)) {
            result.baitBlocked = true;
        }
        const Vec2 nearestPoint =
            closestPoint(collisionArea, position_);
        const Vec2 away = position_ - nearestPoint;
        const float distance = length(away);
        if (distance < nearest) {
            nearest = distance;
            result.nearestValid = true;
            result.nearestMoving = obstacle.moving;
            result.nearestPoint = nearestPoint;
            result.nearestAway = normalized(away);
            result.nearestDistance = distance;
        }

        const ExpandedObstacle influenceArea = expand(
            obstacle, extents, obstacle.moving ? 10.0f : 4.0f);
        const bool overlapping =
            contains(influenceArea, position_);
        const Vec2 sample = overlapping ? position_ : lookAhead;
        Vec2 avoidance =
            sample - closestPoint(influenceArea, sample);
        const float avoidanceDistance = length(avoidance);
        const float influenceDistance =
            config_.bodyLength * (obstacle.moving ? 0.68f : 0.46f);
        float urgency = 0.0f;
        if (overlapping || contains(influenceArea, sample)) {
            const Vec2 obstacleCenter{
                (influenceArea.left + influenceArea.right) * 0.5f,
                (influenceArea.top + influenceArea.bottom) * 0.5f};
            avoidance = sample - obstacleCenter;
            if (length(avoidance) < 0.001f) {
                avoidance =
                    {-currentForward.y, currentForward.x};
            }
            if (obstacleEscapeTime_ <= 0.0f) {
                obstacleEscapeDirection_ = normalized(avoidance);
                obstacleEscapeTime_ =
                    obstacle.moving ? 0.58f : 0.34f;
            }
            avoidance = obstacleEscapeDirection_;
            urgency = 1.0f;
        } else if (avoidanceDistance < influenceDistance) {
            urgency =
                1.0f - avoidanceDistance / influenceDistance;
        } else {
            continue;
        }

        avoidance = normalized(avoidance);
        Vec2 tangent{-avoidance.y, avoidance.x};
        if (dot(tangent, currentForward) < 0.0f) {
            tangent = tangent * -1.0f;
        }
        steering +=
            (avoidance * (obstacle.moving ? 3.45f : 2.55f) +
             tangent * (obstacle.moving ? 1.05f : 0.78f)) *
            urgency;
        result.obstacleUrgency =
            std::max(result.obstacleUrgency, urgency);
        if (obstacle.moving) {
            result.movingObstacleUrgency =
                std::max(result.movingObstacleUrgency, urgency);
        }
    }
    if (obstacleEscapeTime_ > 0.0f &&
        length(obstacleEscapeDirection_) > 0.001f) {
        steering =
            steering * 0.48f +
            obstacleEscapeDirection_ * 1.75f;
        result.obstacleUrgency =
            std::max(result.obstacleUrgency, 0.72f);
    }
    result.avoidanceDirection = normalized(steering);
    return result;
}

MotionFeedback MotionSolver::step(
    float dt, const MotionIntent& intent,
    const std::vector<ScreenObstacle>& obstacles) {
    dt = clampf(dt, 0.0f, 0.05f);
    if (stepCount_ == 0 && intent.initialHeadingValid) {
        heading_ = wrapAngle(intent.initialHeading);
        desiredHeading_ = heading_;
        position_ = clampToWorld(position_, heading_);
    }
    ++stepCount_;
    if (intent.stopImmediately) {
        speed_ = 0.0f;
    }
    if (intent.cancelRecovery) {
        recoveryTime_ = 0.0f;
        obstacleEscapeTime_ = 0.0f;
    }
    if (intent.intentionallyStill && !intent.cancelRecovery) {
        const Vec2 extents = collisionExtents(heading_);
        bool overlapping = false;
        for (const ScreenObstacle& obstacle : obstacles) {
            if (contains(
                    expand(
                        obstacle, extents,
                        obstacle.moving ? 10.0f : 4.0f),
                    position_)) {
                overlapping = true;
                break;
            }
        }
        if (!overlapping) {
            obstacleEscapeTime_ = 0.0f;
        }
    }

    const Vec2 frameStartPosition = position_;
    obstacleEscapeTime_ = std::max(0.0f, obstacleEscapeTime_ - dt);
    recoveryTime_ = std::max(0.0f, recoveryTime_ - dt);

    const Vec2 currentForward{
        std::sin(heading_), -std::cos(heading_)};
    Vec2 direction = normalized(intent.direction);
    if (length(direction) < 0.001f) {
        direction = currentForward;
    }

    const Vec2 bodyExtents = collisionExtents(heading_);
    const float leftDistance =
        position_.x - bodyExtents.x - config_.world.x;
    const float rightDistance =
        config_.world.x + config_.world.width -
        position_.x - bodyExtents.x;
    const float topDistance =
        position_.y - bodyExtents.y - config_.world.y;
    const float bottomDistance =
        config_.world.y + config_.world.height -
        position_.y - bodyExtents.y;
    const float nearestEdgeDistance = std::min(
        std::min(leftDistance, rightDistance),
        std::min(topDistance, bottomDistance));
    if (!intent.allowEdgeRest && nearestEdgeDistance < 22.0f) {
        edgeDwellTime_ += dt;
    } else {
        edgeDwellTime_ =
            std::max(0.0f, edgeDwellTime_ - dt * 3.2f);
    }

    if (length(direction) > 0.001f) {
        desiredHeading_ =
            wrapAngle(std::atan2(direction.x, -direction.y));
    }
    const float headingError =
        wrapAngle(desiredHeading_ - heading_);
    heading_ = wrapAngle(
        heading_ + clampf(
            headingError,
            -intent.turnRate * dt,
            intent.turnRate * dt));

    const float speedDifference = intent.speed - speed_;
    speed_ += clampf(
        speedDifference,
        -intent.acceleration * dt,
        intent.acceleration * dt);

    const Vec2 forward{std::sin(heading_), -std::cos(heading_)};
    const Vec2 sideways{std::cos(heading_), std::sin(heading_)};
    const Vec2 intendedDisplacement =
        (forward * speed_ + sideways * intent.lateralSpeed) * dt;

    const Vec2 resolvedExtents = collisionExtents(heading_);
    float minimumX =
        config_.world.x + resolvedExtents.x + screenEdgeGap;
    float maximumX =
        config_.world.x + config_.world.width -
        resolvedExtents.x - screenEdgeGap;
    float minimumY =
        config_.world.y + resolvedExtents.y + screenEdgeGap;
    float maximumY =
        config_.world.y + config_.world.height -
        resolvedExtents.y - screenEdgeGap;
    if (minimumX > maximumX) {
        minimumX = maximumX =
            config_.world.x + config_.world.width * 0.5f;
    }
    if (minimumY > maximumY) {
        minimumY = maximumY =
            config_.world.y + config_.world.height * 0.5f;
    }
    const auto clampResolved =
        [&](Vec2 point) {
            point.x = clampf(point.x, minimumX, maximumX);
            point.y = clampf(point.y, minimumY, maximumY);
            return point;
        };
    const auto collidesWithStatic =
        [&](Vec2 point) {
            for (const ScreenObstacle& obstacle : obstacles) {
                if (obstacle.moving) {
                    continue;
                }
                if (contains(expand(obstacle, resolvedExtents, 2.0f),
                             point)) {
                    return true;
                }
            }
            return false;
        };

    const float movementDistance = length(intendedDisplacement);
    const float maximumSubstep =
        std::max(4.0f, config_.bodyLength * 0.035f);
    const int movementSteps = std::max(
        1, static_cast<int>(
               std::ceil(movementDistance / maximumSubstep)));
    const Vec2 movementStep =
        intendedDisplacement *
        (1.0f / static_cast<float>(movementSteps));
    const Vec2 intendedDirection = normalized(movementStep);
    for (int step = 0; step < movementSteps; ++step) {
        constexpr float shallowTurn = 35.0f * pi / 180.0f;
        constexpr float steepTurn = 70.0f * pi / 180.0f;
        const Vec2 alternatives[]{
            movementStep,
            {movementStep.x, 0.0f},
            {0.0f, movementStep.y},
            rotate(movementStep, shallowTurn),
            rotate(movementStep, -shallowTurn),
            rotate(movementStep, steepTurn),
            rotate(movementStep, -steepTurn)};

        Vec2 bestPosition = position_;
        float bestScore = -1.0e9f;
        for (const Vec2 alternative : alternatives) {
            const Vec2 candidate =
                clampResolved(position_ + alternative);
            const Vec2 actualDelta = candidate - position_;
            const float actualDistance = length(actualDelta);
            if (actualDistance < 0.05f ||
                collidesWithStatic(candidate)) {
                continue;
            }
            const Vec2 actualDirection = normalized(actualDelta);
            float score =
                actualDistance +
                dot(actualDirection, intendedDirection) *
                    maximumSubstep * 0.42f;
            if (recoveryTime_ > 0.0f) {
                score +=
                    dot(actualDirection, recoveryDirection_) *
                    maximumSubstep * 0.72f;
            }
            if (score > bestScore) {
                bestPosition = candidate;
                bestScore = score;
            }
        }
        if (bestScore <= -1.0e8f) {
            break;
        }
        position_ = bestPosition;
    }

    float remainingSeparation =
        std::min(12.0f, 420.0f * dt + 1.5f);
    bool overlappedObstacle = false;
    const auto separateFromObstacle =
        [&](const ScreenObstacle& obstacle) {
            if (remainingSeparation <= 0.001f) {
                return false;
            }
            const float padding = obstacle.moving ? 8.0f : 2.0f;
            const ExpandedObstacle expanded =
                expand(obstacle, resolvedExtents, padding);
            if (!contains(expanded, position_)) {
                return false;
            }
            overlappedObstacle = true;

            const Vec2 candidates[4]{
                {expanded.left - position_.x - 1.0f, 0.0f},
                {expanded.right - position_.x + 1.0f, 0.0f},
                {0.0f, expanded.top - position_.y - 1.0f},
                {0.0f, expanded.bottom - position_.y + 1.0f}};
            Vec2 selected;
            float selectedDistance = 1.0e9f;
            float selectedScore = 1.0e9f;
            for (const Vec2 candidate : candidates) {
                const Vec2 destination = position_ + candidate;
                if (destination.x < minimumX ||
                    destination.x > maximumX ||
                    destination.y < minimumY ||
                    destination.y > maximumY) {
                    continue;
                }
                const float candidateDistance = length(candidate);
                float score = candidateDistance;
                if (obstacleEscapeTime_ > 0.0f &&
                    candidateDistance > 0.001f) {
                    score -=
                        dot(normalized(candidate),
                            obstacleEscapeDirection_) *
                        std::min(14.0f, config_.bodyLength * 0.08f);
                }
                if (score < selectedScore) {
                    selected = candidate;
                    selectedDistance = candidateDistance;
                    selectedScore = score;
                }
            }

            if (selectedDistance >= 1.0e8f) {
                const Vec2 obstacleCenter{
                    (expanded.left + expanded.right) * 0.5f,
                    (expanded.top + expanded.bottom) * 0.5f};
                const Vec2 worldCenter{
                    config_.world.x + config_.world.width * 0.5f,
                    config_.world.y + config_.world.height * 0.5f};
                Vec2 away = normalized(position_ - obstacleCenter);
                const Vec2 inward = normalized(worldCenter - position_);
                if (length(away) < 0.001f) {
                    away = inward;
                }
                Vec2 preferred =
                    normalized(away * 1.25f + inward * 0.85f);
                if (length(preferred) < 0.001f) {
                    preferred = inward;
                }
                const Vec2 fallbackDirections[]{
                    preferred,
                    inward,
                    {1.0f, 0.0f},
                    {-1.0f, 0.0f},
                    {0.0f, 1.0f},
                    {0.0f, -1.0f}};
                float bestFallbackScore = -1.0e9f;
                for (Vec2 fallbackDirection : fallbackDirections) {
                    fallbackDirection = normalized(fallbackDirection);
                    if (length(fallbackDirection) < 0.001f) {
                        continue;
                    }
                    const Vec2 destination = clampResolved(
                        position_ +
                        fallbackDirection * remainingSeparation);
                    const Vec2 actualDelta = destination - position_;
                    const float actualDistance = length(actualDelta);
                    if (actualDistance < 0.05f) {
                        continue;
                    }
                    const Vec2 actualDirection =
                        normalized(actualDelta);
                    const float score =
                        actualDistance +
                        dot(actualDirection, away) * 3.0f +
                        dot(actualDirection, inward) * 2.0f;
                    if (score > bestFallbackScore) {
                        selected = actualDelta;
                        selectedDistance = actualDistance;
                        bestFallbackScore = score;
                    }
                }
            }

            if (selectedDistance <= 0.001f) {
                return false;
            }
            const Vec2 escapeDirection = normalized(selected);
            const float separationDistance =
                std::min(selectedDistance, remainingSeparation);
            const Vec2 previousPosition = position_;
            position_ = clampResolved(
                position_ + escapeDirection * separationDistance);
            const float actualSeparation =
                length(position_ - previousPosition);
            if (actualSeparation < 0.05f) {
                return false;
            }

            obstacleEscapeDirection_ = escapeDirection;
            obstacleEscapeTime_ = std::max(
                obstacleEscapeTime_, obstacle.moving ? 0.42f : 0.22f);
            remainingSeparation -= actualSeparation;
            desiredHeading_ =
                std::atan2(escapeDirection.x, -escapeDirection.y);
            speed_ = std::max(
                speed_, config_.speedScale *
                            (obstacle.moving ? 96.0f : 66.0f));
            return true;
        };

    bool separated = false;
    for (const ScreenObstacle& obstacle : obstacles) {
        if (obstacle.moving && separateFromObstacle(obstacle)) {
            separated = true;
            break;
        }
    }
    if (!separated) {
        for (const ScreenObstacle& obstacle : obstacles) {
            if (!obstacle.moving && separateFromObstacle(obstacle)) {
                break;
            }
        }
    }
    position_ = clampResolved(position_);

    const float actualMovement =
        length(position_ - frameStartPosition);
    const bool commandedToMove =
        !intent.intentionallyStill ||
        recoveryTime_ > 0.0f ||
        overlappedObstacle;
    const bool insufficientProgress =
        movementDistance > 0.55f &&
        (actualMovement < 0.35f ||
         actualMovement < movementDistance * 0.16f);
    if (commandedToMove &&
        (insufficientProgress ||
         (overlappedObstacle && actualMovement < 0.75f))) {
        blockedMotionTime_ += dt;
    } else {
        blockedMotionTime_ =
            std::max(0.0f, blockedMotionTime_ - dt * 2.8f);
    }

    if (blockedMotionTime_ >= 0.16f ||
        (!intent.allowEdgeRest && edgeDwellTime_ >= 0.72f)) {
        const auto collidesWithAny =
            [&](Vec2 point) {
                for (const ScreenObstacle& obstacle : obstacles) {
                    const float padding = obstacle.moving ? 8.0f : 2.0f;
                    if (contains(
                            expand(obstacle, resolvedExtents, padding),
                            point)) {
                        return true;
                    }
                }
                return false;
            };

        const Vec2 worldCenter{
            config_.world.x + config_.world.width * 0.5f,
            config_.world.y + config_.world.height * 0.5f};
        Vec2 inward = normalized(worldCenter - position_);
        if (length(inward) < 0.001f) {
            inward = currentForward;
        }

        constexpr int directionCount = 24;
        const float probeStep =
            std::max(6.0f, config_.bodyLength * 0.045f);
        const float probeDistance =
            std::max(160.0f, config_.bodyLength * 1.45f);
        const bool startsBlocked = collidesWithAny(position_);
        Vec2 bestDirection = inward;
        float bestScore = -1.0e9f;
        const float angleOffset =
            heading_ + intent.recoveryProbePhase;

        for (int index = 0; index < directionCount; ++index) {
            const float angle =
                angleOffset +
                2.0f * pi * static_cast<float>(index) /
                    static_cast<float>(directionCount);
            const Vec2 candidateDirection{
                std::cos(angle), std::sin(angle)};
            bool reachedClearSpace = !startsBlocked;
            float blockedPrefix = 0.0f;
            float clearDistance = 0.0f;
            for (float distance = probeStep;
                 distance <= probeDistance;
                 distance += probeStep) {
                const Vec2 sample =
                    position_ + candidateDirection * distance;
                if (sample.x < minimumX || sample.x > maximumX ||
                    sample.y < minimumY || sample.y > maximumY) {
                    break;
                }
                const bool blocked = collidesWithAny(sample);
                if (!reachedClearSpace) {
                    if (blocked) {
                        blockedPrefix += probeStep;
                        continue;
                    }
                    reachedClearSpace = true;
                } else if (blocked) {
                    break;
                }
                clearDistance += probeStep;
            }

            float score =
                clearDistance - blockedPrefix * 0.62f +
                dot(candidateDirection, inward) * 18.0f +
                dot(candidateDirection, currentForward) * 5.0f;
            if (!reachedClearSpace) {
                score -= probeDistance * 1.35f;
            }
            if (score > bestScore) {
                bestScore = score;
                bestDirection = candidateDirection;
            }
        }

        recoveryDirection_ = normalized(bestDirection);
        if (length(recoveryDirection_) < 0.001f) {
            recoveryDirection_ = inward;
        }
        recoveryTime_ =
            random("solver.recovery_duration", 0.48f, 0.72f);
        obstacleEscapeDirection_ = recoveryDirection_;
        obstacleEscapeTime_ = recoveryTime_;
        speed_ = std::max(
            speed_, config_.speedScale * 92.0f);
        desiredHeading_ = std::atan2(
            recoveryDirection_.x, -recoveryDirection_.y);
        blockedMotionTime_ = 0.0f;
        edgeDwellTime_ = 0.0f;
    }

    feedback_.actualDisplacement = position_ - frameStartPosition;
    feedback_.overlapping = overlappedObstacle;
    feedback_.blockedTime = blockedMotionTime_;
    feedback_.edgeDwellTime = edgeDwellTime_;
    feedback_.recoveryDirection = recoveryDirection_;
    feedback_.recoveryTime = recoveryTime_;
    return feedback_;
}

} // namespace bug
