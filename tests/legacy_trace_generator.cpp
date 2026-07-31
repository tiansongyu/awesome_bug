#define SDL_MAIN_HANDLED

#include "cockroach.h"
#include "cockroach_parts.h"

#include <SDL.h>

#include <algorithm>
#include <array>
#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iomanip>
#include <iostream>
#include <iterator>
#include <string>
#include <vector>

namespace {
constexpr float dt = 1.0f / 60.0f;
constexpr float pi = 3.14159265358979323846f;

void writeBehaviorHeader(std::ostream& output) {
    output
        << "scenario\tframe\tstate\tx\ty\ttarget_x\ttarget_y\theading"
        << "\tspeed\tstate_timer\tstate_elapsed\tthreat_cooldown"
        << "\tdesired_heading\tdesired_speed\tgait_clock"
        << "\tbehavior_clock\tescape_timer\tblocked_timer\tedge_timer"
        << "\trecovery_timer\tshelter_timer\tfood_retry"
        << "\tpending_flee_x\tpending_flee_y\tfeeding_bait_x"
        << "\tfeeding_bait_y\tescape_x\tescape_y\trecovery_x"
        << "\trecovery_y\tthreat_latched\tgroomed\tfood_consumed"
        << "\trandom_draws\n";
}

void writeBehaviorRow(std::ostream& output, const char* scenario,
                      int frame, const Cockroach& roach) {
    const CockroachDebugSnapshot value = roach.debugSnapshot();
    const CockroachBehaviorSnapshot& behavior = value.behavior;
    output
        << scenario << '\t' << frame << '\t'
        << cockroachBehaviorStateName(behavior.state) << '\t'
        << behavior.position.x << '\t' << behavior.position.y << '\t'
        << behavior.target.x << '\t' << behavior.target.y << '\t'
        << behavior.heading << '\t' << behavior.speed << '\t'
        << value.stateTimer << '\t' << behavior.stateElapsed << '\t'
        << behavior.threatCooldown << '\t' << value.desiredHeading << '\t'
        << value.desiredSpeed << '\t' << value.gaitClock << '\t'
        << value.behaviorClock << '\t' << value.obstacleEscapeTimer << '\t'
        << value.blockedMotionTimer << '\t' << value.edgeDwellTimer << '\t'
        << value.recoveryTimer << '\t' << value.shelterTimer << '\t'
        << value.foodRetryTimer << '\t'
        << value.pendingFleeDirection.x << '\t'
        << value.pendingFleeDirection.y << '\t'
        << value.feedingBaitPosition.x << '\t'
        << value.feedingBaitPosition.y << '\t'
        << value.obstacleEscapeDirection.x << '\t'
        << value.obstacleEscapeDirection.y << '\t'
        << value.recoveryDirection.x << '\t'
        << value.recoveryDirection.y << '\t'
        << (value.threatLatched ? 1 : 0) << '\t'
        << (value.groomedDuringRest ? 1 : 0) << '\t'
        << (behavior.foodConsumed ? 1 : 0) << '\t'
        << value.randomDrawCount << '\n';
}

void generateBehaviorTrace(const std::filesystem::path& destination) {
    std::ofstream output(destination, std::ios::binary);
    if (!output) {
        throw std::runtime_error("cannot create " + destination.string());
    }
    output << std::fixed << std::setprecision(9);
    writeBehaviorHeader(output);

    const SDL_Rect desktop{0, 0, 1280, 752};
    const RoachSettings basic{165.0f, 3.0f, false};
    const RoachSettings extended{165.0f, 3.0f, true};

    {
        Cockroach roach(
            desktop, 290, basic, {640.0f, 376.0f}, 0xC0FFEEu);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        for (int frame = 0; frame < 1200; ++frame) {
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "roam_basic", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, extended, {640.0f, 376.0f}, 90210u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        for (int frame = 0; frame < 2400; ++frame) {
            input.requestCornerRest = frame == 0;
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "corner_cycle", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, extended, {640.0f, 376.0f}, 90211u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        const std::vector<ScreenObstacle> obstacles{
            {0.0f, 0.0f, 250.0f, 220.0f, false}};
        for (int frame = 0; frame < 1200; ++frame) {
            input.requestCornerRest = frame == 0;
            roach.updateWithInput(dt, input, obstacles);
            writeBehaviorRow(output, "corner_blocked", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, extended, {640.0f, 376.0f}, 778u);
        CockroachBehaviorInput input;
        for (int frame = 0; frame < 600; ++frame) {
            if ((frame >= 60 && frame < 180) || frame >= 360) {
                input.cursorValid = true;
                input.cursorScreenPosition = roach.screenCenter();
                input.cursorVelocity =
                    frame == 60 || frame == 360
                        ? Vec2{1000.0f, 0.0f}
                        : Vec2{};
            } else {
                input.cursorValid = false;
                input.cursorVelocity = {};
            }
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "threat_hysteresis", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, extended, {640.0f, 376.0f}, 515151u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        input.baitActive = true;
        input.baitPosition = {1030.0f, 376.0f};
        for (int frame = 0; frame < 900; ++frame) {
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "food_cycle", frame, roach);
            if (roach.behaviorSnapshot().foodConsumed) {
                input.baitActive = false;
            }
        }
    }

    {
        Cockroach roach(
            desktop, 290, extended, {640.0f, 376.0f}, 515153u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        input.baitActive = true;
        input.baitPosition = roach.screenCenter();
        for (int frame = 0; frame < 420; ++frame) {
            if (frame == 30) {
                input.baitPosition = {1120.0f, 650.0f};
            }
            if (frame == 180) {
                input.cursorValid = true;
                input.cursorScreenPosition =
                    roach.screenCenter() + Vec2{260.0f, 0.0f};
                input.cursorVelocity = {-1000.0f, 0.0f};
            } else if (frame > 180 && frame < 240) {
                input.cursorVelocity = {};
            } else {
                input.cursorValid = false;
                input.cursorVelocity = {};
            }
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "food_move_threat", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, basic, {640.0f, 376.0f}, 424242u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        for (int frame = 0; frame < 900; ++frame) {
            const float dragOffset =
                frame < 180 ? static_cast<float>(frame) * 0.45f : 81.0f;
            const std::vector<ScreenObstacle> obstacles{{
                594.0f + dragOffset,
                338.0f,
                92.0f,
                76.0f,
                frame < 180}};
            roach.updateWithInput(dt, input, obstacles);
            writeBehaviorRow(output, "collision_recovery", frame, roach);
        }
    }

    {
        Cockroach roach(
            desktop, 290, basic, {70.0f, 300.0f}, 31337u);
        CockroachBehaviorInput input;
        input.cursorValid = false;
        for (int frame = 0; frame < 600; ++frame) {
            roach.updateWithInput(dt, input, {});
            writeBehaviorRow(output, "edge_recovery", frame, roach);
        }
    }
}

const char* modeName(CockroachAnimationMode mode) {
    switch (mode) {
    case CockroachAnimationMode::Normal:
        return "normal";
    case CockroachAnimationMode::Lurking:
        return "lurking";
    case CockroachAnimationMode::Grooming:
        return "grooming";
    case CockroachAnimationMode::Feeding:
        return "feeding";
    }
    return "unknown";
}

void generateAnimationTrace(const std::filesystem::path& destination) {
    std::ofstream output(destination, std::ios::binary);
    if (!output) {
        throw std::runtime_error("cannot create " + destination.string());
    }
    output << std::fixed << std::setprecision(9);
    output
        << "case\tmode\tgait_clock\tbehavior_clock\tmotion\tprobing"
        << "\taction\tbody_x\tbody_y\tbody_rotation";
    for (int part = 0; part < 8; ++part) {
        output << "\tp" << part << "_rotation"
               << "\tp" << part << "_x"
               << "\tp" << part << "_y";
    }
    output << '\n';

    struct PoseCase {
        CockroachAnimationMode mode;
        float gaitClock;
        float behaviorClock;
        float motion;
        float probing;
        float actionClock;
    };
    const std::array<PoseCase, 16> cases{{
        {CockroachAnimationMode::Normal, 0.0f, 0.0f, 0.0f, 0.42f, 0.0f},
        {CockroachAnimationMode::Normal, 0.37f, 0.50f, 0.4f, 1.0f, 0.0f},
        {CockroachAnimationMode::Normal, 1.10f, 1.70f, 1.0f, 0.08f, 0.0f},
        {CockroachAnimationMode::Normal, 2.40f, 3.20f, 0.8f, 0.42f, 0.0f},
        {CockroachAnimationMode::Lurking, 0.0f, 0.0f, 0.0f, 1.0f, 0.0f},
        {CockroachAnimationMode::Lurking, 0.9f, 0.5f, 0.0f, 1.0f, 0.5f},
        {CockroachAnimationMode::Lurking, 1.8f, 1.7f, 0.0f, 1.0f, 1.7f},
        {CockroachAnimationMode::Lurking, 2.7f, 3.2f, 0.0f, 1.0f, 3.2f},
        {CockroachAnimationMode::Grooming, 0.0f, 0.0f, 0.0f, 1.0f, 0.0f},
        {CockroachAnimationMode::Grooming, 0.7f, 0.5f, 0.0f, 1.0f, 0.25f},
        {CockroachAnimationMode::Grooming, 1.4f, 1.7f, 0.0f, 1.0f, 0.75f},
        {CockroachAnimationMode::Grooming, 2.1f, 3.2f, 0.0f, 1.0f, 1.50f},
        {CockroachAnimationMode::Feeding, 0.0f, 0.0f, 0.0f, 0.75f, 0.0f},
        {CockroachAnimationMode::Feeding, 0.8f, 0.5f, 0.0f, 0.75f, 0.25f},
        {CockroachAnimationMode::Feeding, 1.6f, 1.7f, 0.0f, 0.75f, 0.75f},
        {CockroachAnimationMode::Feeding, 2.4f, 3.2f, 0.0f, 0.75f, 1.50f},
    }};

    constexpr float bodyLength = 165.0f;
    for (std::size_t index = 0; index < cases.size(); ++index) {
        const PoseCase& sample = cases[index];
        const CockroachAnimationPose pose =
            calculateCockroachAnimation(
                sample.gaitClock, sample.behaviorClock, bodyLength,
                sample.motion, sample.probing, sample.mode,
                sample.actionClock);
        const float bodyX =
            (std::sin(sample.gaitClock) * 1.15f +
             std::sin(sample.gaitClock * 2.7f) * 0.20f) *
            sample.motion;
        const float bodyY =
            std::sin(sample.gaitClock * 2.0f) * 0.55f * sample.motion;
        const float bodyRotation =
            (std::sin(sample.gaitClock * 2.0f) * 1.05f +
             std::sin(sample.gaitClock * 0.55f) * 0.22f) *
            sample.motion * pi / 180.0f;

        output << index << '\t' << modeName(sample.mode) << '\t'
               << sample.gaitClock << '\t' << sample.behaviorClock << '\t'
               << sample.motion << '\t' << sample.probing << '\t'
               << sample.actionClock << '\t' << bodyX << '\t' << bodyY
               << '\t' << bodyRotation;
        for (const CockroachAppendagePose& part : pose.legs) {
            output << '\t' << part.rotation
                   << '\t' << part.jointOffset.x
                   << '\t' << part.jointOffset.y;
        }
        for (const CockroachAppendagePose& part : pose.antennae) {
            output << '\t' << part.rotation
                   << '\t' << part.jointOffset.x
                   << '\t' << part.jointOffset.y;
        }
        output << '\n';
    }
}

bool filesEqual(const std::filesystem::path& left,
                const std::filesystem::path& right) {
    std::ifstream leftStream(left, std::ios::binary);
    std::ifstream rightStream(right, std::ios::binary);
    if (!leftStream || !rightStream ||
        std::filesystem::file_size(left) !=
            std::filesystem::file_size(right)) {
        return false;
    }
    return std::equal(
        std::istreambuf_iterator<char>(leftStream),
        std::istreambuf_iterator<char>(),
        std::istreambuf_iterator<char>(rightStream),
        std::istreambuf_iterator<char>());
}

void generateTraces(const std::filesystem::path& outputDirectory) {
    std::filesystem::create_directories(outputDirectory);
    generateBehaviorTrace(outputDirectory / "behavior.tsv");
    generateAnimationTrace(outputDirectory / "animation.tsv");
}
} // namespace

int main(int argc, char** argv) {
    if (argc != 3 ||
        (std::string(argv[1]) != "--write" &&
         std::string(argv[1]) != "--verify")) {
        std::cerr
            << "usage: legacy_trace_generator "
               "(--write|--verify) GOLDEN_DIRECTORY\n";
        return 2;
    }
    try {
        const std::filesystem::path goldenDirectory(argv[2]);
        if (std::string(argv[1]) == "--write") {
            generateTraces(goldenDirectory);
            return 0;
        }

        const auto unique = std::chrono::high_resolution_clock::now()
                                .time_since_epoch()
                                .count();
        const std::filesystem::path actualDirectory =
            std::filesystem::temp_directory_path() /
            ("cockroach-legacy-golden-" + std::to_string(unique));
        generateTraces(actualDirectory);
        const bool behaviorMatches = filesEqual(
            actualDirectory / "behavior.tsv",
            goldenDirectory / "behavior.tsv");
        const bool animationMatches = filesEqual(
            actualDirectory / "animation.tsv",
            goldenDirectory / "animation.tsv");
        std::filesystem::remove_all(actualDirectory);
        if (!behaviorMatches || !animationMatches) {
            std::cerr
                << "legacy characterization changed:"
                << (behaviorMatches ? "" : " behavior.tsv")
                << (animationMatches ? "" : " animation.tsv")
                << '\n';
            return 1;
        }
    } catch (const std::exception& error) {
        std::cerr << error.what() << '\n';
        return 1;
    }
    return 0;
}
