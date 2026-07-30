# SDL2 桌面蟑螂

一个面向 Windows 与 Linux 的透明桌面覆盖程序。启动后，写实的大蟑螂会保持在其他窗口上方爬行，同时不拦截鼠标操作。

## 效果与行为

- SDL2 渲染，透明、无边框、置顶、任务栏隐藏、默认鼠标穿透。
- 使用用户指定的高细节背甲为基础补全的正俯视渲染图，包含完整六足、足刺、复眼和双触角。
- 蟑螂可见区域完全不透明，只有周围背景透明；运行时会适度压低 RGB 亮度。
- 通过随速度变化的微幅横摆、身体轻晃、速度脉冲、快速转向和横向碎步表现爬行感。
- 随机巡游、长短停顿、受惊前瞬间僵停、突然冲刺、沿屏幕边缘移动和自然回避。
- 鼠标靠近时会快速向反方向逃跑。
- Windows 或 Ubuntu GNOME 桌面位于前台时，会读取每个桌面图标（含标签）的实际边界，并只使用蟑螂躯干作为碰撞体转向绕行；腿和触角不参与碰撞。
- 拖拽桌面图标时，会跟踪移动碰撞体；图标压到蟑螂上时会持续选择一个安全方向脱离，不会停住。
- Ubuntu GNOME 通过 DING 的 AT-SPI 可访问性节点读取图标，并按每台显示器的缩放比例转换坐标，支持本机的多显示器与 200% 缩放。
- Windows 使用逐像素 alpha 的 layered window；Linux 使用 ARGB + XShape/XFixes。

## Linux 构建

Ubuntu / Debian：

```bash
sudo apt update
sudo apt install build-essential cmake libsdl2-dev libpng-dev \
  libx11-dev libxext-dev libxfixes-dev libxrender-dev \
  libatspi2.0-dev pkg-config

cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
./build/cockroach_overlay
```

程序会优先使用 X11。Ubuntu 的桌面图标碰撞与拖拽跟踪面向 GNOME
X11 + Desktop Icons NG（DING）。在 Wayland 桌面上，只要系统启用了
XWayland，覆盖窗口仍会尝试通过 XWayland 运行，但桌面窗口识别、绝对定位和
全局置顶会受到合成器限制；纯 Wayland 无法提供相同体验。

## Windows 构建

推荐使用 Visual Studio 2022、CMake 和 [vcpkg](https://github.com/microsoft/vcpkg)：

```powershell
git clone https://github.com/microsoft/vcpkg.git C:\vcpkg
C:\vcpkg\bootstrap-vcpkg.bat
C:\vcpkg\vcpkg.exe install sdl2:x64-windows libpng:x64-windows

cmake -S . -B build-win `
  -DCMAKE_TOOLCHAIN_FILE=C:\vcpkg\scripts\buildsystems\vcpkg.cmake `
  -DVCPKG_TARGET_TRIPLET=x64-windows
cmake --build build-win --config Release
.\build-win\Release\cockroach_overlay.exe
```

CMake 会把 `assets/cockroach_full.png` 自动复制到可执行文件旁边的 `assets` 目录。

也可以在 Ubuntu / Debian 上直接交叉编译 Windows x64 版本：

```bash
sudo apt install g++-mingw-w64-x86-64 libz-mingw-w64-dev
./scripts/build-windows.sh
```

脚本会使用官方 SDL2 2.32.10 和 libpng 1.6.58 源码/开发包，并生成
`dist/windows-x64/` 目录及 `dist/cockroach-overlay-windows-x64.zip`。

## 使用

双击或直接运行即可。默认身体长度为 165 像素，默认速度倍率为 3。

```text
cockroach_overlay [options]
  --size N              身体长度，100..260
  --speed N             速度倍率，0.25..3
  --display N           显示器编号，从 0 开始
  --count N             蟑螂数量，1..50
  --asset PATH          使用另一张透明 PNG 主体素材
  --no-click-through    关闭鼠标穿透，便于调试
  --frames N            渲染 N 帧后退出，便于自动测试
  --help                查看帮助
```

按 `Ctrl+Alt+Q` 可随时退出。使用 `--no-click-through` 时也可以按 `Esc` 或 `Q`。

例如，调整蟑螂大小和速度：

```bash
./build/cockroach_overlay --size 220 --speed 1.35
```

工程默认同时生成两个版本：

```text
cockroach_overlay       单只蟑螂
cockroach_swarm_20      20 只、随机大小与分区随机初始位置
```

## 工程结构

```text
assets/cockroach_full.png  补全器官的正俯视、不透明完整蟑螂
src/cockroach.cpp          移动状态机与 SDL2 绘制
src/desktop_icons.cpp      Windows Explorer / Ubuntu DING 图标与拖拽跟踪
src/overlay_window.cpp     Windows / X11 透明置顶窗口
src/png_loader.cpp         基于 libpng 的 RGBA 纹理加载
CMakeLists.txt             两个平台的构建与资源复制
```

## 素材

主体以用户指定的 PNG 为基础补全，生成与透明度处理说明见 [assets/README.md](assets/README.md)。
