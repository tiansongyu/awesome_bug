# Awesome Bug

基于 C++17 和 SDL2 的跨平台桌面宠物。程序会在 Windows 或 Linux
桌面最上层绘制一只写实蟑螂，并模拟六足步态、触须探测、随机巡游、
受惊逃跑、角落潜伏、清洁、食物诱饵以及桌面图标避障。
覆盖窗口默认鼠标穿透，不影响正常操作。

<p align="center">
  <img src="docs/screenshots/windows11-desktop-pet.png"
       alt="Awesome Bug 在 Windows 11 桌面运行"
       width="900">
</p>

## 功能

- 透明、无边框、置顶、任务栏隐藏的桌面覆盖窗口。
- 身体、六条腿和两根触须由 9 个独立精灵组成。
- 六足采用交替三足步态，每条腿带有独立的摆幅和微小相位变化。
- 两根触须使用不同频率和相位运动；慢行时扩大探测范围，逃跑时收拢。
- 支持快速巡游、低速潜行、短暂停顿、受惊僵停和突然冲刺。
- Windows 单只版本会前往屏幕角落潜伏，静止时仅触须探索，并用前腿清洁触须。
- 根据鼠标接近速度判断威胁：快速逼近会触发短暂停顿和爆发逃跑。
- `Ctrl+Alt+F` 在鼠标位置投放食物；蟑螂会绕开图标靠近并停下进食。
  食物置于所有普通桌面内容之上，但始终位于蟑螂之下。
- Windows 和 Ubuntu GNOME 下可识别桌面图标，使用躯干碰撞体绕行。
- 拖拽图标或发生重叠时主动脱离，不会因屏幕边缘或图标夹角长期卡住。
- Windows 根据显示器分辨率自动调整身体尺寸。
- 支持单只模式和 20 只随机大小、随机位置的群体模式。

蟑螂本身完全不透明；透明区域仅用于显示其周围的桌面。腿和触须不参与
桌面图标碰撞，避免视觉器官扩大实际碰撞范围。

## 生成的程序

| 程序 | 说明 |
| --- | --- |
| `cockroach_overlay` | 单只蟑螂，随机初始位置 |
| `cockroach_swarm_20` | 20 只蟑螂，分区随机生成，大小和速度略有差异 |

两个 Windows EXE 均嵌入由当前写实蟑螂素材生成的多分辨率程序图标。

## 平台支持

| 平台 | 覆盖窗口 | 桌面图标避障 | 说明 |
| --- | --- | --- | --- |
| Windows 10/11 | 完整支持 | 完整支持 | 单只版本支持潜伏、清洁与食物交互 |
| Ubuntu GNOME X11 + DING | 完整支持 | 完整支持 | 通过 AT-SPI 读取图标及拖拽状态 |
| 其他 Linux X11 桌面 | 支持 | 视桌面环境而定 | 透明置顶窗口可用，图标接口可能不同 |
| Wayland | 有限支持 | 不保证 | 合成器会限制绝对定位、全局置顶和窗口探测 |

## 快速开始

### Ubuntu / Debian

```bash
sudo apt update
sudo apt install build-essential cmake libsdl2-dev libpng-dev \
  libx11-dev libxext-dev libxfixes-dev libxrender-dev \
  libatspi2.0-dev pkg-config

git clone https://github.com/tiansongyu/awesome_bug.git
cd awesome_bug
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release
cmake --build build --parallel
./build/cockroach_overlay
```

需要完整的 Ubuntu 桌面图标交互时，建议登录 “Ubuntu on Xorg” 会话并启用
Desktop Icons NG（DING）。

### Windows

推荐使用 Visual Studio 2022、CMake 和
[vcpkg](https://github.com/microsoft/vcpkg)：

```powershell
vcpkg install sdl2:x64-windows libpng:x64-windows

cmake -S . -B build-win `
  -DCMAKE_TOOLCHAIN_FILE="$env:VCPKG_ROOT\scripts\buildsystems\vcpkg.cmake" `
  -DVCPKG_TARGET_TRIPLET=x64-windows
cmake --build build-win --config Release
.\build-win\Release\cockroach_overlay.exe
```

CMake 会把运行所需的 `cockroach_parts_atlas.png` 复制到可执行文件旁边的
`assets` 目录，并把多分辨率蟑螂图标嵌入 Windows EXE。

### 在 Ubuntu 上交叉编译 Windows 版本

```bash
sudo apt install g++-mingw-w64-x86-64 libz-mingw-w64-dev
./scripts/build-windows.sh
```

生成结果：

```text
dist/windows-x64/
dist/cockroach-overlay-windows-x64.zip
```

## 使用

```text
cockroach_overlay [options]
```

| 参数 | 说明 |
| --- | --- |
| `--size N` | 固定身体长度，范围 `100..520`；Windows 下会覆盖自动缩放 |
| `--speed N` | 速度倍率，范围 `0.25..3` |
| `--display N` | 选择显示器，编号从 `0` 开始 |
| `--count N` | 蟑螂数量，范围 `1..50` |
| `--asset PATH` | 使用兼容的 `1536×1024` 组件图 |
| `--no-click-through` | 关闭鼠标穿透，便于调试 |
| `--frames N` | 渲染指定帧数后退出，便于自动测试 |
| `--help` | 显示帮助 |

示例：

```bash
./build/cockroach_overlay --size 220 --speed 1.35
./build/cockroach_overlay --count 8
./build/cockroach_swarm_20
```

Windows 单只版本快捷键：

| 快捷键 | 功能 |
| --- | --- |
| `Ctrl+Alt+F` | 在当前鼠标位置投放或移动食物 |
| `Ctrl+Alt+Q` | 退出程序 |

关闭鼠标穿透进行调试时，也可以使用 `Esc` 或 `Q` 退出。食物功能仅在
Windows 单只程序中启用，`cockroach_swarm_20` 保持原有群体巡游行为。

Windows 默认以 1920×1080 下 165 像素身体长度为基准，根据当前显示器
分辨率缩放：

| 分辨率 | 默认身体长度 |
| --- | ---: |
| 1280×720 | 110 px |
| 1920×1080 | 165 px |
| 2560×1440 | 220 px |
| 3840×2160 | 330 px |

## 实现概览

- `Cockroach` 统一状态机负责巡游、潜伏、清洁、受惊、逃跑和觅食。
- 所有行为接收显式输入并提供只读快照，可用固定随机种子进行确定性测试。
- 六足按两组交替三足运动，并叠加每条腿独立的细微变化。
- 触须由两个不同的连续振荡器驱动，避免机械式镜像。
- Windows 使用逐像素 alpha 的 layered window。
- Linux 使用 ARGB、XShape 和 XFixes 创建透明点击穿透窗口。
- Windows 通过 Explorer 列表视图读取图标边界。
- Windows 使用独立的小型 layered window 绘制食物诱饵，并持续把它放在
  蟑螂窗口正下方。
- Ubuntu GNOME 通过 DING 的 AT-SPI 节点读取图标和拖拽状态。
- 图标碰撞只使用躯干；受阻时会探测多个候选方向并选择可用空间脱离。

## 测试

```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

测试覆盖：

- 分辨率与身体尺寸映射。
- 图标重叠后的脱离时间、连续移动和单帧位移上限。
- 屏幕边缘停留与长时间速度变化。
- 固定随机种子的状态和轨迹复现。
- 角落潜伏、清洁循环以及静止时的身体位移。
- 鼠标速度感知、受惊方向、迟滞和冷却。
- 觅食、进食静止、一次性消费及威胁打断。
- 九个精灵区域、旋转轴和 atlas 边界。
- 交替三足相位、六腿独立变化和双触须非对称运动。

## 项目结构

```text
assets/
  cockroach_parts_atlas.png   运行时组件图
  cockroach_parts/            身体、六足和双触须的独立 PNG
src/
  cockroach.cpp               行为状态机、避障和组合绘制
  cockroach_parts.cpp         精灵坐标、六足步态和触须动作
  desktop_icons.cpp           Windows Explorer / Ubuntu DING 图标跟踪
  overlay_window.cpp          Windows / X11 透明置顶窗口
  windows_interaction.cpp     Windows 食物诱饵和全局快捷键
  png_loader.cpp              libpng RGBA 纹理加载
tests/                        动作、碰撞和组件测试
scripts/
  build-windows.sh            Windows x64 交叉编译与打包
  extract-cockroach-parts.py  器官分离和 atlas 重建
packaging/
  cockroach.ico               Windows EXE 多分辨率图标
  cockroach_icon.png          图标源图
```

组件图的生成、透明度处理和素材说明见
[assets/README.md](assets/README.md)。

## 参与贡献

欢迎提交 Issue 和 Pull Request。修改动作逻辑时，请同时运行
`cockroach_motion_test` 和 `cockroach_parts_test`；修改组件图后，请确保
九个区域互不重叠，且所有可见像素保持完全不透明。

## 许可证

仓库当前尚未包含 `LICENSE` 文件。正式公开分发或接受外部贡献前，建议明确
代码许可证，并单独确认图像素材的授权范围。
