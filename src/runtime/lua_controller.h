#pragma once

#include "runtime/bug_types.h"
#include "runtime/lua_host.h"
#include "runtime/tagged_random.h"

#include <cstdint>
#include <memory>
#include <string_view>

namespace bug {

struct LuaMotionLimits {
    float maximumSpeed = 4096.0f;
    float maximumTurnRate = 64.0f;
    float maximumAcceleration = 32768.0f;
    float maximumLateralSpeed = 4096.0f;
};

struct LuaControllerConfig {
    // Zero selects Species::body.defaultLength.
    float bodyLength = 0.0f;
    float speedMultiplier = 1.0f;
    bool enableExtendedBehaviors = false;
    LuaMotionLimits motionLimits;
};

class LuaController;

// One module is loaded for one species in the process-wide LuaHost. It owns
// the behavior registry reference, but not the host, FSM module, or Species
// source objects supplied while loading it.
class LuaBehaviorModule {
public:
    static LuaResult<std::unique_ptr<LuaBehaviorModule>> load(
        LuaHost& host, const Species& species,
        const LuaHost::Reference& fsmModule);

    ~LuaBehaviorModule() = default;

    LuaBehaviorModule(const LuaBehaviorModule&) = delete;
    LuaBehaviorModule& operator=(const LuaBehaviorModule&) = delete;
    LuaBehaviorModule(LuaBehaviorModule&&) = delete;
    LuaBehaviorModule& operator=(LuaBehaviorModule&&) = delete;

    LuaResult<std::unique_ptr<LuaController>> createController(
        const LuaControllerConfig& config,
        std::unique_ptr<TaggedRandom> random) const;
    LuaResult<std::unique_ptr<LuaController>> createController(
        const LuaControllerConfig& config, std::uint32_t seed) const;

    const Species& species() const { return species_; }

private:
    LuaBehaviorModule(
        LuaHost& host, Species species,
        const LuaHost::Reference& fsmModule,
        LuaHost::Reference behaviorModule);

    LuaHost* host_ = nullptr;
    Species species_;
    const LuaHost::Reference* fsmModule_ = nullptr;
    LuaHost::Reference behaviorModule_;
};

// A controller is an isolated Lua behavior instance. Script failures are
// sticky: after quarantine, step() only returns a safe stop and pose() keeps
// the last valid pose. No C++ behavior fallback exists.
class LuaController {
public:
    ~LuaController() = default;

    LuaController(const LuaController&) = delete;
    LuaController& operator=(const LuaController&) = delete;
    LuaController(LuaController&&) = delete;
    LuaController& operator=(LuaController&&) = delete;

    Decision step(const FrameInput& frame);
    Pose pose(const FrameInput& frame);

    bool quarantined() const { return quarantined_; }
    const LuaError* error() const {
        return quarantined_ ? &error_ : nullptr;
    }

    // MotionSolver uses this method so its recovery samples share the exact
    // per-instance RNG stream used by Lua.
    LuaResult<double> drawRandom(
        std::string_view tag, double low, double high);
    const TaggedRandom& taggedRandom() const { return *random_; }

private:
    friend class LuaBehaviorModule;

    LuaController(
        LuaHost& host, Species species,
        LuaControllerConfig config,
        std::shared_ptr<TaggedRandom> random,
        LuaHost::Reference hostApi,
        LuaHost::Reference controller);

    void quarantine(LuaError error);
    void quarantineContract(
        std::string_view operation, std::string message);
    Decision safeStop(const FrameInput& frame) const;

    LuaHost* host_ = nullptr;
    Species species_;
    LuaControllerConfig config_;

    // The host callback captures a weak_ptr. This prevents a Lua module that
    // illicitly retains an old host table from dereferencing a destroyed RNG.
    std::shared_ptr<TaggedRandom> random_;
    LuaHost::Reference hostApi_;
    LuaHost::Reference controller_;

    bool quarantined_ = false;
    bool hasSuccessfulStep_ = false;
    LuaError error_;
    Decision lastDecision_;
    Pose lastPose_;
};

} // namespace bug
