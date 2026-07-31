#pragma once

#include <cstddef>
#include <cstdint>
#include <filesystem>
#include <functional>
#include <memory>
#include <optional>
#include <string>
#include <string_view>
#include <utility>
#include <variant>
#include <vector>

extern "C" {
struct lua_State;
}

enum class LuaErrorCode {
    None,
    Initialization,
    File,
    Syntax,
    Runtime,
    InstructionLimit,
    MemoryLimit,
    Contract,
    HostCallback
};

struct LuaError {
    LuaErrorCode code = LuaErrorCode::None;
    int luaStatus = 0;
    std::string operation;
    std::string subject;
    std::string message;
    std::string traceback;

    explicit operator bool() const {
        return code != LuaErrorCode::None;
    }

    std::string describe() const;
};

template <typename T>
class LuaResult {
public:
    static LuaResult success(T value) {
        LuaResult result;
        result.value_.emplace(std::move(value));
        return result;
    }

    static LuaResult failure(LuaError error) {
        LuaResult result;
        result.error_ = std::move(error);
        return result;
    }

    explicit operator bool() const {
        return value_.has_value();
    }

    bool hasValue() const {
        return value_.has_value();
    }

    T& value() {
        return *value_;
    }

    const T& value() const {
        return *value_;
    }

    T takeValue() {
        T value = std::move(*value_);
        value_.reset();
        return value;
    }

    const LuaError& error() const {
        return error_;
    }

private:
    std::optional<T> value_;
    LuaError error_;
};

struct LuaTableValue;

class LuaValue {
public:
    using Table = std::shared_ptr<LuaTableValue>;
    using Storage =
        std::variant<std::monostate, bool, double, std::string, Table>;

    LuaValue() = default;
    LuaValue(std::nullptr_t) {}
    LuaValue(bool value);
    LuaValue(double value);
    LuaValue(float value);
    LuaValue(int value);
    LuaValue(std::string value);
    LuaValue(const char* value);

    static LuaValue table(
        std::vector<std::pair<LuaValue, LuaValue>> entries);
    static LuaValue object(
        std::vector<std::pair<std::string, LuaValue>> fields);
    static LuaValue array(std::vector<LuaValue> values);

    bool isNil() const;
    const bool* boolean() const;
    const double* number() const;
    const std::string* string() const;
    const LuaTableValue* table() const;

private:
    explicit LuaValue(Table tableValue);

    Storage value_;

    friend class LuaHost;
};

struct LuaTableValue {
    std::vector<std::pair<LuaValue, LuaValue>> entries;
};

struct LuaHostStateToken;

class LuaHost {
public:
    static constexpr std::size_t defaultMemoryLimitBytes =
        32u * 1024u * 1024u;
    static constexpr int defaultInstructionLimit = 100000;

    struct Options {
        std::size_t memoryLimitBytes = defaultMemoryLimitBytes;
        int instructionLimit = defaultInstructionLimit;
        std::size_t maximumValueDepth = 32;
        std::size_t maximumTableEntries = 8192;
        std::size_t maximumStringBytes = 1024u * 1024u;
    };

    class Reference {
    public:
        Reference() = default;
        ~Reference();

        Reference(const Reference&) = delete;
        Reference& operator=(const Reference&) = delete;

        Reference(Reference&& other) noexcept;
        Reference& operator=(Reference&& other) noexcept;

        bool valid() const;
        void reset();

    private:
        Reference(std::weak_ptr<LuaHostStateToken> token,
                  int registryReference);

        std::weak_ptr<LuaHostStateToken> token_;
        int registryReference_ = -2;

        friend class LuaHost;
        friend class Argument;
    };

    class Argument {
    public:
        Argument();
        Argument(LuaValue value);

        static Argument fromReference(const Reference& reference);

    private:
        LuaValue value_;
        const Reference* reference_ = nullptr;

        friend class LuaHost;
    };

    struct SharedReference {
        std::string name;
        const Reference* reference = nullptr;

        SharedReference(std::string exposedName,
                        const Reference& sharedReference)
            : name(std::move(exposedName)),
              reference(&sharedReference) {}
    };

    enum class CallStyle {
        Function,
        Method
    };

    using RandomCallback = std::function<LuaResult<double>(
        std::string_view tag, double low, double high)>;

    static LuaResult<std::unique_ptr<LuaHost>> create();
    static LuaResult<std::unique_ptr<LuaHost>> create(
        const Options& options);

    ~LuaHost();

    LuaHost(const LuaHost&) = delete;
    LuaHost& operator=(const LuaHost&) = delete;
    LuaHost(LuaHost&&) = delete;
    LuaHost& operator=(LuaHost&&) = delete;

    LuaResult<Reference> loadFileReturningTable(
        const std::filesystem::path& path);

    LuaResult<Reference> createHostApi(std::uint64_t seed);
    LuaResult<Reference> createHostApi(
        std::uint64_t seed,
        const std::vector<SharedReference>& sharedReferences);
    LuaResult<Reference> createHostApi(RandomCallback randomCallback);
    LuaResult<Reference> createHostApi(
        RandomCallback randomCallback,
        const std::vector<SharedReference>& sharedReferences);

    LuaResult<LuaValue> readReference(
        const Reference& reference);
    LuaResult<LuaValue> readTableField(
        const Reference& table, std::string_view fieldName);
    LuaResult<bool> tableFieldIsFunction(
        const Reference& table, std::string_view fieldName);

    LuaResult<std::vector<LuaValue>> callTableFunction(
        const Reference& table, std::string_view functionName,
        const std::vector<Argument>& arguments = {},
        CallStyle style = CallStyle::Function,
        int resultCount = 1);

    LuaResult<Reference> callTableFunctionReturningTable(
        const Reference& table, std::string_view functionName,
        const std::vector<Argument>& arguments = {},
        CallStyle style = CallStyle::Function);

    std::size_t memoryUsedBytes() const;
    std::size_t memoryLimitBytes() const;
    int instructionLimit() const;
    int stackTopForTesting() const;
    void collectGarbage();

private:
    struct AllocatorState;
    struct HostApiBinding;
    struct TableFieldRequest;
    class StackGuard;
    class HookGuard;

    explicit LuaHost(const Options& options);

    static void* allocate(void* userData, void* pointer,
                          std::size_t oldSize,
                          std::size_t newSize) noexcept;
    static int openLibraries(lua_State* state);
    static int tracebackHandler(lua_State* state);
    static void instructionHook(lua_State* state,
                                struct lua_Debug* debug);

    static int hostApiNewIndex(lua_State* state);
    static int hostApiCollect(lua_State* state);
    static int hostRandom(lua_State* state);
    static int hostClamp(lua_State* state);
    static int hostWrapAngle(lua_State* state);
    static int lookupTableField(lua_State* state);

    LuaError makeError(int luaStatus, std::string operation,
                       std::string subject) const;
    LuaError contractError(std::string operation, std::string subject,
                           std::string message) const;

    std::optional<LuaError> protectedCall(
        int argumentCount, int resultCount, std::string operation,
        std::string subject);
    std::optional<LuaError> prepareTableCall(
        const Reference& table, std::string_view functionName,
        const std::vector<Argument>& arguments, CallStyle style);
    bool pushArgument(const Argument& argument, std::string& error);
    bool pushValue(const LuaValue& value, std::size_t depth,
                   std::size_t& entryCount,
                   std::vector<const LuaTableValue*>& activeTables,
                   std::string& error);
    bool pushReference(const Reference& reference,
                       std::string& error);

    bool validateFiniteValue(int index, std::size_t depth,
                             std::size_t& entryCount,
                             std::vector<const void*>& visited,
                             std::string& error) const;
    bool readValue(int index, LuaValue& value, std::size_t depth,
                   std::size_t& entryCount,
                   std::vector<const void*>& activeTables,
                   std::string& error) const;

    LuaResult<Reference> referenceValueAtTop(
        std::string operation, std::string subject,
        bool requireTable);
    LuaResult<Reference> createHostApiImpl(
        std::optional<std::uint64_t> seed,
        RandomCallback randomCallback,
        const std::vector<SharedReference>& sharedReferences);
    bool pushReadOnlyProxy(const Reference& reference,
                           std::string& error);

    lua_State* state_ = nullptr;
    Options options_;
    std::unique_ptr<AllocatorState> allocator_;
    std::shared_ptr<LuaHostStateToken> stateToken_;
};
