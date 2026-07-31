#include "runtime/sprite_rig.h"

#include <algorithm>
#include <cmath>
#include <numeric>

namespace bug {
namespace {
constexpr float pi = 3.14159265358979323846f;

SDL_Rect sourceRect(const SourceRect& source) {
    return {source.x, source.y, source.width, source.height};
}

void drawPart(SDL_Renderer* renderer, const LoadedTexture& texture,
              const PartDefinition& part, Vec2 joint, float scale,
              float rotation) {
    const SDL_Rect source = sourceRect(part.source);
    const SDL_FRect destination{
        joint.x - part.pivot.x * scale,
        joint.y - part.pivot.y * scale,
        part.source.width * scale,
        part.source.height * scale};
    const SDL_FPoint pivot{
        part.pivot.x * scale,
        part.pivot.y * scale};
    SDL_RenderCopyExF(
        renderer, texture.texture, &source, &destination,
        rotation * 180.0 / pi, &pivot, SDL_FLIP_NONE);
}

} // namespace

SpriteRig::SpriteRig(const Species& species)
    : species_(&species),
      drawOrder_(species.parts.size()) {
    std::iota(drawOrder_.begin(), drawOrder_.end(), std::size_t{0});
    std::stable_sort(
        drawOrder_.begin(), drawOrder_.end(),
        [&](std::size_t left, std::size_t right) {
            return species_->parts[left].layer <
                   species_->parts[right].layer;
        });
}

bool SpriteRig::compatible(const LoadedTexture& texture,
                           std::string& error) const {
    if (texture.imageWidth != species_->atlas.width ||
        texture.imageHeight != species_->atlas.height) {
        error =
            "atlas dimensions are " +
            std::to_string(texture.imageWidth) + "x" +
            std::to_string(texture.imageHeight) +
            ", manifest requires " +
            std::to_string(species_->atlas.width) + "x" +
            std::to_string(species_->atlas.height);
        return false;
    }
    return true;
}

void SpriteRig::drawLayer(
    SDL_Renderer* renderer, const LoadedTexture& texture,
    const Pose& pose, const BodyState& body,
    Vec2 canvasCenter, Vec2 screenOffset) const {
    const float poseHeading = body.heading + pose.bodyRotation;
    const Vec2 bodyCenter =
        canvasCenter +
        rotateLocal(pose.bodyOffset, poseHeading);
    const float spriteScale =
        body.length / species_->atlas.referenceLength;

    for (std::size_t index : drawOrder_) {
        const PartDefinition& part = species_->parts[index];
        const PartPose& partPose = pose.parts[index];
        const Vec2 localJoint =
            part.attachment * body.length + partPose.jointOffset;
        const Vec2 joint =
            bodyCenter + rotateLocal(localJoint, poseHeading) +
            screenOffset;
        drawPart(
            renderer, texture, part, joint, spriteScale,
            poseHeading + partPose.rotation);
    }
}

void SpriteRig::render(
    SDL_Renderer* renderer, const LoadedTexture& texture,
    const Pose& pose, const BodyState& body,
    Vec2 canvasCenter) const {
    if (!renderer || !texture.texture ||
        pose.parts.size() != species_->parts.size()) {
        return;
    }

    SDL_SetTextureColorMod(texture.texture, 0, 0, 0);
    SDL_SetTextureAlphaMod(
        texture.texture, species_->visual.shadowAlpha);
    drawLayer(
        renderer, texture, pose, body, canvasCenter,
        species_->visual.shadowOffset);

    SDL_SetTextureColorMod(
        texture.texture,
        species_->visual.red,
        species_->visual.green,
        species_->visual.blue);
    SDL_SetTextureAlphaMod(
        texture.texture, species_->visual.alpha);
    drawLayer(renderer, texture, pose, body, canvasCenter, {});
}

} // namespace bug
