#include "cockroach.h"
#include "cockroach_parts.h"

#include <algorithm>
#include <chrono>
#include <cmath>

namespace {
constexpr float pi = 3.14159265358979323846f;

float dot(Vec2 left, Vec2 right) {
    return left.x * right.x + left.y * right.y;
}

Vec2 rotateVector(Vec2 value, float angle) {
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

ExpandedObstacle expandObstacle(const ScreenObstacle& obstacle,
                                float extentX, float extentY,
                                float padding) {
    return {obstacle.x - extentX - padding,
            obstacle.y - extentY - padding,
            obstacle.x + obstacle.width + extentX + padding,
            obstacle.y + obstacle.height + extentY + padding};
}

bool contains(const ExpandedObstacle& obstacle, Vec2 point) {
    return point.x >= obstacle.left && point.x <= obstacle.right &&
           point.y >= obstacle.top && point.y <= obstacle.bottom;
}

Vec2 closestPoint(const ExpandedObstacle& obstacle, Vec2 point) {
    return {clampf(point.x, obstacle.left, obstacle.right),
            clampf(point.y, obstacle.top, obstacle.bottom)};
}

Vec2 bodyCollisionExtents(const RoachSettings& settings, float heading) {
    // Only the rigid torso participates in collisions. Legs and antennae are
    // intentionally excluded so they may visually sweep near or over icons.
    const float halfLength = settings.bodyLength * 0.43f;
    const float halfWidth = settings.bodyLength * 0.20f;
    const float headingSin = std::abs(std::sin(heading));
    const float headingCos = std::abs(std::cos(heading));
    return {
        headingSin * halfLength + headingCos * halfWidth,
        headingCos * halfLength + headingSin * halfWidth};
}

void renderSpritePart(SDL_Renderer* renderer,
                      const LoadedTexture& sheet,
                      const CockroachPartDefinition& part,
                      Vec2 joint, float scale, float angle) {
    const SDL_FRect destination{
        joint.x - part.pivot.x * scale,
        joint.y - part.pivot.y * scale,
        part.source.w * scale,
        part.source.h * scale};
    const SDL_FPoint pivot{
        part.pivot.x * scale,
        part.pivot.y * scale};
    SDL_RenderCopyExF(
        renderer, sheet.texture, &part.source, &destination,
        angle * 180.0 / pi, &pivot, SDL_FLIP_NONE);
}
} // namespace

Cockroach::Cockroach(SDL_Rect desktopBounds, int overlaySize,
                     RoachSettings settings)
    : Cockroach(desktopBounds, overlaySize, settings,
                Vec2{desktopBounds.x + desktopBounds.w * 0.5f,
                     desktopBounds.y + desktopBounds.h * 0.5f}) {}

Cockroach::Cockroach(SDL_Rect desktopBounds, int overlaySize,
                     RoachSettings settings, Vec2 initialPosition)
    : desktop_(desktopBounds),
      overlaySize_(overlaySize),
      settings_(settings),
      rng_(static_cast<unsigned int>(
          std::chrono::high_resolution_clock::now().time_since_epoch().count())) {
    heading_ = randomRange(-pi, pi);
    desiredHeading_ = heading_;
    const Vec2 initialExtents =
        bodyCollisionExtents(settings_, heading_);
    constexpr float initialEdgeGap = 10.0f;
    position_ = {
        clampf(initialPosition.x,
               desktop_.x + initialExtents.x + initialEdgeGap,
               desktop_.x + desktop_.w -
                   initialExtents.x - initialEdgeGap),
        clampf(initialPosition.y,
               desktop_.y + initialExtents.y + initialEdgeGap,
               desktop_.y + desktop_.h -
                   initialExtents.y - initialEdgeGap)};
    behaviorClock_ = randomRange(0.0f, 20.0f);
    steeringPhase_ = randomRange(-pi, pi);
    speedPulsePhase_ = randomRange(-pi, pi);
    enterWander();
}

float Cockroach::randomRange(float low, float high) {
    std::uniform_real_distribution<float> distribution(low, high);
    return distribution(rng_);
}

void Cockroach::chooseWanderTarget() {
    const float halfLength = settings_.bodyLength * 0.43f;
    const float halfWidth = settings_.bodyLength * 0.20f;
    const float rotationSafeExtent =
        std::sqrt(halfLength * halfLength + halfWidth * halfWidth);
    const float marginX = std::min(
        rotationSafeExtent + 18.0f, desktop_.w * 0.45f);
    const float marginY = std::min(
        rotationSafeExtent + 18.0f, desktop_.h * 0.45f);
    target_ = {
        randomRange(desktop_.x + marginX,
                    desktop_.x + desktop_.w - marginX),
        randomRange(desktop_.y + marginY,
                    desktop_.y + desktop_.h - marginY)};
}

void Cockroach::enterWander() {
    state_ = MotionState::Wander;
    stateTimer_ = randomRange(0.95f, 4.20f);
    desiredSpeed_ =
        randomRange(112.0f, 225.0f) * settings_.speedMultiplier;
    chooseWanderTarget();
}

void Cockroach::enterCreep() {
    state_ = MotionState::Creep;
    stateTimer_ = randomRange(0.85f, 2.10f);
    desiredSpeed_ =
        randomRange(30.0f, 62.0f) * settings_.speedMultiplier;
    chooseWanderTarget();
}

void Cockroach::enterPause() {
    state_ = MotionState::Pause;
    stateTimer_ = randomRange(0.045f, 0.24f);
    if (randomRange(0.0f, 1.0f) < 0.07f) {
        stateTimer_ += randomRange(0.25f, 0.55f);
    }
    desiredSpeed_ = 0.0f;
}

void Cockroach::enterStartled(Vec2 awayFromCursor) {
    state_ = MotionState::Startled;
    stateTimer_ = randomRange(0.055f, 0.12f);
    desiredSpeed_ = 0.0f;
    pendingFleeDirection_ = normalized(awayFromCursor);
}

void Cockroach::enterFlee(Vec2 awayFromCursor) {
    state_ = MotionState::Flee;
    stateTimer_ = randomRange(0.72f, 1.35f);
    desiredSpeed_ = randomRange(320.0f, 450.0f) * settings_.speedMultiplier;
    const Vec2 direction = normalized(awayFromCursor);
    target_ = position_ + direction * randomRange(380.0f, 650.0f);
}

void Cockroach::update(
    float dt, Vec2 cursorScreenPosition,
    const std::vector<ScreenObstacle>& obstacles) {
    const Vec2 frameStartPosition = position_;
    behaviorClock_ += dt;
    stateTimer_ -= dt;
    obstacleEscapeTimer_ =
        std::max(0.0f, obstacleEscapeTimer_ - dt);
    recoveryTimer_ = std::max(0.0f, recoveryTimer_ - dt);

    const Vec2 cursorDelta = position_ - cursorScreenPosition;
    const float alarmRadius = settings_.bodyLength * 1.75f;
    if (length(cursorDelta) < alarmRadius &&
        state_ != MotionState::Flee &&
        state_ != MotionState::Startled) {
        enterStartled(cursorDelta);
    }

    if (stateTimer_ <= 0.0f) {
        if (state_ == MotionState::Startled) {
            enterFlee(pendingFleeDirection_);
        } else if (state_ == MotionState::Pause ||
                   state_ == MotionState::Flee ||
                   state_ == MotionState::Creep) {
            enterWander();
        } else {
            const float nextBehavior =
                randomRange(0.0f, 1.0f);
            if (nextBehavior < 0.18f) {
                enterPause();
            } else if (nextBehavior < 0.34f) {
                enterCreep();
            } else {
                enterWander();
            }
        }
    }

    if ((state_ == MotionState::Wander ||
         state_ == MotionState::Creep) &&
        length(target_ - position_) < settings_.bodyLength * 0.48f) {
        if (state_ == MotionState::Creep) {
            enterWander();
        } else {
            const float nextBehavior =
                randomRange(0.0f, 1.0f);
            if (nextBehavior < 0.20f) {
                enterPause();
            } else if (nextBehavior < 0.37f) {
                enterCreep();
            } else {
                enterWander();
            }
        }
    }

    Vec2 direction = normalized(target_ - position_);
    if (state_ == MotionState::Pause ||
        state_ == MotionState::Startled) {
        direction = {std::sin(heading_), -std::cos(heading_)};
    }

    const Vec2 currentForward{
        std::sin(heading_), -std::cos(heading_)};
    // Screen edges and desktop icons use the same torso-only collision hull.
    const Vec2 bodyExtents =
        bodyCollisionExtents(settings_, heading_);
    const float bodyExtentX = bodyExtents.x;
    const float bodyExtentY = bodyExtents.y;

    const float edgeMargin =
        std::max(72.0f, settings_.bodyLength * 0.58f);
    Vec2 edgePush{};
    const float leftDistance =
        position_.x - bodyExtentX - desktop_.x;
    const float rightDistance =
        desktop_.x + desktop_.w - position_.x - bodyExtentX;
    const float topDistance =
        position_.y - bodyExtentY - desktop_.y;
    const float bottomDistance =
        desktop_.y + desktop_.h - position_.y - bodyExtentY;
    const float nearestEdgeDistance = std::min(
        std::min(leftDistance, rightDistance),
        std::min(topDistance, bottomDistance));
    if (nearestEdgeDistance < 22.0f) {
        edgeDwellTimer_ += dt;
    } else {
        edgeDwellTimer_ =
            std::max(0.0f, edgeDwellTimer_ - dt * 3.2f);
    }
    if (leftDistance < edgeMargin) edgePush.x += (edgeMargin - leftDistance) / edgeMargin;
    if (rightDistance < edgeMargin) edgePush.x -= (edgeMargin - rightDistance) / edgeMargin;
    if (topDistance < edgeMargin) edgePush.y += (edgeMargin - topDistance) / edgeMargin;
    if (bottomDistance < edgeMargin) edgePush.y -= (edgeMargin - bottomDistance) / edgeMargin;
    if (length(edgePush) > 0.001f) {
        const Vec2 inward = normalized(edgePush);
        Vec2 tangent{-inward.y, inward.x};
        const float tangentDot =
            tangent.x * currentForward.x +
            tangent.y * currentForward.y;
        if (tangentDot < 0.0f) tangent = tangent * -1.0f;
        direction =
            normalized(direction + inward * 2.35f + tangent * 0.62f);
    }

    const float lookAheadDistance = clampf(
        speed_ * 0.12f + settings_.bodyLength * 0.18f,
        settings_.bodyLength * 0.25f,
        settings_.bodyLength * 0.90f);
    const Vec2 lookAheadPosition =
        position_ + currentForward * lookAheadDistance;

    Vec2 obstacleSteering{};
    float obstacleUrgency = 0.0f;
    float movingObstacleUrgency = 0.0f;
    for (const ScreenObstacle& obstacle : obstacles) {
        const float safetyPadding =
            obstacle.moving ? 10.0f : 4.0f;
        const ExpandedObstacle expanded = expandObstacle(
            obstacle, bodyExtentX, bodyExtentY, safetyPadding);
        const bool alreadyOverlapping = contains(expanded, position_);
        const Vec2 sample =
            alreadyOverlapping ? position_ : lookAheadPosition;
        const Vec2 nearest = closestPoint(expanded, sample);
        Vec2 away = sample - nearest;
        const float distance = length(away);
        const float influenceDistance =
            settings_.bodyLength * (obstacle.moving ? 0.68f : 0.46f);

        float urgency = 0.0f;
        if (alreadyOverlapping || contains(expanded, sample)) {
            const Vec2 obstacleCenter{
                (expanded.left + expanded.right) * 0.5f,
                (expanded.top + expanded.bottom) * 0.5f};
            away = sample - obstacleCenter;
            if (length(away) < 0.001f) {
                away = Vec2{-currentForward.y, currentForward.x};
            }
            if (obstacleEscapeTimer_ <= 0.0f) {
                obstacleEscapeDirection_ = normalized(away);
                obstacleEscapeTimer_ =
                    obstacle.moving ? 0.58f : 0.34f;
            }
            away = obstacleEscapeDirection_;
            urgency = 1.0f;
        } else if (distance < influenceDistance) {
            urgency = 1.0f - distance / influenceDistance;
        } else {
            continue;
        }

        away = normalized(away);
        Vec2 tangent{-away.y, away.x};
        if (dot(tangent, currentForward) < 0.0f) {
            tangent = tangent * -1.0f;
        }
        const float awayWeight =
            obstacle.moving ? 3.45f : 2.55f;
        const float tangentWeight =
            obstacle.moving ? 1.05f : 0.78f;
        obstacleSteering +=
            (away * awayWeight + tangent * tangentWeight) * urgency;
        obstacleUrgency = std::max(obstacleUrgency, urgency);
        if (obstacle.moving) {
            movingObstacleUrgency =
                std::max(movingObstacleUrgency, urgency);
        }
    }

    if (length(obstacleSteering) > 0.001f) {
        const float preserveRandomMotion =
            1.0f - obstacleUrgency * 0.58f;
        direction = normalized(
            direction * preserveRandomMotion + obstacleSteering);
    }
    if (obstacleEscapeTimer_ > 0.0f &&
        length(obstacleEscapeDirection_) > 0.001f) {
        direction = normalized(
            direction * 0.48f + obstacleEscapeDirection_ * 1.75f);
        obstacleUrgency = std::max(obstacleUrgency, 0.72f);
    }
    if (recoveryTimer_ > 0.0f &&
        length(recoveryDirection_) > 0.001f) {
        // A confirmed no-progress condition gets a short, decisive escape
        // heading. Keeping a little of the normal steering avoids a robotic
        // snap while preventing nearby icons from cancelling the exit route.
        direction = normalized(
            direction * 0.16f + recoveryDirection_ * 2.85f);
        obstacleUrgency = std::max(obstacleUrgency, 0.88f);
    }

    if (length(direction) > 0.001f) {
        desiredHeading_ = std::atan2(direction.x, -direction.y);
        if (state_ == MotionState::Wander) {
            desiredHeading_ +=
                std::sin(behaviorClock_ * 1.7f + steeringPhase_) * 0.055f +
                std::sin(behaviorClock_ * 4.1f + steeringPhase_) * 0.018f;
        } else if (state_ == MotionState::Creep) {
            desiredHeading_ +=
                std::sin(behaviorClock_ * 1.05f + steeringPhase_) * 0.082f +
                std::sin(behaviorClock_ * 2.8f + steeringPhase_) * 0.024f;
        } else if (state_ == MotionState::Flee) {
            desiredHeading_ +=
                std::sin(behaviorClock_ * 9.0f + steeringPhase_) * 0.075f;
        }
        desiredHeading_ = wrapAngle(desiredHeading_);
    }
    float turnRate =
        state_ == MotionState::Flee
            ? 8.8f
            : (state_ == MotionState::Creep ? 3.4f : 4.5f);
    if (obstacleUrgency > 0.0f) {
        turnRate = std::max(
            turnRate, 5.8f + obstacleUrgency * 4.8f +
                          movingObstacleUrgency * 1.8f);
    }
    if (recoveryTimer_ > 0.0f) {
        turnRate = std::max(turnRate, 12.5f);
    }
    const float headingError = wrapAngle(desiredHeading_ - heading_);
    heading_ = wrapAngle(
        heading_ + clampf(headingError, -turnRate * dt, turnRate * dt));

    float effectiveDesiredSpeed = desiredSpeed_;
    if (state_ == MotionState::Wander) {
        const float stridePulse =
            0.5f + 0.5f *
                       std::sin(behaviorClock_ * 5.2f + speedPulsePhase_);
        const float paceDrift =
            0.5f + 0.5f *
                       std::sin(behaviorClock_ * 1.35f +
                                speedPulsePhase_ * 0.61f);
        effectiveDesiredSpeed *=
            0.72f + stridePulse * 0.22f + paceDrift * 0.10f;
    } else if (state_ == MotionState::Creep) {
        const float carefulStep =
            0.5f + 0.5f *
                       std::sin(behaviorClock_ * 3.1f + speedPulsePhase_);
        const float hesitation =
            0.5f + 0.5f *
                       std::sin(behaviorClock_ * 0.92f +
                                speedPulsePhase_ * 0.47f);
        effectiveDesiredSpeed *=
            0.58f + carefulStep * 0.26f + hesitation * 0.12f;
    } else if (state_ == MotionState::Flee) {
        const float pulse =
            0.5f + 0.5f *
                       std::sin(behaviorClock_ * 10.5f + speedPulsePhase_);
        effectiveDesiredSpeed *= 0.92f + pulse * 0.08f;
    }
    if (obstacleUrgency > 0.0f) {
        const float minimumEscapeSpeed =
            settings_.speedMultiplier *
            (movingObstacleUrgency > 0.0f ? 112.0f : 78.0f);
        const float retainedSpeed =
            effectiveDesiredSpeed *
            (1.0f - obstacleUrgency * 0.18f);
        effectiveDesiredSpeed =
            std::max(retainedSpeed, minimumEscapeSpeed);
    }
    if (recoveryTimer_ > 0.0f) {
        effectiveDesiredSpeed = std::max(
            effectiveDesiredSpeed,
            settings_.speedMultiplier * 150.0f);
    }
    float acceleration =
        state_ == MotionState::Flee
            ? 1350.0f
            : (state_ == MotionState::Startled
                   ? 1550.0f
                   : (state_ == MotionState::Creep ? 520.0f : 680.0f));
    if (obstacleUrgency > 0.0f) {
        acceleration = std::max(
            acceleration, 980.0f + movingObstacleUrgency * 520.0f);
    }
    const float speedDifference = effectiveDesiredSpeed - speed_;
    speed_ += clampf(speedDifference, -acceleration * dt, acceleration * dt);

    const Vec2 forward{std::sin(heading_), -std::cos(heading_)};
    const Vec2 sideways{std::cos(heading_), std::sin(heading_)};
    const float scuttle =
        (std::sin(gaitClock_ * 2.0f) * 0.82f +
         std::sin(gaitClock_ * 3.0f + 0.7f) * 0.18f) *
        std::min(2.8f, speed_ * 0.0085f);
    const Vec2 intendedDisplacement =
        (forward * speed_ + sideways * scuttle) * dt;

    const Vec2 resolvedExtents =
        bodyCollisionExtents(settings_, heading_);
    const float resolvedExtentX = resolvedExtents.x;
    const float resolvedExtentY = resolvedExtents.y;
    constexpr float screenEdgeGap = 10.0f;
    float minimumX =
        desktop_.x + resolvedExtentX + screenEdgeGap;
    float maximumX =
        desktop_.x + desktop_.w -
        resolvedExtentX - screenEdgeGap;
    float minimumY =
        desktop_.y + resolvedExtentY + screenEdgeGap;
    float maximumY =
        desktop_.y + desktop_.h -
        resolvedExtentY - screenEdgeGap;
    if (minimumX > maximumX) {
        minimumX = maximumX =
            desktop_.x + desktop_.w * 0.5f;
    }
    if (minimumY > maximumY) {
        minimumY = maximumY =
            desktop_.y + desktop_.h * 0.5f;
    }

    const auto clampToScreen =
        [&](Vec2 point) {
        point.x = clampf(point.x, minimumX, maximumX);
        point.y = clampf(point.y, minimumY, maximumY);
        return point;
    };
    const auto collidesWithStaticIcon =
        [&](Vec2 point) {
        for (const ScreenObstacle& obstacle : obstacles) {
            if (obstacle.moving) continue;
            const ExpandedObstacle expanded = expandObstacle(
                obstacle, resolvedExtentX, resolvedExtentY, 2.0f);
            if (contains(expanded, point)) return true;
        }
        return false;
    };

    // Move in short continuous steps. When the direct step is blocked, also
    // test shallow and steep turns in both directions. This preserves
    // continuous collision while letting the roach flow around icon corners
    // instead of waiting for its normal heading to rotate far enough.
    const float movementDistance = length(intendedDisplacement);
    const float maximumSubstep =
        std::max(4.0f, settings_.bodyLength * 0.035f);
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
            rotateVector(movementStep, shallowTurn),
            rotateVector(movementStep, -shallowTurn),
            rotateVector(movementStep, steepTurn),
            rotateVector(movementStep, -steepTurn)};

        Vec2 bestPosition = position_;
        float bestScore = -1.0e9f;
        for (const Vec2 alternative : alternatives) {
            const Vec2 candidate =
                clampToScreen(position_ + alternative);
            const Vec2 actualDelta = candidate - position_;
            const float actualDistance = length(actualDelta);
            if (actualDistance < 0.05f ||
                collidesWithStaticIcon(candidate)) {
                continue;
            }

            const Vec2 actualDirection = normalized(actualDelta);
            float score =
                actualDistance +
                dot(actualDirection, intendedDirection) *
                    maximumSubstep * 0.42f;
            if (recoveryTimer_ > 0.0f) {
                score += dot(actualDirection, recoveryDirection_) *
                         maximumSubstep * 0.72f;
            }
            if (score > bestScore) {
                bestPosition = candidate;
                bestScore = score;
            }
        }
        if (bestScore > -1.0e8f) {
            position_ = bestPosition;
        } else {
            // Every alternate direction is blocked. Repeating the remaining
            // substeps cannot help and would only waste CPU.
            break;
        }
    }

    // Icons can still be dragged or dropped directly over the pet. Escape
    // from that exceptional overlap with one shared per-frame correction
    // budget, so even an edge trap cannot produce a visible teleport.
    float remainingSeparation =
        std::min(12.0f, 420.0f * dt + 1.5f);
    bool overlappedObstacle = false;
    const auto separateFromObstacle =
        [&](const ScreenObstacle& obstacle) {
        if (remainingSeparation <= 0.001f) return false;
        const float separationPadding = obstacle.moving ? 8.0f : 2.0f;
        const ExpandedObstacle expanded = expandObstacle(
            obstacle, resolvedExtentX, resolvedExtentY, separationPadding);
        if (!contains(expanded, position_)) return false;
        overlappedObstacle = true;

        const Vec2 candidates[4]{
            {expanded.left - position_.x - 1.0f, 0.0f},
            {expanded.right - position_.x + 1.0f, 0.0f},
            {0.0f, expanded.top - position_.y - 1.0f},
            {0.0f, expanded.bottom - position_.y + 1.0f}};
        Vec2 selected{};
        float selectedDistance = 1.0e9f;
        float selectedScore = 1.0e9f;
        for (const Vec2 candidate : candidates) {
            const Vec2 destination = position_ + candidate;
            if (destination.x < minimumX || destination.x > maximumX ||
                destination.y < minimumY || destination.y > maximumY) {
                continue;
            }
            const float candidateDistance = length(candidate);
            float score = candidateDistance;
            if (obstacleEscapeTimer_ > 0.0f &&
                candidateDistance > 0.001f) {
                const float directionPreference =
                    std::min(14.0f,
                             settings_.bodyLength * 0.08f);
                score -= dot(normalized(candidate),
                             obstacleEscapeDirection_) *
                         directionPreference;
            }
            if (score < selectedScore) {
                selected = candidate;
                selectedDistance = candidateDistance;
                selectedScore = score;
            }
        }

        if (selectedDistance >= 1.0e8f) {
            // The shortest complete exit can be outside the work area when an
            // icon is dragged over a roach at an edge. Choose a feasible small
            // step instead of pushing outward and losing the whole correction
            // to screen clamping.
            const Vec2 obstacleCenter{
                (expanded.left + expanded.right) * 0.5f,
                (expanded.top + expanded.bottom) * 0.5f};
            const Vec2 workAreaCenter{
                desktop_.x + desktop_.w * 0.5f,
                desktop_.y + desktop_.h * 0.5f};
            Vec2 away = normalized(position_ - obstacleCenter);
            const Vec2 inward =
                normalized(workAreaCenter - position_);
            if (length(away) < 0.001f) away = inward;
            Vec2 preferred =
                normalized(away * 1.25f + inward * 0.85f);
            if (length(preferred) < 0.001f) preferred = inward;

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
                if (length(fallbackDirection) < 0.001f) continue;
                const Vec2 destination = clampToScreen(
                    position_ +
                    fallbackDirection * remainingSeparation);
                const Vec2 actualDelta = destination - position_;
                const float actualDistance = length(actualDelta);
                if (actualDistance < 0.05f) continue;
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
        if (selectedDistance > 0.001f) {
            const Vec2 escapeDirection = normalized(selected);
            const float separationDistance =
                std::min(selectedDistance, remainingSeparation);
            const Vec2 previousPosition = position_;
            position_ = clampToScreen(
                position_ +
                escapeDirection * separationDistance);
            const float actualSeparation =
                length(position_ - previousPosition);
            if (actualSeparation < 0.05f) return false;

            obstacleEscapeDirection_ = escapeDirection;
            obstacleEscapeTimer_ = std::max(
                obstacleEscapeTimer_, obstacle.moving ? 0.42f : 0.22f);
            remainingSeparation -= actualSeparation;
            desiredHeading_ = std::atan2(
                escapeDirection.x, -escapeDirection.y);
            speed_ = std::max(
                speed_, settings_.speedMultiplier *
                            (obstacle.moving ? 96.0f : 66.0f));
            return true;
        }
        return false;
    };

    // A dragged icon gets first use of the correction budget. Static icons are
    // then checked in a single pass; continuous movement already prevents new
    // static overlaps during normal roaming.
    for (const ScreenObstacle& obstacle : obstacles) {
        if (obstacle.moving) {
            separateFromObstacle(obstacle);
        }
    }
    for (const ScreenObstacle& obstacle : obstacles) {
        if (!obstacle.moving) {
            separateFromObstacle(obstacle);
        }
    }

    position_ = clampToScreen(position_);

    // Detect commanded motion that produces almost no real displacement.
    // This catches icon corners, overlapping labels and edge traps without
    // treating the short intentional pause state as a fault.
    const float actualMovement =
        length(position_ - frameStartPosition);
    const bool commandedToMove =
        state_ == MotionState::Creep ||
        state_ == MotionState::Wander ||
        state_ == MotionState::Flee ||
        recoveryTimer_ > 0.0f ||
        obstacleUrgency > 0.25f ||
        overlappedObstacle;
    const bool insufficientProgress =
        movementDistance > 0.55f &&
        (actualMovement < 0.35f ||
         actualMovement < movementDistance * 0.16f);
    if (commandedToMove &&
        (insufficientProgress ||
         (overlappedObstacle && actualMovement < 0.75f))) {
        blockedMotionTimer_ += dt;
    } else {
        blockedMotionTimer_ =
            std::max(0.0f, blockedMotionTimer_ - dt * 2.8f);
    }

    if (blockedMotionTimer_ >= 0.16f ||
        edgeDwellTimer_ >= 0.72f) {
        const auto collidesWithAnyIcon =
            [&](Vec2 point) {
            for (const ScreenObstacle& obstacle : obstacles) {
                const float padding = obstacle.moving ? 8.0f : 2.0f;
                const ExpandedObstacle expanded = expandObstacle(
                    obstacle, resolvedExtentX, resolvedExtentY, padding);
                if (contains(expanded, point)) return true;
            }
            return false;
        };

        const Vec2 workAreaCenter{
            desktop_.x + desktop_.w * 0.5f,
            desktop_.y + desktop_.h * 0.5f};
        Vec2 inward = normalized(workAreaCenter - position_);
        if (length(inward) < 0.001f) {
            inward = currentForward;
        }

        constexpr int directionCount = 24;
        const float probeStep =
            std::max(6.0f, settings_.bodyLength * 0.045f);
        const float probeDistance =
            std::max(160.0f, settings_.bodyLength * 1.45f);
        const bool startsBlocked =
            collidesWithAnyIcon(position_);
        Vec2 bestDirection = inward;
        float bestScore = -1.0e9f;
        const float angleOffset =
            heading_ + steeringPhase_ * 0.13f;

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

                const bool blocked =
                    collidesWithAnyIcon(sample);
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
        recoveryTimer_ = randomRange(0.48f, 0.72f);
        obstacleEscapeDirection_ = recoveryDirection_;
        obstacleEscapeTimer_ = recoveryTimer_;
        state_ = MotionState::Wander;
        stateTimer_ = randomRange(0.72f, 1.15f);
        desiredSpeed_ =
            randomRange(178.0f, 248.0f) *
            settings_.speedMultiplier;
        speed_ = std::max(
            speed_, settings_.speedMultiplier * 92.0f);
        target_ =
            position_ + recoveryDirection_ *
            std::max(260.0f, settings_.bodyLength * 2.1f);
        desiredHeading_ = std::atan2(
            recoveryDirection_.x, -recoveryDirection_.y);
        blockedMotionTimer_ = 0.0f;
        edgeDwellTimer_ = 0.0f;
    }

    const float cyclesPerSecond = 0.8f + speed_ / (settings_.bodyLength * 0.42f);
    gaitClock_ += dt * cyclesPerSecond * 2.0f * pi;
}

void Cockroach::render(SDL_Renderer* renderer,
                       const LoadedTexture& partsTexture) {
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_NONE);
    SDL_SetRenderDrawColor(renderer, 0, 0, 0, 0);
    SDL_RenderClear(renderer);
    SDL_SetRenderDrawBlendMode(renderer, SDL_BLENDMODE_BLEND);
    renderAt(renderer, partsTexture,
             Vec2{overlaySize_ * 0.5f, overlaySize_ * 0.5f});
}

void Cockroach::renderAt(SDL_Renderer* renderer,
                         const LoadedTexture& partsTexture,
                         Vec2 canvasCenter) {
    const float bodyLength = settings_.bodyLength;
    const float normalizedPace =
        std::max(1.0f, settings_.speedMultiplier * 200.0f);
    const float motionAmount =
        clampf(speed_ / normalizedPace, 0.0f, 1.0f);
    const float bob =
        std::sin(gaitClock_ * 2.0f) * 0.55f * motionAmount;
    const float sway =
        (std::sin(gaitClock_) * 1.15f +
         std::sin(gaitClock_ * 2.7f) * 0.20f) *
        motionAmount;
    const float strideRockDegrees =
        (std::sin(gaitClock_ * 2.0f) * 1.05f +
         std::sin(gaitClock_ * 0.55f) * 0.22f) *
        motionAmount;
    const float poseHeading =
        heading_ + strideRockDegrees * pi / 180.0f;
    const Vec2 bodyCenter =
        canvasCenter + rotateLocal({sway, bob}, poseHeading);

    float probingAmount = 0.42f;
    if (state_ == MotionState::Pause || state_ == MotionState::Creep) {
        probingAmount = 1.0f;
    } else if (state_ == MotionState::Startled) {
        probingAmount = 0.22f;
    } else if (state_ == MotionState::Flee) {
        probingAmount = 0.08f;
    }
    const CockroachAnimationPose animation =
        calculateCockroachAnimation(
            gaitClock_, behaviorClock_, bodyLength,
            motionAmount, probingAmount);
    const auto& parts = cockroachPartDefinitions();
    const float spriteScale =
        bodyLength / cockroachBodySourceLength;

    const auto partAt = [&parts](CockroachPart part)
        -> const CockroachPartDefinition& {
        return parts[static_cast<std::size_t>(part)];
    };
    const auto jointFor =
        [&](const CockroachPartDefinition& part,
            const CockroachAppendagePose& pose,
            Vec2 screenOffset) {
            const Vec2 localJoint =
                part.attachment * bodyLength + pose.jointOffset;
            return bodyCenter +
                   rotateLocal(localJoint, poseHeading) +
                   screenOffset;
        };
    const auto renderAppendages =
        [&](Vec2 screenOffset) {
            constexpr std::array<CockroachPart, 6> legParts{
                CockroachPart::LeftFrontLeg,
                CockroachPart::RightFrontLeg,
                CockroachPart::LeftMiddleLeg,
                CockroachPart::RightMiddleLeg,
                CockroachPart::LeftRearLeg,
                CockroachPart::RightRearLeg};
            for (std::size_t index = 0;
                 index < legParts.size(); ++index) {
                const auto& part = partAt(legParts[index]);
                const auto& pose = animation.legs[index];
                renderSpritePart(
                    renderer, partsTexture, part,
                    jointFor(part, pose, screenOffset),
                    spriteScale, poseHeading + pose.rotation);
            }

            constexpr std::array<CockroachPart, 2> antennaParts{
                CockroachPart::LeftAntenna,
                CockroachPart::RightAntenna};
            for (std::size_t index = 0;
                 index < antennaParts.size(); ++index) {
                const auto& part = partAt(antennaParts[index]);
                const auto& pose = animation.antennae[index];
                renderSpritePart(
                    renderer, partsTexture, part,
                    jointFor(part, pose, screenOffset),
                    spriteScale, poseHeading + pose.rotation);
            }
        };

    // Draw all appendages behind the opaque body. The shadow is a separate
    // visual layer; the cockroach pixels themselves always use alpha 255.
    SDL_SetTextureColorMod(partsTexture.texture, 0, 0, 0);
    SDL_SetTextureAlphaMod(partsTexture.texture, 38);
    const Vec2 shadowOffset{3.0f, 5.0f};
    renderAppendages(shadowOffset);
    renderSpritePart(
        renderer, partsTexture, partAt(CockroachPart::Body),
        bodyCenter + shadowOffset, spriteScale, poseHeading);

    SDL_SetTextureAlphaMod(partsTexture.texture, 255);
    SDL_SetTextureColorMod(partsTexture.texture, 190, 190, 190);
    renderAppendages({});
    renderSpritePart(
        renderer, partsTexture, partAt(CockroachPart::Body),
        bodyCenter, spriteScale, poseHeading);
}
