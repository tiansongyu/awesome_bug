if(NOT CMAKE_C_COMPILER_LOADED)
    enable_language(C)
endif()

add_library(lua_static STATIC
    src/lapi.c
    src/lauxlib.c
    src/lbaselib.c
    src/lcode.c
    src/lcorolib.c
    src/lctype.c
    src/ldblib.c
    src/ldebug.c
    src/ldo.c
    src/ldump.c
    src/lfunc.c
    src/lgc.c
    src/linit.c
    src/liolib.c
    src/llex.c
    src/lmathlib.c
    src/lmem.c
    src/loadlib.c
    src/lobject.c
    src/lopcodes.c
    src/loslib.c
    src/lparser.c
    src/lstate.c
    src/lstring.c
    src/lstrlib.c
    src/ltable.c
    src/ltablib.c
    src/ltm.c
    src/lundump.c
    src/lutf8lib.c
    src/lvm.c
    src/lzio.c
)
add_library(Lua::Lua ALIAS lua_static)

target_include_directories(lua_static
    PUBLIC
        "${CMAKE_CURRENT_SOURCE_DIR}/src"
)

set_target_properties(lua_static PROPERTIES
    C_STANDARD 99
    C_STANDARD_REQUIRED YES
    C_EXTENSIONS YES
    OUTPUT_NAME lua
    POSITION_INDEPENDENT_CODE ON
)

if(WIN32)
    target_compile_definitions(lua_static PRIVATE LUA_USE_WINDOWS)
elseif(UNIX)
    target_compile_definitions(lua_static PRIVATE LUA_USE_POSIX)
endif()

if(UNIX AND NOT APPLE)
    target_link_libraries(lua_static PUBLIC m)
endif()

# Keep upstream warning policy isolated from the project's own warning level.
if(MSVC)
    target_compile_options(lua_static PRIVATE /w)
else()
    target_compile_options(lua_static PRIVATE -w)
endif()
