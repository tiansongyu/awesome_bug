# Awesome Bug

基于 C++17 和 SDL2 的 Windows / Linux 桌面宠物。程序请求置顶于普通应用
窗口之上，绘制写实蟑螂，并模拟六足步态、独立触须、变速巡游、受惊逃跑
和桌面图标避障。Windows 单只模式还支持角落潜伏、触须清洁和食物诱饵。
覆盖窗口默认鼠标穿透，不影响正常桌面操作。

![Awesome Bug 在 Windows 11 桌面运行](docs/screenshots/windows11-desktop-pet.png)

## 功能概览

- 透明、无边框、置顶、无任务栏按钮的桌面覆盖窗口。
- 身体、六条腿和两根触须由 9 个独立精灵实时组合。
- 六足采用交替三足步态，每条腿具有独立摆幅、相位和细微位移。
- 双触须使用不同频率和相位；探测、逃跑、清洁和进食时采用不同动作。
- 巡游速度持续变化，并穿插慢行、短暂停顿、受惊僵停和爆发逃跑。
- Windows 单只模式会主动前往角落潜伏，身体静止时仍保持触须探索。
- 鼠标靠近会惊动蟑螂；Windows 单只模式还能识别鼠标快速逼近。
- Windows 单只模式可投放食物，蟑螂会绕开图标靠近、进食并移除诱饵。
- Windows Explorer 和 Ubuntu GNOME DING 桌面图标可作为躯干障碍物。
- 支持图标拖拽、重叠分离、屏幕边缘修正和多方向卡滞恢复。
- Windows 启动时按显示器分辨率调整尺寸。
- 支持单只和多只模式；多只会以不同位置、大小和速度生成。
- GitHub Actions 可自动生成可直接运行的 Windows x64 ZIP。

蟑螂身体、腿和触须的所有可见像素均为不透明像素；透明区域只用于露出周围
桌面。阴影是单独绘制的低透明度图层。

## 生成的程序

| 程序 | 默认行为 |
| --- | --- |
| `cockroach_overlay` | `--count 1`，随机位置生成一只蟑螂 |
| `cockroach_swarm_20` | `--count 20`，分区随机生成 20 只蟑螂 |

程序名只决定默认数量。两个程序都能用 `--count 1..50` 覆盖，因此 Windows
扩展行为由**实际运行数量是否为 1**决定，而不是由 EXE 名称决定。两个
Windows EXE 均嵌入 16、24、32、48、64、128、256 像素的蟑螂程序图标。

## 平台支持

| 平台 | 覆盖窗口 | 桌面图标避障 | 功能范围 |
| --- | --- | --- | --- |
| Windows 10/11 | 完整支持 | Explorer 桌面完整支持 | 单只时启用全部 10 个状态、速度感知威胁和食物 |
| Ubuntu GNOME X11 + DING | 完整支持 | 完整支持 | 基础巡游、受惊逃跑和图标避障 |
| 其他 Linux X11 桌面 | 支持 | 不保证 | 覆盖层可用；图标读取只针对 DING 实现 |
| XWayland | 可尝试 | 不保证 | 置顶、定位和窗口探测受合成器限制 |
| 纯 Wayland | 不支持 | 不支持 | 当前实现需要 X11 或 XWayland |

## 当前实现机制

### 运行模式

| 环境 | 状态与交互 | 窗口组织 |
| --- | --- | --- |
| Windows，`count == 1` | 全部 10 个状态；速度感知威胁、角落潜伏、清洁、食物 | 一只蟑螂一个小型分层窗口 |
| Windows，`count > 1` | 巡游、慢行、暂停、受惊、逃跑；距离威胁和图标避障 | 每只蟑螂使用独立窗口 |
| Linux，任意数量 | 巡游、慢行、暂停、受惊、逃跑；距离威胁和可用的图标避障 | 单只用小窗口，多只共用所选显示器大小的透明画布 |

多只模式中的每只蟑螂拥有独立随机状态、位置、尺寸和速度，但目前没有
蟑螂之间的碰撞、跟随、聚集或群体协作；它们可以彼此重叠。

### 统一行为状态机

状态优先级为：**鼠标威胁 > 食物 > 角落休息 > 随机巡游**。威胁可以立即
打断潜伏、清洁、觅食或进食。屏幕边缘和图标避障可以临时修正运动方向，
但不会无故改写当前行为状态。

| 状态 | 适用范围 | 当前行为和退出条件 |
| --- | --- | --- |
| `Wander` 巡游 | 全部模式 | 0.95–4.20 秒；112–225 px/s；结束后随机巡游、慢行或暂停 |
| `Creep` 慢行 | 全部模式 | 持续约 0.85–2.10 秒；基础速度 30–62 px/s，结束后回到巡游 |
| `Pause` 暂停 | 全部模式 | 减速停顿 0.045–0.24 秒，少数会延长 0.25–0.55 秒，之后巡游 |
| `Startled` 受惊 | 全部模式 | 立即把期望速度降为零，原地僵停约 0.055–0.12 秒，然后进入逃跑 |
| `Flee` 逃跑 | 全部模式 | 0.72–1.35 秒；320–450 px/s；沿远离鼠标方向选取距当前位置 380–650 px 的目标 |
| `SeekCorner` 寻找角落 | Windows 单只 | 优先较近、未阻塞角落；48–82 px/s；12–32 秒后重新评估 |
| `Lurk` 潜伏 | Windows 单只 | 身体和六足静止；触须探索；4.5–7.5 秒后清洁，再停留 2.0–3.8 秒 |
| `Groom` 清洁 | Windows 单只 | 身体不移动，左右前腿交替梳理触须，持续约 3.2–5.2 秒后返回潜伏 |
| `SeekFood` 觅食 | Windows 单只 | 以 42–70 px/s 追踪诱饵并绕开图标；10–15 秒未到达会暂时放弃，2–4 秒后可重试 |
| `Feeding` 进食 | Windows 单只 | 身体静止 2.4–3.4 秒；六足中仅前腿进食；完成后移除食物 |

表中速度是状态基础速度，最终还会乘以 `--speed` 和连续变化系数。角落休息
首次通常在累计 16–34 秒普通活动后发生；离开后下一次通常间隔 18–38 秒，
完成进食后则为 12–24 秒。主要时长、基础速度、目标和部分转向扰动都会在
范围内随机选择，避免固定循环。

### 鼠标受惊与逃跑

- Windows 单只使用速度感知判定：鼠标距离小于约 `0.82 × 身体长度` 时直接
  触发；距离在约 `2.25 × 身体长度` 内时，只有鼠标速度至少 250 px/s 且朝向
  蟑螂的分量至少 180 px/s 才触发。横向划过或远离不会造成远距离误报。
- Windows 多只和 Linux 使用基础距离判定：鼠标进入约
  `1.75 × 身体长度` 的范围后触发。
- 一次威胁只产生一次 `Startled → Flee`。鼠标离开约
  `2.75 × 身体长度` 后解除锁存，逃跑结束还有约 0.85–1.25 秒冷却，避免
  鼠标持续贴近时反复重置状态。

### 速度、转向和身体运动

- `--speed` 是全局倍率，范围 `0.25..3`，默认值为 `3`。多只模式还为每只
  蟑螂乘以 `0.82..1.18` 的独立倍率。
- 巡游过程中速度继续在约 `0.72..1.04` 倍之间变化；慢行、寻角和觅食在
  `0.58..0.96` 倍之间变化；逃跑在 `0.92..1.00` 倍之间变化，因此不会长期
  保持恒速。
- 转向采用状态相关的角速度和连续谐波扰动：逃跑转向更迅速，慢行和觅食
  更谨慎。速度通过加速度逐步接近期望值，而不是瞬间切换。
- 实际位移由前向运动和极小的横向疾走组成；绘制姿态还叠加最高约 0.55 px
  的起伏、约 1.35 px 的左右摆动和约 1.27° 的身体摇摆。
- 主循环目标约为 60 FPS，单帧模拟时间最多按 50 ms 处理，避免窗口暂停后
  出现一次性大跳跃。

### 六足、触须和渲染

- 运行时 atlas 固定为 `1536×1024`，包含身体、六条腿和两根触须 9 个互不
  重叠的区域。每个器官都有自己的连接点、旋转轴和精灵变换。
- 六足采用交替三足步态：左前、右中、左后为一组，另外三足为另一组，两组
  相差半个周期。每条腿还叠加独立相位、谐波、摆幅和根部微位移。
- 步频根据实际速度和身体尺寸变化，并限制在约 0.35–5.2 周期/秒，避免
  60 Hz 下的动作混叠。潜伏时六足冻结；清洁和进食时只有前腿按对应动作运动。
- 两根触须由非镜像的多频振荡器驱动。暂停、慢行、觅食、潜伏和清洁时采用
  较宽探测动作；巡游和寻角保持中等摆幅；受惊和逃跑时收紧；进食时降低
  摆幅并与前腿动作配合。
- 身体、六足和触须使用原素材的完全不透明像素并统一降低亮度；阴影以约
  38/255 的 alpha 单独偏移绘制，不会降低蟑螂本体的不透明度。

### 桌面图标、碰撞和脱困

- 只有躯干参与图标和屏幕边缘碰撞。局部碰撞半长约为身体长度的 `0.43`，
  半宽约为 `0.20`，再按当前角度计算包围范围；腿、触须和阴影不参与。
- 运动区域保留约 10 px 屏幕边距。Windows 使用所选显示器的工作区，排除
  任务栏；Linux 使用 SDL 返回的完整显示器边界。
- 静态图标会参与前向预判和切向绕行。高速时预判距离更长，路径按小步连续
  检查，并尝试直行、分轴移动以及左右多个转向候选，减少穿过薄障碍的情况。
- 图标被鼠标拖动约 6 px 后会作为移动障碍，使用更大的影响范围和更强的远离
  力。图标直接覆盖躯干时，蟑螂按每帧有上限的距离逐步分离，不会瞬移到
  屏幕另一侧。
- 当实际位移连续多个帧明显小于期望位移时，程序会从 24 个方向探测可用空间，
  进入约 0.48–0.72 秒的短暂脱困过程，提高转向和运动意愿。该过程保留
  原行为状态，结束后继续原来的随机规律。
- 暂停、受惊、潜伏、清洁和进食等有意静止状态不会仅因附近图标而移动，
  但发生真实躯干重叠时仍会脱离；角落潜伏也不会被普通边缘恢复逻辑错误
  驱赶。

Windows 约每 120 ms 从 Explorer 的桌面 `SysListView32` 读取图标和文字的
完整边界，最多处理 2048 项。Ubuntu GNOME X11 通过 DING 窗口、EWMH 和
AT-SPI 获取图标与标签范围。只有桌面位于前台时才发布静态图标障碍；正在
拖拽的图标可继续跟踪。接口不可用或权限不同于 Explorer 时，程序会继续
运行，但降级为没有图标障碍。

图标避障不检测普通应用窗口。蟑螂覆盖层本身位于普通应用窗口之上，因此
打开其他应用后仍会显示，但不会把应用窗口边缘当成碰撞体。
普通应用位于前台时静态图标列表会被清空，此时投放食物也不会依据已被遮住
的桌面图标选择安全点。

### Windows 单只食物机制

- 仅当鼠标已经位于所选 Windows 工作区内时，`Ctrl+Alt+F` 才接受请求；
  接受后以鼠标位置为请求点，并限制在工作区安全边距内。
- 如果请求点与图标冲突，程序先在附近按环形候选搜索，再扫描工作区，选择
  尽量靠近鼠标的安全位置；没有安全位置时忽略此次请求。
- 食物使用独立的 84×84 鼠标穿透分层窗口。它位于普通桌面和应用内容之上，
  并在每帧放到蟑螂窗口正下方。
- 再次按快捷键可移动现有食物。蟑螂会持续追踪新位置；进食时移动食物、
  鼠标威胁或图标覆盖都会让它重新觅食或先逃跑。
- 一次完整进食只消费一次食物，随后食物窗口自动消失。

### 分辨率、生成位置和多显示器

Windows 在启动时请求 Per-Monitor DPI Awareness v2，并按所选显示器的完整
像素分辨率计算默认身体长度：

```text
round(165 × clamp(min(width / 1920, height / 1080), 0.60, 2.00))
```

| 分辨率 | 默认身体长度 |
| --- | ---: |
| 1280×720 | 110 px |
| 1920×1080 | 165 px |
| 2560×1440 | 220 px |
| 3440×1440 | 220 px |
| 3840×2160（等比 2×） | 330 px |

`--size` 会关闭 Windows 自动缩放。Linux 默认身体长度固定为 165 px。
像素分辨率、Windows 工作区/任务栏位置和显示器拓扑只在启动时读取，运行
期间发生变化后需要重启程序；物理 DPI 不参与尺寸公式。只有宽、高两个方向
的缩放比都达到 2 倍时才会触及 330 px 上限；超宽屏仍以宽、高比例中较小的
一项计算。

`--display N` 选择 SDL 当前枚举的一个显示器，每个进程都把身体运动边界
限制在该显示器或工作区。单只模式在有效区域的安全边距内随机出生；多只模式
根据显示器宽高建立网格，打乱单元格并加入随机偏移，再把每只尺寸设为基准的
`0.52..1.02` 倍，减少初始扎堆。

### 覆盖窗口、输入和资源

- Windows 使用逐像素 alpha 的 `UpdateLayeredWindow`，窗口带有 topmost、
  tool-window、no-activate 和默认 click-through 属性；每只蟑螂使用独立
  小窗口，启动时不抢焦点。
- Linux 优先使用 SDL X11 后端，选择 32-bit ARGB visual，通过 XFixes 空输入
  区域实现鼠标穿透。单只小窗口还会用 XShape/XFixes 按 alpha 更新可见
  边界；Linux 多只模式共用不设置 bounding shape 的全屏透明画布。
- `--no-click-through` 仅用于调试。Linux 多只模式下它会使整块共享画布接收
  输入，不适合作为日常运行方式。
- 默认素材依次从可执行文件旁的 `assets`、安装前缀下的
  `share/cockroach-overlay/assets`、当前工作目录和源码回退路径查找；
  `--asset` 可指定其他 atlas，但尺寸必须严格为 `1536×1024`。
- 正常退出会释放覆盖窗口、纹理、Explorer / AT-SPI 资源和 SDL。初始化、
  窗口或素材加载失败时会显示错误并返回非零状态；参数错误会写入标准错误并
  返回状态码 2。运行期呈现或窗口层级更新失败会停止主循环，当前仍走正常
  退出码路径。

## 快速开始

### 下载 Windows CI 构建

每次推送到 `main`、推送 `v*` 标签、提交 Pull Request 或手动运行工作流后，
GitHub Actions 都会交叉编译并生成 `cockroach-overlay-windows-x64.zip`。
在 [Windows x64 package](https://github.com/tiansongyu/awesome_bug/actions/workflows/windows-package.yml)
中打开最近一次成功运行，即可从运行摘要下载。

ZIP 内包含：

```text
windows-x64/
  cockroach_overlay.exe
  cockroach_swarm_20.exe
  SDL2.dll
  README.txt
  assets/cockroach_parts_atlas.png
```

CI 会校验 ZIP CRC、文件内容、两个 EXE 的 PE x64 / Windows GUI 格式和
`.rsrc` 资源节，并在运行摘要中给出 SHA-256。ZIP 以原文件直接上传，不会
被再次套一层 ZIP，默认保留 30 天。CI 只验证交叉编译和包完整性，不执行
Wine、窗口集成或行为测试。

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

需要完整的 Ubuntu 桌面图标交互时，请登录 “Ubuntu on Xorg” 会话并启用
Desktop Icons NG（DING）。如果只需应用而不构建测试，可在配置时添加
`-DBUILD_TESTING=OFF`。

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
sudo apt install binutils-mingw-w64-x86-64 ca-certificates cmake curl \
  g++-mingw-w64-x86-64 libz-mingw-w64-dev make unzip
./scripts/build-windows.sh
```

脚本会校验并使用 SDL2 2.32.10 和 libpng 1.6.58，生成：

```text
dist/windows-x64/
dist/cockroach-overlay-windows-x64.zip
```

Windows 包将 libpng、zlib、libgcc 和 libstdc++ 静态链接。运行时只需所选
EXE、同目录的 `SDL2.dll` 以及包内 `assets` 目录；另一个 EXE 可以不保留。

## 使用

```text
cockroach_overlay [options]
```

| 参数 | 说明 |
| --- | --- |
| `--size N` | 固定身体长度，范围 `100..520`；Windows 下会覆盖自动缩放 |
| `--speed N` | 全局速度倍率，范围 `0.25..3`，默认 `3` |
| `--display N` | 选择显示器，编号从 `0` 开始，默认 `0` |
| `--count N` | 蟑螂数量，范围 `1..50`，默认值由程序目标决定 |
| `--asset PATH` | 使用兼容的 `1536×1024` 组件图 |
| `--no-click-through` | 关闭覆盖窗口的鼠标穿透，仅用于调试 |
| `--frames N` | 渲染 N 帧后正常退出；`0` 表示持续运行 |
| `--help`、`-h` | 显示帮助 |

示例：

```bash
./build/cockroach_overlay
./build/cockroach_overlay --size 220 --speed 1.35
./build/cockroach_overlay --count 8 --display 1
./build/cockroach_swarm_20 --count 1
```

| 快捷键 | 平台和范围 | 功能 |
| --- | --- | --- |
| `Ctrl+Alt+Q` | Windows 和 Linux X11，任意数量 | 全局退出，不要求覆盖窗口获得焦点 |
| `Ctrl+Alt+F` | 仅 Windows 且实际 `count == 1` | 在鼠标附近的安全位置投放或移动食物 |
| `Esc` 或 `Q` | SDL 窗口实际获得键盘焦点时 | 退出；主要用于 Linux/X11 调试 |

两个组合键由程序轮询，不会向系统注册或吞掉按键，前台应用仍可能同时收到。
程序当前没有托盘菜单、自启动或单实例限制。

## 测试

```bash
cmake -S . -B build -DCMAKE_BUILD_TYPE=Release -DBUILD_TESTING=ON
cmake --build build --parallel
ctest --test-dir build --output-on-failure
```

CTest 注册两个无需图形桌面的测试程序：

- `cockroach_motion_test`：覆盖 8 组分辨率缩放、固定随机种子的轨迹和状态
  复现、基础与速度感知威胁、受惊冻结、持续近距威胁的单次触发与冷却、
  角落潜伏和清洁循环、觅食/进食/移动诱饵/威胁打断、180 秒模拟快慢速变化。
- `cockroach_motion_test` 还会注入合成的静态和移动障碍，验证躯干重叠后的
  脱离、连续运动、单帧位移上限、屏幕边缘停留和多方向脱困。
- `cockroach_parts_test`：覆盖 9 个 atlas 区域及旋转轴、交替三足相位、六腿
  独立摆动、双触须非镜像动作，以及潜伏、清洁和进食时的器官动作约束。

这些测试验证运动核心，不会创建真实 Explorer / DING 图标、Win32 / X11
覆盖窗口或全局快捷键。GitHub Actions 的 Windows 工作流当前只构建和检查
发行包；它不会替代本地 CTest 或真实桌面集成测试。

## 项目结构

```text
.github/workflows/
  windows-package.yml         Windows x64 自动构建和 ZIP 产物
assets/
  cockroach_parts_atlas.png   唯一运行时组件图
  cockroach_parts_sheet.png   器官分离和 atlas 重建的输入图
  cockroach_parts/            同时生成的九张独立器官检查素材
src/
  cockroach.cpp               行为状态机、避障、脱困和组合绘制
  cockroach_parts.cpp         精灵区域、六足步态和触须动作
  desktop_icons.cpp           Windows Explorer / Ubuntu DING 图标跟踪
  overlay_window.cpp          Windows / X11 透明置顶窗口
  windows_interaction.cpp     Windows 食物诱饵和投放快捷键
  png_loader.cpp              libpng RGBA 纹理加载
tests/                        运动状态机和器官动画测试
scripts/
  build-windows.sh            Windows x64 交叉编译与打包
  extract-cockroach-parts.py  器官分离和 atlas 重建
packaging/
  cockroach.ico               Windows EXE 多分辨率图标
  cockroach_icon.png          图标源图
docs/screenshots/             README 截图
```

组件图的生成、透明度处理和素材说明见
[assets/README.md](assets/README.md)。脚本以 `cockroach_parts_sheet.png`
为输入，同时输出九张独立器官 PNG 和运行时 atlas；运行时不会逐张读取器官
PNG。重新生成素材需要 Python 3、OpenCV、NumPy 和 Pillow。

## 参与贡献

欢迎提交 Issue 和 Pull Request。修改动作逻辑时，请同时运行
`cockroach_motion_test` 和 `cockroach_parts_test`；修改组件图后，请确保
九个区域互不重叠，且所有可见像素保持完全不透明。

## 许可证

仓库当前尚未包含 `LICENSE` 文件。正式公开分发或接受外部贡献前，建议明确
代码许可证，并单独确认图像素材的授权范围。
