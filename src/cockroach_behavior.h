#pragma once

#include "math2d.h"

enum class CockroachBehaviorState {
    Pause,
    Creep,
    Wander,
    Startled,
    Flee
};

struct CockroachBehaviorInput {
    Vec2 cursorScreenPosition;
    Vec2 cursorVelocity;
    bool cursorValid = true;
};

struct CockroachBehaviorSnapshot {
    CockroachBehaviorState state = CockroachBehaviorState::Wander;
    Vec2 position;
    Vec2 target;
    float heading = 0.0f;
    float speed = 0.0f;
    float stateTimeRemaining = 0.0f;
};

const char* cockroachBehaviorStateName(CockroachBehaviorState state);
