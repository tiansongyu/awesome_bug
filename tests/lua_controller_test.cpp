#include "runtime/bug_species.h"
#include "runtime/lua_controller.h"

#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <memory>
#include <string>

namespace {

struct TemporaryScripts {
    std::filesystem::path directory;

    TemporaryScripts() {
        const auto nonce =
            std::chrono::high_resolution_clock::now()
                .time_since_epoch()
                .count();
        directory =
            std::filesystem::temp_directory_path() /
            ("desktop-display-lua-controller-test-" +
             std::to_string(nonce));
        std::filesystem::create_directories(directory);
    }

    ~TemporaryScripts() {
        std::error_code error;
        std::filesystem::remove_all(directory, error);
    }

    std::filesystem::path write(
        const std::string& name,
        const std::string& contents) const {
        const std::filesystem::path path =
            directory / name;
        std::ofstream output(path, std::ios::binary);
        output << contents;
        output.close();
        return path;
    }
};

void fail(bool& failed, const std::string& message) {
    std::cerr << message << '\n';
    failed = true;
}

bool near(
    float left, float right,
    float tolerance = 1.0e-4f) {
    return std::abs(left - right) <= tolerance;
}

bool finite(Vec2 value) {
    return std::isfinite(value.x) &&
           std::isfinite(value.y);
}

std::filesystem::path sourceRoot() {
#ifdef DESKTOP_DISPLAY_SOURCE_DIR
    const std::filesystem::path sourceDirectory(
        DESKTOP_DISPLAY_SOURCE_DIR);
    if (std::filesystem::is_regular_file(
            sourceDirectory / "bugs/runtime/fsm.lua")) {
        return sourceDirectory;
    }
#endif
#ifdef BUG_SOURCE_DIR
    const std::filesystem::path configured(BUG_SOURCE_DIR);
    if (std::filesystem::is_regular_file(
            configured / "bugs/runtime/fsm.lua")) {
        return configured;
    }
#endif
    const std::filesystem::path sourceFile(__FILE__);
    if (sourceFile.is_absolute()) {
        const std::filesystem::path candidate =
            sourceFile.parent_path().parent_path();
        if (std::filesystem::is_regular_file(
                candidate / "bugs/runtime/fsm.lua")) {
            return candidate;
        }
    }
    std::filesystem::path candidate =
        std::filesystem::current_path();
    for (int depth = 0; depth < 6; ++depth) {
        if (std::filesystem::is_regular_file(
                candidate / "bugs/runtime/fsm.lua")) {
            return candidate;
        }
        if (!candidate.has_parent_path()) break;
        candidate = candidate.parent_path();
    }
    return {};
}

bug::FrameInput frameFor(float bodyLength) {
    bug::FrameInput frame;
    frame.dt = 1.0f / 60.0f;
    frame.clock = 1.0;
    frame.body.position = {960.0f, 500.0f};
    frame.body.heading = 0.25f;
    frame.body.speed = 0.0f;
    frame.body.length = bodyLength;
    frame.world = {0.0f, 0.0f, 1920.0f, 1040.0f};
    frame.cursor.valid = false;
    frame.cursor.position = {200.0f, 200.0f};
    frame.cursor.velocity = {0.0f, 0.0f};
    frame.bait.active = false;
    frame.bait.position = {400.0f, 300.0f};
    const Vec2 cornerPositions[4]{
        {90.0f, 90.0f},
        {1830.0f, 90.0f},
        {90.0f, 950.0f},
        {1830.0f, 950.0f},
    };
    for (std::size_t index = 0; index < 4; ++index) {
        frame.corners[index].position =
            cornerPositions[index];
        frame.corners[index].distance =
            length(cornerPositions[index] -
                   frame.body.position);
        frame.corners[index].blocked = false;
    }
    frame.features.singleInstance = true;
    frame.features.extendedBehaviors = false;
    frame.features.bait = true;
    return frame;
}

bug::Species fixtureSpecies(
    std::string id,
    const std::filesystem::path& behaviorFile,
    bool bait = false) {
    bug::Species species;
    species.apiVersion = bug::apiVersion;
    species.id = std::move(id);
    species.name = species.id;
    species.root = behaviorFile.parent_path();
    species.behaviorFile = behaviorFile;
    species.body.defaultLength = 120.0f;
    species.body.overlayScale = 2.0f;
    species.body.colliderHalfWidth = 0.25f;
    species.body.colliderHalfLength = 0.40f;
    species.body.rootPart = "body";
    species.capabilities.bait = bait;
    bug::PartDefinition body;
    body.name = "body";
    body.source = {0, 0, 1, 1};
    species.parts.push_back(std::move(body));
    species.rootPartIndex = 0;
    return species;
}

const char* validControllerBody = R"lua(
local function decision(frame, initial)
    return {
        state = "moving",
        target = { x = frame.body.x + 10.0, y = frame.body.y },
        motion = {
            direction = { x = 0.0, y = -1.0 },
            speed = 80.0,
            turn_rate = 2.0,
            acceleration = 240.0,
            lateral_speed = 0.0,
            recovery_probe_phase = 0.0,
            intentionally_still = false,
            stop_immediately = false,
            cancel_recovery = false,
            allow_edge_rest = false,
            initial_heading = initial,
        },
        events = { consume_bait = false },
    }
end

local function valid_pose()
    return {
        body = { x = 3.0, y = -2.0, rotation = 0.1 },
        parts = {
            body = {
                rotation = 0.2,
                joint_offset = { x = 1.0, y = -1.0 },
            },
        },
    }
end
)lua";

std::string behavior(
    const std::string& controllerCode,
    int version = 1) {
    return std::string(validControllerBody) +
           "\nreturn { api_version = " +
           std::to_string(version) +
           ", new = function(config, host)\n" +
           controllerCode +
           "\nend }\n";
}

bool sameDecision(
    const bug::Decision& left,
    const bug::Decision& right) {
    return left.state == right.state &&
           near(left.target.x, right.target.x) &&
           near(left.target.y, right.target.y) &&
           near(left.motion.direction.x,
                right.motion.direction.x) &&
           near(left.motion.direction.y,
                right.motion.direction.y) &&
           near(left.motion.speed, right.motion.speed) &&
           near(left.motion.turnRate,
                right.motion.turnRate) &&
           near(left.motion.acceleration,
                right.motion.acceleration) &&
           left.motion.intentionallyStill ==
               right.motion.intentionallyStill &&
           left.motion.initialHeadingValid ==
               right.motion.initialHeadingValid &&
           (!left.motion.initialHeadingValid ||
            near(left.motion.initialHeading,
                 right.motion.initialHeading));
}

} // namespace

int main() {
    bool failed = false;
    const std::filesystem::path root = sourceRoot();
    if (root.empty()) {
        std::cerr << "cannot locate source bugs directory\n";
        return 1;
    }

    auto hostResult = LuaHost::create();
    if (!hostResult) {
        std::cerr << hostResult.error().describe() << '\n';
        return 1;
    }
    std::unique_ptr<LuaHost> host =
        hostResult.takeValue();
    auto fsmResult = host->loadFileReturningTable(
        root / "bugs/runtime/fsm.lua");
    if (!fsmResult) {
        std::cerr << fsmResult.error().describe() << '\n';
        return 1;
    }
    LuaHost::Reference fsm = fsmResult.takeValue();

    // The two real species packages must pass the same generic path.
    auto cockroachSpeciesResult =
        bug::loadSpecies(*host, root / "bugs/cockroach");
    auto templateSpeciesResult =
        bug::loadSpecies(*host, root / "bugs/template");
    if (!cockroachSpeciesResult || !templateSpeciesResult) {
        fail(
            failed,
            !cockroachSpeciesResult
                ? cockroachSpeciesResult.error().describe()
                : templateSpeciesResult.error().describe());
        return 1;
    }
    bug::Species cockroachSpecies =
        cockroachSpeciesResult.takeValue();
    bug::Species templateSpecies =
        templateSpeciesResult.takeValue();

    auto cockroachModuleResult =
        bug::LuaBehaviorModule::load(
            *host, cockroachSpecies, fsm);
    auto templateModuleResult =
        bug::LuaBehaviorModule::load(
            *host, templateSpecies, fsm);
    if (!cockroachModuleResult || !templateModuleResult) {
        fail(
            failed,
            !cockroachModuleResult
                ? cockroachModuleResult.error().describe()
                : templateModuleResult.error().describe());
        return 1;
    }
    std::unique_ptr<bug::LuaBehaviorModule> cockroachModule =
        cockroachModuleResult.takeValue();
    std::unique_ptr<bug::LuaBehaviorModule> templateModule =
        templateModuleResult.takeValue();

    bug::LuaControllerConfig cockroachConfig;
    cockroachConfig.bodyLength =
        cockroachSpecies.body.defaultLength;
    cockroachConfig.speedMultiplier = 3.0f;
    auto cockroachResult =
        cockroachModule->createController(
            cockroachConfig, 0xC0FFEEu);
    if (!cockroachResult) {
        fail(failed, cockroachResult.error().describe());
        return 1;
    }
    std::unique_ptr<bug::LuaController> cockroach =
        cockroachResult.takeValue();
    bug::FrameInput cockroachFrame =
        frameFor(cockroachConfig.bodyLength);
    for (int index = 0; index < 600; ++index) {
        const bug::Decision decision =
            cockroach->step(cockroachFrame);
        const bug::Pose pose =
            cockroach->pose(cockroachFrame);
        if (cockroach->quarantined() ||
            decision.state.empty() ||
            !finite(decision.target) ||
            !finite(decision.motion.direction) ||
            decision.motion.speed < 0.0f ||
            pose.parts.size() !=
                cockroachSpecies.parts.size() ||
            !finite(pose.bodyOffset) ||
            !std::isfinite(pose.bodyRotation)) {
            fail(
                failed,
                cockroach->error()
                    ? cockroach->error()->describe()
                    : "real cockroach controller produced invalid data");
            break;
        }
        if ((index == 0) !=
            decision.motion.initialHeadingValid) {
            fail(
                failed,
                "real cockroach initial_heading was not first-step only");
            break;
        }
        cockroachFrame.clock += cockroachFrame.dt;
    }
    if (cockroach->taggedRandom().drawCount() < 8) {
        fail(
            failed,
            "real cockroach did not use tagged host randomness");
    }

    bug::LuaControllerConfig templateConfig;
    templateConfig.bodyLength =
        templateSpecies.body.defaultLength;
    auto firstResult = templateModule->createController(
        templateConfig, 9123u);
    auto secondResult = templateModule->createController(
        templateConfig, 9123u);
    auto referenceResult = templateModule->createController(
        templateConfig, 9123u);
    if (!firstResult || !secondResult || !referenceResult) {
        fail(failed, "template controllers could not be created");
        return 1;
    }
    std::unique_ptr<bug::LuaController> first =
        firstResult.takeValue();
    std::unique_ptr<bug::LuaController> second =
        secondResult.takeValue();
    std::unique_ptr<bug::LuaController> reference =
        referenceResult.takeValue();
    bug::FrameInput templateFrame =
        frameFor(templateConfig.bodyLength);
    for (int index = 0; index < 300; ++index) {
        (void)first->step(templateFrame);
        templateFrame.clock += templateFrame.dt;
    }
    templateFrame = frameFor(templateConfig.bodyLength);
    const bug::Decision secondFirst =
        second->step(templateFrame);
    const bug::Decision referenceFirst =
        reference->step(templateFrame);
    templateFrame.clock += templateFrame.dt;
    const bug::Decision secondSecond =
        second->step(templateFrame);
    const bug::Decision referenceSecond =
        reference->step(templateFrame);
    if (first->quarantined() || second->quarantined() ||
        reference->quarantined() ||
        !sameDecision(secondFirst, referenceFirst) ||
        !sameDecision(secondSecond, referenceSecond)) {
        fail(
            failed,
            "controllers shared mutable per-instance state");
    }

    auto recording = std::make_unique<bug::TaggedRandom>(
        bug::TaggedRandom::recording(77u));
    auto recordingResult =
        templateModule->createController(
            templateConfig, std::move(recording));
    if (!recordingResult) {
        fail(failed, recordingResult.error().describe());
    } else {
        std::unique_ptr<bug::LuaController> controller =
            recordingResult.takeValue();
        auto solverDraw = controller->drawRandom(
            "solver.recovery.duration", 0.46, 0.82);
        const auto& tape =
            controller->taggedRandom().tape();
        if (!solverDraw || tape.size() != 2 ||
            tape[0].tag != "template.heading" ||
            tape[1].tag != "solver.recovery.duration") {
            fail(
                failed,
                "Lua and MotionSolver did not share one tagged RNG stream");
        }
    }

    TemporaryScripts scripts;

    // A behavior cannot mutate the shared read-only FSM module.
    const std::filesystem::path mutationPath = scripts.write(
        "mutate-fsm.lua",
        behavior(R"lua(
host.fsm.create = nil
return {}
)lua"));
    bug::Species mutationSpecies =
        fixtureSpecies("mutate-fsm", mutationPath);
    auto mutationModule =
        bug::LuaBehaviorModule::load(
            *host, mutationSpecies, fsm);
    if (!mutationModule) {
        fail(failed, mutationModule.error().describe());
    } else {
        auto mutationController =
            mutationModule.value()->createController(
                {}, 1u);
        if (mutationController ||
            mutationController.error().code !=
                LuaErrorCode::Runtime ||
            mutationController.error().message.find(
                "read-only") == std::string::npos) {
            fail(failed, "host.fsm was mutable from a behavior");
        }
    }
    auto templateAfterMutation =
        templateModule->createController(
            templateConfig, 2u);
    if (!templateAfterMutation) {
        fail(
            failed,
            "failed FSM mutation damaged another controller");
    }

    // Missing methods and mismatched ABI are startup errors.
    const std::filesystem::path wrongVersionPath = scripts.write(
        "wrong-version.lua",
        behavior(
            "return { step = function() end, "
            "pose = function() end }",
            2));
    bug::Species wrongVersionSpecies =
        fixtureSpecies("wrong-version", wrongVersionPath);
    auto wrongVersion = bug::LuaBehaviorModule::load(
        *host, wrongVersionSpecies, fsm);
    if (wrongVersion ||
        wrongVersion.error().code !=
            LuaErrorCode::Contract) {
        fail(failed, "behavior api_version mismatch was accepted");
    }

    const std::filesystem::path missingPosePath = scripts.write(
        "missing-pose.lua",
        behavior(R"lua(
local self = {}
function self:step(frame)
    return decision(frame)
end
return self
)lua"));
    bug::Species missingPoseSpecies =
        fixtureSpecies("missing-pose", missingPosePath);
    auto missingPoseModule =
        bug::LuaBehaviorModule::load(
            *host, missingPoseSpecies, fsm);
    if (!missingPoseModule) {
        fail(failed, missingPoseModule.error().describe());
    } else {
        auto missingPose =
            missingPoseModule.value()->createController(
                {}, 3u);
        if (missingPose ||
            missingPose.error().code !=
                LuaErrorCode::Contract) {
            fail(
                failed,
                "controller without pose method passed startup");
        }
    }

    // A runtime error quarantines only the failing instance. It must keep its
    // last state/target while issuing a hard stop, and never call Lua again.
    const std::filesystem::path crashPath = scripts.write(
        "crash.lua",
        behavior(R"lua(
local self = { calls = 0 }
function self:step(frame)
    self.calls = self.calls + 1
    if self.calls == 2 then
        error("instance crash")
    end
    return decision(frame, self.calls == 1 and 0.4 or nil)
end
function self:pose()
    return valid_pose()
end
return self
)lua"));
    bug::Species crashSpecies =
        fixtureSpecies("crash", crashPath);
    auto crashModule = bug::LuaBehaviorModule::load(
        *host, crashSpecies, fsm);
    if (!crashModule) {
        fail(failed, crashModule.error().describe());
    } else {
        auto brokenResult =
            crashModule.value()->createController({}, 4u);
        auto healthyResult =
            crashModule.value()->createController({}, 5u);
        if (!brokenResult || !healthyResult) {
            fail(failed, "crash-isolation controllers failed startup");
        } else {
            std::unique_ptr<bug::LuaController> broken =
                brokenResult.takeValue();
            std::unique_ptr<bug::LuaController> healthy =
                healthyResult.takeValue();
            bug::FrameInput frame = frameFor(120.0f);
            const bug::Decision before = broken->step(frame);
            const bug::Pose beforePose = broken->pose(frame);
            const bug::Decision stopped = broken->step(frame);
            const bug::Decision stoppedAgain = broken->step(frame);
            const bug::Decision healthyDecision =
                healthy->step(frame);
            if (!broken->quarantined() ||
                !broken->error() ||
                broken->error()->message.find("instance crash") ==
                    std::string::npos ||
                stopped.state != before.state ||
                !near(stopped.target.x, before.target.x) ||
                stopped.motion.speed != 0.0f ||
                !stopped.motion.stopImmediately ||
                !stopped.motion.cancelRecovery ||
                stopped.consumeBait ||
                !sameDecision(stopped, stoppedAgain) ||
                !near(broken->pose(frame).bodyOffset.x,
                      beforePose.bodyOffset.x) ||
                healthy->quarantined() ||
                healthyDecision.state != "moving") {
                fail(
                    failed,
                    "runtime quarantine was unsafe or crossed instances");
            }
        }
    }

    // Unknown fields and non-finite numbers are contract failures.
    const std::filesystem::path unknownFieldPath = scripts.write(
        "unknown-field.lua",
        behavior(R"lua(
local self = {}
function self:step(frame)
    local result = decision(frame, 0.1)
    result.secret_cpp_switch = true
    return result
end
function self:pose() return valid_pose() end
return self
)lua"));
    bug::Species unknownFieldSpecies =
        fixtureSpecies("unknown-field", unknownFieldPath);
    auto unknownFieldModule =
        bug::LuaBehaviorModule::load(
            *host, unknownFieldSpecies, fsm);
    if (!unknownFieldModule) {
        fail(failed, unknownFieldModule.error().describe());
    } else {
        auto controller =
            unknownFieldModule.value()->createController({}, 6u);
        if (!controller) {
            fail(failed, controller.error().describe());
        } else {
            const bug::Decision stopped =
                controller.value()->step(frameFor(120.0f));
            const LuaError* error =
                controller.value()->error();
            if (!controller.value()->quarantined() ||
                !error ||
                error->code != LuaErrorCode::Contract ||
                error->message.find("unknown field") ==
                    std::string::npos ||
                !stopped.motion.stopImmediately) {
                fail(failed, "unknown step field was accepted");
            }
        }
    }

    const std::filesystem::path nanPath = scripts.write(
        "nan.lua",
        behavior(R"lua(
local self = {}
function self:step(frame)
    local result = decision(frame, 0.1)
    result.motion.speed = 0.0 / 0.0
    return result
end
function self:pose() return valid_pose() end
return self
)lua"));
    bug::Species nanSpecies =
        fixtureSpecies("nan", nanPath);
    auto nanModule = bug::LuaBehaviorModule::load(
        *host, nanSpecies, fsm);
    if (!nanModule) {
        fail(failed, nanModule.error().describe());
    } else {
        auto controller =
            nanModule.value()->createController({}, 7u);
        if (!controller) {
            fail(failed, controller.error().describe());
        } else {
            (void)controller.value()->step(frameFor(120.0f));
            if (!controller.value()->quarantined() ||
                !controller.value()->error() ||
                controller.value()->error()->code !=
                    LuaErrorCode::Contract) {
                fail(failed, "non-finite step number was accepted");
            }
        }
    }

    // initial_heading is legal only in the first successful step.
    const std::filesystem::path repeatedHeadingPath = scripts.write(
        "repeated-heading.lua",
        behavior(R"lua(
local self = {}
function self:step(frame)
    return decision(frame, 0.2)
end
function self:pose() return valid_pose() end
return self
)lua"));
    bug::Species repeatedHeadingSpecies =
        fixtureSpecies("repeated-heading", repeatedHeadingPath);
    auto repeatedHeadingModule =
        bug::LuaBehaviorModule::load(
            *host, repeatedHeadingSpecies, fsm);
    if (!repeatedHeadingModule) {
        fail(failed, repeatedHeadingModule.error().describe());
    } else {
        auto controller =
            repeatedHeadingModule.value()->createController({}, 8u);
        if (!controller) {
            fail(failed, controller.error().describe());
        } else {
            bug::FrameInput frame = frameFor(120.0f);
            const bug::Decision firstDecision =
                controller.value()->step(frame);
            const bug::Decision stopped =
                controller.value()->step(frame);
            const LuaError* error =
                controller.value()->error();
            if (!firstDecision.motion.initialHeadingValid ||
                !controller.value()->quarantined() ||
                !error ||
                error->message.find("first successful step") ==
                    std::string::npos ||
                !stopped.motion.stopImmediately) {
                fail(failed, "repeated initial_heading was accepted");
            }
        }
    }

    // A bad pose keeps the most recent legal pose and names the bad part.
    const std::filesystem::path badPosePath = scripts.write(
        "bad-pose.lua",
        behavior(R"lua(
local self = { poses = 0, first = true }
function self:step(frame)
    local initial
    if self.first then
        initial = 0.0
        self.first = false
    end
    return decision(frame, initial)
end
function self:pose()
    self.poses = self.poses + 1
    if self.poses == 1 then
        return valid_pose()
    end
    return {
        body = { x = 0.0, y = 0.0, rotation = 0.0 },
        parts = {
            ghost_wing = {
                rotation = 0.0,
                joint_offset = { x = 0.0, y = 0.0 },
            },
        },
    }
end
return self
)lua"));
    bug::Species badPoseSpecies =
        fixtureSpecies("bad-pose", badPosePath);
    auto badPoseModule = bug::LuaBehaviorModule::load(
        *host, badPoseSpecies, fsm);
    if (!badPoseModule) {
        fail(failed, badPoseModule.error().describe());
    } else {
        auto controller =
            badPoseModule.value()->createController({}, 9u);
        if (!controller) {
            fail(failed, controller.error().describe());
        } else {
            bug::FrameInput frame = frameFor(120.0f);
            (void)controller.value()->step(frame);
            const bug::Pose valid =
                controller.value()->pose(frame);
            const bug::Pose retained =
                controller.value()->pose(frame);
            const LuaError* error =
                controller.value()->error();
            if (!controller.value()->quarantined() ||
                !error ||
                error->message.find("ghost_wing") ==
                    std::string::npos ||
                !near(valid.bodyOffset.x, 3.0f) ||
                !near(retained.bodyOffset.x,
                      valid.bodyOffset.x) ||
                retained.parts.size() != 1 ||
                !near(retained.parts[0].rotation,
                      valid.parts[0].rotation)) {
                fail(
                    failed,
                    "invalid pose did not retain the last legal pose");
            }
        }
    }

    // Invalid host-side frame values cannot cross the Lua boundary.
    auto invalidFrameResult =
        templateModule->createController(
            templateConfig, 10u);
    if (!invalidFrameResult) {
        fail(failed, invalidFrameResult.error().describe());
    } else {
        std::unique_ptr<bug::LuaController> controller =
            invalidFrameResult.takeValue();
        bug::FrameInput frame =
            frameFor(templateConfig.bodyLength);
        frame.world.width =
            std::numeric_limits<float>::infinity();
        const bug::Decision stopped =
            controller->step(frame);
        if (!controller->quarantined() ||
            !controller->error() ||
            controller->error()->code !=
                LuaErrorCode::Contract ||
            !stopped.motion.stopImmediately) {
            fail(failed, "invalid host frame crossed into Lua");
        }
    }

    return failed ? 1 : 0;
}
