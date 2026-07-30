#pragma once

#include "cockroach_behavior.h"
#include "desktop_icons.h"
#include "math2d.h"
#include "png_loader.h"

#include <SDL.h>

#include <optional>
#include <random>

struct RoachSettings {
    float bodyLength = 165.0f;
    float speedMultiplier = 1.0f;
    bool enableExtendedBehaviors = false;
};

class Cockroach {
public:
    Cockroach(SDL_Rect desktopBounds, int overlaySize, RoachSettings settings);
    Cockroach(SDL_Rect desktopBounds, int overlaySize, RoachSettings settings,
              Vec2 initialPosition,
              std::optional<unsigned int> randomSeed = std::nullopt);

    void update(float dt, Vec2 cursorScreenPosition,
                const std::vector<ScreenObstacle>& obstacles);
    void updateWithInput(float dt, const CockroachBehaviorInput& input,
                         const std::vector<ScreenObstacle>& obstacles);
    void render(SDL_Renderer* renderer, const LoadedTexture& partsTexture);
    void renderAt(SDL_Renderer* renderer, const LoadedTexture& partsTexture,
                  Vec2 canvasCenter);

    Vec2 screenCenter() const { return position_; }
    CockroachBehaviorSnapshot behaviorSnapshot() const;

private:
    SDL_Rect desktop_;
    int overlaySize_ = 0;
    RoachSettings settings_;
    std::mt19937 rng_;
    Vec2 position_;
    Vec2 target_;
    Vec2 pendingFleeDirection_;
    Vec2 feedingBaitPosition_;
    Vec2 obstacleEscapeDirection_;
    float heading_ = 0.0f;
    float desiredHeading_ = 0.0f;
    float speed_ = 0.0f;
    float desiredSpeed_ = 0.0f;
    float stateTimer_ = 0.0f;
    float stateClock_ = 0.0f;
    float gaitClock_ = 0.0f;
    float behaviorClock_ = 0.0f;
    float steeringPhase_ = 0.0f;
    float speedPulsePhase_ = 0.0f;
    float obstacleEscapeTimer_ = 0.0f;
    float blockedMotionTimer_ = 0.0f;
    float edgeDwellTimer_ = 0.0f;
    float recoveryTimer_ = 0.0f;
    float shelterTimer_ = 0.0f;
    float foodRetryTimer_ = 0.0f;
    float threatCooldown_ = 0.0f;
    bool threatLatched_ = false;
    bool groomedDuringRest_ = false;
    bool foodConsumedThisFrame_ = false;
    Vec2 recoveryDirection_;
    CockroachBehaviorState state_ = CockroachBehaviorState::Wander;

    float randomRange(float low, float high);
    void chooseWanderTarget();
    void chooseCornerTarget(
        const std::vector<ScreenObstacle>& obstacles);
    void transitionTo(CockroachBehaviorState state,
                      Vec2 direction = {});
    void chooseRoamingBehavior(float pauseThreshold,
                               float creepThreshold);
    void updateBehavior(
        float dt, const CockroachBehaviorInput& input,
        const std::vector<ScreenObstacle>& obstacles);
};
