#include "cockroach.h"
#include "cockroach_parts.h"
#include "desktop_icons.h"
#include "display_scale.h"
#include "overlay_window.h"
#include "png_loader.h"
#include "windows_interaction.h"

#include <SDL.h>

#include <algorithm>
#include <chrono>
#include <cmath>
#include <filesystem>
#include <iostream>
#include <memory>
#include <random>
#include <string>
#include <vector>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#endif

#ifndef COCKROACH_DEFAULT_COUNT
#define COCKROACH_DEFAULT_COUNT 1
#endif

namespace {
struct Options {
    float bodySize = 165.0f;
    float speed = 3.0f;
    int display = 0;
    int count = COCKROACH_DEFAULT_COUNT;
    int maxFrames = 0;
    bool clickThrough = true;
    bool autoBodySize = true;
    bool showHelp = false;
    std::string asset;
};

#if defined(_WIN32)
SDL_Rect windowsWorkAreaForDisplay(SDL_Rect displayBounds) {
    const POINT displayCenter{
        displayBounds.x + displayBounds.w / 2,
        displayBounds.y + displayBounds.h / 2};
    const HMONITOR monitor = MonitorFromPoint(
        displayCenter, MONITOR_DEFAULTTONEAREST);
    MONITORINFO monitorInfo{};
    monitorInfo.cbSize = sizeof(monitorInfo);
    if (!monitor || !GetMonitorInfoW(monitor, &monitorInfo)) {
        return displayBounds;
    }

    const RECT& work = monitorInfo.rcWork;
    const SDL_Rect workArea{
        work.left, work.top,
        work.right - work.left,
        work.bottom - work.top};
    return workArea.w > 0 && workArea.h > 0
               ? workArea
               : displayBounds;
}
#endif

void printUsage(const char* programName) {
    const std::string executable =
        std::filesystem::path(programName).filename().string();
    std::cout
        << "Cockroach Overlay (SDL2)\n\n"
        << "Usage: " << executable << " [options]\n"
        << "  --size N              fixed body length in pixels (100..520)\n"
        << "                        (Windows default: auto, 165 at 1920x1080)\n"
        << "  --speed N             speed multiplier (0.25..3, default 3)\n"
        << "  --display N           SDL display index (default 0)\n"
        << "  --count N             number of cockroaches (1..50, default "
        << COCKROACH_DEFAULT_COUNT << ")\n"
        << "  --asset PATH          alternate compatible parts-sheet PNG\n"
        << "  --no-click-through    let the overlay receive mouse input\n"
        << "  --frames N            exit after N frames (useful for testing)\n"
        << "  --help                 show this help\n\n"
        << "Press Ctrl+Alt+Q to close the running overlay.\n";
}

bool parseNumber(const char* text, float& value) {
    try {
        std::size_t consumed = 0;
        value = std::stof(text, &consumed);
        return consumed == std::string(text).size();
    } catch (...) {
        return false;
    }
}

bool parseInteger(const char* text, int& value) {
    try {
        std::size_t consumed = 0;
        value = std::stoi(text, &consumed);
        return consumed == std::string(text).size();
    } catch (...) {
        return false;
    }
}

bool parseOptions(int argc, char** argv, Options& options, std::string& error) {
    for (int i = 1; i < argc; ++i) {
        const std::string argument = argv[i];
        if (argument == "--help" || argument == "-h") {
            options.showHelp = true;
        } else if (argument == "--no-click-through") {
            options.clickThrough = false;
        } else if (argument == "--size" || argument == "--speed" ||
                   argument == "--display" || argument == "--count" ||
                   argument == "--frames" || argument == "--asset") {
            if (++i >= argc) {
                error = "Missing value after " + argument;
                return false;
            }
            if (argument == "--size") {
                if (!parseNumber(argv[i], options.bodySize)) {
                    error = "Invalid --size value";
                    return false;
                }
                options.autoBodySize = false;
            } else if (argument == "--speed") {
                if (!parseNumber(argv[i], options.speed)) {
                    error = "Invalid --speed value";
                    return false;
                }
            } else if (argument == "--display") {
                if (!parseInteger(argv[i], options.display)) {
                    error = "Invalid --display value";
                    return false;
                }
            } else if (argument == "--count") {
                if (!parseInteger(argv[i], options.count)) {
                    error = "Invalid --count value";
                    return false;
                }
            } else if (argument == "--frames") {
                if (!parseInteger(argv[i], options.maxFrames)) {
                    error = "Invalid --frames value";
                    return false;
                }
            } else {
                options.asset = argv[i];
            }
        } else {
            error = "Unknown option: " + argument;
            return false;
        }
    }

    if (options.bodySize < 100.0f || options.bodySize > 520.0f) {
        error = "--size must be between 100 and 520";
        return false;
    }
    if (options.speed < 0.25f || options.speed > 3.0f) {
        error = "--speed must be between 0.25 and 3";
        return false;
    }
    if (options.count < 1 || options.count > 50) {
        error = "--count must be between 1 and 50";
        return false;
    }
    if (options.display < 0 || options.maxFrames < 0) {
        error = "--display and --frames cannot be negative";
        return false;
    }
    return true;
}

std::string locateAsset(const Options& options) {
    namespace fs = std::filesystem;
    if (!options.asset.empty()) return options.asset;

    std::vector<fs::path> candidates;
    if (char* basePath = SDL_GetBasePath()) {
        const fs::path base(basePath);
        candidates.push_back(
            base / "assets" / "cockroach_parts_atlas.png");
        candidates.push_back(
            base / ".." / "share" / "cockroach-overlay" /
            "assets" / "cockroach_parts_atlas.png");
        SDL_free(basePath);
    }
    candidates.emplace_back("assets/cockroach_parts_atlas.png");
#ifdef COCKROACH_SOURCE_ASSET
    candidates.emplace_back(COCKROACH_SOURCE_ASSET);
#endif
    for (const auto& candidate : candidates) {
        std::error_code ec;
        if (fs::is_regular_file(candidate, ec)) {
            return fs::absolute(candidate, ec).string();
        }
    }
    return candidates.empty() ? "assets/cockroach_parts_atlas.png"
                              : candidates.front().string();
}

void showError(const std::string& message) {
    SDL_ShowSimpleMessageBox(SDL_MESSAGEBOX_ERROR, "Cockroach Overlay",
                             message.c_str(), nullptr);
    std::cerr << message << '\n';
}

struct RoachInstance {
    std::unique_ptr<OverlayWindow> overlay;
    LoadedTexture parts;
    std::unique_ptr<Cockroach> roach;
    int overlaySize = 0;

    ~RoachInstance() {
        if (parts.texture) SDL_DestroyTexture(parts.texture);
    }
};

bool isCompatiblePartsSheet(const LoadedTexture& texture) {
    return texture.imageWidth == cockroachSheetWidth &&
           texture.imageHeight == cockroachSheetHeight;
}

std::vector<Vec2> makeSpawnPoints(SDL_Rect desktop, int count,
                                  std::mt19937& rng) {
    std::vector<Vec2> result;
    result.reserve(static_cast<std::size_t>(count));

    if (count == 1) {
        const float marginX = desktop.w * 0.08f;
        const float marginY = desktop.h * 0.08f;
        std::uniform_real_distribution<float> x(
            desktop.x + marginX, desktop.x + desktop.w - marginX);
        std::uniform_real_distribution<float> y(
            desktop.y + marginY, desktop.y + desktop.h - marginY);
        result.push_back({x(rng), y(rng)});
        return result;
    }

    const float aspect =
        desktop.h > 0 ? desktop.w / static_cast<float>(desktop.h) : 1.0f;
    const int columns = std::max(
        1, static_cast<int>(std::ceil(std::sqrt(count * aspect))));
    const int rows =
        std::max(1, static_cast<int>(std::ceil(count /
                                              static_cast<float>(columns))));
    std::vector<int> cells(static_cast<std::size_t>(columns * rows));
    for (int i = 0; i < static_cast<int>(cells.size()); ++i) cells[i] = i;
    std::shuffle(cells.begin(), cells.end(), rng);

    const float cellWidth = desktop.w / static_cast<float>(columns);
    const float cellHeight = desktop.h / static_cast<float>(rows);
    std::uniform_real_distribution<float> jitter(-0.28f, 0.28f);
    for (int i = 0; i < count; ++i) {
        const int cell = cells[static_cast<std::size_t>(i)];
        const int column = cell % columns;
        const int row = cell / columns;
        result.push_back({
            desktop.x +
                (column + 0.5f + jitter(rng)) * cellWidth,
            desktop.y +
                (row + 0.5f + jitter(rng)) * cellHeight,
        });
    }
    return result;
}
} // namespace

int main(int argc, char** argv) {
    Options options;
    std::string optionError;
    if (!parseOptions(argc, argv, options, optionError)) {
        std::cerr << optionError << "\n\n";
        printUsage(argv[0]);
        return 2;
    }
    if (options.showHelp) {
        printUsage(argv[0]);
        return 0;
    }

    OverlayWindow::prepareVideoDriver();
    SDL_SetHint(SDL_HINT_RENDER_SCALE_QUALITY, "2");
    if (SDL_Init(SDL_INIT_VIDEO | SDL_INIT_TIMER) != 0) {
        showError(std::string("SDL initialization failed: ") + SDL_GetError());
        return 1;
    }

    const int displayCount = SDL_GetNumVideoDisplays();
    if (displayCount <= 0 || options.display >= displayCount) {
        showError("The requested display does not exist");
        SDL_Quit();
        return 1;
    }
    SDL_Rect displayBounds{};
    if (SDL_GetDisplayBounds(options.display, &displayBounds) != 0) {
        showError(std::string("Cannot read display bounds: ") + SDL_GetError());
        SDL_Quit();
        return 1;
    }
    SDL_Rect desktop = displayBounds;
#if defined(_WIN32)
    desktop = windowsWorkAreaForDisplay(desktop);
    if (options.autoBodySize) {
        options.bodySize = resolutionScaledBodyLength(
            options.bodySize, displayBounds.w, displayBounds.h);
    }
#endif

    const int result = [&]() -> int {
        const std::string assetPath = locateAsset(options);
        std::mt19937 spawnRng(static_cast<unsigned int>(
            std::chrono::high_resolution_clock::now()
                .time_since_epoch()
                .count()));
        const std::vector<Vec2> spawnPoints =
            makeSpawnPoints(desktop, options.count, spawnRng);
        std::uniform_real_distribution<float> sizeScale(0.52f, 1.02f);
        std::uniform_real_distribution<float> speedScale(0.82f, 1.18f);
        DesktopIconTracker desktopIcons;
        const bool windowsSinglePet =
#if defined(_WIN32)
            options.count == 1;
#else
            false;
#endif
        WindowsInteractionController interaction(windowsSinglePet);
        std::unique_ptr<OverlayWindow> interactionOverlay;
        if (windowsSinglePet) {
            interactionOverlay = std::make_unique<OverlayWindow>(
                WindowsInteractionController::overlaySize,
                false);
            if (!interactionOverlay->valid()) {
                showError(
                    "Cannot create Windows interaction overlay: " +
                    interactionOverlay->error());
                return 1;
            }
        }

        bool sharedCanvas = false;
#if defined(__linux__)
        sharedCanvas = options.count > 1;
#endif
        std::unique_ptr<OverlayWindow> sharedOverlay;
        LoadedTexture sharedParts;
        if (sharedCanvas) {
            sharedOverlay = std::make_unique<OverlayWindow>(
                desktop.w, desktop.h, options.clickThrough, false);
            if (!sharedOverlay->valid()) {
                showError("Cannot create shared overlay: " +
                          sharedOverlay->error());
                return 1;
            }
            std::string textureError;
            sharedParts = loadPngTexture(
                sharedOverlay->renderer(), assetPath, textureError);
            if (!sharedParts.texture) {
                showError(textureError);
                return 1;
            }
            if (!isCompatiblePartsSheet(sharedParts)) {
                showError(
                    "The parts sheet must be 1536x1024 pixels: " +
                    assetPath);
                return 1;
            }
        }

        std::vector<std::unique_ptr<RoachInstance>> instances;
        instances.reserve(static_cast<std::size_t>(options.count));
        for (int index = 0; index < options.count; ++index) {
            const float bodySize =
                options.count == 1
                    ? options.bodySize
                    : options.bodySize * sizeScale(spawnRng);
            const float speed =
                options.speed *
                (options.count == 1 ? 1.0f : speedScale(spawnRng));
            const int overlaySize = static_cast<int>(
                std::ceil(std::max(210.0f, bodySize * 2.15f)));

            auto instance = std::make_unique<RoachInstance>();
            instance->overlaySize = overlaySize;
            if (!sharedCanvas) {
                instance->overlay = std::make_unique<OverlayWindow>(
                    overlaySize, options.clickThrough);
                if (!instance->overlay->valid()) {
                    showError("Cannot create overlay " +
                              std::to_string(index + 1) + ": " +
                              instance->overlay->error());
                    return 1;
                }

                std::string textureError;
                instance->parts = loadPngTexture(
                    instance->overlay->renderer(), assetPath, textureError);
                if (!instance->parts.texture) {
                    showError(textureError);
                    return 1;
                }
                if (!isCompatiblePartsSheet(instance->parts)) {
                    showError(
                        "The parts sheet must be 1536x1024 pixels: " +
                        assetPath);
                    return 1;
                }
            }
            instance->roach = std::make_unique<Cockroach>(
                desktop, overlaySize,
                RoachSettings{
                    bodySize, speed,
#if defined(_WIN32)
                    options.count == 1
#else
                    false
#endif
                },
                spawnPoints[static_cast<std::size_t>(index)]);
            instances.push_back(std::move(instance));
        }

        std::cout << "Detected display " << options.display << ": "
                  << displayBounds.w << 'x' << displayBounds.h << " at ("
                  << displayBounds.x << ", " << displayBounds.y << ").\n";
#if defined(_WIN32)
        std::cout << "Windows work area: "
                  << desktop.w << 'x' << desktop.h << " at ("
                  << desktop.x << ", " << desktop.y << ").\n";
#endif
        std::cout << "Body length: " << options.bodySize << " px";
#if defined(_WIN32)
        std::cout << (options.autoBodySize
                          ? " (resolution-scaled from 1920x1080)."
                          : " (manual override).");
#endif
        std::cout << "\nRunning " << options.count
                  << (options.count == 1 ? " cockroach. " : " cockroaches. ")
                  << "Press Ctrl+Alt+Q to exit.\n";

        bool running = true;
        int frameCount = 0;
        Uint64 previousCounter = SDL_GetPerformanceCounter();
        const double counterFrequency =
            static_cast<double>(SDL_GetPerformanceFrequency());
        Vec2 previousCursor;
        bool havePreviousCursor = false;

        while (running) {
            SDL_Event event{};
            while (SDL_PollEvent(&event)) {
                if (event.type == SDL_QUIT) running = false;
                if (event.type == SDL_KEYDOWN &&
                    ((event.key.keysym.sym == SDLK_ESCAPE &&
                      !interaction.slipperMode()) ||
                     event.key.keysym.sym == SDLK_q)) {
                    running = false;
                }
            }
            OverlayWindow* hotkeyOverlay =
                sharedCanvas ? sharedOverlay.get()
                             : instances.front()->overlay.get();
            if (hotkeyOverlay->quitHotkeyPressed()) {
                running = false;
            }

            const Uint64 now = SDL_GetPerformanceCounter();
            float dt =
                static_cast<float>((now - previousCounter) / counterFrequency);
            previousCounter = now;
            dt = std::min(dt, 0.05f);
            if (options.maxFrames > 0 && frameCount == 0) {
                dt = 1.0f / 60.0f;
            }

            int mouseX = desktop.x - 10000;
            int mouseY = desktop.y - 10000;
            SDL_GetGlobalMouseState(&mouseX, &mouseY);
            const Vec2 cursor{static_cast<float>(mouseX),
                              static_cast<float>(mouseY)};
            CockroachBehaviorInput behaviorInput;
            behaviorInput.cursorScreenPosition = cursor;
            behaviorInput.cursorValid = true;
            if (havePreviousCursor && dt > 0.0001f) {
                behaviorInput.cursorVelocity =
                    (cursor - previousCursor) * (1.0f / dt);
                const float measuredSpeed =
                    length(behaviorInput.cursorVelocity);
                constexpr float maximumMeasuredSpeed = 6000.0f;
                if (measuredSpeed > maximumMeasuredSpeed) {
                    behaviorInput.cursorVelocity =
                        normalized(behaviorInput.cursorVelocity) *
                        maximumMeasuredSpeed;
                }
            }
            previousCursor = cursor;
            havePreviousCursor = true;
            const SlipperInteractionEvents interactionEvents =
                interaction.update(dt, cursor);
            if (interactionEvents.strikeStarted) {
                const bool hit =
                    instances.front()->roach->hitTestBody(
                        interactionEvents.strikePosition);
                interaction.setStrikeHitBody(hit);
                behaviorInput.slipperStrikeStarted = true;
                behaviorInput.slipperHitBody = hit;
                behaviorInput.slipperPosition =
                    interactionEvents.strikePosition;
            }
            if (interactionEvents.strikeImpact) {
                behaviorInput.slipperImpact = true;
                behaviorInput.slipperHitBody =
                    interaction.strikeHitBody();
                behaviorInput.slipperPosition =
                    interactionEvents.strikePosition;
            }
            desktopIcons.update(
                cursor, !interaction.capturesMouse());
            if (sharedCanvas) {
                SDL_SetRenderDrawBlendMode(sharedOverlay->renderer(),
                                           SDL_BLENDMODE_NONE);
                SDL_SetRenderDrawColor(sharedOverlay->renderer(), 0, 0, 0, 0);
                SDL_RenderClear(sharedOverlay->renderer());
                SDL_SetRenderDrawBlendMode(sharedOverlay->renderer(),
                                           SDL_BLENDMODE_BLEND);
            }
            for (const auto& instance : instances) {
                instance->roach->updateWithInput(
                    dt, behaviorInput, desktopIcons.obstacles());
                const Vec2 center = instance->roach->screenCenter();
                if (sharedCanvas) {
                    instance->roach->renderAt(
                        sharedOverlay->renderer(), sharedParts,
                        Vec2{center.x - desktop.x, center.y - desktop.y});
                } else {
                    instance->roach->render(instance->overlay->renderer(),
                                            instance->parts);
                    if (!instance->overlay->presentAt(
                            static_cast<int>(std::round(
                                center.x -
                                instance->overlaySize * 0.5f)),
                            static_cast<int>(std::round(
                                center.y -
                                instance->overlaySize * 0.5f)))) {
                        showError(std::string(
                                      "Overlay presentation failed: ") +
                                  SDL_GetError());
                        running = false;
                        break;
                    }
                }
            }
            if (sharedCanvas &&
                !sharedOverlay->presentAt(desktop.x, desktop.y)) {
                showError(std::string("Overlay presentation failed: ") +
                          SDL_GetError());
                running = false;
            } else if (!sharedCanvas && !instances.empty()) {
                instances.front()->overlay->finishFrame();
            }
            if (interactionOverlay &&
                !interaction.render(
                    *interactionOverlay, cursor)) {
                showError(
                    std::string(
                        "Interaction overlay presentation failed: ") +
                    SDL_GetError());
                running = false;
            }

            ++frameCount;
            if (options.maxFrames > 0 && frameCount >= options.maxFrames) {
                running = false;
            }

            const Uint64 frameEnd = SDL_GetPerformanceCounter();
            const double elapsed = (frameEnd - now) / counterFrequency;
            constexpr double targetFrameTime = 1.0 / 60.0;
            if (elapsed < targetFrameTime) {
                SDL_Delay(static_cast<Uint32>(
                    (targetFrameTime - elapsed) * 1000.0));
            }
        }
        if (sharedParts.texture) SDL_DestroyTexture(sharedParts.texture);
        return 0;
    }();
    SDL_Quit();
    return result;
}
