cmake_minimum_required(VERSION 3.16)

project(cockroach_overlay VERSION 1.0.0 LANGUAGES CXX)

set(CMAKE_CXX_STANDARD 17)
set(CMAKE_CXX_STANDARD_REQUIRED ON)
set(CMAKE_CXX_EXTENSIONS OFF)

if(WIN32)
    enable_language(RC)
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
endif()

if(MINGW)
    # Keep tests attached to a console so Wine and CI can display failures.
    # Application targets remain GUI executables via WIN32_EXECUTABLE.
    set(SDL2_NO_MWINDOWS ON)
endif()

find_package(SDL2 REQUIRED)
find_package(PNG REQUIRED)

set(COCKROACH_SOURCES
    src/main.cpp
    src/cockroach.cpp
    src/cockroach_parts.cpp
    src/desktop_icons.cpp
    src/overlay_window.cpp
    src/png_loader.cpp
    src/windows_interaction.cpp
)

function(add_cockroach_executable target_name default_count)
    add_executable(${target_name} ${COCKROACH_SOURCES})
    if(WIN32)
        target_sources(${target_name} PRIVATE
            "${CMAKE_CURRENT_BINARY_DIR}/cockroach_windows_app.rc")
    endif()
    target_include_directories(${target_name} PRIVATE src)
    target_compile_definitions(${target_name} PRIVATE
        COCKROACH_DEFAULT_COUNT=${default_count}
        COCKROACH_SOURCE_ASSET="${CMAKE_CURRENT_SOURCE_DIR}/assets/cockroach_parts_atlas.png"
    )

    if(TARGET SDL2::SDL2)
        if(WIN32 AND TARGET SDL2::SDL2main)
            target_link_libraries(${target_name} PRIVATE SDL2::SDL2main)
        endif()
        target_link_libraries(${target_name} PRIVATE SDL2::SDL2)
    else()
        target_include_directories(${target_name} PRIVATE ${SDL2_INCLUDE_DIRS})
        target_link_libraries(${target_name} PRIVATE ${SDL2_LIBRARIES})
    endif()

    target_link_libraries(${target_name} PRIVATE PNG::PNG)

    if(WIN32)
        set_target_properties(${target_name} PROPERTIES WIN32_EXECUTABLE TRUE)
    endif()

    if(UNIX AND NOT APPLE)
        target_include_directories(${target_name} PRIVATE ${X11_INCLUDE_DIR})
        target_link_libraries(${target_name} PRIVATE
            PkgConfig::ATSPI2
            ${X11_LIBRARIES}
            ${X11_Xext_LIB}
            ${X11_Xfixes_LIB}
            ${X11_Xrender_LIB}
        )
    endif()

    if(MSVC)
        target_compile_options(${target_name} PRIVATE /W4 /permissive-)
    else()
        target_compile_options(${target_name} PRIVATE
            -Wall -Wextra -Wpedantic)
    endif()

    if(MINGW)
        target_link_options(${target_name} PRIVATE
            -static-libgcc -static-libstdc++)
    endif()

    add_custom_command(TARGET ${target_name} POST_BUILD
        COMMAND ${CMAKE_COMMAND} -E make_directory
                "$<TARGET_FILE_DIR:${target_name}>/assets"
        COMMAND ${CMAKE_COMMAND} -E copy_if_different
                "${CMAKE_CURRENT_SOURCE_DIR}/assets/cockroach_parts_atlas.png"
                "$<TARGET_FILE_DIR:${target_name}>/assets/cockroach_parts_atlas.png"
    )
endfunction()

if(UNIX AND NOT APPLE)
    find_package(X11 REQUIRED)
    find_package(PkgConfig REQUIRED)
    pkg_check_modules(ATSPI2 REQUIRED IMPORTED_TARGET
        atspi-2
        gobject-2.0
    )
endif()

add_cockroach_executable(cockroach_overlay 1)
add_cockroach_executable(cockroach_swarm_20 20)

include(CTest)
if(BUILD_TESTING)
    add_executable(cockroach_motion_test
        tests/cockroach_motion_test.cpp
        src/cockroach.cpp
        src/cockroach_parts.cpp
    )
    target_include_directories(cockroach_motion_test PRIVATE src)
    if(TARGET SDL2::SDL2)
        target_link_libraries(cockroach_motion_test PRIVATE SDL2::SDL2)
    else()
        target_include_directories(cockroach_motion_test
            PRIVATE ${SDL2_INCLUDE_DIRS})
        target_link_libraries(cockroach_motion_test
            PRIVATE ${SDL2_LIBRARIES})
    endif()
    if(MSVC)
        target_compile_options(cockroach_motion_test
            PRIVATE /W4 /permissive-)
    else()
        target_compile_options(cockroach_motion_test
            PRIVATE -Wall -Wextra -Wpedantic)
    endif()
    if(MINGW)
        target_link_options(cockroach_motion_test PRIVATE
            -static-libgcc -static-libstdc++)
    endif()
    add_test(NAME cockroach_motion COMMAND cockroach_motion_test)

    add_executable(cockroach_parts_test
        tests/cockroach_parts_test.cpp
        src/cockroach_parts.cpp
    )
    target_include_directories(cockroach_parts_test PRIVATE src)
    if(TARGET SDL2::SDL2)
        target_link_libraries(cockroach_parts_test PRIVATE SDL2::SDL2)
    else()
        target_include_directories(cockroach_parts_test
            PRIVATE ${SDL2_INCLUDE_DIRS})
        target_link_libraries(cockroach_parts_test
            PRIVATE ${SDL2_LIBRARIES})
    endif()
    if(MSVC)
        target_compile_options(cockroach_parts_test
            PRIVATE /W4 /permissive-)
    else()
        target_compile_options(cockroach_parts_test
            PRIVATE -Wall -Wextra -Wpedantic)
    endif()
    if(MINGW)
        target_link_options(cockroach_parts_test PRIVATE
            -static-libgcc -static-libstdc++)
    endif()
    add_test(NAME cockroach_parts COMMAND cockroach_parts_test)
endif()

install(TARGETS cockroach_overlay cockroach_swarm_20 RUNTIME DESTINATION bin)
install(FILES assets/cockroach_parts_atlas.png
        DESTINATION share/cockroach-overlay/assets)
if(UNIX AND NOT APPLE)
    install(FILES packaging/UBUNTU-README.txt
            DESTINATION .
            RENAME README.txt)
endif()
