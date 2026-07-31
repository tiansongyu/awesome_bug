#include "runtime/lua_host.h"

#include <chrono>
#include <cmath>
#include <filesystem>
#include <fstream>
#include <iostream>
#include <limits>
#include <string>
#include <vector>

namespace {
struct TemporaryScripts {
    std::filesystem::path directory;

    TemporaryScripts() {
        const auto nonce =
            std::chrono::high_resolution_clock::now()
                .time_since_epoch()
                .count();
        directory =
            std::filesystem::temp_directory_path() /
            ("desktop-display-lua-host-test-" +
             std::to_string(nonce));
        std::filesystem::create_directories(directory);
    }

    ~TemporaryScripts() {
        std::error_code error;
        std::filesystem::remove_all(directory, error);
    }

    std::filesystem::path write(
        const std::string& name,
        const std::string& contents) const {
        const std::filesystem::path path = directory / name;
        std::ofstream output(path, std::ios::binary);
        output << contents;
        output.close();
        return path;
    }
};

const LuaValue* field(const LuaValue& value,
                      const std::string& name) {
    const LuaTableValue* table = value.table();
    if (!table) return nullptr;
    for (const auto& entry : table->entries) {
        const std::string* key = entry.first.string();
        if (key && *key == name) return &entry.second;
    }
    return nullptr;
}

bool near(double left, double right,
          double tolerance = 1.0e-5) {
    return std::abs(left - right) <= tolerance;
}

void fail(bool& failed, const std::string& message) {
    std::cerr << message << '\n';
    failed = true;
}
} // namespace

int main() {
    bool failed = false;
    TemporaryScripts scripts;
    const std::filesystem::path modulePath = scripts.write(
        "module.lua",
        R"lua(
return {
    new = function()
        return {}
    end,

    sandbox = function()
        return {
            base = type(assert) == "function",
            table_lib = type(table) == "table",
            string_lib = type(string) == "table",
            math_lib = type(math) == "table",
            utf8_lib = type(utf8) == "table",
            no_dofile = dofile == nil,
            no_load = load == nil,
            no_loadfile = loadfile == nil,
            no_collect = collectgarbage == nil,
            no_package = package == nil and require == nil,
            no_io = io == nil,
            no_os = os == nil,
            no_debug = debug == nil,
            no_coroutine = coroutine == nil,
            no_rawset = rawset == nil,
            no_random = math.random == nil and math.randomseed == nil,
        }
    end,

    add = function(a, b)
        return a + b
    end,

    is_finite = function(value)
        return value == value and
               value ~= math.huge and value ~= -math.huge
    end,

    new_controller = function(config, host)
        local machine = host.fsm.create(config.name)
        local self = { count = 0, name = machine.name }
        function self:run(value)
            self.count = self.count + 1
            return {
                name = self.name,
                count = self.count,
                sample = host.random("motion.target", -2.0, 2.0),
                clamped = host.clamp(value, -1.0, 1.0),
                angle = host.wrap_angle(7.0),
                host_kind = type(host),
                fsm_kind = type(host.fsm),
            }
        end
        function self:step(frame)
            return frame
        end
        function self:pose(frame)
            return frame
        end
        return self
    end,

    draw = function(host)
        return host.random("deterministic", 10.0, 20.0)
    end,

    mutate_host = function(host)
        host.random = nil
    end,

    mutate_fsm = function(host)
        host.fsm.create = nil
    end,

    crash = function()
        error("intentional runtime failure")
    end,

    non_string_error = function()
        error({ reason = "table error" })
    end,

    spin = function()
        while true do
        end
    end,

    memory_bomb = function()
        return string.rep("x", 40 * 1024 * 1024)
    end,

    nan = function()
        return 0.0 / 0.0
    end,

    infinity = function()
        return math.huge
    end,
}
)lua");

    auto hostResult = LuaHost::create();
    if (!hostResult) {
        std::cerr << hostResult.error().describe() << '\n';
        return 1;
    }
    std::unique_ptr<LuaHost> host = hostResult.takeValue();
    if (host->memoryLimitBytes() !=
            LuaHost::defaultMemoryLimitBytes ||
        host->instructionLimit() !=
            LuaHost::defaultInstructionLimit ||
        host->memoryUsedBytes() >= host->memoryLimitBytes()) {
        fail(failed, "Lua host resource limits were not applied");
    }
    const int baselineStack = host->stackTopForTesting();

    auto moduleResult =
        host->loadFileReturningTable(modulePath);
    if (!moduleResult) {
        std::cerr << moduleResult.error().describe() << '\n';
        return 1;
    }
    LuaHost::Reference module = moduleResult.takeValue();

    auto moduleFactory =
        host->tableFieldIsFunction(module, "new");
    if (!moduleFactory || !moduleFactory.value()) {
        fail(failed, "module.new was not recognized as a function");
    }

    const std::filesystem::path fsmPath = scripts.write(
        "fsm.lua",
        R"lua(
return {
    create = function(name)
        return { name = name }
    end,
}
)lua");
    auto fsmResult = host->loadFileReturningTable(fsmPath);
    if (!fsmResult) {
        fail(failed, fsmResult.error().describe());
        return 1;
    }
    LuaHost::Reference fsm = fsmResult.takeValue();

    auto sandboxResult = host->callTableFunction(
        module, "sandbox", {}, LuaHost::CallStyle::Function, 1);
    if (!sandboxResult ||
        sandboxResult.value().size() != 1) {
        fail(failed, "sandbox inspection did not return one value");
    } else {
        const LuaTableValue* sandbox =
            sandboxResult.value()[0].table();
        if (!sandbox || sandbox->entries.size() != 16) {
            fail(failed, "sandbox exposes an unexpected library set");
        } else {
            for (const auto& entry : sandbox->entries) {
                const bool* enabled = entry.second.boolean();
                if (!enabled || !*enabled) {
                    fail(failed,
                         "sandbox left a forbidden capability available");
                    break;
                }
            }
        }
    }

    std::vector<std::string> randomTags;
    auto apiResult = host->createHostApi(
        [&randomTags](
            std::string_view tag, double low,
            double high) -> LuaResult<double> {
            randomTags.emplace_back(tag);
            return LuaResult<double>::success(
                low + (high - low) * 0.25);
        },
        {LuaHost::SharedReference("fsm", fsm)});
    if (!apiResult) {
        fail(failed, apiResult.error().describe());
        return 1;
    }
    LuaHost::Reference hostApi = apiResult.takeValue();

    const LuaValue config = LuaValue::object(
        {{"name", LuaValue("fixture")}});
    std::vector<LuaHost::Argument> factoryArguments;
    factoryArguments.emplace_back(config);
    factoryArguments.push_back(
        LuaHost::Argument::fromReference(hostApi));
    auto controllerResult =
        host->callTableFunctionReturningTable(
            module, "new_controller", factoryArguments,
            LuaHost::CallStyle::Function);
    if (!controllerResult) {
        fail(failed, controllerResult.error().describe());
        return 1;
    }
    LuaHost::Reference controller =
        controllerResult.takeValue();

    for (const char* function : {"step", "pose"}) {
        auto controllerFunction =
            host->tableFieldIsFunction(controller, function);
        if (!controllerFunction || !controllerFunction.value()) {
            fail(failed,
                 std::string("controller.") + function +
                     " was not recognized as a function");
        }
    }
    auto absentControllerFunction =
        host->tableFieldIsFunction(controller, "not_present");
    if (!absentControllerFunction ||
        absentControllerFunction.value()) {
        fail(failed,
             "missing controller function did not return false");
    }

    for (int call = 1; call <= 2; ++call) {
        auto runResult = host->callTableFunction(
            controller, "run", {LuaHost::Argument(LuaValue(3.0))},
            LuaHost::CallStyle::Method, 1);
        if (!runResult || runResult.value().size() != 1) {
            fail(failed, "controller method call failed");
            break;
        }
        const LuaValue& output = runResult.value()[0];
        const LuaValue* name = field(output, "name");
        const LuaValue* count = field(output, "count");
        const LuaValue* sample = field(output, "sample");
        const LuaValue* clamped = field(output, "clamped");
        const LuaValue* angle = field(output, "angle");
        const LuaValue* hostKind = field(output, "host_kind");
        const LuaValue* fsmKind = field(output, "fsm_kind");
        if (!name || !name->string() ||
            *name->string() != "fixture" ||
            !count || !count->number() ||
            !near(*count->number(), call) ||
            !sample || !sample->number() ||
            !near(*sample->number(), -1.0) ||
            !clamped || !clamped->number() ||
            !near(*clamped->number(), 1.0) ||
            !angle || !angle->number() ||
            !near(*angle->number(),
                  7.0 - 2.0 * 3.14159265358979323846) ||
            !hostKind || !hostKind->string() ||
            *hostKind->string() != "table" ||
            !fsmKind || !fsmKind->string() ||
            *fsmKind->string() != "table") {
            fail(failed,
                 "host API or controller state produced wrong values");
            break;
        }
    }
    if (randomTags.size() != 2 ||
        randomTags[0] != "motion.target" ||
        randomTags[1] != "motion.target") {
        fail(failed, "tagged random callback was not used exactly");
    }

    auto deterministicA = host->createHostApi(0x12345678u);
    auto deterministicB = host->createHostApi(0x12345678u);
    if (!deterministicA || !deterministicB) {
        fail(failed, "seeded host APIs could not be created");
    } else {
        LuaHost::Reference first = deterministicA.takeValue();
        LuaHost::Reference second = deterministicB.takeValue();
        for (int draw = 0; draw < 8; ++draw) {
            auto left = host->callTableFunction(
                module, "draw",
                {LuaHost::Argument::fromReference(first)});
            auto right = host->callTableFunction(
                module, "draw",
                {LuaHost::Argument::fromReference(second)});
            if (!left || !right ||
                !left.value()[0].number() ||
                !right.value()[0].number() ||
                *left.value()[0].number() !=
                    *right.value()[0].number()) {
                fail(failed,
                     "seeded host RNG is not deterministic");
                break;
            }
        }
    }

    auto addResult = host->callTableFunction(
        module, "add",
        {LuaHost::Argument(LuaValue(2.0)),
         LuaHost::Argument(LuaValue(5.0))});
    if (!addResult || !addResult.value()[0].number() ||
        !near(*addResult.value()[0].number(), 7.0)) {
        fail(failed, "generic scalar function call failed");
    }

    auto mutationResult = host->callTableFunction(
        module, "mutate_host",
        {LuaHost::Argument::fromReference(hostApi)},
        LuaHost::CallStyle::Function, 0);
    if (mutationResult ||
        mutationResult.error().code != LuaErrorCode::Runtime) {
        fail(failed, "host API was mutable from Lua");
    }

    auto fsmMutationResult = host->callTableFunction(
        module, "mutate_fsm",
        {LuaHost::Argument::fromReference(hostApi)},
        LuaHost::CallStyle::Function, 0);
    if (fsmMutationResult ||
        fsmMutationResult.error().code != LuaErrorCode::Runtime) {
        fail(failed, "shared fsm module was mutable from Lua");
    }

    auto reservedSharedName = host->createHostApi(
        7u, {LuaHost::SharedReference("random", fsm)});
    if (reservedSharedName ||
        reservedSharedName.error().code != LuaErrorCode::Contract) {
        fail(failed,
             "reserved host API field accepted a shared reference");
    }

    auto duplicateSharedName = host->createHostApi(
        7u, {LuaHost::SharedReference("fsm", fsm),
             LuaHost::SharedReference("fsm", fsm)});
    if (duplicateSharedName ||
        duplicateSharedName.error().code != LuaErrorCode::Contract) {
        fail(failed,
             "duplicate shared reference name was accepted");
    }

    auto crashResult =
        host->callTableFunction(module, "crash");
    if (crashResult ||
        crashResult.error().code != LuaErrorCode::Runtime ||
        crashResult.error().message.find(
            "intentional runtime failure") == std::string::npos ||
        crashResult.error().traceback.find("stack traceback") ==
            std::string::npos) {
        fail(failed, "runtime error lacks actionable traceback");
    }

    auto nonStringResult =
        host->callTableFunction(module, "non_string_error");
    if (nonStringResult ||
        nonStringResult.error().code != LuaErrorCode::Runtime ||
        nonStringResult.error().message.find("non-string") ==
            std::string::npos) {
        fail(failed, "non-string Lua error was not isolated");
    }

    auto spinResult =
        host->callTableFunction(module, "spin");
    if (spinResult ||
        spinResult.error().code !=
            LuaErrorCode::InstructionLimit) {
        fail(failed, "infinite loop escaped instruction hook");
    }

    auto memoryResult =
        host->callTableFunction(module, "memory_bomb");
    if (memoryResult ||
        memoryResult.error().code != LuaErrorCode::MemoryLimit ||
        host->memoryUsedBytes() > host->memoryLimitBytes()) {
        fail(failed, "memory bomb escaped allocator limit");
    }

    for (const char* function : {"nan", "infinity"}) {
        auto numericResult =
            host->callTableFunction(module, function);
        if (numericResult ||
            numericResult.error().code !=
                LuaErrorCode::Contract) {
            fail(failed,
                 std::string("illegal numeric result accepted from ") +
                     function);
        }
    }

    auto badArgument = host->callTableFunction(
        module, "add",
        {LuaHost::Argument(LuaValue(
             std::numeric_limits<double>::infinity())),
         LuaHost::Argument(LuaValue(1.0))});
    if (badArgument ||
        badArgument.error().code != LuaErrorCode::Contract) {
        fail(failed, "non-finite host argument crossed Lua boundary");
    }

    auto numericRangeResult = host->callTableFunction(
        module, "is_finite",
        {LuaHost::Argument(LuaValue(
            std::numeric_limits<double>::max()))});
    if (numericRangeResult) {
        const bool* remainedFinite =
            numericRangeResult.value()[0].boolean();
        if (!remainedFinite || !*remainedFinite) {
            fail(failed,
                 "host argument overflowed the Lua numeric type");
        }
    } else if (numericRangeResult.error().code !=
               LuaErrorCode::Contract) {
        fail(failed,
             "Lua numeric range rejection had wrong classification");
    }

    const std::filesystem::path syntaxPath = scripts.write(
        "syntax.lua", "return { broken = function( }\n");
    auto syntaxResult =
        host->loadFileReturningTable(syntaxPath);
    if (syntaxResult ||
        syntaxResult.error().code != LuaErrorCode::Syntax ||
        syntaxResult.error().subject.find("syntax.lua") ==
            std::string::npos) {
        fail(failed, "syntax error lacks path and classification");
    }

    const std::filesystem::path wrongTypePath = scripts.write(
        "wrong-type.lua", "return 42\n");
    auto wrongTypeResult =
        host->loadFileReturningTable(wrongTypePath);
    if (wrongTypeResult ||
        wrongTypeResult.error().code != LuaErrorCode::Contract) {
        fail(failed, "non-table Lua module was accepted");
    }

    const std::filesystem::path manifestPath = scripts.write(
        "manifest.lua",
        "return { api_version = 1, id = 'fixture', "
        "nested = { enabled = true }, values = { 2, 4, 8 } }\n");
    auto manifestReference =
        host->loadFileReturningTable(manifestPath);
    if (!manifestReference) {
        fail(failed, manifestReference.error().describe());
    } else {
        LuaHost::Reference manifest =
            manifestReference.takeValue();
        auto manifestValue = host->readReference(manifest);
        const LuaValue* apiVersion =
            manifestValue ? field(manifestValue.value(),
                                  "api_version")
                          : nullptr;
        const LuaValue* identifier =
            manifestValue ? field(manifestValue.value(), "id")
                          : nullptr;
        if (!manifestValue || !apiVersion ||
            !apiVersion->number() ||
            !near(*apiVersion->number(), 1.0) ||
            !identifier || !identifier->string() ||
            *identifier->string() != "fixture") {
            fail(failed,
                 "registry data reference could not be parsed");
        }

        auto apiVersionField =
            host->readTableField(manifest, "api_version");
        if (!apiVersionField ||
            !apiVersionField.value().number() ||
            !near(*apiVersionField.value().number(), 1.0)) {
            fail(failed,
                 "numeric Lua registry table field was not read");
        }

        auto missingField =
            host->readTableField(manifest, "not_present");
        if (!missingField || !missingField.value().isNil()) {
            fail(failed,
                 "missing Lua registry table field was not nil");
        }

        auto dataFieldIsFunction =
            host->tableFieldIsFunction(manifest, "api_version");
        if (!dataFieldIsFunction ||
            dataFieldIsFunction.value()) {
            fail(failed,
                 "numeric table field was reported as a function");
        }
    }

    auto functionField = host->readTableField(module, "add");
    if (functionField ||
        functionField.error().code != LuaErrorCode::Contract) {
        fail(failed,
             "Lua function crossed the narrow table field ABI");
    }

    const std::filesystem::path illegalModulePath = scripts.write(
        "illegal-module.lua", "return { value = math.huge }\n");
    auto illegalModuleResult =
        host->loadFileReturningTable(illegalModulePath);
    if (illegalModuleResult ||
        illegalModuleResult.error().code !=
            LuaErrorCode::Contract) {
        fail(failed, "non-finite module data was accepted");
    }

    auto missingFileResult = host->loadFileReturningTable(
        scripts.directory / "does-not-exist.lua");
    if (missingFileResult ||
        missingFileResult.error().code != LuaErrorCode::File ||
        missingFileResult.error().subject.find(
            "does-not-exist.lua") == std::string::npos) {
        fail(failed, "missing file error lacks path/classification");
    }

    auto missingFunction = host->callTableFunction(
        module, "does_not_exist");
    if (missingFunction ||
        missingFunction.error().code != LuaErrorCode::Contract) {
        fail(failed, "missing table function was not rejected");
    }

    auto callbackApiResult = host->createHostApi(
        [](std::string_view, double,
           double) -> LuaResult<double> {
            LuaError error;
            error.code = LuaErrorCode::HostCallback;
            error.operation = "reading RNG tape";
            error.message = "tape exhausted";
            return LuaResult<double>::failure(std::move(error));
        });
    if (!callbackApiResult) {
        fail(failed, callbackApiResult.error().describe());
    } else {
        LuaHost::Reference callbackApi =
            callbackApiResult.takeValue();
        auto callbackFailure = host->callTableFunction(
            module, "draw",
            {LuaHost::Argument::fromReference(callbackApi)});
        if (callbackFailure ||
            callbackFailure.error().code !=
                LuaErrorCode::HostCallback ||
            callbackFailure.error().message.find(
                "tape exhausted") == std::string::npos) {
            fail(failed,
                 "host callback failure was not preserved");
        }
    }

    auto secondHostResult = LuaHost::create();
    if (!secondHostResult) {
        fail(failed, secondHostResult.error().describe());
    } else {
        std::unique_ptr<LuaHost> secondHost =
            secondHostResult.takeValue();
        auto foreignCall = secondHost->callTableFunction(
            module, "sandbox");
        if (foreignCall ||
            foreignCall.error().code != LuaErrorCode::Contract) {
            fail(failed,
                 "registry reference crossed Lua host instances");
        }

        auto foreignField =
            secondHost->readTableField(module, "api_version");
        if (foreignField ||
            foreignField.error().code != LuaErrorCode::Contract) {
            fail(failed,
                 "foreign registry reference crossed table field ABI");
        }

        auto foreignFunction =
            secondHost->tableFieldIsFunction(module, "new");
        if (foreignFunction ||
            foreignFunction.error().code != LuaErrorCode::Contract) {
            fail(failed,
                 "foreign registry reference crossed function check ABI");
        }
    }

    auto postFailureAdd = host->callTableFunction(
        module, "add",
        {LuaHost::Argument(LuaValue(10.0)),
         LuaHost::Argument(LuaValue(5.0))});
    if (!postFailureAdd ||
        !postFailureAdd.value()[0].number() ||
        !near(*postFailureAdd.value()[0].number(), 15.0)) {
        fail(failed,
             "Lua host was unusable after isolated failures");
    }

    host->collectGarbage();
    const std::size_t stressBaselineMemory =
        host->memoryUsedBytes();
    for (int frame = 0; frame < 100000; ++frame) {
        auto frameResult = host->callTableFunction(
            module, "add",
            {LuaHost::Argument(LuaValue(frame)),
             LuaHost::Argument(LuaValue(1))});
        if (!frameResult ||
            frameResult.value().size() != 1 ||
            !frameResult.value()[0].number() ||
            !near(*frameResult.value()[0].number(),
                  static_cast<double>(frame + 1))) {
            fail(failed,
                 "100,000-call Lua host stress test failed");
            break;
        }
    }
    host->collectGarbage();
    if (host->memoryUsedBytes() > stressBaselineMemory + 4096) {
        fail(failed,
             "Lua memory grew continuously during stress test");
    }
    if (host->stackTopForTesting() != baselineStack) {
        fail(failed, "Lua stack was not restored after calls");
    }

    return failed ? 1 : 0;
}
