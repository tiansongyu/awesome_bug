#pragma once

#include "runtime/bug_types.h"
#include "runtime/lua_host.h"

#include <filesystem>

namespace bug {

LuaResult<Species> loadSpecies(
    LuaHost& host, const std::filesystem::path& speciesRoot);

} // namespace bug
