#pragma once

#include <SDL.h>

#include <memory>
#include <string>

class OverlayWindow {
public:
    OverlayWindow(int size, bool clickThrough);
    OverlayWindow(int width, int height, bool clickThrough,
                  bool useBoundingShape = true);
    ~OverlayWindow();

    OverlayWindow(const OverlayWindow&) = delete;
    OverlayWindow& operator=(const OverlayWindow&) = delete;

    bool valid() const;
    const std::string& error() const;
    SDL_Renderer* renderer() const;
    SDL_Surface* canvas() const;
    int size() const;
    int width() const;
    int height() const;

    bool presentAt(int screenX, int screenY);
    void hide();
    void finishFrame();
    bool quitHotkeyPressed() const;

    static void prepareVideoDriver();

private:
    struct NativeState;
    std::unique_ptr<NativeState> native_;
    SDL_Window* window_ = nullptr;
    SDL_Surface* canvas_ = nullptr;
    SDL_Renderer* renderer_ = nullptr;
    std::string error_;
    int width_ = 0;
    int height_ = 0;
    bool clickThrough_ = true;
    bool useBoundingShape_ = true;
    bool directRenderer_ = false;
    bool shown_ = false;

    bool configureNative();
};
