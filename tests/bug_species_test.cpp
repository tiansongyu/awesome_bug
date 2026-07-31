#include "runtime/bug_species.h"

#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <memory>
#include <sstream>
#include <stdexcept>
#include <string>
#include <system_error>
#include <utility>
#include <vector>

namespace {

struct TemporaryTree {
    std::filesystem::path root;

    TemporaryTree() {
        const auto nonce =
            std::chrono::high_resolution_clock::now()
                .time_since_epoch()
                .count();
        root = std::filesystem::temp_directory_path() /
               ("desktop-display-species-test-" +
                std::to_string(nonce));
        std::filesystem::create_directories(root);
    }

    ~TemporaryTree() {
        std::error_code error;
        std::filesystem::remove_all(root, error);
    }

    void write(const std::filesystem::path& relative,
               const std::string& contents) const {
        const std::filesystem::path path = root / relative;
        std::filesystem::create_directories(path.parent_path());
        std::ofstream output(path, std::ios::binary);
        output.write(
            contents.data(),
            static_cast<std::streamsize>(contents.size()));
        output.close();
        if (!output) {
            throw std::runtime_error(
                "failed to write test file: " + path.u8string());
        }
    }
};

struct Manifest {
    std::string apiVersion = "1";
    std::string identifier = "'fixture'";
    std::string displayName = "'Fixture Bug'";
    std::string behavior = "'behavior.lua'";
    std::string atlasFile = "'atlas.png'";
    std::string atlasWidth = "16";
    std::string atlasHeight = "16";
    std::string referenceLength = "16";
    std::string defaultLength = "12";
    std::string overlayScale = "2";
    std::string colliderHalfWidth = "0.25";
    std::string colliderHalfLength = "0.4";
    std::string rootPart = "'body'";
    std::string capabilities = "{ bait = false }";
    std::string render =
        "{ color = { 255, 254, 253, 255 }, "
        "shadow = { color = { 0, 0, 0, 20 }, "
        "offset = { 1.5, -2.5 } } }";
    std::string parts =
        "{{ name = 'body', source = { 0, 0, 8, 8 }, "
        "pivot = { 4, 4 }, attachment = { 0, 0 }, layer = 0 }}";

    std::string text() const {
        std::ostringstream output;
        output
            << "return {\n"
            << "  api_version = " << apiVersion << ",\n"
            << "  id = " << identifier << ",\n"
            << "  name = " << displayName << ",\n"
            << "  behavior = " << behavior << ",\n"
            << "  atlas = { file = " << atlasFile
            << ", width = " << atlasWidth
            << ", height = " << atlasHeight
            << ", reference_length = " << referenceLength
            << " },\n"
            << "  body = { default_length = " << defaultLength
            << ", overlay_scale = " << overlayScale
            << ", collider_half_width = " << colliderHalfWidth
            << ", collider_half_length = " << colliderHalfLength
            << ", root_part = " << rootPart << " },\n"
            << "  capabilities = " << capabilities << ",\n"
            << "  render = " << render << ",\n"
            << "  parts = " << parts << ",\n"
            << "}\n";
        return output.str();
    }
};

void fail(bool& failed, const std::string& message) {
    std::cerr << message << '\n';
    failed = true;
}

bool near(float left, float right,
          float tolerance = 1.0e-5f) {
    return std::abs(left - right) <= tolerance;
}

bool isSourceRoot(const std::filesystem::path& candidate) {
    std::error_code error;
    return std::filesystem::is_regular_file(
               candidate / "bugs/cockroach/manifest.lua", error) &&
           !error &&
           std::filesystem::is_regular_file(
               candidate / "bugs/template/manifest.lua", error) &&
           !error &&
           std::filesystem::is_regular_file(
               candidate / "src/runtime/bug_species.cpp", error) &&
           !error;
}

std::filesystem::path findSourceRoot() {
    std::vector<std::filesystem::path> candidates;
#ifdef DESKTOP_DISPLAY_SOURCE_DIR
    candidates.emplace_back(DESKTOP_DISPLAY_SOURCE_DIR);
#endif
    std::error_code error;
    std::filesystem::path sourceFile =
        std::filesystem::absolute(__FILE__, error);
    if (!error) {
        candidates.push_back(
            sourceFile.parent_path().parent_path());
    }
    candidates.push_back(std::filesystem::current_path());

    for (std::filesystem::path candidate : candidates) {
        for (int depth = 0; depth < 10; ++depth) {
            if (isSourceRoot(candidate)) {
                return std::filesystem::weakly_canonical(candidate);
            }
            if (!candidate.has_parent_path() ||
                candidate.parent_path() == candidate) {
                break;
            }
            candidate = candidate.parent_path();
        }
    }
    return {};
}

std::filesystem::path createFixture(
    TemporaryTree& tree, const std::string& name,
    const Manifest& manifest,
    bool writeBehavior = true, bool writeAtlas = true) {
    const std::filesystem::path relativeRoot = name;
    if (writeBehavior) {
        tree.write(
            relativeRoot / "behavior.lua",
            "return { api_version = 1, new = function() return {} end }\n");
    }
    if (writeAtlas) {
        tree.write(relativeRoot / "atlas.png", "fixture");
    }
    tree.write(relativeRoot / "manifest.lua", manifest.text());
    return tree.root / relativeRoot;
}

void expectContractFailure(
    bool& failed, LuaHost& host,
    const std::filesystem::path& speciesRoot,
    const std::string& label,
    const std::string& messageFragment = {}) {
    LuaResult<bug::Species> result =
        bug::loadSpecies(host, speciesRoot);
    if (result) {
        fail(failed, label + ": invalid species was accepted");
        return;
    }
    if (result.error().code != LuaErrorCode::Contract) {
        fail(
            failed, label + ": expected contract error, got " +
                        result.error().describe());
    }
    if (!messageFragment.empty() &&
        result.error().message.find(messageFragment) ==
            std::string::npos) {
        fail(
            failed, label + ": error did not mention '" +
                        messageFragment + "': " +
                        result.error().describe());
    }
}

std::string makeParts(std::size_t count) {
    std::ostringstream output;
    output << "{\n";
    for (std::size_t index = 0; index < count; ++index) {
        output
            << "{ name = 'part_" << (index + 1)
            << "', source = { 0, 0, 1, 1 }, "
            << "pivot = { 0, 0 }, attachment = { 0, 0 }, "
            << "layer = " << index << " },\n";
    }
    output << "}";
    return output.str();
}

} // namespace

int main() {
    bool failed = false;

    LuaResult<std::unique_ptr<LuaHost>> hostResult =
        LuaHost::create();
    if (!hostResult) {
        std::cerr << hostResult.error().describe() << '\n';
        return 1;
    }
    std::unique_ptr<LuaHost> host = hostResult.takeValue();

    const std::filesystem::path sourceRoot = findSourceRoot();
    if (sourceRoot.empty()) {
        fail(failed, "could not locate the project source directory");
    } else {
        LuaResult<bug::Species> cockroachResult =
            bug::loadSpecies(
                *host, sourceRoot / "bugs/cockroach");
        if (!cockroachResult) {
            fail(
                failed,
                "real cockroach species failed: " +
                    cockroachResult.error().describe());
        } else {
            const bug::Species& species =
                cockroachResult.value();
            if (species.apiVersion != bug::apiVersion ||
                species.id != "cockroach" ||
                species.name != "Cockroach" ||
                species.parts.size() != 9 ||
                species.rootPartIndex != 0 ||
                species.parts[species.rootPartIndex].name != "body" ||
                species.atlas.width != 1536 ||
                species.atlas.height != 1024 ||
                !near(species.atlas.referenceLength, 799.0f) ||
                !near(species.body.defaultLength, 165.0f) ||
                !species.capabilities.bait ||
                species.visual.red != 190 ||
                species.visual.green != 190 ||
                species.visual.blue != 190 ||
                species.visual.alpha != 255 ||
                species.visual.shadowAlpha != 38 ||
                !species.root.is_absolute() ||
                !std::filesystem::is_regular_file(
                    species.behaviorFile) ||
                !std::filesystem::is_regular_file(
                    species.atlas.file)) {
                fail(
                    failed,
                    "real cockroach species parsed incorrectly");
            }
        }

        LuaResult<bug::Species> templateResult =
            bug::loadSpecies(
                *host, sourceRoot / "bugs/template");
        if (!templateResult) {
            fail(
                failed,
                "template species failed: " +
                    templateResult.error().describe());
        } else {
            const bug::Species& species = templateResult.value();
            if (species.id != "template" ||
                species.parts.size() != 1 ||
                species.rootPartIndex != 0 ||
                species.capabilities.bait ||
                species.visual.red != 255 ||
                species.visual.green != 255 ||
                species.visual.blue != 255 ||
                species.visual.alpha != 255 ||
                species.visual.shadowAlpha != 0) {
                fail(failed, "template species defaults are incorrect");
            }
        }
    }

    TemporaryTree tree;

    {
        Manifest manifest;
        manifest.apiVersion = "2";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "wrong-api", manifest),
            "API version", "api_version");
    }
    {
        tree.write("outside.lua", "return {}\n");
        Manifest manifest;
        manifest.behavior = "'../outside.lua'";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "path-traversal", manifest),
            "path traversal", "cannot contain");
    }
    {
        Manifest manifest;
        manifest.behavior = "'behavior.lua\\0suffix'";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "nul-path", manifest),
            "NUL path", "without NUL");
    }
    {
        Manifest manifest;
        manifest.parts =
            "{{ name = 'body', source = { 0, 0, 8, 8 }, "
            "pivot = { 4, 4 }, attachment = { 0, 0 }, layer = 0 },"
            "{ name = 'body', source = { 8, 0, 8, 8 }, "
            "pivot = { 4, 4 }, attachment = { 0, 0 }, layer = 1 }}";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "duplicate-part", manifest),
            "duplicate part", "duplicate part name");
    }
    {
        Manifest manifest;
        manifest.rootPart = "'missing'";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "missing-root", manifest),
            "missing root part", "root_part");
    }
    {
        Manifest manifest;
        manifest.parts =
            "{{ name = 'body', source = { 9, 9, 8, 8 }, "
            "pivot = { 4, 4 }, attachment = { 0, 0 }, layer = 0 }}";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "atlas-bounds", manifest),
            "atlas bounds", "outside the atlas");
    }
    {
        Manifest manifest;
        manifest.parts =
            "{{ name = 'body', "
            "source = { 2147483520, 0, 128, 1 }, "
            "pivot = { 0, 0 }, attachment = { 0, 0 }, layer = 0 }}";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "rectangle-overflow", manifest),
            "source rectangle integer overflow", "outside the atlas");
    }
    {
        Manifest manifest;
        manifest.atlasWidth = "2147483648";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "integer-overflow", manifest),
            "host integer overflow", "integer");
    }
    {
        Manifest manifest;
        manifest.referenceLength = "0 / 0";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "nan", manifest),
            "NaN", "NaN or infinity");
    }
    {
        Manifest manifest;
        manifest.overlayScale = "math.huge";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "infinity", manifest),
            "infinity", "NaN or infinity");
    }
    {
        Manifest manifest;
        expectContractFailure(
            failed, *host,
            createFixture(
                tree, "missing-behavior", manifest,
                false, true),
            "missing behavior", "regular file");
    }
    {
        Manifest manifest;
        expectContractFailure(
            failed, *host,
            createFixture(
                tree, "missing-atlas", manifest,
                true, false),
            "missing atlas", "regular file");
    }
    {
        Manifest manifest;
        manifest.rootPart = "'part_1'";
        manifest.parts = makeParts(bug::maximumParts + 1);
        expectContractFailure(
            failed, *host,
            createFixture(tree, "too-many-parts", manifest),
            "part count limit", "parts");
    }
    {
        Manifest manifest;
        manifest.render = "{ color = { 0, 1, 2, 256 } }";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "bad-render-color", manifest),
            "render color range", "0..255");
    }
    {
        Manifest manifest;
        manifest.render =
            "{ shadow = { color = { 1, 0, 0, 20 } } }";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "colored-shadow", manifest),
            "shadow RGB", "must be black");
    }
    {
        Manifest manifest;
        manifest.render =
            "{ color = { 0, 1, 2.5, 255 } }";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "fractional-color", manifest),
            "fractional render color", "integer");
    }
    {
        Manifest manifest;
        manifest.parts =
            "{{ name = 'body', source = { 0, 0, 8, 8 }, "
            "pivot = { 4, 4 }, attachment = { 33, 0 }, layer = 0 }}";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "attachment-range", manifest),
            "attachment coordinate range", "magnitude");
    }
    {
        Manifest manifest;
        manifest.capabilities = "{ bait = 'yes' }";
        expectContractFailure(
            failed, *host,
            createFixture(tree, "capability-type", manifest),
            "capability type", "boolean");
    }

    {
        tree.write("symlink-outside.lua", "return {}\n");
        Manifest manifest;
        const std::filesystem::path root =
            createFixture(tree, "symlink-escape", manifest);
        std::error_code linkError;
        std::filesystem::create_symlink(
            tree.root / "symlink-outside.lua",
            root / "linked-behavior.lua", linkError);
        if (!linkError) {
            manifest.behavior = "'linked-behavior.lua'";
            tree.write(
                "symlink-escape/manifest.lua",
                manifest.text());
            expectContractFailure(
                failed, *host, root,
                "behavior symlink escape", "escapes");
        } else {
            std::cout
                << "SKIP: filesystem does not permit test symlinks: "
                << linkError.message() << '\n';
        }
    }

    {
        Manifest manifest;
        const std::filesystem::path root =
            tree.root / "manifest-symlink-escape";
        std::filesystem::create_directories(root);
        tree.write(
            "outside-manifest.lua", manifest.text());
        std::error_code linkError;
        std::filesystem::create_symlink(
            tree.root / "outside-manifest.lua",
            root / "manifest.lua", linkError);
        if (!linkError) {
            LuaResult<bug::Species> result =
                bug::loadSpecies(*host, root);
            if (result ||
                result.error().code != LuaErrorCode::File ||
                result.error().message.find("escapes") ==
                    std::string::npos) {
                fail(
                    failed,
                    "manifest symlink escape was not rejected");
            }
        }
    }

    if (failed) {
        return 1;
    }
    std::cout << "bug species tests passed\n";
    return 0;
}
