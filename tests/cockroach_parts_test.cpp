#define SDL_MAIN_HANDLED
#include "cockroach_parts.h"

#include <cmath>
#include <iostream>

namespace {
constexpr float pi = 3.14159265358979323846f;

bool insideSheet(const CockroachPartDefinition& part) {
    return part.source.x >= 0 && part.source.y >= 0 &&
           part.source.w > 0 && part.source.h > 0 &&
           part.source.x + part.source.w <= cockroachSheetWidth &&
           part.source.y + part.source.h <= cockroachSheetHeight;
}

bool pivotInsidePart(const CockroachPartDefinition& part) {
    return part.pivot.x >= 0.0f && part.pivot.y >= 0.0f &&
           part.pivot.x <= part.source.w &&
           part.pivot.y <= part.source.h;
}

bool overlaps(const SDL_Rect& left, const SDL_Rect& right) {
    return left.x < right.x + right.w &&
           left.x + left.w > right.x &&
           left.y < right.y + right.h &&
           left.y + left.h > right.y;
}
} // namespace

int main() {
    bool failed = false;
    const auto& parts = cockroachPartDefinitions();
    if (parts.size() != 9) {
        std::cerr << "expected nine independently rendered parts\n";
        failed = true;
    }
    for (std::size_t index = 0; index < parts.size(); ++index) {
        if (!insideSheet(parts[index]) ||
            !pivotInsidePart(parts[index])) {
            std::cerr << "invalid atlas definition for part "
                      << index << '\n';
            failed = true;
        }
        for (std::size_t other = index + 1;
             other < parts.size(); ++other) {
            if (overlaps(parts[index].source,
                         parts[other].source)) {
                std::cerr
                    << "atlas rectangles overlap: "
                    << index << " and " << other << '\n';
                failed = true;
            }
        }
    }

    // At a quarter gait cycle, tripod A must be advancing while tripod B
    // retreats. Half a cycle later their roles must reverse.
    const CockroachAnimationPose first =
        calculateCockroachAnimation(
            pi * 0.5f, 2.0f, 165.0f, 1.0f, 0.5f);
    const CockroachAnimationPose second =
        calculateCockroachAnimation(
            pi * 1.5f, 2.0f, 165.0f, 1.0f, 0.5f);
    constexpr float side[6]{1.0f, -1.0f, 1.0f,
                            -1.0f, 1.0f, -1.0f};
    constexpr bool tripodA[6]{true, false, false,
                              true, true, false};
    bool legsHaveIndividualMotion = false;
    for (std::size_t leg = 0; leg < first.legs.size(); ++leg) {
        const float firstSweep =
            first.legs[leg].rotation * side[leg];
        const float secondSweep =
            second.legs[leg].rotation * side[leg];
        const bool correctFirstDirection =
            tripodA[leg] ? firstSweep > 0.08f
                         : firstSweep < -0.08f;
        const bool correctSecondDirection =
            tripodA[leg] ? secondSweep < -0.08f
                         : secondSweep > 0.08f;
        if (!correctFirstDirection || !correctSecondDirection) {
            std::cerr << "tripod phase failed for leg "
                      << leg << '\n';
            failed = true;
        }
        if (leg > 0 &&
            std::abs(first.legs[leg].rotation -
                     first.legs[leg - 1].rotation) > 0.01f) {
            legsHaveIndividualMotion = true;
        }
    }
    if (!legsHaveIndividualMotion) {
        std::cerr << "all six legs share one rigid animation\n";
        failed = true;
    }

    // Independent frequencies keep the antennae from mirroring one another.
    bool antennaeEverDiffer = false;
    float wideProbeRange = 0.0f;
    float tuckedProbeRange = 0.0f;
    for (int sample = 0; sample < 240; ++sample) {
        const float time = sample / 30.0f;
        const auto probing =
            calculateCockroachAnimation(
                time * 5.0f, time, 165.0f, 0.4f, 1.0f);
        const auto fleeing =
            calculateCockroachAnimation(
                time * 5.0f, time, 165.0f, 1.0f, 0.0f);
        const float left = probing.antennae[0].rotation;
        const float right = probing.antennae[1].rotation;
        antennaeEverDiffer |= std::abs(left + right) > 0.025f;
        wideProbeRange = std::max(
            wideProbeRange, std::max(std::abs(left),
                                     std::abs(right)));
        tuckedProbeRange = std::max(
            tuckedProbeRange,
            std::max(std::abs(fleeing.antennae[0].rotation),
                     std::abs(fleeing.antennae[1].rotation)));
    }
    if (!antennaeEverDiffer ||
        wideProbeRange < tuckedProbeRange * 2.4f) {
        std::cerr << "independent antenna probing failed\n";
        failed = true;
    }

    return failed ? 1 : 0;
}
