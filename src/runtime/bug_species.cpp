#include "runtime/bug_species.h"

#include <algorithm>
#include <cmath>
#include <cstdint>
#include <exception>
#include <limits>
#include <set>
#include <string>
#include <system_error>
#include <vector>

namespace bug {
namespace {

constexpr std::size_t maximumPathBytes = 1024;
constexpr float maximumManifestCoordinate = 1000000.0f;
constexpr float maximumAttachmentCoordinate = 32.0f;

LuaResult<Species> failure(
    LuaErrorCode code, const std::filesystem::path& subject,
    std::string message) {
    LuaError error;
    error.code = code;
    error.operation = "loading bug species";
    error.subject = subject.u8string();
    error.message = std::move(message);
    return LuaResult<Species>::failure(std::move(error));
}

const LuaValue* field(const LuaValue& value, const char* name) {
    const LuaTableValue* table = value.table();
    if (!table) {
        return nullptr;
    }
    for (const auto& entry : table->entries) {
        const std::string* key = entry.first.string();
        if (key && *key == name) {
            return &entry.second;
        }
    }
    return nullptr;
}

bool requireTable(
    const LuaValue* value, const std::string& path,
    const LuaTableValue*& output, std::string& error) {
    if (!value || !(output = value->table())) {
        error = path + " must be a table";
        return false;
    }
    return true;
}

bool requireString(
    const LuaValue* value, const std::string& path,
    std::string& output, std::string& error) {
    const std::string* string = value ? value->string() : nullptr;
    if (!string || string->empty()) {
        error = path + " must be a non-empty string";
        return false;
    }
    output = *string;
    return true;
}

bool requireNumber(
    const LuaValue* value, const std::string& path,
    double& output, std::string& error) {
    const double* number = value ? value->number() : nullptr;
    if (!number || !std::isfinite(*number)) {
        error = path + " must be a finite number";
        return false;
    }
    output = *number;
    return true;
}

bool requireInteger(
    const LuaValue* value, const std::string& path,
    int& output, std::string& error) {
    double number = 0.0;
    if (!requireNumber(value, path, number, error)) {
        return false;
    }
    if (std::floor(number) != number ||
        number < static_cast<double>(std::numeric_limits<int>::min()) ||
        number > static_cast<double>(std::numeric_limits<int>::max())) {
        error = path + " must be an integer in the host range";
        return false;
    }
    output = static_cast<int>(number);
    return true;
}

bool optionalBoolean(
    const LuaValue* value, const std::string& path,
    bool fallback, bool& output, std::string& error) {
    if (!value || value->isNil()) {
        output = fallback;
        return true;
    }
    const bool* boolean = value->boolean();
    if (!boolean) {
        error = path + " must be a boolean";
        return false;
    }
    output = *boolean;
    return true;
}

bool indexedValues(
    const LuaValue* value, const std::string& path,
    std::size_t maximumEntries,
    std::vector<const LuaValue*>& output, std::string& error) {
    const LuaTableValue* table = nullptr;
    if (!requireTable(value, path, table, error)) {
        return false;
    }
    std::size_t maximum = 0;
    for (const auto& entry : table->entries) {
        const double* key = entry.first.number();
        if (!key || !std::isfinite(*key) ||
            std::floor(*key) != *key || *key < 1.0 ||
            *key > static_cast<double>(maximumEntries)) {
            error = path + " must contain only consecutive numeric keys";
            return false;
        }
        maximum = std::max(
            maximum, static_cast<std::size_t>(*key));
    }
    output.assign(maximum, nullptr);
    for (const auto& entry : table->entries) {
        const std::size_t index =
            static_cast<std::size_t>(*entry.first.number()) - 1;
        if (output[index]) {
            error = path + " contains a duplicate array index";
            return false;
        }
        output[index] = &entry.second;
    }
    if (std::find(output.begin(), output.end(), nullptr) !=
        output.end()) {
        error = path + " must not contain array holes";
        return false;
    }
    return true;
}

bool numberArray(
    const LuaValue* value, const std::string& path,
    std::size_t expectedSize, std::vector<double>& output,
    std::string& error) {
    std::vector<const LuaValue*> entries;
    if (!indexedValues(
            value, path, expectedSize, entries, error) ||
        entries.size() != expectedSize) {
        if (error.empty()) {
            error = path + " must contain exactly " +
                    std::to_string(expectedSize) + " numbers";
        }
        return false;
    }
    output.resize(entries.size());
    for (std::size_t index = 0; index < entries.size(); ++index) {
        if (!requireNumber(
                entries[index],
                path + "[" + std::to_string(index + 1) + "]",
                output[index], error)) {
            return false;
        }
    }
    return true;
}

bool validIdentifier(const std::string& value) {
    if (value.empty() || value.size() > 64) {
        return false;
    }
    for (char character : value) {
        const bool valid =
            (character >= 'a' && character <= 'z') ||
            (character >= 'A' && character <= 'Z') ||
            (character >= '0' && character <= '9') ||
            character == '_' || character == '-';
        if (!valid) {
            return false;
        }
    }
    return true;
}

bool isInside(const std::filesystem::path& root,
              const std::filesystem::path& candidate) {
    auto rootPart = root.begin();
    auto candidatePart = candidate.begin();
    while (rootPart != root.end()) {
        if (candidatePart == candidate.end() ||
            *rootPart != *candidatePart) {
            return false;
        }
        ++rootPart;
        ++candidatePart;
    }
    return true;
}

bool resolveFile(
    const std::filesystem::path& root, const std::string& raw,
    const std::string& fieldPath, std::filesystem::path& output,
    std::string& error) {
    if (raw.empty() || raw.size() > maximumPathBytes ||
        raw.find('\0') != std::string::npos) {
        error = fieldPath +
                " must be a non-empty path of at most " +
                std::to_string(maximumPathBytes) +
                " bytes without NUL";
        return false;
    }

    std::filesystem::path relative;
    try {
        relative = std::filesystem::u8path(raw);
    } catch (const std::exception&) {
        error = fieldPath + " is not a valid filesystem path";
        return false;
    }
    if (relative.empty() || relative.is_absolute() ||
        relative.has_root_name() || relative.has_root_directory()) {
        error = fieldPath + " must be a relative file path";
        return false;
    }
    for (const std::filesystem::path& component : relative) {
        if (component == ".." || component == ".") {
            error = fieldPath + " cannot contain '.' or '..'";
            return false;
        }
    }

    std::error_code filesystemError;
    const std::filesystem::path canonicalRoot =
        std::filesystem::weakly_canonical(root, filesystemError);
    if (filesystemError) {
        error = "cannot resolve species root: " +
                filesystemError.message();
        return false;
    }
    const std::filesystem::path candidate =
        std::filesystem::weakly_canonical(
            canonicalRoot / relative, filesystemError);
    if (filesystemError || !isInside(canonicalRoot, candidate)) {
        error = fieldPath + " escapes the species directory";
        return false;
    }
    if (!std::filesystem::is_regular_file(
            candidate, filesystemError) ||
        filesystemError) {
        error = fieldPath + " does not name a readable regular file: " +
                candidate.u8string();
        return false;
    }
    output = candidate;
    return true;
}

bool parseColor(
    const LuaValue* value, const std::string& path,
    std::uint8_t& red, std::uint8_t& green,
    std::uint8_t& blue, std::uint8_t& alpha,
    std::string& error) {
    std::vector<double> color;
    if (!numberArray(value, path, 4, color, error)) {
        return false;
    }
    for (std::size_t index = 0; index < color.size(); ++index) {
        if (std::floor(color[index]) != color[index] ||
            color[index] < 0.0 || color[index] > 255.0) {
            error = path + "[" + std::to_string(index + 1) +
                    "] must be an integer in 0..255";
            return false;
        }
    }
    red = static_cast<std::uint8_t>(color[0]);
    green = static_cast<std::uint8_t>(color[1]);
    blue = static_cast<std::uint8_t>(color[2]);
    alpha = static_cast<std::uint8_t>(color[3]);
    return true;
}

bool positive(
    double value, double maximum,
    const std::string& path, std::string& error) {
    if (value <= 0.0 || value > maximum) {
        error = path + " must be in (0, " +
                std::to_string(maximum) + "]";
        return false;
    }
    return true;
}

bool boundedFloat(
    double value, float maximumMagnitude,
    const std::string& path, float& output,
    std::string& error) {
    if (!std::isfinite(value) ||
        value < -static_cast<double>(maximumMagnitude) ||
        value > static_cast<double>(maximumMagnitude)) {
        error = path + " must be finite and have magnitude at most " +
                std::to_string(maximumMagnitude);
        return false;
    }
    output = static_cast<float>(value);
    if (!std::isfinite(output)) {
        error = path + " is outside the host numeric range";
        return false;
    }
    return true;
}

bool parsePoint(
    const LuaValue* value, const std::string& path,
    float maximumMagnitude, Vec2& output,
    std::string& error) {
    std::vector<double> values;
    if (!numberArray(value, path, 2, values, error)) {
        return false;
    }
    return boundedFloat(
               values[0], maximumMagnitude, path + "[1]",
               output.x, error) &&
           boundedFloat(
               values[1], maximumMagnitude, path + "[2]",
               output.y, error);
}

} // namespace

LuaResult<Species> loadSpecies(
    LuaHost& host, const std::filesystem::path& speciesRoot) {
    std::error_code filesystemError;
    if (!std::filesystem::is_directory(
            speciesRoot, filesystemError) ||
        filesystemError) {
        return failure(
            LuaErrorCode::File, speciesRoot,
            "species path is not a readable directory");
    }

    const std::filesystem::path canonicalRoot =
        std::filesystem::weakly_canonical(
            speciesRoot, filesystemError);
    if (filesystemError ||
        !std::filesystem::is_directory(
            canonicalRoot, filesystemError) ||
        filesystemError) {
        return failure(
            LuaErrorCode::File, speciesRoot,
            "cannot canonicalize species directory");
    }

    std::filesystem::path manifestPath;
    std::string error;
    if (!resolveFile(
            canonicalRoot, "manifest.lua", "manifest",
            manifestPath, error)) {
        return failure(
            LuaErrorCode::File, speciesRoot, std::move(error));
    }
    LuaResult<LuaHost::Reference> moduleResult =
        host.loadFileReturningTable(manifestPath);
    if (!moduleResult) {
        return LuaResult<Species>::failure(moduleResult.error());
    }
    LuaHost::Reference module = moduleResult.takeValue();
    LuaResult<LuaValue> valueResult = host.readReference(module);
    if (!valueResult) {
        return LuaResult<Species>::failure(valueResult.error());
    }
    const LuaValue manifest = valueResult.takeValue();
    if (!manifest.table()) {
        return failure(
            LuaErrorCode::Contract, manifestPath,
            "manifest must return a table");
    }

    Species species;
    if (!requireInteger(
            field(manifest, "api_version"),
            "api_version", species.apiVersion, error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    if (species.apiVersion != apiVersion) {
        return failure(
            LuaErrorCode::Contract, manifestPath,
            "api_version must be " + std::to_string(apiVersion));
    }
    if (!requireString(
            field(manifest, "id"), "id", species.id, error) ||
        !validIdentifier(species.id)) {
        if (error.empty()) {
            error = "id must contain 1..64 ASCII letters, digits, '-' or '_'";
        }
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    if (!requireString(
            field(manifest, "name"), "name", species.name, error) ||
        species.name.size() > 128) {
        if (error.empty()) {
            error = "name must contain at most 128 bytes";
        }
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }

    std::string behaviorFile;
    if (!requireString(
            field(manifest, "behavior"),
            "behavior", behaviorFile, error) ||
        !resolveFile(
            canonicalRoot, behaviorFile, "behavior",
            species.behaviorFile, error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }

    const LuaValue* atlasValue = field(manifest, "atlas");
    const LuaTableValue* atlasTable = nullptr;
    if (!requireTable(atlasValue, "atlas", atlasTable, error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    std::string atlasFile;
    double number = 0.0;
    if (!requireString(
            field(*atlasValue, "file"),
            "atlas.file", atlasFile, error) ||
        !resolveFile(
            canonicalRoot, atlasFile, "atlas.file",
            species.atlas.file, error) ||
        !requireInteger(
            field(*atlasValue, "width"),
            "atlas.width", species.atlas.width, error) ||
        !requireInteger(
            field(*atlasValue, "height"),
            "atlas.height", species.atlas.height, error) ||
        !requireNumber(
            field(*atlasValue, "reference_length"),
            "atlas.reference_length", number, error) ||
        !positive(number, 100000.0, "atlas.reference_length", error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    (void)atlasTable;
    if (species.atlas.width <= 0 || species.atlas.width > 16384 ||
        species.atlas.height <= 0 || species.atlas.height > 16384) {
        return failure(
            LuaErrorCode::Contract, manifestPath,
            "atlas width and height must be in 1..16384");
    }
    species.atlas.referenceLength = static_cast<float>(number);

    const LuaValue* bodyValue = field(manifest, "body");
    const LuaTableValue* bodyTable = nullptr;
    if (!requireTable(bodyValue, "body", bodyTable, error) ||
        !requireNumber(
            field(*bodyValue, "default_length"),
            "body.default_length", number, error) ||
        !positive(number, 100000.0, "body.default_length", error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    species.body.defaultLength = static_cast<float>(number);
    if (!requireNumber(
            field(*bodyValue, "overlay_scale"),
            "body.overlay_scale", number, error) ||
        !positive(number, 32.0, "body.overlay_scale", error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    species.body.overlayScale = static_cast<float>(number);
    if (!requireNumber(
            field(*bodyValue, "collider_half_width"),
            "body.collider_half_width", number, error) ||
        !positive(number, 2.0, "body.collider_half_width", error)) {
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    species.body.colliderHalfWidth = static_cast<float>(number);
    if (!requireNumber(
            field(*bodyValue, "collider_half_length"),
            "body.collider_half_length", number, error) ||
        !positive(number, 2.0, "body.collider_half_length", error) ||
        !requireString(
            field(*bodyValue, "root_part"),
            "body.root_part", species.body.rootPart, error) ||
        !validIdentifier(species.body.rootPart)) {
        if (error.empty()) {
            error = "body.root_part is not a valid part name";
        }
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }
    species.body.colliderHalfLength = static_cast<float>(number);
    (void)bodyTable;

    if (const LuaValue* capabilities =
            field(manifest, "capabilities")) {
        const LuaTableValue* capabilitiesTable = nullptr;
        if (!requireTable(
                capabilities, "capabilities",
                capabilitiesTable, error) ||
            !optionalBoolean(
                field(*capabilities, "bait"),
                "capabilities.bait", false,
                species.capabilities.bait, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        (void)capabilitiesTable;
    }

    if (const LuaValue* render = field(manifest, "render")) {
        const LuaTableValue* renderTable = nullptr;
        if (!requireTable(render, "render", renderTable, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        if (const LuaValue* color = field(*render, "color")) {
            if (!parseColor(
                    color, "render.color",
                    species.visual.red, species.visual.green,
                    species.visual.blue, species.visual.alpha,
                    error)) {
                return failure(
                    LuaErrorCode::Contract, manifestPath,
                    std::move(error));
            }
        }
        if (const LuaValue* shadow = field(*render, "shadow")) {
            const LuaTableValue* shadowTable = nullptr;
            if (!requireTable(
                    shadow, "render.shadow",
                    shadowTable, error)) {
                return failure(
                    LuaErrorCode::Contract, manifestPath,
                    std::move(error));
            }
            std::uint8_t shadowRed = 0;
            std::uint8_t shadowGreen = 0;
            std::uint8_t shadowBlue = 0;
            if (const LuaValue* shadowColor =
                    field(*shadow, "color")) {
                if (!parseColor(
                        shadowColor, "render.shadow.color",
                        shadowRed, shadowGreen, shadowBlue,
                        species.visual.shadowAlpha, error) ||
                    shadowRed != 0 || shadowGreen != 0 ||
                    shadowBlue != 0) {
                    if (error.empty()) {
                        error =
                            "render.shadow.color RGB must be black";
                    }
                    return failure(
                        LuaErrorCode::Contract, manifestPath,
                        std::move(error));
                }
            }
            if (const LuaValue* offset =
                    field(*shadow, "offset")) {
                if (!parsePoint(
                        offset, "render.shadow.offset",
                        maximumManifestCoordinate,
                        species.visual.shadowOffset, error)) {
                    return failure(
                        LuaErrorCode::Contract, manifestPath,
                        std::move(error));
                }
            }
            (void)shadowTable;
        }
        (void)renderTable;
    }

    std::vector<const LuaValue*> partValues;
    if (!indexedValues(
            field(manifest, "parts"), "parts",
            maximumParts,
            partValues, error) ||
        partValues.empty() ||
        partValues.size() > maximumParts) {
        if (error.empty()) {
            error = "parts must contain 1.." +
                    std::to_string(maximumParts) + " entries";
        }
        return failure(
            LuaErrorCode::Contract, manifestPath, std::move(error));
    }

    std::set<std::string> names;
    species.parts.reserve(partValues.size());
    for (std::size_t index = 0; index < partValues.size(); ++index) {
        const std::string path =
            "parts[" + std::to_string(index + 1) + "]";
        const LuaValue& partValue = *partValues[index];
        const LuaTableValue* partTable = nullptr;
        PartDefinition part;
        if (!requireTable(
                &partValue, path, partTable, error) ||
            !requireString(
                field(partValue, "name"), path + ".name",
                part.name, error) ||
            !validIdentifier(part.name)) {
            if (error.empty()) {
                error = path + ".name is not a valid identifier";
            }
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        if (!names.insert(part.name).second) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                "duplicate part name: " + part.name);
        }

        std::vector<double> values;
        if (!numberArray(
                field(partValue, "source"), path + ".source",
                4, values, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        for (double value : values) {
            if (std::floor(value) != value ||
                value < 0.0 ||
                value > static_cast<double>(
                    std::numeric_limits<int>::max())) {
                return failure(
                    LuaErrorCode::Contract, manifestPath,
                    path + ".source must contain non-negative integers");
            }
        }
        part.source = {
            static_cast<int>(values[0]),
            static_cast<int>(values[1]),
            static_cast<int>(values[2]),
            static_cast<int>(values[3])};
        const std::int64_t sourceRight =
            static_cast<std::int64_t>(part.source.x) +
            static_cast<std::int64_t>(part.source.width);
        const std::int64_t sourceBottom =
            static_cast<std::int64_t>(part.source.y) +
            static_cast<std::int64_t>(part.source.height);
        if (part.source.width <= 0 || part.source.height <= 0 ||
            sourceRight > species.atlas.width ||
            sourceBottom > species.atlas.height) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                path + ".source lies outside the atlas");
        }

        if (!parsePoint(
                field(partValue, "pivot"), path + ".pivot",
                maximumManifestCoordinate, part.pivot, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        if (!parsePoint(
                field(partValue, "attachment"),
                path + ".attachment",
                maximumAttachmentCoordinate,
                part.attachment, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        if (!requireInteger(
                field(partValue, "layer"),
                path + ".layer", part.layer, error)) {
            return failure(
                LuaErrorCode::Contract, manifestPath,
                std::move(error));
        }
        (void)partTable;
        species.parts.push_back(std::move(part));
    }

    const auto rootPart = std::find_if(
        species.parts.begin(), species.parts.end(),
        [&](const PartDefinition& part) {
            return part.name == species.body.rootPart;
        });
    if (rootPart == species.parts.end()) {
        return failure(
            LuaErrorCode::Contract, manifestPath,
            "body.root_part does not name exactly one part");
    }
    species.rootPartIndex =
        static_cast<std::size_t>(
            std::distance(species.parts.begin(), rootPart));
    species.root = canonicalRoot;
    return LuaResult<Species>::success(std::move(species));
}

} // namespace bug
