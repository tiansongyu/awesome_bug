#pragma once

#include "math2d.h"

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <string>
#include <vector>

namespace bug {

constexpr int apiVersion = 1;
constexpr std::size_t maximumParts = 64;

struct Rect {
    float x = 0.0f;
    float y = 0.0f;
    float width = 0.0f;
    float height = 0.0f;
};

struct SourceRect {
    int x = 0;
    int y = 0;
    int width = 0;
    int height = 0;
};

struct PartDefinition {
    std::string name;
    SourceRect source;
    Vec2 pivot;
    Vec2 attachment;
    int layer = 0;
};

struct AtlasDefinition {
    std::filesystem::path file;
    int width = 0;
    int height = 0;
    float referenceLength = 0.0f;
};

struct BodyDefinition {
    float defaultLength = 0.0f;
    float overlayScale = 0.0f;
    float colliderHalfWidth = 0.0f;
    float colliderHalfLength = 0.0f;
    std::string rootPart;
};

struct VisualDefinition {
    std::uint8_t red = 255;
    std::uint8_t green = 255;
    std::uint8_t blue = 255;
    std::uint8_t alpha = 255;
    std::uint8_t shadowAlpha = 0;
    Vec2 shadowOffset;
};

struct Capabilities {
    bool bait = false;
};

struct Species {
    int apiVersion = 0;
    std::string id;
    std::string name;
    std::filesystem::path root;
    std::filesystem::path behaviorFile;
    AtlasDefinition atlas;
    BodyDefinition body;
    VisualDefinition visual;
    Capabilities capabilities;
    std::vector<PartDefinition> parts;
    std::size_t rootPartIndex = 0;
};

struct CursorInput {
    bool valid = false;
    Vec2 position;
    Vec2 velocity;
};

struct BaitInput {
    bool active = false;
    Vec2 position;
};

struct CornerSensor {
    Vec2 position;
    float distance = 0.0f;
    bool blocked = false;
};

struct ObstacleSensor {
    bool overlapping = false;
    bool baitBlocked = false;
    bool nearestValid = false;
    bool nearestMoving = false;
    Vec2 avoidanceDirection;
    float obstacleUrgency = 0.0f;
    float movingObstacleUrgency = 0.0f;
    Vec2 nearestPoint;
    Vec2 nearestAway;
    float nearestDistance = 0.0f;
};

struct MotionFeedback {
    Vec2 actualDisplacement;
    bool overlapping = false;
    float blockedTime = 0.0f;
    float edgeDwellTime = 0.0f;
    Vec2 recoveryDirection;
    float recoveryTime = 0.0f;
};

struct BodyState {
    Vec2 position;
    float heading = 0.0f;
    float speed = 0.0f;
    float length = 0.0f;
};

struct FeatureFlags {
    bool singleInstance = false;
    bool extendedBehaviors = false;
    bool bait = false;
};

struct FrameInput {
    float dt = 0.0f;
    double clock = 0.0;
    BodyState body;
    Rect world;
    CursorInput cursor;
    BaitInput bait;
    CornerSensor corners[4];
    ObstacleSensor sensors;
    MotionFeedback feedback;
    FeatureFlags features;
    bool requestCornerRest = false;
};

struct MotionIntent {
    Vec2 direction;
    float speed = 0.0f;
    float turnRate = 0.0f;
    float acceleration = 0.0f;
    float lateralSpeed = 0.0f;
    float recoveryProbePhase = 0.0f;
    bool intentionallyStill = false;
    bool stopImmediately = false;
    bool cancelRecovery = false;
    bool allowEdgeRest = false;
    bool initialHeadingValid = false;
    float initialHeading = 0.0f;
};

struct Decision {
    std::string state;
    Vec2 target;
    MotionIntent motion;
    bool consumeBait = false;
};

struct PartPose {
    float rotation = 0.0f;
    Vec2 jointOffset;
};

struct Pose {
    Vec2 bodyOffset;
    float bodyRotation = 0.0f;
    std::vector<PartPose> parts;
};

} // namespace bug
