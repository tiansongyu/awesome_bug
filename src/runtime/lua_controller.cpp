#include "runtime/lua_controller.h"

#include <algorithm>
#include <array>
#include <cmath>
#include <initializer_list>
#include <limits>
#include <memory>
#include <string>
#include <utility>
#include <vector>

namespace bug {
namespace {

constexpr double pi = 3.14159265358979323846264338327950288;
constexpr double maximumCoordinate = 10000000.0;
constexpr double maximumBodyLength = 100000.0;
constexpr double maximumClock = 1.0e12;

template <typename T>
LuaResult<T> failure(
    LuaErrorCode code, std::string operation,
    std::string subject, std::string message) {
    LuaError error;
    error.code = code;
    error.operation = std::move(operation);
    error.subject = std::move(subject);
    error.message = std::move(message);
    return LuaResult<T>::failure(std::move(error));
}

template <typename T>
LuaResult<T> forwardFailure(
    LuaError error, std::string_view operation,
    const std::filesystem::path& subject) {
    if (error.operation.empty()) {
        error.operation = std::string(operation);
    }
    if (error.subject.empty()) {
        error.subject = subject.u8string();
    }
    return LuaResult<T>::failure(std::move(error));
}

const LuaValue* field(
    const LuaValue& value, std::string_view name) {
    const LuaTableValue* table = value.table();
    if (!table) return nullptr;
    for (const auto& entry : table->entries) {
        const std::string* key = entry.first.string();
        if (key && *key == name) return &entry.second;
    }
    return nullptr;
}

bool contains(
    std::initializer_list<std::string_view> names,
    std::string_view candidate) {
    return std::find(names.begin(), names.end(), candidate) !=
           names.end();
}

bool objectHasOnly(
    const LuaValue& value, std::string_view path,
    std::initializer_list<std::string_view> allowed,
    std::string& error) {
    const LuaTableValue* table = value.table();
    if (!table) {
        error = std::string(path) + " must be a table";
        return false;
    }
    for (const auto& entry : table->entries) {
        const std::string* key = entry.first.string();
        if (!key) {
            error = std::string(path) +
                    " must contain only named fields";
            return false;
        }
        if (!contains(allowed, *key)) {
            error = std::string(path) +
                    " contains unknown field '" + *key + "'";
            return false;
        }
    }
    return true;
}

bool requireNumber(
    const LuaValue* value, std::string_view path,
    double low, double high, double& output,
    std::string& error) {
    const double* number = value ? value->number() : nullptr;
    if (!number || !std::isfinite(*number)) {
        error = std::string(path) + " must be a finite number";
        return false;
    }
    if (*number < low || *number > high) {
        error = std::string(path) + " is outside the allowed range [" +
                std::to_string(low) + ", " +
                std::to_string(high) + "]";
        return false;
    }
    output = *number;
    return true;
}

bool requireBoolean(
    const LuaValue* value, std::string_view path,
    bool& output, std::string& error) {
    const bool* boolean = value ? value->boolean() : nullptr;
    if (!boolean) {
        error = std::string(path) + " must be a boolean";
        return false;
    }
    output = *boolean;
    return true;
}

bool optionalBoolean(
    const LuaValue& object, std::string_view name,
    std::string_view path, bool fallback,
    bool& output, std::string& error) {
    const LuaValue* value = field(object, name);
    if (!value || value->isNil()) {
        output = fallback;
        return true;
    }
    return requireBoolean(value, path, output, error);
}

bool optionalNumber(
    const LuaValue& object, std::string_view name,
    std::string_view path, double low, double high,
    double fallback, double& output, bool* present,
    std::string& error) {
    const LuaValue* value = field(object, name);
    if (!value || value->isNil()) {
        output = fallback;
        if (present) *present = false;
        return true;
    }
    if (present) *present = true;
    return requireNumber(value, path, low, high, output, error);
}

bool parseVector(
    const LuaValue* value, std::string_view path,
    double low, double high, Vec2& output,
    std::string& error) {
    if (!value ||
        !objectHasOnly(*value, path, {"x", "y"}, error)) {
        if (!value && error.empty()) {
            error = std::string(path) + " must be a table";
        }
        return false;
    }
    double x = 0.0;
    double y = 0.0;
    if (!requireNumber(
            field(*value, "x"),
            std::string(path) + ".x", low, high, x, error) ||
        !requireNumber(
            field(*value, "y"),
            std::string(path) + ".y", low, high, y, error)) {
        return false;
    }
    output = {
        static_cast<float>(x),
        static_cast<float>(y)};
    return true;
}

bool validStateName(const std::string& value) {
    if (value.empty() || value.size() > 64 ||
        value.find('\0') != std::string::npos) {
        return false;
    }
    for (const unsigned char character : value) {
        const bool valid =
            (character >= 'a' && character <= 'z') ||
            (character >= 'A' && character <= 'Z') ||
            (character >= '0' && character <= '9') ||
            character == '_' || character == '-';
        if (!valid) return false;
    }
    return true;
}

bool finite(float value) {
    return std::isfinite(static_cast<double>(value));
}

bool finite(double value) {
    return std::isfinite(value);
}

bool finiteVector(Vec2 value) {
    return finite(value.x) && finite(value.y);
}

bool boundedVector(Vec2 value, double maximum) {
    return finiteVector(value) &&
           std::abs(static_cast<double>(value.x)) <= maximum &&
           std::abs(static_cast<double>(value.y)) <= maximum;
}

bool validateFrame(
    const FrameInput& frame,
    const LuaControllerConfig& config,
    std::string& error) {
    if (!finite(frame.dt) || frame.dt < 0.0f ||
        frame.dt > 0.25f) {
        error = "frame.dt must be in [0, 0.25]";
        return false;
    }
    if (!finite(frame.clock) || frame.clock < 0.0 ||
        frame.clock > maximumClock) {
        error = "frame.clock must be in [0, 1e12]";
        return false;
    }
    if (!boundedVector(frame.body.position, maximumCoordinate) ||
        !finite(frame.body.heading) ||
        std::abs(static_cast<double>(frame.body.heading)) > 1.0e6 ||
        !finite(frame.body.speed) || frame.body.speed < 0.0f ||
        frame.body.speed >
            config.motionLimits.maximumSpeed + 0.01f ||
        !finite(frame.body.length) ||
        frame.body.length <= 0.0f ||
        frame.body.length > maximumBodyLength) {
        error =
            "frame.body contains an invalid position, heading, speed, "
            "or length";
        return false;
    }
    const float lengthTolerance =
        std::max(0.01f, config.bodyLength * 1.0e-5f);
    if (std::abs(frame.body.length - config.bodyLength) >
        lengthTolerance) {
        error =
            "frame.body.length does not match controller body_length";
        return false;
    }
    if (!finite(frame.world.x) || !finite(frame.world.y) ||
        !finite(frame.world.width) || !finite(frame.world.height) ||
        std::abs(static_cast<double>(frame.world.x)) >
            maximumCoordinate ||
        std::abs(static_cast<double>(frame.world.y)) >
            maximumCoordinate ||
        frame.world.width <= 0.0f ||
        frame.world.height <= 0.0f ||
        frame.world.width > maximumCoordinate ||
        frame.world.height > maximumCoordinate) {
        error = "frame.world must be a finite, positive rectangle";
        return false;
    }
    if (!boundedVector(frame.cursor.position, maximumCoordinate) ||
        !boundedVector(frame.cursor.velocity, 1000000.0)) {
        error = "frame.cursor contains invalid coordinates or velocity";
        return false;
    }
    if (!boundedVector(frame.bait.position, maximumCoordinate)) {
        error = "frame.bait contains invalid coordinates";
        return false;
    }
    for (std::size_t index = 0; index < 4; ++index) {
        const CornerSensor& corner = frame.corners[index];
        if (!boundedVector(corner.position, maximumCoordinate) ||
            !finite(corner.distance) || corner.distance < 0.0f ||
            corner.distance > maximumCoordinate * 3.0) {
            error = "frame.corners[" + std::to_string(index + 1) +
                    "] is invalid";
            return false;
        }
    }

    const ObstacleSensor& sensors = frame.sensors;
    if (!boundedVector(sensors.avoidanceDirection, 2.0) ||
        !finite(sensors.obstacleUrgency) ||
        sensors.obstacleUrgency < 0.0f ||
        sensors.obstacleUrgency > 1.0f ||
        !finite(sensors.movingObstacleUrgency) ||
        sensors.movingObstacleUrgency < 0.0f ||
        sensors.movingObstacleUrgency > 1.0f ||
        !boundedVector(sensors.nearestPoint, maximumCoordinate) ||
        !boundedVector(sensors.nearestAway, 2.0) ||
        !finite(sensors.nearestDistance) ||
        sensors.nearestDistance < 0.0f ||
        sensors.nearestDistance > maximumCoordinate * 3.0) {
        error = "frame.sensors contains an invalid obstacle summary";
        return false;
    }

    const MotionFeedback& feedback = frame.feedback;
    if (!boundedVector(feedback.actualDisplacement, 1000000.0) ||
        !finite(feedback.blockedTime) ||
        feedback.blockedTime < 0.0f ||
        feedback.blockedTime > 1000000.0f ||
        !finite(feedback.edgeDwellTime) ||
        feedback.edgeDwellTime < 0.0f ||
        feedback.edgeDwellTime > 1000000.0f ||
        !boundedVector(feedback.recoveryDirection, 2.0) ||
        !finite(feedback.recoveryTime) ||
        feedback.recoveryTime < 0.0f ||
        feedback.recoveryTime > 1000000.0f) {
        error = "frame.feedback contains invalid motion feedback";
        return false;
    }
    return true;
}

LuaValue vectorValue(Vec2 value) {
    return LuaValue::object({
        {"x", LuaValue(value.x)},
        {"y", LuaValue(value.y)},
    });
}

LuaValue frameValue(
    const FrameInput& frame, const Species& species) {
    std::vector<LuaValue> corners;
    corners.reserve(4);
    for (const CornerSensor& corner : frame.corners) {
        corners.push_back(LuaValue::object({
            {"x", LuaValue(corner.position.x)},
            {"y", LuaValue(corner.position.y)},
            {"distance", LuaValue(corner.distance)},
            {"blocked", LuaValue(corner.blocked)},
        }));
    }

    const bool baitEnabled =
        species.capabilities.bait && frame.features.bait;
    return LuaValue::object({
        {"dt", LuaValue(frame.dt)},
        {"clock", LuaValue(frame.clock)},
        {"body", LuaValue::object({
            {"x", LuaValue(frame.body.position.x)},
            {"y", LuaValue(frame.body.position.y)},
            {"heading", LuaValue(frame.body.heading)},
            {"speed", LuaValue(frame.body.speed)},
            {"length", LuaValue(frame.body.length)},
        })},
        {"world", LuaValue::object({
            {"x", LuaValue(frame.world.x)},
            {"y", LuaValue(frame.world.y)},
            {"width", LuaValue(frame.world.width)},
            {"height", LuaValue(frame.world.height)},
        })},
        {"cursor", LuaValue::object({
            {"valid", LuaValue(frame.cursor.valid)},
            {"x", LuaValue(frame.cursor.position.x)},
            {"y", LuaValue(frame.cursor.position.y)},
            {"vx", LuaValue(frame.cursor.velocity.x)},
            {"vy", LuaValue(frame.cursor.velocity.y)},
        })},
        {"bait", LuaValue::object({
            {"active", LuaValue(
                baitEnabled && frame.bait.active)},
            {"x", LuaValue(frame.bait.position.x)},
            {"y", LuaValue(frame.bait.position.y)},
        })},
        {"corners", LuaValue::array(std::move(corners))},
        {"sensors", LuaValue::object({
            {"overlapping", LuaValue(frame.sensors.overlapping)},
            {"bait_blocked", LuaValue(frame.sensors.baitBlocked)},
            {"nearest_valid", LuaValue(frame.sensors.nearestValid)},
            {"nearest_moving", LuaValue(frame.sensors.nearestMoving)},
            {"avoidance_direction",
             vectorValue(frame.sensors.avoidanceDirection)},
            {"obstacle_urgency",
             LuaValue(frame.sensors.obstacleUrgency)},
            {"moving_obstacle_urgency",
             LuaValue(frame.sensors.movingObstacleUrgency)},
            {"nearest_point",
             vectorValue(frame.sensors.nearestPoint)},
            {"nearest_away",
             vectorValue(frame.sensors.nearestAway)},
            {"nearest_distance",
             LuaValue(frame.sensors.nearestDistance)},
        })},
        {"feedback", LuaValue::object({
            {"actual_displacement",
             vectorValue(frame.feedback.actualDisplacement)},
            {"overlapping", LuaValue(frame.feedback.overlapping)},
            {"blocked_time", LuaValue(frame.feedback.blockedTime)},
            {"edge_dwell_time",
             LuaValue(frame.feedback.edgeDwellTime)},
            {"recovery_direction",
             vectorValue(frame.feedback.recoveryDirection)},
            {"recovery_time",
             LuaValue(frame.feedback.recoveryTime)},
        })},
        {"features", LuaValue::object({
            {"single_instance",
             LuaValue(frame.features.singleInstance)},
            {"extended_behaviors",
             LuaValue(frame.features.extendedBehaviors)},
            {"bait", LuaValue(baitEnabled)},
        })},
        {"request_corner_rest",
         LuaValue(frame.requestCornerRest)},
    });
}

LuaValue configValue(
    const Species& species,
    const LuaControllerConfig& config) {
    return LuaValue::object({
        {"api_version", LuaValue(apiVersion)},
        {"species_id", LuaValue(species.id)},
        {"body_length", LuaValue(config.bodyLength)},
        {"default_body_length",
         LuaValue(species.body.defaultLength)},
        {"speed_multiplier",
         LuaValue(config.speedMultiplier)},
        {"enable_extended_behaviors",
         LuaValue(config.enableExtendedBehaviors)},
        {"capabilities", LuaValue::object({
            {"bait", LuaValue(species.capabilities.bait)},
        })},
        {"limits", LuaValue::object({
            {"speed",
             LuaValue(config.motionLimits.maximumSpeed)},
            {"turn_rate",
             LuaValue(config.motionLimits.maximumTurnRate)},
            {"acceleration",
             LuaValue(config.motionLimits.maximumAcceleration)},
            {"lateral_speed",
             LuaValue(config.motionLimits.maximumLateralSpeed)},
        })},
    });
}

bool validateConfig(
    const Species& species, LuaControllerConfig& config,
    std::string& error) {
    if (config.bodyLength == 0.0f) {
        config.bodyLength = species.body.defaultLength;
    }
    if (!finite(config.bodyLength) ||
        config.bodyLength <= 0.0f ||
        config.bodyLength > maximumBodyLength) {
        error = "bodyLength must be in (0, 100000]";
        return false;
    }
    if (!finite(config.speedMultiplier) ||
        config.speedMultiplier < 0.01f ||
        config.speedMultiplier > 32.0f) {
        error = "speedMultiplier must be in [0.01, 32]";
        return false;
    }
    const LuaMotionLimits& limits = config.motionLimits;
    if (!finite(limits.maximumSpeed) ||
        limits.maximumSpeed <= 0.0f ||
        limits.maximumSpeed > 1000000.0f ||
        !finite(limits.maximumTurnRate) ||
        limits.maximumTurnRate <= 0.0f ||
        limits.maximumTurnRate > 10000.0f ||
        !finite(limits.maximumAcceleration) ||
        limits.maximumAcceleration <= 0.0f ||
        limits.maximumAcceleration > 10000000.0f ||
        !finite(limits.maximumLateralSpeed) ||
        limits.maximumLateralSpeed <= 0.0f ||
        limits.maximumLateralSpeed > 1000000.0f) {
        error = "motion limits must be finite, positive, and bounded";
        return false;
    }
    return true;
}

bool parseDecision(
    const LuaValue& value, const FrameInput& frame,
    const Species& species, const LuaControllerConfig& config,
    bool hasSuccessfulStep, Decision& output,
    std::string& error) {
    if (!objectHasOnly(
            value, "step",
            {"state", "target", "motion", "events"}, error)) {
        return false;
    }

    const LuaValue* stateValue = field(value, "state");
    const std::string* state =
        stateValue ? stateValue->string() : nullptr;
    if (!state || !validStateName(*state)) {
        error =
            "step.state must contain 1..64 ASCII letters, digits, "
            "'_' or '-'";
        return false;
    }
    output.state = *state;

    const double targetMargin = std::max(
        static_cast<double>(frame.body.length) * 8.0,
        std::max(
            static_cast<double>(frame.world.width),
            static_cast<double>(frame.world.height)) * 2.0);
    const double targetLow =
        std::min(
            static_cast<double>(frame.world.x),
            static_cast<double>(frame.world.y)) -
        targetMargin;
    const double targetHigh =
        std::max(
            static_cast<double>(frame.world.x + frame.world.width),
            static_cast<double>(frame.world.y + frame.world.height)) +
        targetMargin;
    if (!parseVector(
            field(value, "target"), "step.target",
            std::max(-maximumCoordinate, targetLow),
            std::min(maximumCoordinate, targetHigh),
            output.target, error)) {
        return false;
    }

    const LuaValue* motion = field(value, "motion");
    if (!motion ||
        !objectHasOnly(
            *motion, "step.motion",
            {"direction", "speed", "turn_rate", "acceleration",
             "lateral_speed", "recovery_probe_phase",
             "intentionally_still", "stop_immediately",
             "cancel_recovery", "allow_edge_rest",
             "initial_heading"},
            error)) {
        if (!motion && error.empty()) {
            error = "step.motion must be a table";
        }
        return false;
    }
    if (!parseVector(
            field(*motion, "direction"),
            "step.motion.direction", -1.0, 1.0,
            output.motion.direction, error)) {
        return false;
    }

    double number = 0.0;
    if (!requireNumber(
            field(*motion, "speed"),
            "step.motion.speed", 0.0,
            config.motionLimits.maximumSpeed,
            number, error)) {
        return false;
    }
    output.motion.speed = static_cast<float>(number);
    if (!requireNumber(
            field(*motion, "turn_rate"),
            "step.motion.turn_rate", 0.0,
            config.motionLimits.maximumTurnRate,
            number, error)) {
        return false;
    }
    output.motion.turnRate = static_cast<float>(number);
    if (!requireNumber(
            field(*motion, "acceleration"),
            "step.motion.acceleration", 0.0,
            config.motionLimits.maximumAcceleration,
            number, error)) {
        return false;
    }
    output.motion.acceleration = static_cast<float>(number);
    if (!requireNumber(
            field(*motion, "lateral_speed"),
            "step.motion.lateral_speed",
            -config.motionLimits.maximumLateralSpeed,
            config.motionLimits.maximumLateralSpeed,
            number, error)) {
        return false;
    }
    output.motion.lateralSpeed = static_cast<float>(number);
    if (!optionalNumber(
            *motion, "recovery_probe_phase",
            "step.motion.recovery_probe_phase",
            -1000000.0, 1000000.0, 0.0, number,
            nullptr, error)) {
        return false;
    }
    output.motion.recoveryProbePhase =
        static_cast<float>(number);
    if (!requireBoolean(
            field(*motion, "intentionally_still"),
            "step.motion.intentionally_still",
            output.motion.intentionallyStill, error) ||
        !optionalBoolean(
            *motion, "stop_immediately",
            "step.motion.stop_immediately", false,
            output.motion.stopImmediately, error) ||
        !optionalBoolean(
            *motion, "cancel_recovery",
            "step.motion.cancel_recovery", false,
            output.motion.cancelRecovery, error) ||
        !requireBoolean(
            field(*motion, "allow_edge_rest"),
            "step.motion.allow_edge_rest",
            output.motion.allowEdgeRest, error)) {
        return false;
    }
    const float directionLength =
        length(output.motion.direction);
    if (directionLength > 1.415f ||
        (directionLength < 0.0001f &&
         !output.motion.intentionallyStill)) {
        error =
            "step.motion.direction must be a bounded, non-zero "
            "direction while moving";
        return false;
    }

    bool initialHeadingPresent = false;
    if (!optionalNumber(
            *motion, "initial_heading",
            "step.motion.initial_heading",
            -pi, pi, 0.0, number,
            &initialHeadingPresent, error)) {
        return false;
    }
    if (initialHeadingPresent && hasSuccessfulStep) {
        error =
            "step.motion.initial_heading is only legal on the first "
            "successful step";
        return false;
    }
    output.motion.initialHeadingValid =
        initialHeadingPresent;
    output.motion.initialHeading =
        static_cast<float>(number);

    const LuaValue* events = field(value, "events");
    if (!events ||
        !objectHasOnly(
            *events, "step.events",
            {"consume_bait"}, error) ||
        !requireBoolean(
            field(*events, "consume_bait"),
            "step.events.consume_bait",
            output.consumeBait, error)) {
        if (!events && error.empty()) {
            error = "step.events must be a table";
        }
        return false;
    }
    const bool baitEnabled =
        species.capabilities.bait &&
        frame.features.bait && frame.bait.active;
    if (output.consumeBait && !baitEnabled) {
        error =
            "step.events.consume_bait requires an active, enabled "
            "bait capability";
        return false;
    }
    return true;
}

bool parsePose(
    const LuaValue& value, const FrameInput& frame,
    const Species& species, Pose& output,
    std::string& error) {
    if (!objectHasOnly(
            value, "pose", {"body", "parts"}, error)) {
        return false;
    }
    const LuaValue* body = field(value, "body");
    if (!body ||
        !objectHasOnly(
            *body, "pose.body",
            {"x", "y", "rotation"}, error)) {
        if (!body && error.empty()) {
            error = "pose.body must be a table";
        }
        return false;
    }
    double x = 0.0;
    double y = 0.0;
    double rotation = 0.0;
    const double bodyLimit = frame.body.length;
    if (!requireNumber(
            field(*body, "x"), "pose.body.x",
            -bodyLimit, bodyLimit, x, error) ||
        !requireNumber(
            field(*body, "y"), "pose.body.y",
            -bodyLimit, bodyLimit, y, error) ||
        !requireNumber(
            field(*body, "rotation"),
            "pose.body.rotation",
            -2.0 * pi, 2.0 * pi, rotation, error)) {
        return false;
    }
    output.bodyOffset = {
        static_cast<float>(x),
        static_cast<float>(y)};
    output.bodyRotation =
        static_cast<float>(rotation);
    output.parts.assign(species.parts.size(), PartPose{});

    const LuaValue* partsValue = field(value, "parts");
    const LuaTableValue* parts =
        partsValue ? partsValue->table() : nullptr;
    if (!parts) {
        error = "pose.parts must be a table";
        return false;
    }
    std::vector<bool> seen(species.parts.size(), false);
    const double jointLimit =
        static_cast<double>(frame.body.length) * 2.0;
    for (const auto& entry : parts->entries) {
        const std::string* partName = entry.first.string();
        if (!partName || partName->empty()) {
            error =
                "pose.parts must use non-empty part names as keys";
            return false;
        }
        const auto definition = std::find_if(
            species.parts.begin(), species.parts.end(),
            [&](const PartDefinition& part) {
                return part.name == *partName;
            });
        if (definition == species.parts.end()) {
            error = "pose.parts contains unknown part '" +
                    *partName + "'";
            return false;
        }
        const std::size_t index =
            static_cast<std::size_t>(
                std::distance(species.parts.begin(), definition));
        if (seen[index]) {
            error = "pose.parts contains duplicate part '" +
                    *partName + "'";
            return false;
        }
        seen[index] = true;
        const LuaValue& part = entry.second;
        if (!objectHasOnly(
                part, "pose.parts." + *partName,
                {"rotation", "joint_offset"}, error)) {
            return false;
        }
        if (!requireNumber(
                field(part, "rotation"),
                "pose.parts." + *partName + ".rotation",
                -8.0 * pi, 8.0 * pi,
                rotation, error)) {
            return false;
        }
        output.parts[index].rotation =
            static_cast<float>(rotation);
        if (!parseVector(
                field(part, "joint_offset"),
                "pose.parts." + *partName + ".joint_offset",
                -jointLimit, jointLimit,
                output.parts[index].jointOffset, error)) {
            return false;
        }
    }
    return true;
}

bool validSpeciesForController(
    const Species& species, std::string& error) {
    if (species.apiVersion != apiVersion) {
        error = "species api_version must be " +
                std::to_string(apiVersion);
        return false;
    }
    if (species.id.empty() ||
        species.behaviorFile.empty() ||
        species.parts.empty() ||
        species.parts.size() > maximumParts ||
        !finite(species.body.defaultLength) ||
        species.body.defaultLength <= 0.0f) {
        error =
            "species metadata is incomplete or was not validated";
        return false;
    }
    std::vector<std::string> names;
    names.reserve(species.parts.size());
    for (const PartDefinition& part : species.parts) {
        if (part.name.empty() ||
            std::find(names.begin(), names.end(), part.name) !=
                names.end()) {
            error = "species part names must be non-empty and unique";
            return false;
        }
        names.push_back(part.name);
    }
    return true;
}

bool exactApiVersion(
    const LuaValue& value, std::string& error) {
    const double* number = value.number();
    if (!number || !std::isfinite(*number) ||
        std::floor(*number) != *number ||
        *number != static_cast<double>(apiVersion)) {
        error = "api_version must be exactly " +
                std::to_string(apiVersion);
        return false;
    }
    return true;
}

} // namespace

LuaBehaviorModule::LuaBehaviorModule(
    LuaHost& host, Species species,
    const LuaHost::Reference& fsmModule,
    LuaHost::Reference behaviorModule)
    : host_(&host),
      species_(std::move(species)),
      fsmModule_(&fsmModule),
      behaviorModule_(std::move(behaviorModule)) {}

LuaResult<std::unique_ptr<LuaBehaviorModule>>
LuaBehaviorModule::load(
    LuaHost& host, const Species& species,
    const LuaHost::Reference& fsmModule) {
    std::string error;
    if (!validSpeciesForController(species, error)) {
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "loading Lua behavior module", species.id,
            std::move(error));
    }
    if (!fsmModule.valid()) {
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "loading Lua behavior module", species.id,
            "FSM registry reference is invalid");
    }

    LuaResult<LuaValue> fsmVersion =
        host.readTableField(fsmModule, "api_version");
    if (!fsmVersion) {
        return forwardFailure<
            std::unique_ptr<LuaBehaviorModule>>(
                fsmVersion.error(),
                "validating shared FSM module",
                species.behaviorFile);
    }
    if (!exactApiVersion(fsmVersion.value(), error)) {
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "validating shared FSM module", "fsm.api_version",
            std::move(error));
    }
    LuaResult<bool> fsmCreate =
        host.tableFieldIsFunction(fsmModule, "create");
    if (!fsmCreate || !fsmCreate.value()) {
        if (!fsmCreate) {
            return forwardFailure<
                std::unique_ptr<LuaBehaviorModule>>(
                    fsmCreate.error(),
                    "validating shared FSM module",
                    species.behaviorFile);
        }
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "validating shared FSM module", "fsm.create",
            "FSM module must expose a create function");
    }

    LuaResult<LuaHost::Reference> behaviorResult =
        host.loadFileReturningTable(species.behaviorFile);
    if (!behaviorResult) {
        return forwardFailure<
            std::unique_ptr<LuaBehaviorModule>>(
                behaviorResult.error(),
                "loading Lua behavior module",
                species.behaviorFile);
    }
    LuaHost::Reference behavior =
        behaviorResult.takeValue();
    LuaResult<LuaValue> behaviorVersion =
        host.readTableField(behavior, "api_version");
    if (!behaviorVersion) {
        return forwardFailure<
            std::unique_ptr<LuaBehaviorModule>>(
                behaviorVersion.error(),
                "validating Lua behavior module",
                species.behaviorFile);
    }
    if (!exactApiVersion(behaviorVersion.value(), error)) {
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "validating Lua behavior module",
            species.behaviorFile.u8string(),
            std::move(error));
    }
    LuaResult<bool> hasFactory =
        host.tableFieldIsFunction(behavior, "new");
    if (!hasFactory || !hasFactory.value()) {
        if (!hasFactory) {
            return forwardFailure<
                std::unique_ptr<LuaBehaviorModule>>(
                    hasFactory.error(),
                    "validating Lua behavior module",
                    species.behaviorFile);
        }
        return failure<std::unique_ptr<LuaBehaviorModule>>(
            LuaErrorCode::Contract,
            "validating Lua behavior module",
            species.behaviorFile.u8string(),
            "behavior module must expose a new function");
    }

    return LuaResult<
        std::unique_ptr<LuaBehaviorModule>>::success(
            std::unique_ptr<LuaBehaviorModule>(
                new LuaBehaviorModule(
                    host, species, fsmModule,
                    std::move(behavior))));
}

LuaResult<std::unique_ptr<LuaController>>
LuaBehaviorModule::createController(
    const LuaControllerConfig& requestedConfig,
    std::uint32_t seed) const {
    return createController(
        requestedConfig,
        std::make_unique<TaggedRandom>(seed));
}

LuaResult<std::unique_ptr<LuaController>>
LuaBehaviorModule::createController(
    const LuaControllerConfig& requestedConfig,
    std::unique_ptr<TaggedRandom> random) const {
    if (!host_ || !fsmModule_ ||
        !fsmModule_->valid() ||
        !behaviorModule_.valid()) {
        return failure<std::unique_ptr<LuaController>>(
            LuaErrorCode::Contract,
            "creating Lua behavior controller", species_.id,
            "behavior module dependencies are no longer valid");
    }
    if (!random) {
        return failure<std::unique_ptr<LuaController>>(
            LuaErrorCode::Contract,
            "creating Lua behavior controller", species_.id,
            "TaggedRandom must not be null");
    }
    LuaControllerConfig config = requestedConfig;
    std::string configError;
    if (!validateConfig(species_, config, configError)) {
        return failure<std::unique_ptr<LuaController>>(
            LuaErrorCode::Contract,
            "creating Lua behavior controller", species_.id,
            std::move(configError));
    }

    std::shared_ptr<TaggedRandom> sharedRandom(
        std::move(random));
    const std::weak_ptr<TaggedRandom> weakRandom =
        sharedRandom;
    LuaResult<LuaHost::Reference> apiResult =
        host_->createHostApi(
            [weakRandom](
                std::string_view tag, double low,
                double high) -> LuaResult<double> {
                const std::shared_ptr<TaggedRandom> locked =
                    weakRandom.lock();
                if (!locked) {
                    LuaError callbackError;
                    callbackError.code =
                        LuaErrorCode::HostCallback;
                    callbackError.operation =
                        "drawing controller random value";
                    callbackError.message =
                        "controller RNG no longer exists";
                    return LuaResult<double>::failure(
                        std::move(callbackError));
                }
                return locked->draw(tag, low, high);
            },
            {LuaHost::SharedReference(
                "fsm", *fsmModule_)});
    if (!apiResult) {
        return forwardFailure<
            std::unique_ptr<LuaController>>(
                apiResult.error(),
                "creating controller host API",
                species_.behaviorFile);
    }
    LuaHost::Reference hostApi =
        apiResult.takeValue();

    std::vector<LuaHost::Argument> arguments;
    arguments.emplace_back(
        configValue(species_, config));
    arguments.push_back(
        LuaHost::Argument::fromReference(hostApi));
    LuaResult<LuaHost::Reference> controllerResult =
        host_->callTableFunctionReturningTable(
            behaviorModule_, "new", arguments,
            LuaHost::CallStyle::Function);
    if (!controllerResult) {
        return forwardFailure<
            std::unique_ptr<LuaController>>(
                controllerResult.error(),
                "creating Lua behavior controller",
                species_.behaviorFile);
    }
    LuaHost::Reference controller =
        controllerResult.takeValue();
    for (const char* method : {"step", "pose"}) {
        LuaResult<bool> hasMethod =
            host_->tableFieldIsFunction(
                controller, method);
        if (!hasMethod) {
            return forwardFailure<
                std::unique_ptr<LuaController>>(
                    hasMethod.error(),
                    "validating Lua behavior controller",
                    species_.behaviorFile);
        }
        if (!hasMethod.value()) {
            return failure<std::unique_ptr<LuaController>>(
                LuaErrorCode::Contract,
                "validating Lua behavior controller",
                species_.behaviorFile.u8string(),
                std::string("controller must expose a ") +
                    method + " method");
        }
    }

    return LuaResult<std::unique_ptr<LuaController>>::success(
        std::unique_ptr<LuaController>(
            new LuaController(
                *host_, species_, config,
                std::move(sharedRandom),
                std::move(hostApi),
                std::move(controller))));
}

LuaController::LuaController(
    LuaHost& host, Species species,
    LuaControllerConfig config,
    std::shared_ptr<TaggedRandom> random,
    LuaHost::Reference hostApi,
    LuaHost::Reference controller)
    : host_(&host),
      species_(std::move(species)),
      config_(config),
      random_(std::move(random)),
      hostApi_(std::move(hostApi)),
      controller_(std::move(controller)) {
    lastPose_.parts.assign(
        species_.parts.size(), PartPose{});
}

void LuaController::quarantine(LuaError error) {
    if (quarantined_) return;
    if (error.operation.empty()) {
        error.operation = "running Lua behavior controller";
    }
    if (error.subject.empty()) {
        error.subject =
            species_.behaviorFile.u8string();
    }
    error_ = std::move(error);
    quarantined_ = true;
}

void LuaController::quarantineContract(
    std::string_view operation, std::string message) {
    LuaError contractError;
    contractError.code = LuaErrorCode::Contract;
    contractError.operation = std::string(operation);
    contractError.subject =
        species_.behaviorFile.u8string();
    contractError.message = std::move(message);
    quarantine(std::move(contractError));
}

Decision LuaController::safeStop(
    const FrameInput& frame) const {
    Decision stopped = lastDecision_;
    if (!hasSuccessfulStep_) {
        stopped.state = "quarantined";
        stopped.target = {
            finite(frame.body.position.x)
                ? frame.body.position.x
                : 0.0f,
            finite(frame.body.position.y)
                ? frame.body.position.y
                : 0.0f};
        const float heading =
            finite(frame.body.heading)
                ? frame.body.heading
                : 0.0f;
        stopped.motion.direction = {
            std::sin(heading), -std::cos(heading)};
    }
    stopped.motion.speed = 0.0f;
    stopped.motion.turnRate = 0.0f;
    stopped.motion.acceleration = 0.0f;
    stopped.motion.lateralSpeed = 0.0f;
    stopped.motion.recoveryProbePhase = 0.0f;
    stopped.motion.intentionallyStill = true;
    stopped.motion.stopImmediately = true;
    stopped.motion.cancelRecovery = true;
    stopped.motion.allowEdgeRest = true;
    stopped.motion.initialHeadingValid = false;
    stopped.motion.initialHeading = 0.0f;
    stopped.consumeBait = false;
    return stopped;
}

Decision LuaController::step(const FrameInput& frame) {
    if (quarantined_) return safeStop(frame);

    std::string frameError;
    if (!validateFrame(frame, config_, frameError)) {
        quarantineContract(
            "validating Lua step frame",
            std::move(frameError));
        return safeStop(frame);
    }

    LuaResult<std::vector<LuaValue>> result =
        host_->callTableFunction(
            controller_, "step",
            {LuaHost::Argument(
                frameValue(frame, species_))},
            LuaHost::CallStyle::Method, 1);
    if (!result) {
        LuaError scriptError = result.error();
        scriptError.operation =
            "running Lua behavior step";
        scriptError.subject =
            species_.behaviorFile.u8string();
        quarantine(std::move(scriptError));
        return safeStop(frame);
    }
    if (result.value().size() != 1) {
        quarantineContract(
            "reading Lua behavior step",
            "controller.step must return exactly one value");
        return safeStop(frame);
    }

    Decision decision;
    std::string parseError;
    if (!parseDecision(
            result.value().front(), frame,
            species_, config_, hasSuccessfulStep_,
            decision, parseError)) {
        quarantineContract(
            "validating Lua behavior step result",
            std::move(parseError));
        return safeStop(frame);
    }
    hasSuccessfulStep_ = true;
    lastDecision_ = decision;
    return decision;
}

Pose LuaController::pose(const FrameInput& frame) {
    if (quarantined_) return lastPose_;
    if (!hasSuccessfulStep_) {
        quarantineContract(
            "running Lua behavior pose",
            "controller.pose cannot run before the first "
            "successful step");
        return lastPose_;
    }

    std::string frameError;
    if (!validateFrame(frame, config_, frameError)) {
        quarantineContract(
            "validating Lua pose frame",
            std::move(frameError));
        return lastPose_;
    }
    LuaResult<std::vector<LuaValue>> result =
        host_->callTableFunction(
            controller_, "pose",
            {LuaHost::Argument(
                frameValue(frame, species_))},
            LuaHost::CallStyle::Method, 1);
    if (!result) {
        LuaError scriptError = result.error();
        scriptError.operation =
            "running Lua behavior pose";
        scriptError.subject =
            species_.behaviorFile.u8string();
        quarantine(std::move(scriptError));
        return lastPose_;
    }
    if (result.value().size() != 1) {
        quarantineContract(
            "reading Lua behavior pose",
            "controller.pose must return exactly one value");
        return lastPose_;
    }

    Pose parsed;
    std::string parseError;
    if (!parsePose(
            result.value().front(), frame,
            species_, parsed, parseError)) {
        quarantineContract(
            "validating Lua behavior pose result",
            std::move(parseError));
        return lastPose_;
    }
    lastPose_ = parsed;
    return parsed;
}

LuaResult<double> LuaController::drawRandom(
    std::string_view tag, double low, double high) {
    if (!random_) {
        return failure<double>(
            LuaErrorCode::HostCallback,
            "drawing controller random value",
            species_.id,
            "controller RNG is unavailable");
    }
    return random_->draw(tag, low, high);
}

} // namespace bug
