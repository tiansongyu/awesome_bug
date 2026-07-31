#include "runtime/lua_host.h"

#include <algorithm>
#include <cmath>
#include <cstdio>
#include <cstdlib>
#include <cstring>
#include <limits>
#include <new>
#include <random>
#include <sstream>
#include <stdexcept>

extern "C" {
#if __has_include(<lua.h>)
#include <lua.h>
#include <lauxlib.h>
#include <lualib.h>
#elif __has_include(<lua5.4/lua.h>)
#include <lua5.4/lua.h>
#include <lua5.4/lauxlib.h>
#include <lua5.4/lualib.h>
#else
#error "Lua 5.4 headers are required to build lua_host.cpp"
#endif
}

static_assert(sizeof(lua_Number) == sizeof(float),
              "embedded Lua ABI must use single-precision lua_Number");
static_assert(sizeof(lua_Integer) >= sizeof(std::int64_t),
              "embedded Lua ABI must retain 64-bit integers");

namespace {
constexpr const char* hostApiBindingMetatable =
    "desktop_display.lua_host_api_binding.v1";
constexpr const char* instructionLimitMarker =
    "__desktop_display_instruction_limit__";
constexpr const char* hostCallbackMarker =
    "__desktop_display_host_callback__";
constexpr double pi = 3.14159265358979323846264338327950288;

const char* errorCodeName(LuaErrorCode code) {
    switch (code) {
    case LuaErrorCode::None:
        return "none";
    case LuaErrorCode::Initialization:
        return "initialization";
    case LuaErrorCode::File:
        return "file";
    case LuaErrorCode::Syntax:
        return "syntax";
    case LuaErrorCode::Runtime:
        return "runtime";
    case LuaErrorCode::InstructionLimit:
        return "instruction-limit";
    case LuaErrorCode::MemoryLimit:
        return "memory-limit";
    case LuaErrorCode::Contract:
        return "contract";
    case LuaErrorCode::HostCallback:
        return "host-callback";
    }
    return "unknown";
}

bool containsPointer(const std::vector<const void*>& values,
                     const void* value) {
    return std::find(values.begin(), values.end(), value) != values.end();
}

bool containsTablePointer(
    const std::vector<const LuaTableValue*>& values,
    const LuaTableValue* value) {
    return std::find(values.begin(), values.end(), value) != values.end();
}

std::string firstLine(const std::string& text) {
    const std::size_t newline = text.find('\n');
    return text.substr(0, newline);
}
} // namespace

struct LuaHostStateToken {
    lua_State* state = nullptr;
    bool alive = false;
};

struct LuaHost::AllocatorState {
    std::size_t used = 0;
    std::size_t limit = defaultMemoryLimitBytes;
    bool denied = false;
};

struct LuaHost::HostApiBinding {
    LuaHost* owner = nullptr;
    RandomCallback callback;
    std::mt19937_64 generator;
    bool usesCallback = false;

    HostApiBinding(LuaHost* host, std::uint64_t seed)
        : owner(host), generator(seed) {}

    HostApiBinding(LuaHost* host, RandomCallback randomCallback)
        : owner(host),
          callback(std::move(randomCallback)),
          generator(0),
          usesCallback(true) {}
};

struct LuaHost::TableFieldRequest {
    const char* data = nullptr;
    std::size_t size = 0;
};

class LuaHost::StackGuard {
public:
    explicit StackGuard(lua_State* state)
        : state_(state), top_(lua_gettop(state)) {}

    ~StackGuard() {
        if (state_) lua_settop(state_, top_);
    }

    int top() const {
        return top_;
    }

private:
    lua_State* state_ = nullptr;
    int top_ = 0;
};

class LuaHost::HookGuard {
public:
    HookGuard(lua_State* state, int instructionLimit)
        : state_(state),
          oldHook_(lua_gethook(state)),
          oldMask_(lua_gethookmask(state)),
          oldCount_(lua_gethookcount(state)) {
        lua_sethook(state_, &LuaHost::instructionHook,
                    LUA_MASKCOUNT, instructionLimit);
    }

    ~HookGuard() {
        if (state_) {
            lua_sethook(state_, oldHook_, oldMask_, oldCount_);
        }
    }

private:
    lua_State* state_ = nullptr;
    lua_Hook oldHook_ = nullptr;
    int oldMask_ = 0;
    int oldCount_ = 0;
};

std::string LuaError::describe() const {
    std::ostringstream output;
    output << "Lua " << errorCodeName(code) << " error";
    if (!operation.empty()) output << " while " << operation;
    if (!subject.empty()) output << " [" << subject << ']';
    if (!message.empty()) output << ": " << message;
    return output.str();
}

LuaValue::LuaValue(bool value) : value_(value) {}
LuaValue::LuaValue(double value) : value_(value) {}
LuaValue::LuaValue(float value) : value_(static_cast<double>(value)) {}
LuaValue::LuaValue(int value) : value_(static_cast<double>(value)) {}
LuaValue::LuaValue(std::string value) : value_(std::move(value)) {}
LuaValue::LuaValue(const char* value)
    : value_(std::string(value ? value : "")) {}
LuaValue::LuaValue(Table tableValue)
    : value_(std::move(tableValue)) {}

LuaValue LuaValue::table(
    std::vector<std::pair<LuaValue, LuaValue>> entries) {
    auto tableValue = std::make_shared<LuaTableValue>();
    tableValue->entries = std::move(entries);
    return LuaValue(std::move(tableValue));
}

LuaValue LuaValue::object(
    std::vector<std::pair<std::string, LuaValue>> fields) {
    std::vector<std::pair<LuaValue, LuaValue>> entries;
    entries.reserve(fields.size());
    for (auto& field : fields) {
        entries.emplace_back(
            LuaValue(std::move(field.first)),
            std::move(field.second));
    }
    return table(std::move(entries));
}

LuaValue LuaValue::array(std::vector<LuaValue> values) {
    std::vector<std::pair<LuaValue, LuaValue>> entries;
    entries.reserve(values.size());
    for (std::size_t index = 0; index < values.size(); ++index) {
        entries.emplace_back(
            LuaValue(static_cast<double>(index + 1)),
            std::move(values[index]));
    }
    return table(std::move(entries));
}

bool LuaValue::isNil() const {
    return std::holds_alternative<std::monostate>(value_);
}

const bool* LuaValue::boolean() const {
    return std::get_if<bool>(&value_);
}

const double* LuaValue::number() const {
    return std::get_if<double>(&value_);
}

const std::string* LuaValue::string() const {
    return std::get_if<std::string>(&value_);
}

const LuaTableValue* LuaValue::table() const {
    const Table* tableValue = std::get_if<Table>(&value_);
    return tableValue && *tableValue ? tableValue->get() : nullptr;
}

LuaHost::Reference::Reference(
    std::weak_ptr<LuaHostStateToken> token,
    int registryReference)
    : token_(std::move(token)),
      registryReference_(registryReference) {}

LuaHost::Reference::~Reference() {
    reset();
}

LuaHost::Reference::Reference(Reference&& other) noexcept
    : token_(std::move(other.token_)),
      registryReference_(other.registryReference_) {
    other.registryReference_ = LUA_NOREF;
}

LuaHost::Reference& LuaHost::Reference::operator=(
    Reference&& other) noexcept {
    if (this == &other) return *this;
    reset();
    token_ = std::move(other.token_);
    registryReference_ = other.registryReference_;
    other.registryReference_ = LUA_NOREF;
    return *this;
}

bool LuaHost::Reference::valid() const {
    const std::shared_ptr<LuaHostStateToken> token = token_.lock();
    return token && token->alive && token->state &&
           registryReference_ != LUA_NOREF &&
           registryReference_ != LUA_REFNIL;
}

void LuaHost::Reference::reset() {
    const std::shared_ptr<LuaHostStateToken> token = token_.lock();
    if (token && token->alive && token->state &&
        registryReference_ != LUA_NOREF &&
        registryReference_ != LUA_REFNIL) {
        luaL_unref(token->state, LUA_REGISTRYINDEX,
                   registryReference_);
    }
    registryReference_ = LUA_NOREF;
    token_.reset();
}

LuaHost::Argument::Argument() = default;

LuaHost::Argument::Argument(LuaValue value)
    : value_(std::move(value)) {}

LuaHost::Argument LuaHost::Argument::fromReference(
    const Reference& reference) {
    Argument argument;
    argument.reference_ = &reference;
    return argument;
}

LuaHost::LuaHost(const Options& options)
    : options_(options),
      allocator_(std::make_unique<AllocatorState>()),
      stateToken_(std::make_shared<LuaHostStateToken>()) {
    allocator_->limit = options_.memoryLimitBytes;
}

LuaHost::~LuaHost() {
    if (stateToken_) {
        stateToken_->alive = false;
    }
    if (state_) {
        lua_close(state_);
        state_ = nullptr;
    }
    if (stateToken_) {
        stateToken_->state = nullptr;
    }
}

LuaResult<std::unique_ptr<LuaHost>> LuaHost::create() {
    return create(Options{});
}

LuaResult<std::unique_ptr<LuaHost>> LuaHost::create(
    const Options& options) {
    if (options.memoryLimitBytes == 0 ||
        options.instructionLimit <= 0 ||
        options.maximumValueDepth == 0 ||
        options.maximumTableEntries == 0 ||
        options.maximumStringBytes == 0) {
        LuaError error;
        error.code = LuaErrorCode::Initialization;
        error.operation = "validating Lua host options";
        error.message = "all resource limits must be positive";
        return LuaResult<std::unique_ptr<LuaHost>>::failure(
            std::move(error));
    }

    std::unique_ptr<LuaHost> host(new LuaHost(options));
    host->state_ = lua_newstate(&LuaHost::allocate,
                                host->allocator_.get());
    if (!host->state_) {
        LuaError error;
        error.code = LuaErrorCode::MemoryLimit;
        error.operation = "creating Lua state";
        error.message =
            "Lua could not allocate its initial state within the " +
            std::to_string(options.memoryLimitBytes) +
            " byte budget";
        return LuaResult<std::unique_ptr<LuaHost>>::failure(
            std::move(error));
    }

    host->stateToken_->state = host->state_;
    host->stateToken_->alive = true;
    *static_cast<LuaHost**>(lua_getextraspace(host->state_)) =
        host.get();

    StackGuard stack(host->state_);
    lua_pushcfunction(host->state_, &LuaHost::openLibraries);
    if (std::optional<LuaError> error = host->protectedCall(
            0, 0, "opening sandbox libraries", {})) {
        return LuaResult<std::unique_ptr<LuaHost>>::failure(
            std::move(*error));
    }
    return LuaResult<std::unique_ptr<LuaHost>>::success(
        std::move(host));
}

void* LuaHost::allocate(void* userData, void* pointer,
                        std::size_t oldSize,
                        std::size_t newSize) noexcept {
    auto* allocator = static_cast<AllocatorState*>(userData);
    if (!allocator) return nullptr;
    if (!pointer) oldSize = 0;

    if (newSize == 0) {
        std::free(pointer);
        allocator->used =
            oldSize <= allocator->used
                ? allocator->used - oldSize
                : 0;
        return nullptr;
    }

    const std::size_t increase =
        newSize > oldSize ? newSize - oldSize : 0;
    if (increase > allocator->limit ||
        allocator->used > allocator->limit - increase) {
        allocator->denied = true;
        return nullptr;
    }

    void* replacement = std::realloc(pointer, newSize);
    if (!replacement) {
        allocator->denied = true;
        return nullptr;
    }
    allocator->used =
        allocator->used - std::min(oldSize, allocator->used) +
        newSize;
    return replacement;
}

int LuaHost::openLibraries(lua_State* state) {
    const luaL_Reg libraries[] = {
        {LUA_GNAME, luaopen_base},
        {LUA_TABLIBNAME, luaopen_table},
        {LUA_STRLIBNAME, luaopen_string},
        {LUA_MATHLIBNAME, luaopen_math},
        {LUA_UTF8LIBNAME, luaopen_utf8},
        {nullptr, nullptr}};

    for (const luaL_Reg* library = libraries;
         library->name; ++library) {
        luaL_requiref(state, library->name,
                      library->func, 1);
        lua_pop(state, 1);
    }

    const char* removedGlobals[] = {
        "dofile", "load", "loadfile", "collectgarbage",
        "require", "package", "io", "os", "debug", "coroutine",
        "rawset",
        nullptr};
    for (const char** name = removedGlobals; *name; ++name) {
        lua_pushnil(state);
        lua_setglobal(state, *name);
    }

    lua_getglobal(state, LUA_MATHLIBNAME);
    if (lua_istable(state, -1)) {
        lua_pushnil(state);
        lua_setfield(state, -2, "random");
        lua_pushnil(state);
        lua_setfield(state, -2, "randomseed");
    }
    lua_pop(state, 1);
    return 0;
}

int LuaHost::tracebackHandler(lua_State* state) {
    const char* message = lua_tostring(state, 1);
    if (message) {
        luaL_traceback(state, state, message, 1);
    } else {
        lua_pushliteral(state,
                        "Lua raised a non-string error object");
    }
    return 1;
}

void LuaHost::instructionHook(lua_State* state,
                              lua_Debug*) {
    luaL_error(state, "%s", instructionLimitMarker);
}

LuaError LuaHost::makeError(
    int luaStatus, std::string operation,
    std::string subject) const {
    LuaError error;
    error.luaStatus = luaStatus;
    error.operation = std::move(operation);
    error.subject = std::move(subject);

    const char* text =
        state_ && lua_gettop(state_) > 0
            ? lua_tostring(state_, -1)
            : nullptr;
    error.traceback =
        text ? text : "Lua did not provide an error message";
    error.message = firstLine(error.traceback);

    if ((allocator_ && allocator_->denied) ||
        luaStatus == LUA_ERRMEM) {
        error.code = LuaErrorCode::MemoryLimit;
        error.message =
            "Lua exceeded the " +
            std::to_string(options_.memoryLimitBytes) +
            " byte memory budget";
    } else if (error.traceback.find(instructionLimitMarker) !=
               std::string::npos) {
        error.code = LuaErrorCode::InstructionLimit;
        error.message =
            "Lua exceeded the " +
            std::to_string(options_.instructionLimit) +
            " instruction budget";
    } else if (error.traceback.find(hostCallbackMarker) !=
               std::string::npos) {
        error.code = LuaErrorCode::HostCallback;
        const std::size_t marker =
            error.message.find(hostCallbackMarker);
        if (marker != std::string::npos) {
            error.message.erase(marker,
                                std::strlen(hostCallbackMarker));
            while (!error.message.empty() &&
                   (error.message.front() == ':' ||
                    error.message.front() == ' ')) {
                error.message.erase(error.message.begin());
            }
        }
    } else if (luaStatus == LUA_ERRSYNTAX) {
        error.code = LuaErrorCode::Syntax;
    } else if (luaStatus == LUA_ERRFILE) {
        error.code = LuaErrorCode::File;
    } else {
        error.code = LuaErrorCode::Runtime;
    }
    return error;
}

LuaError LuaHost::contractError(
    std::string operation, std::string subject,
    std::string message) const {
    LuaError error;
    error.code = LuaErrorCode::Contract;
    error.operation = std::move(operation);
    error.subject = std::move(subject);
    error.message = std::move(message);
    return error;
}

std::optional<LuaError> LuaHost::protectedCall(
    int argumentCount, int resultCount,
    std::string operation, std::string subject) {
    allocator_->denied = false;
    const int functionIndex =
        lua_gettop(state_) - argumentCount;
    lua_pushcfunction(state_, &LuaHost::tracebackHandler);
    lua_insert(state_, functionIndex);
    const int errorHandler = functionIndex;

    int status = LUA_OK;
    {
        HookGuard hook(state_, options_.instructionLimit);
        status = lua_pcall(state_, argumentCount, resultCount,
                           errorHandler);
    }
    if (status != LUA_OK) {
        LuaError error =
            makeError(status, std::move(operation),
                      std::move(subject));
        lua_remove(state_, errorHandler);
        lua_gc(state_, LUA_GCCOLLECT, 0);
        return error;
    }
    lua_remove(state_, errorHandler);
    return std::nullopt;
}

LuaResult<LuaHost::Reference>
LuaHost::loadFileReturningTable(
    const std::filesystem::path& path) {
    StackGuard stack(state_);
    const std::string pathText = path.u8string();
    allocator_->denied = false;
    const int loadStatus =
        luaL_loadfilex(state_, pathText.c_str(), nullptr);
    if (loadStatus != LUA_OK) {
        LuaError error =
            makeError(loadStatus, "loading Lua file", pathText);
        lua_gc(state_, LUA_GCCOLLECT, 0);
        return LuaResult<Reference>::failure(std::move(error));
    }
    if (std::optional<LuaError> error = protectedCall(
            0, 1, "executing Lua file", pathText)) {
        return LuaResult<Reference>::failure(std::move(*error));
    }
    return referenceValueAtTop(
        "loading Lua module", pathText, true);
}

LuaResult<LuaHost::Reference> LuaHost::createHostApi(
    std::uint64_t seed) {
    return createHostApiImpl(seed, {}, {});
}

LuaResult<LuaHost::Reference> LuaHost::createHostApi(
    std::uint64_t seed,
    const std::vector<SharedReference>& sharedReferences) {
    return createHostApiImpl(
        seed, {}, sharedReferences);
}

LuaResult<LuaHost::Reference> LuaHost::createHostApi(
    RandomCallback randomCallback) {
    return createHostApi(
        std::move(randomCallback), {});
}

LuaResult<LuaHost::Reference> LuaHost::createHostApi(
    RandomCallback randomCallback,
    const std::vector<SharedReference>& sharedReferences) {
    if (!randomCallback) {
        return LuaResult<Reference>::failure(contractError(
            "creating host API", {},
            "random callback must not be empty"));
    }
    return createHostApiImpl(std::nullopt,
                             std::move(randomCallback),
                             sharedReferences);
}

LuaResult<LuaHost::Reference> LuaHost::createHostApiImpl(
    std::optional<std::uint64_t> seed,
    RandomCallback randomCallback,
    const std::vector<SharedReference>& sharedReferences) {
    StackGuard stack(state_);
    allocator_->denied = false;

    std::vector<std::string> sharedNames;
    sharedNames.reserve(sharedReferences.size());
    for (const SharedReference& shared : sharedReferences) {
        if (!shared.reference || shared.name.empty() ||
            shared.name.find('\0') != std::string::npos) {
            return LuaResult<Reference>::failure(contractError(
                "creating host API", shared.name,
                "shared reference requires a non-empty name and value"));
        }
        if (shared.name == "random" ||
            shared.name == "clamp" ||
            shared.name == "wrap_angle" ||
            std::find(sharedNames.begin(), sharedNames.end(),
                      shared.name) != sharedNames.end()) {
            return LuaResult<Reference>::failure(contractError(
                "creating host API", shared.name,
                "shared reference name is reserved or duplicated"));
        }
        sharedNames.push_back(shared.name);
    }

    lua_newtable(state_);
    const int proxyIndex = lua_gettop(state_);
    lua_newtable(state_);
    const int proxyMetatableIndex = lua_gettop(state_);
    lua_newtable(state_);
    const int methodTableIndex = lua_gettop(state_);

    void* storage =
        lua_newuserdatauv(state_, sizeof(HostApiBinding), 0);
    if (!storage) {
        return LuaResult<Reference>::failure(contractError(
            "creating host API", {},
            "Lua did not allocate host API userdata"));
    }
    try {
        if (seed) {
            new (storage) HostApiBinding(this, *seed);
        } else {
            new (storage) HostApiBinding(
                this, std::move(randomCallback));
        }
    } catch (const std::exception& exception) {
        return LuaResult<Reference>::failure(contractError(
            "creating host API", {}, exception.what()));
    } catch (...) {
        return LuaResult<Reference>::failure(contractError(
            "creating host API", {},
            "unknown exception while storing random callback"));
    }

    const int bindingIndex = lua_gettop(state_);
    if (luaL_newmetatable(state_, hostApiBindingMetatable)) {
        lua_pushcfunction(state_, &LuaHost::hostApiCollect);
        lua_setfield(state_, -2, "__gc");
    }
    lua_setmetatable(state_, -2);

    const struct {
        const char* name;
        lua_CFunction function;
    } methods[] = {
        {"random", &LuaHost::hostRandom},
        {"clamp", &LuaHost::hostClamp},
        {"wrap_angle", &LuaHost::hostWrapAngle}};
    for (const auto& method : methods) {
        lua_pushvalue(state_, bindingIndex);
        lua_pushcclosure(state_, method.function, 1);
        lua_setfield(state_, methodTableIndex, method.name);
    }
    lua_pop(state_, 1);

    for (const SharedReference& shared : sharedReferences) {
        std::string proxyError;
        if (!pushReadOnlyProxy(
                *shared.reference, proxyError)) {
            return LuaResult<Reference>::failure(contractError(
                "creating host API", shared.name,
                std::move(proxyError)));
        }
        lua_setfield(
            state_, methodTableIndex, shared.name.c_str());
    }

    lua_pushvalue(state_, methodTableIndex);
    lua_setfield(state_, proxyMetatableIndex, "__index");
    lua_pushcfunction(state_, &LuaHost::hostApiNewIndex);
    lua_setfield(state_, proxyMetatableIndex, "__newindex");
    lua_pushboolean(state_, 0);
    lua_setfield(state_, proxyMetatableIndex, "__metatable");
    lua_pushvalue(state_, proxyMetatableIndex);
    lua_setmetatable(state_, proxyIndex);

    lua_pushvalue(state_, proxyIndex);
    const int reference = luaL_ref(state_, LUA_REGISTRYINDEX);
    return LuaResult<Reference>::success(
        Reference(stateToken_, reference));
}

bool LuaHost::pushReadOnlyProxy(
    const Reference& reference, std::string& error) {
    if (!pushReference(reference, error)) return false;
    const int sourceIndex = lua_gettop(state_);
    if (!lua_istable(state_, sourceIndex)) {
        error = "shared registry reference must point to a table";
        lua_pop(state_, 1);
        return false;
    }

    std::size_t entries = 0;
    std::vector<const void*> visited;
    if (!validateFiniteValue(
            sourceIndex, 0, entries, visited, error)) {
        lua_pop(state_, 1);
        return false;
    }

    lua_newtable(state_);
    const int proxyIndex = lua_gettop(state_);
    lua_newtable(state_);
    const int metatableIndex = lua_gettop(state_);
    lua_pushvalue(state_, sourceIndex);
    lua_setfield(state_, metatableIndex, "__index");
    lua_pushcfunction(state_, &LuaHost::hostApiNewIndex);
    lua_setfield(state_, metatableIndex, "__newindex");
    lua_pushboolean(state_, 0);
    lua_setfield(state_, metatableIndex, "__metatable");
    lua_pushvalue(state_, metatableIndex);
    lua_setmetatable(state_, proxyIndex);
    lua_pop(state_, 1);
    lua_remove(state_, sourceIndex);
    return true;
}

LuaResult<LuaValue> LuaHost::readReference(
    const Reference& reference) {
    StackGuard stack(state_);
    std::string pushError;
    if (!pushReference(reference, pushError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua registry reference", {},
            std::move(pushError)));
    }

    std::size_t entries = 0;
    std::vector<const void*> visited;
    std::string validationError;
    if (!validateFiniteValue(
            -1, 0, entries, visited, validationError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "validating Lua registry reference", {},
            std::move(validationError)));
    }

    entries = 0;
    std::vector<const void*> activeTables;
    LuaValue value;
    std::string readError;
    if (!readValue(-1, value, 0, entries,
                   activeTables, readError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua registry reference", {},
            std::move(readError)));
    }
    return LuaResult<LuaValue>::success(std::move(value));
}

LuaResult<LuaValue> LuaHost::readTableField(
    const Reference& table, std::string_view fieldName) {
    StackGuard stack(state_);
    if (fieldName.empty() ||
        fieldName.find('\0') != std::string_view::npos ||
        fieldName.size() > options_.maximumStringBytes) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua table field", {},
            "field name must be a non-empty string without NUL "
            "within the configured string size limit"));
    }
    const std::string field(fieldName);

    std::string pushError;
    if (!pushReference(table, pushError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua table field", field,
            std::move(pushError)));
    }
    if (!lua_istable(state_, -1)) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua table field", field,
            "registry reference is not a table"));
    }

    TableFieldRequest request{fieldName.data(), fieldName.size()};
    lua_pushcfunction(state_, &LuaHost::lookupTableField);
    lua_insert(state_, -2);
    lua_pushlightuserdata(state_, &request);
    if (std::optional<LuaError> error = protectedCall(
            2, 1, "reading Lua table field", field)) {
        return LuaResult<LuaValue>::failure(std::move(*error));
    }

    std::size_t entries = 0;
    std::vector<const void*> visited;
    std::string validationError;
    if (!validateFiniteValue(
            -1, 0, entries, visited, validationError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "validating Lua table field", field,
            std::move(validationError)));
    }

    entries = 0;
    std::vector<const void*> activeTables;
    LuaValue value;
    std::string readError;
    if (!readValue(-1, value, 0, entries,
                   activeTables, readError)) {
        return LuaResult<LuaValue>::failure(contractError(
            "reading Lua table field", field,
            std::move(readError)));
    }
    return LuaResult<LuaValue>::success(std::move(value));
}

LuaResult<bool> LuaHost::tableFieldIsFunction(
    const Reference& table, std::string_view fieldName) {
    StackGuard stack(state_);
    if (fieldName.empty() ||
        fieldName.find('\0') != std::string_view::npos ||
        fieldName.size() > options_.maximumStringBytes) {
        return LuaResult<bool>::failure(contractError(
            "checking Lua table function", {},
            "field name must be a non-empty string without NUL "
            "within the configured string size limit"));
    }
    const std::string field(fieldName);

    std::string pushError;
    if (!pushReference(table, pushError)) {
        return LuaResult<bool>::failure(contractError(
            "checking Lua table function", field,
            std::move(pushError)));
    }
    if (!lua_istable(state_, -1)) {
        return LuaResult<bool>::failure(contractError(
            "checking Lua table function", field,
            "registry reference is not a table"));
    }

    TableFieldRequest request{fieldName.data(), fieldName.size()};
    lua_pushcfunction(state_, &LuaHost::lookupTableField);
    lua_insert(state_, -2);
    lua_pushlightuserdata(state_, &request);
    if (std::optional<LuaError> error = protectedCall(
            2, 1, "checking Lua table function", field)) {
        return LuaResult<bool>::failure(std::move(*error));
    }
    return LuaResult<bool>::success(
        lua_isfunction(state_, -1) != 0);
}

int LuaHost::lookupTableField(lua_State* state) {
    luaL_checktype(state, 1, LUA_TTABLE);
    const auto* request = static_cast<const TableFieldRequest*>(
        lua_touserdata(state, 2));
    if (!request || (!request->data && request->size != 0)) {
        return luaL_error(
            state, "host table field request is invalid");
    }
    lua_pushlstring(state, request->data, request->size);
    lua_rawget(state, 1);
    return 1;
}

int LuaHost::hostApiNewIndex(lua_State* state) {
    luaL_checktype(state, 1, LUA_TTABLE);
    const char* key = luaL_optstring(state, 2, "<non-string>");
    return luaL_error(
        state, "host API is read-only; cannot assign '%s'", key);
}

int LuaHost::hostApiCollect(lua_State* state) {
    void* storage =
        luaL_testudata(state, 1, hostApiBindingMetatable);
    if (storage) {
        static_cast<HostApiBinding*>(storage)->~HostApiBinding();
    }
    return 0;
}

int LuaHost::hostRandom(lua_State* state) {
    auto* binding = static_cast<HostApiBinding*>(
        lua_touserdata(state, lua_upvalueindex(1)));
    if (!binding || !binding->owner) {
        return luaL_error(
            state, "%s: host API binding is unavailable",
            hostCallbackMarker);
    }
    std::size_t tagLength = 0;
    const char* tag =
        luaL_checklstring(state, 1, &tagLength);
    const double low = luaL_checknumber(state, 2);
    const double high = luaL_checknumber(state, 3);
    if (tagLength == 0 || tagLength > 256) {
        return luaL_error(
            state, "%s: random tag must contain 1..256 bytes",
            hostCallbackMarker);
    }
    if (!std::isfinite(low) || !std::isfinite(high) ||
        low > high) {
        return luaL_error(
            state,
            "%s: random range must be finite and ordered",
            hostCallbackMarker);
    }

    double value = low;
    if (binding->usesCallback) {
        char callbackError[1024]{};
        bool callbackFailed = false;
        try {
            {
                LuaResult<double> result =
                    binding->callback(
                        std::string_view(tag, tagLength), low, high);
                if (!result) {
                    const std::string description =
                        result.error().describe();
                    std::snprintf(
                        callbackError, sizeof(callbackError),
                        "%s", description.c_str());
                    callbackFailed = true;
                } else {
                    value = result.takeValue();
                }
            }
        } catch (const std::exception& exception) {
            std::snprintf(
                callbackError, sizeof(callbackError),
                "random callback threw: %s", exception.what());
            callbackFailed = true;
        } catch (...) {
            std::snprintf(
                callbackError, sizeof(callbackError),
                "%s",
                "random callback threw an unknown exception");
            callbackFailed = true;
        }
        if (callbackFailed) {
            return luaL_error(
                state, "%s: %s", hostCallbackMarker,
                callbackError);
        }
    } else {
        const std::uint64_t bits = binding->generator();
        const double unit =
            static_cast<double>(bits >> 11) *
            (1.0 / 9007199254740992.0);
        value = low + (high - low) * unit;
    }

    if (!std::isfinite(value) || value < low || value > high) {
        return luaL_error(
            state,
            "%s: random callback returned a non-finite or "
            "out-of-range value",
            hostCallbackMarker);
    }
    lua_pushnumber(state, value);
    return 1;
}

int LuaHost::hostClamp(lua_State* state) {
    (void)lua_touserdata(state, lua_upvalueindex(1));
    const double value = luaL_checknumber(state, 1);
    const double low = luaL_checknumber(state, 2);
    const double high = luaL_checknumber(state, 3);
    if (!std::isfinite(value) || !std::isfinite(low) ||
        !std::isfinite(high) || low > high) {
        return luaL_error(
            state,
            "host.clamp requires finite value/limits and low <= high");
    }
    lua_pushnumber(state, std::max(low, std::min(value, high)));
    return 1;
}

int LuaHost::hostWrapAngle(lua_State* state) {
    (void)lua_touserdata(state, lua_upvalueindex(1));
    const double value = luaL_checknumber(state, 1);
    if (!std::isfinite(value)) {
        return luaL_error(
            state, "host.wrap_angle requires a finite number");
    }
    double wrapped = std::fmod(value + pi, 2.0 * pi);
    if (wrapped < 0.0) wrapped += 2.0 * pi;
    lua_pushnumber(state, wrapped - pi);
    return 1;
}

bool LuaHost::pushReference(
    const Reference& reference, std::string& error) {
    const std::shared_ptr<LuaHostStateToken> token =
        reference.token_.lock();
    if (!token || token.get() != stateToken_.get() ||
        !reference.valid()) {
        error =
            "registry reference is invalid or belongs to another Lua host";
        return false;
    }
    lua_rawgeti(state_, LUA_REGISTRYINDEX,
                reference.registryReference_);
    return true;
}

bool LuaHost::pushArgument(
    const Argument& argument, std::string& error) {
    if (argument.reference_) {
        return pushReference(*argument.reference_, error);
    }
    std::size_t entries = 0;
    std::vector<const LuaTableValue*> activeTables;
    return pushValue(argument.value_, 0, entries,
                     activeTables, error);
}

bool LuaHost::pushValue(
    const LuaValue& value, std::size_t depth,
    std::size_t& entryCount,
    std::vector<const LuaTableValue*>& activeTables,
    std::string& error) {
    if (depth > options_.maximumValueDepth) {
        error = "argument exceeds maximum Lua value depth";
        return false;
    }
    if (value.isNil()) {
        lua_pushnil(state_);
        return true;
    }
    if (const bool* boolean = value.boolean()) {
        lua_pushboolean(state_, *boolean);
        return true;
    }
    if (const double* number = value.number()) {
        if (!std::isfinite(*number)) {
            error = "argument contains NaN or infinity";
            return false;
        }
        const lua_Number luaNumber =
            static_cast<lua_Number>(*number);
        if (!std::isfinite(
                static_cast<long double>(luaNumber))) {
            error =
                "argument number is outside the Lua numeric range";
            return false;
        }
        lua_pushnumber(state_, luaNumber);
        return true;
    }
    if (const std::string* string = value.string()) {
        if (string->size() > options_.maximumStringBytes) {
            error = "argument string exceeds size limit";
            return false;
        }
        lua_pushlstring(state_, string->data(), string->size());
        return true;
    }
    const LuaTableValue* tableValue = value.table();
    if (!tableValue) {
        error = "argument has an unsupported value type";
        return false;
    }
    if (containsTablePointer(activeTables, tableValue)) {
        error = "argument table contains a cycle";
        return false;
    }
    activeTables.push_back(tableValue);
    lua_createtable(
        state_, 0,
        static_cast<int>(std::min<std::size_t>(
            tableValue->entries.size(),
            static_cast<std::size_t>(
                std::numeric_limits<int>::max()))));
    for (const auto& entry : tableValue->entries) {
        if (++entryCount > options_.maximumTableEntries) {
            error = "argument table exceeds entry limit";
            activeTables.pop_back();
            return false;
        }
        if (entry.first.isNil() || entry.first.table()) {
            error =
                "argument table keys must be booleans, finite numbers, "
                "or strings";
            activeTables.pop_back();
            return false;
        }
        if (!pushValue(entry.first, depth + 1, entryCount,
                       activeTables, error) ||
            !pushValue(entry.second, depth + 1, entryCount,
                       activeTables, error)) {
            activeTables.pop_back();
            return false;
        }
        lua_settable(state_, -3);
    }
    activeTables.pop_back();
    return true;
}

std::optional<LuaError> LuaHost::prepareTableCall(
    const Reference& table, std::string_view functionName,
    const std::vector<Argument>& arguments, CallStyle style) {
    if (functionName.empty() ||
        functionName.find('\0') != std::string_view::npos) {
        return contractError(
            "preparing Lua table call", {},
            "function name must be a non-empty string without NUL");
    }
    if (arguments.size() >
        static_cast<std::size_t>(std::numeric_limits<int>::max() - 1)) {
        return contractError(
            "preparing Lua table call",
            std::string(functionName),
            "too many call arguments");
    }

    std::string pushError;
    if (!pushReference(table, pushError)) {
        return contractError(
            "preparing Lua table call",
            std::string(functionName), std::move(pushError));
    }
    const int tableIndex = lua_gettop(state_);
    if (!lua_istable(state_, tableIndex)) {
        return contractError(
            "preparing Lua table call",
            std::string(functionName),
            "registry reference is not a table");
    }

    lua_pushlstring(state_, functionName.data(),
                    functionName.size());
    lua_rawget(state_, tableIndex);
    if (!lua_isfunction(state_, -1)) {
        return contractError(
            "preparing Lua table call",
            std::string(functionName),
            "table field is missing or is not a function");
    }
    if (style == CallStyle::Method) {
        lua_pushvalue(state_, tableIndex);
    }
    for (const Argument& argument : arguments) {
        if (!pushArgument(argument, pushError)) {
            return contractError(
                "preparing Lua table call",
                std::string(functionName), std::move(pushError));
        }
    }
    lua_remove(state_, tableIndex);
    return std::nullopt;
}

LuaResult<std::vector<LuaValue>>
LuaHost::callTableFunction(
    const Reference& table, std::string_view functionName,
    const std::vector<Argument>& arguments,
    CallStyle style, int resultCount) {
    StackGuard stack(state_);
    if (resultCount < 0 || resultCount > 64) {
        return LuaResult<std::vector<LuaValue>>::failure(
            contractError(
                "calling Lua table function",
                std::string(functionName),
                "result count must be between 0 and 64"));
    }
    if (std::optional<LuaError> error = prepareTableCall(
            table, functionName, arguments, style)) {
        return LuaResult<std::vector<LuaValue>>::failure(
            std::move(*error));
    }
    const int argumentCount =
        static_cast<int>(arguments.size()) +
        (style == CallStyle::Method ? 1 : 0);
    if (std::optional<LuaError> error = protectedCall(
            argumentCount, resultCount,
            "calling Lua table function",
            std::string(functionName))) {
        return LuaResult<std::vector<LuaValue>>::failure(
            std::move(*error));
    }

    std::vector<LuaValue> values;
    values.reserve(static_cast<std::size_t>(resultCount));
    const int firstResult =
        lua_gettop(state_) - resultCount + 1;
    std::size_t entries = 0;
    std::vector<const void*> visited;
    for (int index = 0; index < resultCount; ++index) {
        std::string validationError;
        if (!validateFiniteValue(
                firstResult + index, 0, entries,
                visited, validationError)) {
            return LuaResult<std::vector<LuaValue>>::failure(
                contractError(
                    "validating Lua function result",
                    std::string(functionName),
                    std::move(validationError)));
        }
    }

    entries = 0;
    std::vector<const void*> activeTables;
    for (int index = 0; index < resultCount; ++index) {
        LuaValue value;
        std::string readError;
        if (!readValue(firstResult + index, value, 0, entries,
                       activeTables, readError)) {
            return LuaResult<std::vector<LuaValue>>::failure(
                contractError(
                    "reading Lua function result",
                    std::string(functionName),
                    std::move(readError)));
        }
        values.push_back(std::move(value));
    }
    return LuaResult<std::vector<LuaValue>>::success(
        std::move(values));
}

LuaResult<LuaHost::Reference>
LuaHost::callTableFunctionReturningTable(
    const Reference& table, std::string_view functionName,
    const std::vector<Argument>& arguments,
    CallStyle style) {
    StackGuard stack(state_);
    if (std::optional<LuaError> error = prepareTableCall(
            table, functionName, arguments, style)) {
        return LuaResult<Reference>::failure(std::move(*error));
    }
    const int argumentCount =
        static_cast<int>(arguments.size()) +
        (style == CallStyle::Method ? 1 : 0);
    if (std::optional<LuaError> error = protectedCall(
            argumentCount, 1,
            "calling Lua table factory",
            std::string(functionName))) {
        return LuaResult<Reference>::failure(std::move(*error));
    }
    return referenceValueAtTop(
        "validating Lua table factory result",
        std::string(functionName), true);
}

LuaResult<LuaHost::Reference> LuaHost::referenceValueAtTop(
    std::string operation, std::string subject,
    bool requireTable) {
    if (requireTable && !lua_istable(state_, -1)) {
        return LuaResult<Reference>::failure(contractError(
            std::move(operation), std::move(subject),
            "Lua value must be a table"));
    }
    std::size_t entries = 0;
    std::vector<const void*> visited;
    std::string validationError;
    if (!validateFiniteValue(
            -1, 0, entries, visited, validationError)) {
        return LuaResult<Reference>::failure(contractError(
            std::move(operation), std::move(subject),
            std::move(validationError)));
    }
    const int reference =
        luaL_ref(state_, LUA_REGISTRYINDEX);
    return LuaResult<Reference>::success(
        Reference(stateToken_, reference));
}

bool LuaHost::validateFiniteValue(
    int index, std::size_t depth,
    std::size_t& entryCount,
    std::vector<const void*>& visited,
    std::string& error) const {
    index = lua_absindex(state_, index);
    if (depth > options_.maximumValueDepth) {
        error = "Lua value exceeds maximum nesting depth";
        return false;
    }
    const int type = lua_type(state_, index);
    if (type == LUA_TNUMBER) {
        if (!std::isfinite(lua_tonumber(state_, index))) {
            error = "Lua value contains NaN or infinity";
            return false;
        }
        return true;
    }
    if (type == LUA_TSTRING) {
        std::size_t size = 0;
        (void)lua_tolstring(state_, index, &size);
        if (size > options_.maximumStringBytes) {
            error = "Lua string exceeds size limit";
            return false;
        }
        return true;
    }
    if (type == LUA_TTHREAD) {
        error = "Lua threads/coroutines cannot cross the host boundary";
        return false;
    }
    if (type != LUA_TTABLE) {
        return true;
    }

    const void* pointer = lua_topointer(state_, index);
    if (containsPointer(visited, pointer)) return true;
    visited.push_back(pointer);
    lua_pushnil(state_);
    while (lua_next(state_, index) != 0) {
        if (++entryCount > options_.maximumTableEntries) {
            error = "Lua table graph exceeds entry limit";
            return false;
        }
        if (!validateFiniteValue(
                -2, depth + 1, entryCount, visited, error) ||
            !validateFiniteValue(
                -1, depth + 1, entryCount, visited, error)) {
            return false;
        }
        lua_pop(state_, 1);
    }
    return true;
}

bool LuaHost::readValue(
    int index, LuaValue& value, std::size_t depth,
    std::size_t& entryCount,
    std::vector<const void*>& activeTables,
    std::string& error) const {
    index = lua_absindex(state_, index);
    if (depth > options_.maximumValueDepth) {
        error = "Lua result exceeds maximum nesting depth";
        return false;
    }
    switch (lua_type(state_, index)) {
    case LUA_TNIL:
        value = LuaValue();
        return true;
    case LUA_TBOOLEAN:
        value = LuaValue(lua_toboolean(state_, index) != 0);
        return true;
    case LUA_TNUMBER: {
        const double number = lua_tonumber(state_, index);
        if (!std::isfinite(number)) {
            error = "Lua result contains NaN or infinity";
            return false;
        }
        value = LuaValue(number);
        return true;
    }
    case LUA_TSTRING: {
        std::size_t size = 0;
        const char* string =
            lua_tolstring(state_, index, &size);
        if (size > options_.maximumStringBytes) {
            error = "Lua result string exceeds size limit";
            return false;
        }
        value = LuaValue(std::string(string, size));
        return true;
    }
    case LUA_TTABLE:
        break;
    default:
        error = std::string("unsupported Lua result type: ") +
                lua_typename(state_, lua_type(state_, index));
        return false;
    }

    const void* pointer = lua_topointer(state_, index);
    if (containsPointer(activeTables, pointer)) {
        error = "Lua result table contains a cycle";
        return false;
    }
    activeTables.push_back(pointer);
    std::vector<std::pair<LuaValue, LuaValue>> resultEntries;
    lua_pushnil(state_);
    while (lua_next(state_, index) != 0) {
        if (++entryCount > options_.maximumTableEntries) {
            error = "Lua result table exceeds entry limit";
            activeTables.pop_back();
            return false;
        }
        LuaValue key;
        LuaValue entryValue;
        if (!readValue(-2, key, depth + 1, entryCount,
                       activeTables, error) ||
            !readValue(-1, entryValue, depth + 1, entryCount,
                       activeTables, error)) {
            activeTables.pop_back();
            return false;
        }
        resultEntries.emplace_back(
            std::move(key), std::move(entryValue));
        lua_pop(state_, 1);
    }
    activeTables.pop_back();
    value = LuaValue::table(std::move(resultEntries));
    return true;
}

std::size_t LuaHost::memoryUsedBytes() const {
    return allocator_ ? allocator_->used : 0;
}

std::size_t LuaHost::memoryLimitBytes() const {
    return allocator_ ? allocator_->limit : 0;
}

int LuaHost::instructionLimit() const {
    return options_.instructionLimit;
}

int LuaHost::stackTopForTesting() const {
    return state_ ? lua_gettop(state_) : 0;
}

void LuaHost::collectGarbage() {
    if (state_) lua_gc(state_, LUA_GCCOLLECT, 0);
}
