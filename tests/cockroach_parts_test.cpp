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
    float previousLeftAntenna = 0.0f;
    float previousRightAntenna = 0.0f;
    int visiblyChangingAntennaFrames = 0;
    std::array<float, 6> minimumLegRotation{};
    std::array<float, 6> maximumLegRotation{};
    minimumLegRotation.fill(1000.0f);
    maximumLegRotation.fill(-1000.0f);
    for (int sample = 0; sample < 240; ++sample) {
        // Four seconds sampled at the Windows presentation rate. This guards
        // against technically changing poses whose per-frame movement is too
        // small to be perceived on a 60 Hz layered window.
        const float time = sample / 60.0f;
        const auto probing =
            calculateCockroachAnimation(
                time * 5.0f, time, 165.0f, 0.4f, 1.0f);
        const auto fleeing =
            calculateCockroachAnimation(
                time * 5.0f, time, 165.0f, 1.0f, 0.0f);
        const float left = probing.antennae[0].rotation;
        const float right = probing.antennae[1].rotation;
        antennaeEverDiffer |= std::abs(left + right) > 0.025f;
        if (sample > 0 &&
            (std::abs(left - previousLeftAntenna) > 0.006f ||
             std::abs(right - previousRightAntenna) > 0.006f)) {
            ++visiblyChangingAntennaFrames;
        }
        previousLeftAntenna = left;
        previousRightAntenna = right;
        wideProbeRange = std::max(
            wideProbeRange, std::max(std::abs(left),
                                     std::abs(right)));
        tuckedProbeRange = std::max(
            tuckedProbeRange,
            std::max(std::abs(fleeing.antennae[0].rotation),
                     std::abs(fleeing.antennae[1].rotation)));
        for (std::size_t leg = 0; leg < probing.legs.size(); ++leg) {
            minimumLegRotation[leg] = std::min(
                minimumLegRotation[leg],
                probing.legs[leg].rotation);
            maximumLegRotation[leg] = std::max(
                maximumLegRotation[leg],
                probing.legs[leg].rotation);
        }
    }
    if (!antennaeEverDiffer ||
        wideProbeRange < tuckedProbeRange * 2.4f ||
        visiblyChangingAntennaFrames < 180) {
        std::cerr << "independent antenna probing failed\n";
        failed = true;
    }
    for (std::size_t leg = 0; leg < minimumLegRotation.size(); ++leg) {
        if (maximumLegRotation[leg] -
                minimumLegRotation[leg] < 0.30f) {
            std::cerr << "leg swing is not visibly animated: "
                      << leg << '\n';
            failed = true;
        }
    }

    // Lurking freezes all legs while the antennae keep probing. Grooming
    // alternates the two front legs through a clearly visible combing arc.
    const auto lurkFirst = calculateCockroachAnimation(
        1.0f, 1.0f, 165.0f, 0.0f, 1.0f,
        CockroachAnimationMode::Lurking, 0.0f);
    const auto lurkSecond = calculateCockroachAnimation(
        4.0f, 1.2f, 165.0f, 0.0f, 1.0f,
        CockroachAnimationMode::Lurking, 0.2f);
    for (std::size_t leg = 0; leg < lurkFirst.legs.size(); ++leg) {
        if (std::abs(lurkFirst.legs[leg].rotation) > 0.0001f ||
            std::abs(lurkSecond.legs[leg].rotation) > 0.0001f) {
            std::cerr << "lurking leg did not stay still: "
                      << leg << '\n';
            failed = true;
        }
    }
    if (std::abs(lurkFirst.antennae[0].rotation -
                 lurkSecond.antennae[0].rotation) < 0.01f &&
        std::abs(lurkFirst.antennae[1].rotation -
                 lurkSecond.antennae[1].rotation) < 0.01f) {
        std::cerr << "lurking antennae stopped probing\n";
        failed = true;
    }

    float leftGroomMinimum = 1000.0f;
    float leftGroomMaximum = -1000.0f;
    float rightGroomMinimum = 1000.0f;
    float rightGroomMaximum = -1000.0f;
    for (int sample = 0; sample < 120; ++sample) {
        const float time = sample / 60.0f;
        const auto grooming = calculateCockroachAnimation(
            0.0f, time, 165.0f, 0.0f, 1.0f,
            CockroachAnimationMode::Grooming, time);
        leftGroomMinimum = std::min(
            leftGroomMinimum, grooming.legs[0].rotation);
        leftGroomMaximum = std::max(
            leftGroomMaximum, grooming.legs[0].rotation);
        rightGroomMinimum = std::min(
            rightGroomMinimum, grooming.legs[1].rotation);
        rightGroomMaximum = std::max(
            rightGroomMaximum, grooming.legs[1].rotation);
    }
    if (leftGroomMaximum - leftGroomMinimum < 0.45f ||
        rightGroomMaximum - rightGroomMinimum < 0.45f) {
        std::cerr << "front-leg grooming arc is not visible\n";
        failed = true;
    }

    return failed ? 1 : 0;
}
