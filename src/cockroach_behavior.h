#pragma once

#include "math2d.h"

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

const char* cockroachBehaviorStateName(CockroachBehaviorState state);
