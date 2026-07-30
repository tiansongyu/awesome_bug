#pragma once

#include <algorithm>
#include <cmath>

struct Vec2 {
    float x = 0.0f;
    float y = 0.0f;

    Vec2 operator+(const Vec2& other) const { return {x + other.x, y + other.y}; }
    Vec2 operator-(const Vec2& other) const { return {x - other.x, y - other.y}; }
    Vec2 operator*(float scalar) const { return {x * scalar, y * scalar}; }
    Vec2& operator+=(const Vec2& other) {
        x += other.x;
        y += other.y;
        return *this;
    }
};

inline float length(Vec2 value) {
    return std::sqrt(value.x * value.x + value.y * value.y);
}

inline Vec2 normalized(Vec2 value) {
    const float magnitude = length(value);
    return magnitude > 0.0001f ? value * (1.0f / magnitude) : Vec2{};
}

inline float clampf(float value, float low, float high) {
    return std::max(low, std::min(value, high));
}

inline float wrapAngle(float angle) {
    constexpr float pi = 3.14159265358979323846f;
    while (angle > pi) angle -= 2.0f * pi;
    while (angle < -pi) angle += 2.0f * pi;
    return angle;
}

inline Vec2 rotateLocal(Vec2 local, float angle) {
    // Local forward is negative Y; a positive angle turns clockwise on screen.
    const float c = std::cos(angle);
    const float s = std::sin(angle);
    return {local.x * c - local.y * s, local.x * s + local.y * c};
}

