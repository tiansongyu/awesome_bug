#pragma once

#include <SDL.h>

#include <string>

struct LoadedTexture {
    SDL_Texture* texture = nullptr;
    SDL_Rect visibleBounds{0, 0, 0, 0};
    int imageWidth = 0;
    int imageHeight = 0;
};

LoadedTexture loadPngTexture(SDL_Renderer* renderer, const std::string& path,
                             std::string& error);

