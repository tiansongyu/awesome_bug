#include "desktop_icons.h"

#include <algorithm>
#include <chrono>
#include <cmath>
#include <cstddef>
#include <string>
#include <utility>

#if defined(_WIN32)
#define WIN32_LEAN_AND_MEAN
#include <windows.h>
#include <commctrl.h>
#elif defined(__linux__)
#include <X11/Xatom.h>
#include <X11/Xlib.h>
#include <atspi/atspi.h>
#endif

struct DesktopIconTracker::Impl {
    std::vector<ScreenObstacle> obstacles;

#if defined(_WIN32)
    HWND listView = nullptr;
    HANDLE explorerProcess = nullptr;
    void* remoteRectangle = nullptr;
    DWORD explorerProcessId = 0;
    std::vector<ScreenObstacle> iconObstacles;
    std::chrono::steady_clock::time_point lastRefresh{};
    bool wasDesktopActive = false;
    bool wasLeftButtonDown = false;
    bool dragCandidate = false;
    bool dragging = false;
    std::size_t draggedIndex = 0;
    Vec2 mouseDownPosition{};
    Vec2 dragPointerOffset{};
    float draggedWidth = 76.0f;
    float draggedHeight = 82.0f;

    ~Impl() {
        releaseExplorerAccess();
    }

    void releaseExplorerAccess() {
        if (remoteRectangle && explorerProcess) {
            VirtualFreeEx(explorerProcess, remoteRectangle, 0, MEM_RELEASE);
        }
        remoteRectangle = nullptr;
        if (explorerProcess) CloseHandle(explorerProcess);
        explorerProcess = nullptr;
        explorerProcessId = 0;
    }

    static HWND listViewInHost(HWND host) {
        if (!host) return nullptr;
        HWND shellView =
            FindWindowExW(host, nullptr, L"SHELLDLL_DefView", nullptr);
        if (!shellView) return nullptr;
        return FindWindowExW(shellView, nullptr, L"SysListView32", nullptr);
    }

    static BOOL CALLBACK findWorkerListView(HWND window, LPARAM parameter) {
        HWND* result = reinterpret_cast<HWND*>(parameter);
        if (HWND listView = listViewInHost(window)) {
            *result = listView;
            return FALSE;
        }
        return TRUE;
    }

    static HWND findDesktopListView() {
        if (HWND listView = listViewInHost(
                FindWindowW(L"Progman", nullptr))) {
            return listView;
        }
        HWND result = nullptr;
        EnumWindows(findWorkerListView,
                    reinterpret_cast<LPARAM>(&result));
        return result;
    }

    bool connectToExplorer() {
        HWND discovered = findDesktopListView();
        if (!discovered || !IsWindow(discovered)) {
            listView = nullptr;
            releaseExplorerAccess();
            return false;
        }

        DWORD processId = 0;
        GetWindowThreadProcessId(discovered, &processId);
        if (discovered == listView && explorerProcess &&
            processId == explorerProcessId && remoteRectangle) {
            return true;
        }

        listView = discovered;
        releaseExplorerAccess();
        explorerProcessId = processId;
        explorerProcess = OpenProcess(
            PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_OPERATION |
                PROCESS_VM_READ | PROCESS_VM_WRITE,
            FALSE, explorerProcessId);
        if (!explorerProcess) return false;

        remoteRectangle = VirtualAllocEx(
            explorerProcess, nullptr, sizeof(RECT),
            MEM_COMMIT | MEM_RESERVE, PAGE_READWRITE);
        if (!remoteRectangle) {
            releaseExplorerAccess();
            return false;
        }
        return true;
    }

    bool desktopIsForeground() const {
        if (!listView || !IsWindowVisible(listView)) return false;
        HWND foreground = GetForegroundWindow();
        if (!foreground) return false;

        HWND desktopRoot = GetAncestor(listView, GA_ROOT);
        HWND foregroundRoot = GetAncestor(foreground, GA_ROOT);
        if (foreground == listView || foregroundRoot == desktopRoot ||
            foreground == GetShellWindow()) {
            return true;
        }

        DWORD foregroundProcessId = 0;
        GetWindowThreadProcessId(foreground, &foregroundProcessId);
        if (foregroundProcessId == GetCurrentProcessId()) {
            return true;
        }

        wchar_t className[64]{};
        if (GetClassNameW(foreground, className,
                          static_cast<int>(std::size(className))) <= 0) {
            return false;
        }
        return std::wcscmp(className, L"Progman") == 0 ||
               std::wcscmp(className, L"WorkerW") == 0 ||
               std::wcscmp(className, L"Shell_TrayWnd") == 0 ||
               std::wcscmp(className, L"Shell_SecondaryTrayWnd") == 0;
    }

    bool sendListViewMessage(UINT message, WPARAM wParam, LPARAM lParam,
                             DWORD_PTR& result) const {
        return SendMessageTimeoutW(
                   listView, message, wParam, lParam,
                   SMTO_ABORTIFHUNG | SMTO_BLOCK, 100, &result) != 0;
    }

    bool readItemRectangle(int index, RECT& rectangle) const {
        RECT request{};
        request.left = LVIR_BOUNDS;
        SIZE_T transferred = 0;
        if (!WriteProcessMemory(explorerProcess, remoteRectangle, &request,
                                sizeof(request), &transferred) ||
            transferred != sizeof(request)) {
            return false;
        }

        DWORD_PTR messageResult = 0;
        if (!sendListViewMessage(
                LVM_GETITEMRECT, static_cast<WPARAM>(index),
                reinterpret_cast<LPARAM>(remoteRectangle), messageResult) ||
            messageResult == 0) {
            return false;
        }

        transferred = 0;
        if (!ReadProcessMemory(explorerProcess, remoteRectangle, &rectangle,
                               sizeof(rectangle), &transferred) ||
            transferred != sizeof(rectangle)) {
            return false;
        }

        POINT corners[2]{{rectangle.left, rectangle.top},
                         {rectangle.right, rectangle.bottom}};
        SetLastError(ERROR_SUCCESS);
        if (MapWindowPoints(listView, nullptr, corners, 2) == 0 &&
            GetLastError() != ERROR_SUCCESS) {
            return false;
        }
        rectangle = {corners[0].x, corners[0].y,
                     corners[1].x, corners[1].y};
        return true;
    }

    bool refreshIconRectangles() {
        if (!connectToExplorer()) return false;

        DWORD_PTR countResult = 0;
        if (!sendListViewMessage(LVM_GETITEMCOUNT, 0, 0, countResult)) {
            releaseExplorerAccess();
            return false;
        }
        const int count = std::clamp(
            static_cast<int>(countResult), 0, 2048);

        std::vector<ScreenObstacle> refreshed;
        refreshed.reserve(static_cast<std::size_t>(count));
        for (int index = 0; index < count; ++index) {
            RECT rectangle{};
            if (!readItemRectangle(index, rectangle)) continue;
            const int width = rectangle.right - rectangle.left;
            const int height = rectangle.bottom - rectangle.top;
            if (width < 8 || height < 8 || width > 360 || height > 300) {
                continue;
            }

            // Keep a real visible gap around both the icon and its label.
            // Cockroach collision bounds add their own sprite-sized margin.
            constexpr float padding = 9.0f;
            refreshed.push_back(
                {static_cast<float>(rectangle.left) - padding,
                 static_cast<float>(rectangle.top) - padding,
                 static_cast<float>(width) + padding * 2.0f,
                 static_cast<float>(height) + padding * 2.0f,
                 false});
        }
        iconObstacles = std::move(refreshed);
        return true;
    }

    static bool contains(const ScreenObstacle& obstacle, Vec2 point,
                         float padding = 0.0f) {
        return point.x >= obstacle.x - padding &&
               point.x <= obstacle.x + obstacle.width + padding &&
               point.y >= obstacle.y - padding &&
               point.y <= obstacle.y + obstacle.height + padding;
    }

    void resetDrag() {
        dragCandidate = false;
        dragging = false;
    }

    void updateDrag(Vec2 cursor) {
        const bool leftButtonDown =
            (GetAsyncKeyState(VK_LBUTTON) & 0x8000) != 0;

        if (leftButtonDown && !wasLeftButtonDown) {
            resetDrag();
            mouseDownPosition = cursor;
            for (std::size_t index = 0;
                 index < iconObstacles.size(); ++index) {
                const ScreenObstacle& obstacle = iconObstacles[index];
                if (!contains(obstacle, cursor, 4.0f)) continue;
                dragCandidate = true;
                draggedIndex = index;
                draggedWidth = obstacle.width;
                draggedHeight = obstacle.height;
                const Vec2 center{
                    obstacle.x + obstacle.width * 0.5f,
                    obstacle.y + obstacle.height * 0.5f};
                dragPointerOffset = cursor - center;
                break;
            }
        } else if (!leftButtonDown && wasLeftButtonDown) {
            resetDrag();
            lastRefresh = {};
        }

        if (leftButtonDown && dragCandidate && !dragging &&
            length(cursor - mouseDownPosition) >= 6.0f) {
            dragging = true;
        }
        wasLeftButtonDown = leftButtonDown;
    }

    void publishObstacles(Vec2 cursor) {
        obstacles.clear();
        obstacles.reserve(iconObstacles.size() + (dragging ? 1u : 0u));
        for (std::size_t index = 0; index < iconObstacles.size(); ++index) {
            if (dragging && index == draggedIndex) continue;
            obstacles.push_back(iconObstacles[index]);
        }
        if (dragging) {
            const Vec2 center = cursor - dragPointerOffset;
            constexpr float dragPadding = 12.0f;
            obstacles.push_back(
                {center.x - draggedWidth * 0.5f - dragPadding,
                 center.y - draggedHeight * 0.5f - dragPadding,
                 draggedWidth + dragPadding * 2.0f,
                 draggedHeight + dragPadding * 2.0f,
                 true});
        }
    }

    void update(Vec2 cursor) {
        if (!connectToExplorer() || !desktopIsForeground()) {
            obstacles.clear();
            wasDesktopActive = false;
            wasLeftButtonDown = false;
            resetDrag();
            return;
        }

        const auto now = std::chrono::steady_clock::now();
        const bool refreshDue =
            !wasDesktopActive || lastRefresh.time_since_epoch().count() == 0 ||
            now - lastRefresh >= std::chrono::milliseconds(120);
        if (refreshDue) {
            if (!refreshIconRectangles()) {
                // Explorer can briefly stop responding while it rearranges or
                // redraws desktop icons. Keep the last valid rectangles for
                // that frame instead of opening a collision-free gap.
                updateDrag(cursor);
                publishObstacles(cursor);
                return;
            }
            lastRefresh = now;
        }
        wasDesktopActive = true;
        updateDrag(cursor);
        publishObstacles(cursor);
    }
#elif defined(__linux__)
    struct NativeDesktopFrame {
        ::Window window = 0;
        std::string title;
        int x = 0;
        int y = 0;
        int width = 0;
        int height = 0;
    };

    Display* display = nullptr;
    ::Window rootWindow = 0;
    Atom netClientList = None;
    Atom netActiveWindow = None;
    Atom netWindowType = None;
    Atom netWindowTypeDesktop = None;
    Atom netWindowName = None;
    Atom utf8String = None;
    bool atspiReady = false;
    bool ownsAtspi = false;
    AtspiAccessible* dingApplication = nullptr;
    std::vector<NativeDesktopFrame> nativeFrames;
    std::vector<ScreenObstacle> iconObstacles;
    std::chrono::steady_clock::time_point lastNativeRefresh{};
    std::chrono::steady_clock::time_point lastIconRefresh{};
    bool wasDesktopActive = false;
    bool wasLeftButtonDown = false;
    bool dragCandidate = false;
    bool dragging = false;
    std::size_t draggedIndex = 0;
    Vec2 mouseDownPosition{};
    Vec2 dragPointerOffset{};
    float draggedWidth = 96.0f;
    float draggedHeight = 104.0f;

    Impl() {
        display = XOpenDisplay(nullptr);
        if (display) {
            rootWindow = DefaultRootWindow(display);
            netClientList =
                XInternAtom(display, "_NET_CLIENT_LIST", False);
            netActiveWindow =
                XInternAtom(display, "_NET_ACTIVE_WINDOW", False);
            netWindowType =
                XInternAtom(display, "_NET_WM_WINDOW_TYPE", False);
            netWindowTypeDesktop = XInternAtom(
                display, "_NET_WM_WINDOW_TYPE_DESKTOP", False);
            netWindowName =
                XInternAtom(display, "_NET_WM_NAME", False);
            utf8String = XInternAtom(display, "UTF8_STRING", False);
        }

        ownsAtspi = !atspi_is_initialized();
        atspiReady = !ownsAtspi || atspi_init() == 0;
        if (atspiReady) {
            // A stale accessibility provider must not stall the render loop.
            atspi_set_timeout(90, 300);
        }
    }

    ~Impl() {
        releaseDingApplication();
        if (ownsAtspi && atspiReady) atspi_exit();
        if (display) XCloseDisplay(display);
    }

    void releaseDingApplication() {
        if (dingApplication) g_object_unref(dingApplication);
        dingApplication = nullptr;
    }

    static void clearError(GError*& error) {
        if (error) g_error_free(error);
        error = nullptr;
    }

    static bool accessibleName(AtspiAccessible* accessible,
                               std::string& result) {
        GError* error = nullptr;
        gchar* name =
            atspi_accessible_get_name(accessible, &error);
        if (error || !name) {
            if (name) g_free(name);
            clearError(error);
            return false;
        }
        result.assign(name);
        g_free(name);
        return true;
    }

    static int accessibleChildCount(AtspiAccessible* accessible) {
        GError* error = nullptr;
        const int count =
            atspi_accessible_get_child_count(accessible, &error);
        if (error) {
            clearError(error);
            return -1;
        }
        return std::clamp(count, 0, 4096);
    }

    static bool accessibleRole(AtspiAccessible* accessible,
                               AtspiRole& role) {
        GError* error = nullptr;
        role = atspi_accessible_get_role(accessible, &error);
        if (error) {
            clearError(error);
            return false;
        }
        return true;
    }

    static bool accessibleExtents(AtspiAccessible* accessible,
                                  AtspiRect& result) {
        AtspiComponent* component =
            atspi_accessible_get_component_iface(accessible);
        if (!component) return false;

        GError* error = nullptr;
        AtspiRect* rectangle = atspi_component_get_extents(
            component, ATSPI_COORD_TYPE_SCREEN, &error);
        g_object_unref(component);
        if (error || !rectangle) {
            if (rectangle) {
                g_boxed_free(ATSPI_TYPE_RECT, rectangle);
            }
            clearError(error);
            return false;
        }

        result = *rectangle;
        g_boxed_free(ATSPI_TYPE_RECT, rectangle);
        return true;
    }

    bool readWindowProperty(::Window window, Atom property,
                            Atom requestedType,
                            std::vector<unsigned long>& values) const {
        values.clear();
        if (!display || property == None) return false;

        Atom actualType = None;
        int actualFormat = 0;
        unsigned long count = 0;
        unsigned long bytesAfter = 0;
        unsigned char* data = nullptr;
        const int status = XGetWindowProperty(
            display, window, property, 0, 4096, False,
            requestedType, &actualType, &actualFormat, &count,
            &bytesAfter, &data);
        if (status != Success || actualType != requestedType ||
            actualFormat != 32 || !data) {
            if (data) XFree(data);
            return false;
        }

        const auto* source =
            reinterpret_cast<const unsigned long*>(data);
        values.assign(source, source + count);
        XFree(data);
        return true;
    }

    bool readWindowTitle(::Window window, std::string& title) const {
        title.clear();
        Atom actualType = None;
        int actualFormat = 0;
        unsigned long count = 0;
        unsigned long bytesAfter = 0;
        unsigned char* data = nullptr;
        const int status = XGetWindowProperty(
            display, window, netWindowName, 0, 1024, False,
            utf8String, &actualType, &actualFormat, &count,
            &bytesAfter, &data);
        if (status == Success && actualType == utf8String &&
            actualFormat == 8 && data) {
            title.assign(reinterpret_cast<const char*>(data), count);
            XFree(data);
            return true;
        }
        if (data) XFree(data);

        char* legacyTitle = nullptr;
        if (XFetchName(display, window, &legacyTitle) == 0 ||
            !legacyTitle) {
            return false;
        }
        title.assign(legacyTitle);
        XFree(legacyTitle);
        return true;
    }

    bool readWindowGeometry(::Window window,
                            NativeDesktopFrame& frame) const {
        XWindowAttributes attributes{};
        if (!XGetWindowAttributes(display, window, &attributes) ||
            attributes.map_state != IsViewable ||
            attributes.width <= 0 || attributes.height <= 0) {
            return false;
        }

        int rootX = 0;
        int rootY = 0;
        ::Window child = 0;
        if (!XTranslateCoordinates(display, window, rootWindow, 0, 0,
                                   &rootX, &rootY, &child)) {
            return false;
        }
        frame.x = rootX;
        frame.y = rootY;
        frame.width = attributes.width;
        frame.height = attributes.height;
        return true;
    }

    bool refreshNativeFrames() {
        std::vector<unsigned long> clients;
        if (!readWindowProperty(rootWindow, netClientList, XA_WINDOW,
                                clients)) {
            nativeFrames.clear();
            return false;
        }

        std::vector<NativeDesktopFrame> refreshed;
        for (const unsigned long value : clients) {
            const ::Window window = static_cast<::Window>(value);
            std::vector<unsigned long> types;
            if (!readWindowProperty(window, netWindowType, XA_ATOM,
                                    types) ||
                std::find(types.begin(), types.end(),
                          static_cast<unsigned long>(
                              netWindowTypeDesktop)) == types.end()) {
                continue;
            }

            NativeDesktopFrame frame;
            frame.window = window;
            if (!readWindowTitle(window, frame.title) ||
                frame.title.rfind("@!", 0) != 0 ||
                !readWindowGeometry(window, frame)) {
                continue;
            }
            refreshed.push_back(std::move(frame));
        }
        nativeFrames = std::move(refreshed);
        return !nativeFrames.empty();
    }

    bool desktopIsForeground() const {
        std::vector<unsigned long> active;
        if (!readWindowProperty(rootWindow, netActiveWindow, XA_WINDOW,
                                active) ||
            active.empty()) {
            return false;
        }
        const ::Window activeWindow =
            static_cast<::Window>(active.front());
        return std::any_of(
            nativeFrames.begin(), nativeFrames.end(),
            [activeWindow](const NativeDesktopFrame& frame) {
                return frame.window == activeWindow;
            });
    }

    const NativeDesktopFrame* nativeFrameNamed(
        const std::string& name) const {
        const auto found = std::find_if(
            nativeFrames.begin(), nativeFrames.end(),
            [&name](const NativeDesktopFrame& frame) {
                return frame.title == name;
            });
        return found == nativeFrames.end() ? nullptr : &*found;
    }

    bool applicationHasDesktopFrame(AtspiAccessible* application) const {
        const int count = accessibleChildCount(application);
        if (count < 0) return false;
        for (int index = 0; index < count; ++index) {
            AtspiAccessible* child =
                atspi_accessible_get_child_at_index(
                    application, index, nullptr);
            if (!child) continue;
            AtspiRole role = ATSPI_ROLE_INVALID;
            std::string name;
            const bool matches =
                accessibleRole(child, role) &&
                role == ATSPI_ROLE_FRAME &&
                accessibleName(child, name) &&
                nativeFrameNamed(name) != nullptr;
            g_object_unref(child);
            if (matches) return true;
        }
        return false;
    }

    bool discoverDingApplication() {
        releaseDingApplication();
        AtspiAccessible* desktop = atspi_get_desktop(0);
        if (!desktop) return false;

        const int count = accessibleChildCount(desktop);
        if (count >= 0) {
            for (int index = 0; index < count; ++index) {
                AtspiAccessible* application =
                    atspi_accessible_get_child_at_index(
                        desktop, index, nullptr);
                if (!application) continue;
                if (applicationHasDesktopFrame(application)) {
                    dingApplication = application;
                    break;
                }
                g_object_unref(application);
            }
        }
        g_object_unref(desktop);
        return dingApplication != nullptr;
    }

    static unsigned int subtreeContentMask(
        AtspiAccessible* accessible, int remainingDepth) {
        AtspiRole role = ATSPI_ROLE_INVALID;
        if (!accessibleRole(accessible, role)) return 0;

        unsigned int mask = 0;
        if (role == ATSPI_ROLE_ICON || role == ATSPI_ROLE_IMAGE) {
            mask |= 1u;
        }
        if (role == ATSPI_ROLE_LABEL || role == ATSPI_ROLE_STATIC ||
            role == ATSPI_ROLE_TEXT) {
            mask |= 2u;
        }
        if (mask == 3u || remainingDepth <= 0) return mask;

        const int count = accessibleChildCount(accessible);
        if (count < 0) return mask;
        for (int index = 0; index < count && mask != 3u; ++index) {
            AtspiAccessible* child =
                atspi_accessible_get_child_at_index(
                    accessible, index, nullptr);
            if (!child) continue;
            mask |= subtreeContentMask(child, remainingDepth - 1);
            g_object_unref(child);
        }
        return mask;
    }

    static bool possibleItemContainer(AtspiRole role) {
        return role == ATSPI_ROLE_FILLER ||
               role == ATSPI_ROLE_PANEL ||
               role == ATSPI_ROLE_PUSH_BUTTON ||
               role == ATSPI_ROLE_LIST_ITEM ||
               role == ATSPI_ROLE_DESKTOP_ICON;
    }

    void collectIconRectangles(
        AtspiAccessible* accessible, const AtspiRect& logicalFrame,
        const NativeDesktopFrame& nativeFrame, int remainingDepth,
        std::vector<ScreenObstacle>& destination) const {
        if (remainingDepth <= 0) return;

        AtspiRole role = ATSPI_ROLE_INVALID;
        if (!accessibleRole(accessible, role)) return;
        const int count = accessibleChildCount(accessible);
        if (count < 0) return;

        if (count >= 2 && possibleItemContainer(role)) {
            AtspiRect logicalItem{};
            if (accessibleExtents(accessible, logicalItem) &&
                logicalItem.width >= 8 && logicalItem.height >= 8 &&
                logicalItem.width <= 420 &&
                logicalItem.height <= 360 &&
                subtreeContentMask(accessible, 6) == 3u) {
                const float scaleX =
                    nativeFrame.width /
                    static_cast<float>(logicalFrame.width);
                const float scaleY =
                    nativeFrame.height /
                    static_cast<float>(logicalFrame.height);
                constexpr float padding = 5.0f;
                destination.push_back({
                    nativeFrame.x +
                        (logicalItem.x - logicalFrame.x) * scaleX -
                        padding,
                    nativeFrame.y +
                        (logicalItem.y - logicalFrame.y) * scaleY -
                        padding,
                    logicalItem.width * scaleX + padding * 2.0f,
                    logicalItem.height * scaleY + padding * 2.0f,
                    false});
                return;
            }
        }

        for (int index = 0; index < count; ++index) {
            AtspiAccessible* child =
                atspi_accessible_get_child_at_index(
                    accessible, index, nullptr);
            if (!child) continue;
            collectIconRectangles(
                child, logicalFrame, nativeFrame,
                remainingDepth - 1, destination);
            g_object_unref(child);
        }
    }

    bool scanDingApplication(
        std::vector<ScreenObstacle>& refreshed) const {
        if (!dingApplication) return false;
        const int count = accessibleChildCount(dingApplication);
        if (count < 0) return false;

        bool matchedFrame = false;
        for (int index = 0; index < count; ++index) {
            AtspiAccessible* frame =
                atspi_accessible_get_child_at_index(
                    dingApplication, index, nullptr);
            if (!frame) continue;

            AtspiRole role = ATSPI_ROLE_INVALID;
            std::string name;
            AtspiRect logicalFrame{};
            const NativeDesktopFrame* nativeFrame = nullptr;
            if (accessibleRole(frame, role) &&
                role == ATSPI_ROLE_FRAME &&
                accessibleName(frame, name)) {
                nativeFrame = nativeFrameNamed(name);
            }
            if (nativeFrame &&
                accessibleExtents(frame, logicalFrame) &&
                logicalFrame.width > 0 && logicalFrame.height > 0) {
                matchedFrame = true;
                collectIconRectangles(
                    frame, logicalFrame, *nativeFrame, 12, refreshed);
            }
            g_object_unref(frame);
        }
        return matchedFrame;
    }

    bool refreshIconRectangles() {
        if (!dingApplication && !discoverDingApplication()) return false;

        std::vector<ScreenObstacle> refreshed;
        if (!scanDingApplication(refreshed)) {
            if (!discoverDingApplication()) return false;
            refreshed.clear();
            if (!scanDingApplication(refreshed)) return false;
        }
        iconObstacles = std::move(refreshed);
        return true;
    }

    static bool contains(const ScreenObstacle& obstacle, Vec2 point,
                         float padding = 0.0f) {
        return point.x >= obstacle.x - padding &&
               point.x <= obstacle.x + obstacle.width + padding &&
               point.y >= obstacle.y - padding &&
               point.y <= obstacle.y + obstacle.height + padding;
    }

    bool leftButtonDown() const {
        if (!display) return false;
        ::Window rootReturned = 0;
        ::Window childReturned = 0;
        int rootX = 0;
        int rootY = 0;
        int windowX = 0;
        int windowY = 0;
        unsigned int mask = 0;
        return XQueryPointer(
                   display, rootWindow, &rootReturned, &childReturned,
                   &rootX, &rootY, &windowX, &windowY, &mask) != 0 &&
               (mask & Button1Mask) != 0;
    }

    void resetDrag() {
        dragCandidate = false;
        dragging = false;
    }

    void updateDrag(Vec2 cursor) {
        const bool buttonDown = leftButtonDown();
        if (buttonDown && !wasLeftButtonDown) {
            resetDrag();
            mouseDownPosition = cursor;
            for (std::size_t index = 0;
                 index < iconObstacles.size(); ++index) {
                const ScreenObstacle& obstacle = iconObstacles[index];
                if (!contains(obstacle, cursor, 4.0f)) continue;
                dragCandidate = true;
                draggedIndex = index;
                draggedWidth = obstacle.width;
                draggedHeight = obstacle.height;
                const Vec2 center{
                    obstacle.x + obstacle.width * 0.5f,
                    obstacle.y + obstacle.height * 0.5f};
                dragPointerOffset = cursor - center;
                break;
            }
        } else if (!buttonDown && wasLeftButtonDown) {
            resetDrag();
            lastIconRefresh = {};
        }

        if (buttonDown && dragCandidate && !dragging &&
            length(cursor - mouseDownPosition) >= 6.0f) {
            dragging = true;
        }
        wasLeftButtonDown = buttonDown;
    }

    void publishObstacles(Vec2 cursor) {
        obstacles.clear();
        obstacles.reserve(iconObstacles.size() + (dragging ? 1u : 0u));
        for (std::size_t index = 0; index < iconObstacles.size(); ++index) {
            if (dragging && index == draggedIndex) continue;
            obstacles.push_back(iconObstacles[index]);
        }
        if (dragging) {
            const Vec2 center = cursor - dragPointerOffset;
            constexpr float dragPadding = 12.0f;
            obstacles.push_back({
                center.x - draggedWidth * 0.5f - dragPadding,
                center.y - draggedHeight * 0.5f - dragPadding,
                draggedWidth + dragPadding * 2.0f,
                draggedHeight + dragPadding * 2.0f,
                true});
        }
    }

    void update(Vec2 cursor) {
        if (!display || !atspiReady) {
            obstacles.clear();
            return;
        }

        const auto now = std::chrono::steady_clock::now();
        const bool nativeRefreshDue =
            nativeFrames.empty() ||
            lastNativeRefresh.time_since_epoch().count() == 0 ||
            now - lastNativeRefresh >= std::chrono::milliseconds(500);
        if (nativeRefreshDue) {
            refreshNativeFrames();
            lastNativeRefresh = now;
        }

        const bool buttonDown = leftButtonDown();
        const bool keepTrackingDrag =
            buttonDown && (dragCandidate || dragging);
        if (!desktopIsForeground() && !keepTrackingDrag) {
            obstacles.clear();
            wasDesktopActive = false;
            wasLeftButtonDown = buttonDown;
            resetDrag();
            return;
        }

        const bool iconRefreshDue =
            !wasDesktopActive ||
            lastIconRefresh.time_since_epoch().count() == 0 ||
            now - lastIconRefresh >= std::chrono::milliseconds(120);
        if (iconRefreshDue) {
            if (!refreshIconRectangles()) {
                obstacles.clear();
                wasDesktopActive = false;
                return;
            }
            lastIconRefresh = now;
        }

        wasDesktopActive = true;
        updateDrag(cursor);
        publishObstacles(cursor);
    }
#else
    void update(Vec2) {
        obstacles.clear();
    }
#endif
};

DesktopIconTracker::DesktopIconTracker()
    : impl_(std::make_unique<Impl>()) {}

DesktopIconTracker::~DesktopIconTracker() = default;

void DesktopIconTracker::update(Vec2 cursorScreenPosition) {
    impl_->update(cursorScreenPosition);
}

const std::vector<ScreenObstacle>& DesktopIconTracker::obstacles() const {
    return impl_->obstacles;
}
