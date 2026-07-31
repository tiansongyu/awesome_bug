#pragma once

#include "png_loader.h"
#include "runtime/bug_types.h"

#include <SDL.h>

#include <string>
#include <vector>

namespace bug {

class SpriteRig {
public:
    explicit SpriteRig(const Species& species);

    bool compatible(const LoadedTexture& texture,
                    std::string& error) const;
    void render(SDL_Renderer* renderer, const LoadedTexture& texture,
                const Pose& pose, const BodyState& body,
                Vec2 canvasCenter) const;

private:
    const Species* species_ = nullptr;
    std::vector<std::size_t> drawOrder_;

    void drawLayer(SDL_Renderer* renderer, const LoadedTexture& texture,
                   const Pose& pose, const BodyState& body,
                   Vec2 canvasCenter, Vec2 screenOffset) const;
};

} // namespace bug
