#pragma once

#include "math2d.h"

#include <SDL.h>

#include <array>
#include <cstddef>

constexpr int cockroachSheetWidth = 1536;
constexpr int cockroachSheetHeight = 1024;
constexpr float cockroachBodySourceLength = 799.0f;

enum class CockroachPart : std::size_t {
    Body,
    LeftFrontLeg,
    RightFrontLeg,
    LeftMiddleLeg,
    RightMiddleLeg,
    LeftRearLeg,
    RightRearLeg,
    LeftAntenna,
    RightAntenna,
    Count
};

enum class CockroachLeg : std::size_t {
    LeftFront,
    RightFront,
    LeftMiddle,
    RightMiddle,
    LeftRear,
    RightRear,
    Count
};

enum class CockroachAntenna : std::size_t {
    Left,
    Right,
    Count
};

struct CockroachPartDefinition {
    SDL_Rect source;
    SDL_FPoint pivot;
    // Joint position relative to the torso center, in body-length units.
    Vec2 attachment;
};

struct CockroachAppendagePose {
    float rotation = 0.0f;
    Vec2 jointOffset;
};

struct CockroachAnimationPose {
    std::array<CockroachAppendagePose,
               static_cast<std::size_t>(CockroachLeg::Count)>
        legs;
    std::array<CockroachAppendagePose,
               static_cast<std::size_t>(CockroachAntenna::Count)>
        antennae;
};

enum class CockroachAnimationMode {
    Normal,
    Lurking,
    Grooming,
    Dead
};

const std::array<CockroachPartDefinition,
                 static_cast<std::size_t>(CockroachPart::Count)>&
cockroachPartDefinitions();

CockroachAnimationPose calculateCockroachAnimation(
    float gaitClock, float behaviorClock, float bodyLength,
    float motionAmount, float probingAmount,
    CockroachAnimationMode mode = CockroachAnimationMode::Normal,
    float actionClock = 0.0f);
