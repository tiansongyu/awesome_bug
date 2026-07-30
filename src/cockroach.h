#pragma once

#include "desktop_icons.h"
#include "math2d.h"
#include "png_loader.h"

#include <SDL.h>

#include <random>

struct RoachSettings {
    float bodyLength = 165.0f;
    float speedMultiplier = 1.0f;
};

class Cockroach {
public:
    Cockroach(SDL_Rect desktopBounds, int overlaySize, RoachSettings settings);
    Cockroach(SDL_Rect desktopBounds, int overlaySize, RoachSettings settings,
              Vec2 initialPosition);

    void update(float dt, Vec2 cursorScreenPosition,
                const std::vector<ScreenObstacle>& obstacles);
    void render(SDL_Renderer* renderer, const LoadedTexture& partsTexture);
    void renderAt(SDL_Renderer* renderer, const LoadedTexture& partsTexture,
                  Vec2 canvasCenter);

    Vec2 screenCenter() const { return position_; }

private:
    enum class MotionState { Pause, Creep, Wander, Startled, Flee };

    SDL_Rect desktop_;
    int overlaySize_ = 0;
    RoachSettings settings_;
    std::mt19937 rng_;
    Vec2 position_;
    Vec2 target_;
    Vec2 pendingFleeDirection_;
    Vec2 obstacleEscapeDirection_;
    float heading_ = 0.0f;
    float desiredHeading_ = 0.0f;
    float speed_ = 0.0f;
    float desiredSpeed_ = 0.0f;
    float stateTimer_ = 0.0f;
    float gaitClock_ = 0.0f;
    float behaviorClock_ = 0.0f;
    float steeringPhase_ = 0.0f;
    float speedPulsePhase_ = 0.0f;
    float obstacleEscapeTimer_ = 0.0f;
    float blockedMotionTimer_ = 0.0f;
    float edgeDwellTimer_ = 0.0f;
    float recoveryTimer_ = 0.0f;
    Vec2 recoveryDirection_;
    MotionState state_ = MotionState::Wander;

    float randomRange(float low, float high);
    void chooseWanderTarget();
    void enterWander();
    void enterCreep();
    void enterPause();
    void enterStartled(Vec2 awayFromCursor);
    void enterFlee(Vec2 awayFromCursor);

};
