#pragma once

#include "math2d.h"

enum class CockroachBehaviorState {
    Pause,
    Creep,
    Wander,
    SeekCorner,
    Lurk,
    Groom,
    SlapTargeted,
    Dead,
    Startled,
    Flee
};

struct CockroachBehaviorInput {
    Vec2 cursorScreenPosition;
    Vec2 cursorVelocity;
    bool cursorValid = true;
    bool requestCornerRest = false;
    bool slipperStrikeStarted = false;
    bool slipperImpact = false;
    bool slipperHitBody = false;
    Vec2 slipperPosition;
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
    unsigned int respawnCount = 0;
    bool alive = true;
};

const char* cockroachBehaviorStateName(CockroachBehaviorState state);
