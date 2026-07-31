#include "runtime/motion_solver.h"

#include <cmath>
#include <cstdlib>
#include <iostream>
#include <string>
#include <vector>

namespace {
constexpr float pi = 3.14159265358979323846f;

[[noreturn]] void fail(const std::string& message) {
    std::cerr << "motion solver test failed: " << message << '\n';
    std::exit(1);
}

void require(bool condition, const std::string& message) {
    if (!condition) {
        fail(message);
    }
}

Vec2 extents(const bug::BodyState& body) {
    const float halfLength = body.length * 0.43f;
    const float halfWidth = body.length * 0.20f;
    const float sine = std::abs(std::sin(body.heading));
    const float cosine = std::abs(std::cos(body.heading));
    return {
        sine * halfLength + cosine * halfWidth,
        cosine * halfLength + sine * halfWidth};
}

bool overlaps(const bug::BodyState& body,
              const ScreenObstacle& obstacle, float padding) {
    const Vec2 bodyExtents = extents(body);
    return body.position.x >=
               obstacle.x - bodyExtents.x - padding &&
           body.position.x <=
               obstacle.x + obstacle.width + bodyExtents.x + padding &&
           body.position.y >=
               obstacle.y - bodyExtents.y - padding &&
           body.position.y <=
               obstacle.y + obstacle.height + bodyExtents.y + padding;
}

bug::MotionSolver makeSolver(Vec2 position, float heading = 0.0f) {
    return bug::MotionSolver(
        {{0.0f, 0.0f, 1280.0f, 720.0f},
         165.0f, 0.20f, 0.43f, 3.0f},
        position, heading,
        [](std::string_view, float low, float high) {
            return (low + high) * 0.5f;
        });
}

bug::MotionIntent fastIntent(Vec2 direction) {
    bug::MotionIntent result;
    result.direction = direction;
    result.speed = 540.0f;
    result.turnRate = 8.0f;
    result.acceleration = 1350.0f;
    return result;
}

void testStaticObstacleAndWorldBounds() {
    bug::MotionSolver solver = makeSolver({130.0f, 360.0f}, pi * 0.5f);
    const std::vector<ScreenObstacle> obstacles{
        {570.0f, 285.0f, 110.0f, 150.0f, false}};
    const bug::MotionIntent intent = fastIntent({1.0f, 0.0f});

    for (int frame = 0; frame < 1800; ++frame) {
        solver.step(1.0f / 60.0f, intent, obstacles);
        const bug::BodyState body = solver.body();
        const Vec2 bodyExtents = extents(body);
        require(
            body.position.x >= bodyExtents.x + 10.0f - 0.001f &&
                body.position.x <=
                    1280.0f - bodyExtents.x - 10.0f + 0.001f &&
                body.position.y >= bodyExtents.y + 10.0f - 0.001f &&
                body.position.y <=
                    720.0f - bodyExtents.y - 10.0f + 0.001f,
            "body escaped the work area");
        require(
            !overlaps(body, obstacles.front(), 2.0f),
            "body entered a static obstacle");
    }
}

void testDraggedOverlapSeparatesWithoutTeleport() {
    bug::MotionSolver solver = makeSolver({620.0f, 360.0f});
    std::vector<ScreenObstacle> obstacles{
        {575.0f, 320.0f, 90.0f, 80.0f, true}};
    bug::MotionIntent intent = fastIntent({1.0f, 0.0f});

    float maximumFrameMovement = 0.0f;
    bool becameClear = false;
    for (int frame = 0; frame < 360; ++frame) {
        const Vec2 before = solver.body().position;
        solver.step(1.0f / 60.0f, intent, obstacles);
        const bug::BodyState body = solver.body();
        maximumFrameMovement = std::max(
            maximumFrameMovement, length(body.position - before));
        if (!overlaps(body, obstacles.front(), 8.0f)) {
            becameClear = true;
        }
    }
    require(becameClear, "moving obstacle overlap never cleared");
    require(maximumFrameMovement < 24.0f,
            "overlap correction teleported the body");
}

void testStillIntentAndInitialHeading() {
    bug::MotionSolver solver = makeSolver({640.0f, 360.0f});
    bug::MotionIntent still;
    still.direction = {1.0f, 0.0f};
    still.turnRate = 4.5f;
    still.acceleration = 680.0f;
    still.intentionallyStill = true;
    still.initialHeadingValid = true;
    still.initialHeading = pi * 0.5f;

    const Vec2 initial = solver.body().position;
    solver.step(1.0f / 60.0f, still, {});
    require(length(solver.body().position - initial) < 0.001f,
            "still intent moved without an overlap");
    require(std::abs(wrapAngle(solver.body().heading - pi * 0.5f)) <
                0.0001f,
            "first-frame heading was not applied");

    still.initialHeading = -pi * 0.5f;
    solver.step(1.0f / 60.0f, still, {});
    require(std::abs(wrapAngle(solver.body().heading - pi * 0.5f)) <
                0.0001f,
            "initial heading was accepted more than once");
}

void testSensorsAreGeometryOnly() {
    bug::MotionSolver solver = makeSolver({640.0f, 360.0f});
    std::vector<ScreenObstacle> obstacles{
        {0.0f, 0.0f, 260.0f, 240.0f, false},
        {610.0f, 330.0f, 60.0f, 60.0f, true}};
    const bug::CornerSensor topLeft = solver.corner(0, obstacles);
    const bug::CornerSensor bottomRight = solver.corner(3, obstacles);
    require(topLeft.blocked, "blocked corner was reported clear");
    require(!bottomRight.blocked, "clear corner was reported blocked");

    bug::BaitInput bait;
    bait.active = true;
    bait.position = {630.0f, 350.0f};
    const bug::ObstacleSensor sensors =
        solver.sensors(obstacles, bait);
    require(sensors.overlapping, "body overlap sensor was false");
    require(sensors.baitBlocked, "bait obstruction sensor was false");
    require(sensors.nearestValid && sensors.nearestMoving,
            "nearest obstacle summary was incorrect");
    require(
        sensors.obstacleUrgency > 0.0f &&
            length(sensors.avoidanceDirection) > 0.0f,
        "avoidance summary was not produced");
}

} // namespace

int main() {
    testStaticObstacleAndWorldBounds();
    testDraggedOverlapSeparatesWithoutTeleport();
    testStillIntentAndInitialHeading();
    testSensorsAreGeometryOnly();
    return 0;
}
