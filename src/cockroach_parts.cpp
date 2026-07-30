#include "cockroach_parts.h"

#include <algorithm>
#include <cmath>

namespace {
constexpr float pi = 3.14159265358979323846f;
constexpr float degreesToRadians = pi / 180.0f;

constexpr std::size_t indexOf(CockroachAntenna antenna) {
    return static_cast<std::size_t>(antenna);
}
} // namespace

const std::array<CockroachPartDefinition,
                 static_cast<std::size_t>(CockroachPart::Count)>&
cockroachPartDefinitions() {
    // Pivots sit at the body-side coxa or antenna socket in the source atlas.
    // The six attachment points remain on the torso while the distal sprites
    // rotate independently around those pivots.
    static const std::array<CockroachPartDefinition,
                            static_cast<std::size_t>(
                                CockroachPart::Count)>
        parts{{
            {{0, 0, 283, 799}, {141.5f, 399.5f}, {0.0f, 0.0f}},
            {{284, 248, 301, 273}, {286.0f, 89.0f}, {-0.155f, -0.305f}},
            {{585, 248, 304, 273}, {14.0f, 89.0f}, {0.155f, -0.305f}},
            {{889, 248, 280, 350}, {266.0f, 35.0f}, {-0.170f, -0.075f}},
            {{1169, 248, 286, 348}, {17.0f, 40.0f}, {0.170f, -0.075f}},
            {{284, 598, 219, 313}, {208.0f, 14.0f}, {-0.150f, 0.180f}},
            {{503, 598, 218, 312}, {11.0f, 14.0f}, {0.150f, 0.180f}},
            {{284, 0, 526, 248}, {517.0f, 237.0f}, {-0.070f, -0.430f}},
            {{810, 0, 531, 248}, {10.0f, 237.0f}, {0.070f, -0.430f}},
        }};
    return parts;
}

CockroachAnimationPose calculateCockroachAnimation(
    float gaitClock, float behaviorClock, float bodyLength,
    float motionAmount, float probingAmount) {
    CockroachAnimationPose pose;
    motionAmount = clampf(motionAmount, 0.0f, 1.0f);
    probingAmount = clampf(probingAmount, 0.0f, 1.0f);

    // Cockroaches use an alternating tripod gait:
    // A = left-front, right-middle, left-rear
    // B = right-front, left-middle, right-rear
    constexpr std::array<float, 6> tripodPhase{
        0.0f, pi, pi, 0.0f, 0.0f, pi};
    constexpr std::array<float, 6> individualPhase{
        0.13f, 1.17f, 2.41f, 0.73f, 1.91f, 2.87f};
    constexpr std::array<float, 6> side{
        1.0f, -1.0f, 1.0f, -1.0f, 1.0f, -1.0f};
    constexpr std::array<float, 6> rangeScale{
        1.10f, 1.07f, 0.82f, 0.85f, 1.18f, 1.14f};

    const float strideRange =
        (2.0f + 18.0f * std::sqrt(motionAmount)) *
        degreesToRadians;
    for (std::size_t leg = 0; leg < pose.legs.size(); ++leg) {
        const float phase = gaitClock + tripodPhase[leg];
        const float individualMotion =
            std::sin(gaitClock * 1.83f + individualPhase[leg]);
        const float sweep =
            std::sin(phase) + individualMotion * 0.105f +
            std::sin(phase * 2.0f + individualPhase[leg]) * 0.055f;
        pose.legs[leg].rotation =
            side[leg] * sweep * strideRange * rangeScale[leg];

        // The root does not slide visibly off the thorax. A sub-pixel
        // fore/aft coxa motion prevents the six sprites looking hinged to one
        // rigid clock, while the tripod phase still controls planted legs.
        const float reach =
            bodyLength * (0.0020f + motionAmount * 0.0060f);
        const float lift =
            std::max(0.0f, std::cos(phase)) *
            bodyLength * motionAmount * 0.0035f;
        pose.legs[leg].jointOffset = {
            side[leg] * lift,
            -std::cos(phase) * reach};
    }

    // Each antenna probes independently. Slower/paused animals use a wider
    // search arc; fleeing animals pull their antennae into a smaller sweep.
    const float antennaRange =
        (10.0f + probingAmount * 20.0f) * degreesToRadians;
    pose.antennae[indexOf(CockroachAntenna::Left)].rotation =
        antennaRange *
        (0.65f * std::sin(behaviorClock * 2.55f + 0.21f) +
         0.24f * std::sin(behaviorClock * 5.70f + 1.10f) +
         0.11f * std::sin(behaviorClock * 11.30f));
    pose.antennae[indexOf(CockroachAntenna::Right)].rotation =
        -antennaRange *
        (0.61f * std::sin(behaviorClock * 2.13f + 1.37f) +
         0.26f * std::sin(behaviorClock * 5.10f + 0.44f) +
         0.13f * std::sin(behaviorClock * 10.70f + 2.02f));

    const float feelerShift =
        bodyLength * (0.0030f + probingAmount * 0.0040f);
    pose.antennae[indexOf(CockroachAntenna::Left)].jointOffset = {
        -feelerShift * std::sin(behaviorClock * 3.21f),
        feelerShift * std::sin(behaviorClock * 4.39f + 0.30f)};
    pose.antennae[indexOf(CockroachAntenna::Right)].jointOffset = {
        feelerShift * std::sin(behaviorClock * 2.87f + 1.10f),
        feelerShift * std::sin(behaviorClock * 4.07f + 1.90f)};
    return pose;
}
