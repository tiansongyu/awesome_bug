# SDL2 桌面蟑螂

一个面向 Windows 与 Linux 的透明桌面覆盖程序。启动后，写实的大蟑螂会保持在其他窗口上方爬行，同时不拦截鼠标操作。

## 效果与行为

- SDL2 渲染，透明、无边框、置顶、任务栏隐藏、默认鼠标穿透。
- 使用高细节正俯视组件图：身体、六条腿与两根触须是 9 个独立精灵。
- 蟑螂可见区域完全不透明，只有周围背景透明；运行时会适度压低 RGB 亮度。
- 六足使用交替三足步态（左前+右中+左后 / 右前+左中+右后），每条腿另有独立的微小相位变化；两根触须使用不同频率和相位探测，低速或停顿时摆幅增大，逃跑时收窄。
- 通过随速度变化的步幅、微幅横摆、身体轻晃、速度脉冲和快速转向表现爬行感。
- 以快速巡游为主，偶尔切换到带有轻微踌躇和柔和转向的低速潜行，再自然加速；另有长短停顿、受惊前瞬间僵停和突然冲刺。
- Windows 默认以 1920×1080 下 165 像素身体长度为基准，按当前显示器完整分辨率自动缩放；`--size` 可覆盖自动结果。
- 鼠标靠近时会快速向反方向逃跑。
- Windows 或 Ubuntu GNOME 桌面位于前台时，会读取每个桌面图标（含标签）的实际边界，并只使用蟑螂躯干作为碰撞体转向绕行；腿和触角不参与碰撞。
- 拖拽桌面图标时，会跟踪移动碰撞体；图标压到蟑螂上时会持续选择一个安全方向脱离，不会停住。
- 连续受阻约 0.16 秒时会探测周围 24 个方向，选择空间最大的出口短暂加速脱困；图标夹角与屏幕边缘不会让它长时间卡住。
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

CMake 会把 `assets/cockroach_parts_atlas.png` 自动复制到可执行文件旁边的 `assets` 目录。

也可以在 Ubuntu / Debian 上直接交叉编译 Windows x64 版本：

```bash
sudo apt install g++-mingw-w64-x86-64 libz-mingw-w64-dev
./scripts/build-windows.sh
```

脚本会使用官方 SDL2 2.32.10 和 libpng 1.6.58 源码/开发包，并生成
`dist/windows-x64/` 目录及 `dist/cockroach-overlay-windows-x64.zip`。

## 使用

双击或直接运行即可。Windows 会按当前显示器分辨率自动计算身体长度，
1920×1080 时为 165 像素；Linux 默认身体长度为 165 像素。默认速度倍率为 3。

Windows 默认尺寸示例：

```text
1280×720    110 px
1366×768    117 px
1920×1080   165 px
2560×1440   220 px
3840×2160   330 px
```

超宽屏按宽、高比例中较小的一项计算，避免蟑螂过大；自动缩放范围为
0.6～2.0 倍。显式使用 `--size` 会关闭自动缩放并固定为指定像素长度。

```text
cockroach_overlay [options]
  --size N              固定身体长度，100..520；Windows 下覆盖自动缩放
  --speed N             速度倍率，0.25..3
  --display N           显示器编号，从 0 开始
  --count N             蟑螂数量，1..50
  --asset PATH          使用另一张兼容的 1536×1024 组件图
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
assets/cockroach_parts_atlas.png  身体、六足、双触须的运行时组件图
assets/cockroach_parts/           九张可单独检查或替换的透明 PNG
src/cockroach.cpp                 移动状态机与 SDL2 组合绘制
src/cockroach_parts.cpp           组件坐标、三足步态与双触须动作
src/desktop_icons.cpp      Windows Explorer / Ubuntu DING 图标与拖拽跟踪
src/overlay_window.cpp     Windows / X11 透明置顶窗口
src/png_loader.cpp         基于 libpng 的 RGBA 纹理加载
CMakeLists.txt             两个平台的构建与资源复制
```

## 素材

主体以用户指定的 PNG 为基础补全，生成与透明度处理说明见 [assets/README.md](assets/README.md)。
