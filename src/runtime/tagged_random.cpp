#include "runtime/tagged_random.h"

#include <algorithm>
#include <cmath>
#include <limits>
#include <utility>

namespace bug {
namespace {

LuaResult<double> randomFailure(std::string message) {
    LuaError error;
    error.code = LuaErrorCode::HostCallback;
    error.operation = "reading tagged RNG";
    error.message = std::move(message);
    return LuaResult<double>::failure(std::move(error));
}

bool sameRange(float left, float right) {
    const float scale = std::max(
        1.0f, std::max(std::abs(left), std::abs(right)));
    return std::abs(left - right) <=
           std::numeric_limits<float>::epsilon() * scale * 2.0f;
}

} // namespace

TaggedRandom::TaggedRandom(std::uint32_t seed)
    : TaggedRandom(Mode::Generate, seed) {}

TaggedRandom::TaggedRandom(Mode mode, std::uint32_t seed)
    : mode_(mode), generator_(seed) {}

TaggedRandom TaggedRandom::recording(std::uint32_t seed) {
    return TaggedRandom(Mode::Record, seed);
}

TaggedRandom TaggedRandom::replay(
    std::vector<RandomSample> tape) {
    TaggedRandom result(Mode::Replay, 0);
    result.tape_ = std::move(tape);
    return result;
}

LuaResult<double> TaggedRandom::draw(
    std::string_view tag, double low, double high) {
    if (tag.empty() || tag.size() > 256) {
        return randomFailure("tag must contain 1..256 bytes");
    }
    if (!std::isfinite(low) || !std::isfinite(high) ||
        low > high ||
        low < -std::numeric_limits<float>::max() ||
        high > std::numeric_limits<float>::max()) {
        return randomFailure("range must be finite, ordered floats");
    }
    const float floatLow = static_cast<float>(low);
    const float floatHigh = static_cast<float>(high);

    float value = 0.0f;
    if (mode_ == Mode::Replay) {
        if (replayIndex_ >= tape_.size()) {
            return randomFailure(
                "RNG tape ended at draw " +
                std::to_string(drawCount_));
        }
        const RandomSample& sample = tape_[replayIndex_];
        if (sample.tag != tag ||
            !sameRange(sample.low, floatLow) ||
            !sameRange(sample.high, floatHigh)) {
            return randomFailure(
                "RNG tape mismatch at draw " +
                std::to_string(drawCount_) +
                ": expected '" + sample.tag +
                "', requested '" + std::string(tag) + "'");
        }
        value = sample.value;
        ++replayIndex_;
    } else {
        std::uniform_real_distribution<float> distribution(
            floatLow, floatHigh);
        value = distribution(generator_);
        if (mode_ == Mode::Record) {
            tape_.push_back({
                std::string(tag), floatLow, floatHigh, value});
        }
    }
    ++drawCount_;
    return LuaResult<double>::success(
        static_cast<double>(value));
}

bool TaggedRandom::replayComplete() const {
    return mode_ != Mode::Replay ||
           replayIndex_ == tape_.size();
}

} // namespace bug
