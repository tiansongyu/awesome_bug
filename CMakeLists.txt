cmake_minimum_required(VERSION 3.16)

project(cockroach_overlay VERSION 1.0.0 LANGUAGES C CXX)

set(CMAKE_C_STANDARD 99)
set(CMAKE_C_STANDARD_REQUIRED ON)
set(CMAKE_C_EXTENSIONS ON)
set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

function(enable_project_warnings target)
    if(MSVC)
        target_compile_options(${target} PRIVATE /W4 /permissive-)
    else()
        target_compile_options(${target} PRIVATE
            -Wall -Wextra -Wpedantic)
    endif()
endfunction()

function(enable_mingw_static_runtime target)
    if(MINGW)
        target_link_options(${target} PRIVATE
            -static-libgcc -static-libstdc++)
    endif()
endfunction()

add_subdirectory(third_party/lua)

# This library deliberately has no SDL, X11, or Win32 dependency. It is the
# deterministic, headless contract between platform input and species scripts.
add_library(bug_runtime_core STATIC
    src/runtime/bug_species.cpp
    src/runtime/lua_controller.cpp
    src/runtime/lua_host.cpp
    src/runtime/motion_solver.cpp
    src/runtime/tagged_random.cpp
)
target_include_directories(bug_runtime_core PUBLIC src)
target_link_libraries(bug_runtime_core PUBLIC Lua::Lua)
enable_project_warnings(bug_runtime_core)

if(WIN32)
    enable_language(RC)

    if(MINGW)
        # Tests stay attached to a console so Wine and CI expose failures.
        # Application targets remain GUI executables.
        set(SDL2_NO_MWINDOWS ON)
    endif()

    find_package(SDL2 REQUIRED)
    find_package(PNG REQUIRED)

    file(TO_CMAKE_PATH
        "${CMAKE_CURRENT_SOURCE_DIR}/packaging/cockroach.ico"
        COCKROACH_ICON_PATH)
    configure_file(
        "${CMAKE_CURRENT_SOURCE_DIR}/packaging/windows_app.rc.in"
        "${CMAKE_CURRENT_BINARY_DIR}/cockroach_windows_app.rc"
        @ONLY)
    set_source_files_properties(
        "${CMAKE_CURRENT_BINARY_DIR}/cockroach_windows_app.rc"
        PROPERTIES OBJECT_DEPENDS
        "${CMAKE_CURRENT_SOURCE_DIR}/packaging/cockroach.ico")

    set(COCKROACH_SOURCES
        src/main.cpp
        src/cockroach.cpp
        src/cockroach_parts.cpp
        src/desktop_icons.cpp
        src/overlay_window.cpp
        src/png_loader.cpp
        src/windows_interaction.cpp
    )

    function(link_sdl target use_sdl_main)
        if(TARGET SDL2::SDL2)
            if(use_sdl_main AND TARGET SDL2::SDL2main)
                target_link_libraries(${target} PRIVATE SDL2::SDL2main)
            endif()
            target_link_libraries(${target} PRIVATE SDL2::SDL2)
        else()
            target_include_directories(${target} PRIVATE ${SDL2_INCLUDE_DIRS})
            target_link_libraries(${target} PRIVATE ${SDL2_LIBRARIES})
        endif()
    endfunction()

    function(add_cockroach_executable target_name default_count)
        add_executable(${target_name} ${COCKROACH_SOURCES})
        target_sources(${target_name} PRIVATE
            "${CMAKE_CURRENT_BINARY_DIR}/cockroach_windows_app.rc")
        target_include_directories(${target_name} PRIVATE src)
        target_compile_definitions(${target_name} PRIVATE
            COCKROACH_DEFAULT_COUNT=${default_count}
            COCKROACH_SOURCE_ASSET="${CMAKE_CURRENT_SOURCE_DIR}/assets/cockroach_parts_atlas.png"
        )
        link_sdl(${target_name} TRUE)
        target_link_libraries(${target_name} PRIVATE PNG::PNG)
        set_target_properties(${target_name} PROPERTIES
            WIN32_EXECUTABLE TRUE)
        enable_project_warnings(${target_name})

        enable_mingw_static_runtime(${target_name})

        add_custom_command(TARGET ${target_name} POST_BUILD
            COMMAND ${CMAKE_COMMAND} -E make_directory
                    "$<TARGET_FILE_DIR:${target_name}>/assets"
            COMMAND ${CMAKE_COMMAND} -E copy_if_different
                    "${CMAKE_CURRENT_SOURCE_DIR}/assets/cockroach_parts_atlas.png"
                    "$<TARGET_FILE_DIR:${target_name}>/assets/cockroach_parts_atlas.png"
        )
    endfunction()

    add_library(bug_runtime_sdl STATIC
        src/runtime/sprite_rig.cpp)
    target_include_directories(bug_runtime_sdl PUBLIC src)
    target_link_libraries(bug_runtime_sdl PUBLIC bug_runtime_core)
    link_sdl(bug_runtime_sdl FALSE)
    enable_project_warnings(bug_runtime_sdl)

    add_cockroach_executable(cockroach_overlay 1)
    add_cockroach_executable(cockroach_swarm_20 20)

    install(TARGETS cockroach_overlay cockroach_swarm_20
            RUNTIME DESTINATION bin)
    install(FILES assets/cockroach_parts_atlas.png
            DESTINATION share/cockroach-overlay/assets)
else()
    message(STATUS
        "Non-Windows host: building headless runtime tests only")
endif()

include(CTest)
if(BUILD_TESTING)
    add_executable(lua_host_test tests/lua_host_test.cpp)
    target_link_libraries(lua_host_test PRIVATE bug_runtime_core)
    enable_project_warnings(lua_host_test)
    enable_mingw_static_runtime(lua_host_test)
    add_test(NAME lua_host COMMAND lua_host_test)

    add_executable(motion_solver_test tests/motion_solver_test.cpp)
    target_link_libraries(motion_solver_test PRIVATE bug_runtime_core)
    enable_project_warnings(motion_solver_test)
    enable_mingw_static_runtime(motion_solver_test)
    add_test(NAME motion_solver COMMAND motion_solver_test)

    add_executable(bug_species_test tests/bug_species_test.cpp)
    target_link_libraries(bug_species_test PRIVATE bug_runtime_core)
    target_compile_definitions(bug_species_test PRIVATE
        DESKTOP_DISPLAY_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
    enable_project_warnings(bug_species_test)
    enable_mingw_static_runtime(bug_species_test)
    add_test(NAME bug_species COMMAND bug_species_test)

    add_executable(lua_controller_test tests/lua_controller_test.cpp)
    target_link_libraries(lua_controller_test PRIVATE bug_runtime_core)
    target_compile_definitions(lua_controller_test PRIVATE
        DESKTOP_DISPLAY_SOURCE_DIR="${CMAKE_CURRENT_SOURCE_DIR}")
    enable_project_warnings(lua_controller_test)
    enable_mingw_static_runtime(lua_controller_test)
    add_test(NAME lua_controller COMMAND lua_controller_test)

    # Characterization targets remain Windows-only while the old implementation
    # is used as the migration oracle. They disappear with the legacy runtime.
    if(WIN32)
        add_executable(cockroach_motion_test
            tests/cockroach_motion_test.cpp
            src/cockroach.cpp
            src/cockroach_parts.cpp
        )
        target_include_directories(cockroach_motion_test PRIVATE src)
        link_sdl(cockroach_motion_test FALSE)
        enable_project_warnings(cockroach_motion_test)
        enable_mingw_static_runtime(cockroach_motion_test)
        add_test(NAME cockroach_motion COMMAND cockroach_motion_test)

        add_executable(cockroach_parts_test
            tests/cockroach_parts_test.cpp
            src/cockroach_parts.cpp
        )
        target_include_directories(cockroach_parts_test PRIVATE src)
        link_sdl(cockroach_parts_test FALSE)
        enable_project_warnings(cockroach_parts_test)
        enable_mingw_static_runtime(cockroach_parts_test)
        add_test(NAME cockroach_parts COMMAND cockroach_parts_test)

        add_executable(legacy_trace_generator
            tests/legacy_trace_generator.cpp
            src/cockroach.cpp
            src/cockroach_parts.cpp
        )
        target_include_directories(legacy_trace_generator PRIVATE src)
        link_sdl(legacy_trace_generator FALSE)
        enable_project_warnings(legacy_trace_generator)
        enable_mingw_static_runtime(legacy_trace_generator)
        add_test(
            NAME legacy_characterization
            COMMAND legacy_trace_generator --verify
                    "${CMAKE_CURRENT_SOURCE_DIR}/tests/golden/legacy-v1/windows-mingw")
    endif()
endif()
