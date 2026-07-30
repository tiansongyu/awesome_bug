#include "png_loader.h"

#include <png.h>

#include <cstdio>
#include <vector>

LoadedTexture loadPngTexture(SDL_Renderer* renderer, const std::string& path,
                             std::string& error) {
    LoadedTexture result;
    FILE* file = std::fopen(path.c_str(), "rb");
    if (!file) {
        error = "Cannot open PNG: " + path;
        return result;
    }

    png_structp png = png_create_read_struct(PNG_LIBPNG_VER_STRING, nullptr, nullptr, nullptr);
    png_infop info = png ? png_create_info_struct(png) : nullptr;
    if (!png || !info) {
        error = "libpng initialization failed";
        if (png) png_destroy_read_struct(&png, nullptr, nullptr);
        std::fclose(file);
        return result;
    }

    if (setjmp(png_jmpbuf(png))) {
        error = "libpng failed while reading: " + path;
        png_destroy_read_struct(&png, &info, nullptr);
        std::fclose(file);
        return result;
    }

    png_init_io(png, file);
    png_read_info(png, info);

    const png_uint_32 width = png_get_image_width(png, info);
    const png_uint_32 height = png_get_image_height(png, info);
    const int colorType = png_get_color_type(png, info);
    const int bitDepth = png_get_bit_depth(png, info);

    if (bitDepth == 16) png_set_strip_16(png);
    if (colorType == PNG_COLOR_TYPE_PALETTE) png_set_palette_to_rgb(png);
    if (colorType == PNG_COLOR_TYPE_GRAY && bitDepth < 8) png_set_expand_gray_1_2_4_to_8(png);
    if (png_get_valid(png, info, PNG_INFO_tRNS)) png_set_tRNS_to_alpha(png);
    if (colorType == PNG_COLOR_TYPE_GRAY || colorType == PNG_COLOR_TYPE_GRAY_ALPHA) {
        png_set_gray_to_rgb(png);
    }
    if ((colorType & PNG_COLOR_MASK_ALPHA) == 0 &&
        !png_get_valid(png, info, PNG_INFO_tRNS)) {
        png_set_add_alpha(png, 0xff, PNG_FILLER_AFTER);
    }

    png_read_update_info(png, info);
    SDL_Surface* surface = SDL_CreateRGBSurfaceWithFormat(
        0, static_cast<int>(width), static_cast<int>(height), 32, SDL_PIXELFORMAT_RGBA32);
    if (!surface) {
        error = SDL_GetError();
        png_destroy_read_struct(&png, &info, nullptr);
        std::fclose(file);
        return result;
    }

    std::vector<png_bytep> rows(height);
    auto* pixels = static_cast<unsigned char*>(surface->pixels);
    for (png_uint_32 y = 0; y < height; ++y) {
        rows[y] = pixels + y * surface->pitch;
    }
    png_read_image(png, rows.data());
    png_read_end(png, nullptr);
    png_destroy_read_struct(&png, &info, nullptr);
    std::fclose(file);

    int minX = static_cast<int>(width);
    int minY = static_cast<int>(height);
    int maxX = -1;
    int maxY = -1;
    for (int y = 0; y < static_cast<int>(height); ++y) {
        const auto* row = pixels + y * surface->pitch;
        for (int x = 0; x < static_cast<int>(width); ++x) {
            const unsigned char alpha = row[x * 4 + 3];
            if (alpha > 8) {
                minX = std::min(minX, x);
                minY = std::min(minY, y);
                maxX = std::max(maxX, x);
                maxY = std::max(maxY, y);
            }
        }
    }

    if (maxX < minX || maxY < minY) {
        error = "PNG is fully transparent: " + path;
        SDL_FreeSurface(surface);
        return result;
    }

    result.texture = SDL_CreateTextureFromSurface(renderer, surface);
    result.visibleBounds = {minX, minY, maxX - minX + 1, maxY - minY + 1};
    result.imageWidth = static_cast<int>(width);
    result.imageHeight = static_cast<int>(height);
    SDL_FreeSurface(surface);

    if (!result.texture) {
        error = SDL_GetError();
        return {};
    }
    SDL_SetTextureBlendMode(result.texture, SDL_BLENDMODE_BLEND);
    return result;
}

