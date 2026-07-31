#pragma once

#include "runtime/lua_host.h"

#include <cstdint>
#include <random>
#include <string>
#include <string_view>
#include <vector>

namespace bug {

struct RandomSample {
    std::string tag;
    float low = 0.0f;
    float high = 0.0f;
    float value = 0.0f;
};

class TaggedRandom {
public:
    explicit TaggedRandom(std::uint32_t seed);

    static TaggedRandom recording(std::uint32_t seed);
    static TaggedRandom replay(std::vector<RandomSample> tape);

    LuaResult<double> draw(
        std::string_view tag, double low, double high);
    std::uint64_t drawCount() const { return drawCount_; }
    const std::vector<RandomSample>& tape() const { return tape_; }
    bool replayComplete() const;

private:
    enum class Mode {
        Generate,
        Record,
        Replay
    };

    explicit TaggedRandom(Mode mode, std::uint32_t seed);

    Mode mode_ = Mode::Generate;
    std::mt19937 generator_;
    std::vector<RandomSample> tape_;
    std::size_t replayIndex_ = 0;
    std::uint64_t drawCount_ = 0;
};

} // namespace bug
