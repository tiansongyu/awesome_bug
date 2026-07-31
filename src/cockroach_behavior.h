#pragma once

#include "math2d.h"

#include <cstdint>

enum class CockroachBehaviorState {
    Pause,
    Creep,
    Wander,
    SeekCorner,
    Lurk,
    Groom,
    SeekFood,
    Feeding,
    Startled,
    Flee
};

struct CockroachBehaviorInput {
    Vec2 cursorScreenPosition;
    Vec2 cursorVelocity;
    bool cursorValid = true;
    bool requestCornerRest = false;
    bool baitActive = false;
    Vec2 baitPosition;
};

struct CockroachBehaviorSnapshot {
    CockroachBehaviorState state = CockroachBehaviorState::Wander;
    Vec2 position;
    Vec2 target;
    float heading = 0.0f;
    float speed = 0.0f;
    float stateTimeRemaining = 0.0f;
    float stateElapsed = 0.0f;
    float threatCooldown = 0.0f;
    bool foodConsumed = false;
};

// Temporary characterization surface used while the behavior is migrated to
// Lua. It exposes every persistent value that can affect a later transition
// without adding mutating test hooks to the production controller.
struct CockroachDebugSnapshot {
    CockroachBehaviorSnapshot behavior;
    float desiredHeading = 0.0f;
    float desiredSpeed = 0.0f;
    float stateTimer = 0.0f;
    float gaitClock = 0.0f;
    float behaviorClock = 0.0f;
    float obstacleEscapeTimer = 0.0f;
    float blockedMotionTimer = 0.0f;
    float edgeDwellTimer = 0.0f;
    float recoveryTimer = 0.0f;
    float shelterTimer = 0.0f;
    float foodRetryTimer = 0.0f;
    Vec2 pendingFleeDirection;
    Vec2 feedingBaitPosition;
    Vec2 obstacleEscapeDirection;
    Vec2 recoveryDirection;
    bool threatLatched = false;
    bool groomedDuringRest = false;
    std::uint64_t randomDrawCount = 0;
};

const char* cockroachBehaviorStateName(CockroachBehaviorState state);
