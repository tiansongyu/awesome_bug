#pragma once

#include <algorithm>
#include <cmath>

inline float displayResolutionScale(int width, int height) {
    if (width <= 0 || height <= 0) return 1.0f;

    constexpr float referenceWidth = 1920.0f;
    constexpr float referenceHeight = 1080.0f;
    const float proportionalScale = std::min(
        width / referenceWidth,
        height / referenceHeight);

    // Cover common Windows displays from compact/remote desktops through 4K
    // without making the pet unusably tiny or excessively large.
    return std::clamp(proportionalScale, 0.60f, 2.0f);
}

inline float resolutionScaledBodyLength(float referenceBodyLength,
                                        int width, int height) {
    return std::round(
        referenceBodyLength *
        displayResolutionScale(width, height));
}
