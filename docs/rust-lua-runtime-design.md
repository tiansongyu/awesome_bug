# Rust + Lua Windows 虫子运行时设计

状态：最终目标架构，2026-07-31。

本文取代 `lua-bug-runtime-design.md` 中“长期使用 C++ 作为宿主”的决定。原文
关于 Lua 行为契约、物种包、碰撞边界和验证门禁仍然有效，但最终交付物只保留
Rust、Lua 和资源文件；C++ 只在迁移期间充当不可发布的行为 oracle，达到删除
门槛后全部移除。

## 1. 当前仓库审计

当前代码证明了需要保留的行为，也暴露了重构边界：

- `src/main.cpp` 仍直接创建 `Cockroach`，单只/20 只、出生位置、食物和窗口
  生命周期还没有接入通用运行时。
- `src/cockroach.cpp` 和 `src/cockroach_parts.cpp` 是现有行为 oracle。
- `src/desktop_icons.cpp` 已实现 Windows Explorer `SysListView32` 图标矩形、
  120 ms 刷新、拖拽推断和 Explorer 短暂无响应时的缓存保留；同文件仍含 Linux
  X11/AT-SPI 实现。
- `src/overlay_window.cpp` 已验证 SDL 软件渲染到 ARGB surface，再通过
  `UpdateLayeredWindow` 提交透明置顶窗口的路径；同文件仍含 X11 实现。
- `bugs/cockroach/behavior.lua` 已包含十状态行为、目标、动态速度、转向、六足、
  双触须和身体姿态；`bugs/runtime/fsm.lua` 是小型、事件驱动且显式拒绝非法
  转换的 FSM。
- `src/runtime/` 是正在形成的 C++ 通用契约，可继续用于 lockstep，但不是最终
  架构的一部分。
- 当前 CI 从 Ubuntu 使用 MinGW/CMake 生成两个 Windows EXE 和 ZIP；现有
  `cockroach-win11` 虚拟机是最终交互验证环境。

因此不做逐文件的 Rust 语法翻译。先冻结现有输出，再以 Lua 文件为稳定边界
重建宿主，最后删除 oracle。

## 2. 目标与非目标

最终必须满足：

1. Lua 独占所有物种行为：状态机、目标选择、快慢变化、转向意图、受惊、潜伏、
   清洁、觅食、六足、触须和身体微动作。
2. Rust 只拥有机制：Lua 沙箱、强类型契约、工作区和躯干硬碰撞、SDL 渲染、
   Win32 桌面能力、资源和进程生命周期。
3. 新虫子是一个自包含 `bugs/<id>/` 目录，不需要 Rust 子类、枚举分支或修改
   主循环。
4. 单只和 20 只版本、图标避障、拖拽脱离、边缘脱困、动态尺寸、食物层级、
   快捷键、透明点击穿透和随机运动规律不退化。
5. 发布程序仅支持 Windows x64。Linux 只运行无窗口核心测试，不能产出桌面
   应用。
6. 最终仓库不长期保留 C++、CMake、X11、AT-SPI、libpng 或手写 Lua C API
   宿主。

本轮不做：

- ECS、行为树框架、插件 DLL、脚本热重载或网络服务；
- async runtime、后台渲染线程或跨线程 Lua；
- 把 Win32、SDL 指针或 Explorer 原始图标数组暴露给 Lua；
- 多物种同时运行；框架支持它，但交付只迁移并验证蟑螂；
- Linux 桌面宠物。

## 3. 依赖选择

工具链固定为 Rust 1.97.1，提交 `rust-toolchain.toml` 和 `Cargo.lock`，所有 CI
使用 `--locked`。1.97.1 是 2026-07-31 的稳定修复版本。

核心依赖保持小而明确：

```toml
[dependencies]
mlua = { version = "0.12.0", default-features = false,
         features = ["lua54", "vendored"] }

[target.'cfg(windows)'.dependencies]
sdl2 = { version = "0.38.0", default-features = false,
         features = ["raw-window-handle"] }
windows = { version = "0.62.2", features = [
    "Win32_Foundation",
    "Win32_Graphics_Gdi",
    "Win32_System_Diagnostics_Debug",
    "Win32_System_Memory",
    "Win32_System_Threading",
    "Win32_UI_Controls",
    "Win32_UI_HiDpi",
    "Win32_UI_Input_KeyboardAndMouse",
    "Win32_UI_WindowsAndMessaging",
] }
png = { version = "0.18.1", default-features = false }

[target.'cfg(windows)'.build-dependencies]
embed-resource = "3.0.11"
```

选择理由：

- `mlua 0.12` 提供安全的高层栈管理、registry key、hook 和 VM 内存限制；
  `lua54 + vendored` 静态嵌入 Cargo.lock 固定的官方 Lua 5.4.8，不发布 Lua
  DLL。不开 `async`、`send`、`module`、`macros` 或 `serde`。
- `sdl2 0.38` 是稳定的 SDL2 封装。保留上游 SDL 2.32.10 动态库和当前 ZIP
  中的 `SDL2.dll`，不启用 `bundled`、`static-link`、`image` 或
  `unsafe_textures`。GNU 和 MSVC 使用各自的 import library，但发布同一架构
  对应的上游 DLL。
- `windows 0.62` 只打开实际调用的 Win32 feature。所有 unsafe 都收敛在
  `platform/windows/`，业务代码看不到裸句柄。
- `png 0.18` 只解码 PNG，取代 libpng/zlib 和它们的交叉编译脚本；不用体积
  更大的通用 `image` crate。
- `embed-resource` 只在构建时为 GNU/MSVC 选择 `windres`/Windows resource
  compiler，保留现有蟑螂图标和 GUI subsystem。

不引入 `rand`、`serde`、`clap`、`anyhow`、`tokio`、`tracing` 或第三方 FSM。
对应功能很窄，标准库和少量项目内代码更容易审计，也不会把依赖的默认行为变成
运行时契约。

权威来源：

- Rust 1.97.1：<https://blog.rust-lang.org/2026/07/16/Rust-1.97.1/>
- `mlua`：<https://github.com/mlua-rs/mlua>
- `mlua::Lua` 限制 API：<https://docs.rs/mlua/0.12.0/mlua/struct.Lua.html>
- `sdl2`：<https://github.com/Rust-SDL2/rust-sdl2>
- `TextureCreator` 生命周期：
  <https://docs.rs/sdl2/0.38.0/sdl2/render/struct.TextureCreator.html>
- Microsoft `windows` crate：<https://github.com/microsoft/windows-rs>
- `UpdateLayeredWindow` 绑定：
  <https://microsoft.github.io/windows-docs-rs/doc/windows/Win32/UI/WindowsAndMessaging/fn.UpdateLayeredWindow.html>
- `png`：<https://docs.rs/png/0.18.1/png/>
- Cargo.lock 的职责：
  <https://doc.rust-lang.org/cargo/guide/cargo-toml-vs-cargo-lock.html>

## 4. 总体结构

```text
SDL/Win32 输入、Explorer 图标、食物快捷键
                        │
                        ▼
                   FrameInput
                        │
                        ▼
            LuaController::step()
   FSM、目标、速度/转向意图、行为事件（Lua）
                        │
                        ▼
            MotionSolver::integrate()
  工作区、旋转躯干、连续碰撞、有限重叠分离（Rust）
                        │
                        ▼
            LuaController::pose()
      身体、任意器官、六足和触须姿态（Lua）
                        │
                        ▼
               RigPlan / DrawCommand
                        │
                        ▼
        SDL SurfaceCanvas → Win32 layered window
```

每帧只有这一条控制流，不允许第二个隐藏状态机：

1. Win32 宿主采集工作区、鼠标、食物和图标。
2. `MotionSolver` 从原始障碍计算只读摘要。
3. Lua `step` 返回行为状态、目标、运动意图和事件。
4. Rust 校验意图，再执行不可绕过的硬几何约束。
5. 把实际 body 和反馈交给 Lua `pose`。
6. Rust 校验姿态、生成 draw commands 并渲染。
7. 宿主消费一次性事件，例如清除食物。

Lua 决定“想怎么动”；Rust 只决定“这个位移在物理和平台边界内是否允许”。
`MotionSolver` 不认识 `wander`、`flee`、`groom` 或任何蟑螂状态名，也不擅自
设置目标、速度和转向。

## 5. Workspace 和目录

```text
Cargo.toml
Cargo.lock
rust-toolchain.toml

crates/
  bug-runtime/
    Cargo.toml
    src/
      lib.rs
      contract.rs
      species.rs
      rng.rs
      motion.rs
      rig.rs
      world.rs
      lua/
        mod.rs
        sandbox.rs
        value.rs
        module.rs
        controller.rs
        budget.rs
    tests/
      contract.rs
      lua_failures.rs
      lockstep.rs
      stress.rs

  bug-windows/
    Cargo.toml
    build.rs
    resources/app.rc
    src/
      lib.rs
      app.rs
      cli.rs
      resource.rs
      render.rs
      platform/
        mod.rs
        dpi.rs
        layered_window.rs
        desktop_icons.rs
        interaction.rs
      bin/
        cockroach_overlay.rs
        cockroach_swarm_20.rs

bugs/
  runtime/fsm.lua
  cockroach/
    manifest.lua
    behavior.lua
    cockroach_parts_atlas.png
  template/
    README.md
    manifest.lua
    behavior.lua
    atlas.png

tests/
  golden/rust-v2/
  fixtures/bugs/
scripts/
  build-windows-gnu.sh
  build-windows-msvc.ps1
  package-windows.ps1
vm/windows11/
```

`bug-runtime` 不能依赖 SDL 或 Win32，Linux 只构建和测试它。`bug-windows` 的
两个 bin 只选择不同默认数量，共用同一个 `run(DefaultMode)`，不复制业务代码。
在非 Windows 目标直接 `compile_error!`；Linux CI 不尝试构建该 crate。

只建立这两个 crate。模块先用普通 struct 和函数；没有 `Bug` trait hierarchy、
service locator、事件总线或 ECS。将来确有第二种平台实现时，再从已有两份代码
中抽象，而不是预先设计空接口。

## 6. 所有权边界

| 所有者 | 独占资源 |
| --- | --- |
| `App` | SDL context、Windows platform、`RuntimeWorld`、所有 overlay |
| `RuntimeWorld` | 所有 `BugInstance`，并保证它们先于 `LuaHost` 销毁 |
| `LuaHost` | 单个 `mlua::Lua`、FSM registry key、每物种 module key |
| `BugInstance` | controller key、实例 RNG、`MotionSolver`、最后有效 decision/pose |
| `Species` | 已验证 manifest、canonical root、part 索引和资源元数据 |
| `Overlay` | SDL Window、software canvas、HWND、DIB/DC RAII 包装 |
| `RenderSession` | 每个 renderer 对应的 `TextureCreator` 和 atlas texture |
| `DesktopIconTracker` | Explorer process handle、远端 RECT、最后有效障碍缓存 |

约束：

- SDL、Lua 和 Win32 全部在主线程运行；不启用 `mlua/send`。
- `RuntimeWorld::drop` 先清空实例和 module registry keys，再销毁 `Lua`。
- Rust 回调捕获 `Weak<RefCell<TaggedRng>>`，被销毁 controller 遗留的 Lua
  host table 不能解引用旧 RNG。
- 裸 `HWND`、`HDC`、`HBITMAP`、`HANDLE` 只存在于小型 RAII newtype；每个
  unsafe block 都写明前置条件，并启用 `#![deny(unsafe_op_in_unsafe_fn)]`。
- 应用路径不使用 `unwrap`/`expect`；只有测试或编译期不变量可以使用。

## 7. 单 Lua VM 与实例 controller

进程只有一个 Lua VM：

```text
LuaHost
  ├─ fsm.lua                  加载一次、只读注入
  ├─ cockroach behavior      每物种加载一次
  ├─ controller #0           new(config, host)
  ├─ controller #1           new(config, host)
  └─ ...
```

每个 `behavior.lua` 返回 `{ api_version, new }`。`new` 必须返回具有 `step` 和
`pose` 方法的独立 table；可变状态只能存在于这个 controller 的闭包/table 中。
模块级 table 在校验后只读。20 只共享代码，不共享 controller 状态。

controller 由 `mlua::RegistryKey` 持有。每次调用都通过同一个窄入口：

```rust
fn step(&self, id: InstanceId, frame: &FrameInput) -> Decision;
fn pose(&self, id: InstanceId, frame: &FrameInput) -> Pose;
```

`InstanceId` 只用于诊断，Lua 看不到其他实例。所有调用串行，既不需要锁，也不
需要 async。

## 8. FSM

保留 `bugs/runtime/fsm.lua`，不引入 Rust FSM 或旧 Lua FSM 包：

- 它只有 `create/current/is/can/send`；
- 状态和事件表完全由物种脚本声明；
- 非法状态、未知事件和重入转换立即报错；
- `leave → 切换 state → enter` 顺序固定；
- controller 的 `fsm:current()` 是状态的唯一真相。

宿主从固定的 `EXE_DIR/bugs/runtime/fsm.lua` 加载一次，通过不可修改的
`host.fsm` proxy 注入每个实例。没有 `require` 或 package search path。Rust
只把返回的 `state` 当作长度受限的诊断字符串，不含对应 enum 或状态分支。

## 9. Lua 沙箱与预算

`mlua` 的 Lua 5.4 安全构造能阻止 C module，但完整 `Lua::sandbox` 只适用于
Luau，不能误认为 Lua 5.4 自动沙箱。宿主还必须显式执行：

1. `Lua::new_with` 只开放 base 的安全子集、table、string、math 和 utf8。
2. 从环境删除 `dofile`、`load`、`loadfile`、`require`、`collectgarbage`、
   `pcall`、`xpcall`、`rawset` 和 `setmetatable`。
3. 不开放 `package`、`io`、`os`、`debug`、`coroutine`、FFI 或 DLL 加载。
4. 删除 `math.random` 和 `math.randomseed`；唯一随机源是
   `host.random(tag, low, high)`。
5. chunk 只允许 text mode，拒绝 Lua bytecode。
6. safe globals、FSM 和 host API 使用带受保护 metatable 的只读 proxy。
7. manifest、FSM、behavior 各用独立 environment；脚本的全局写入不会修改
   共享 globals。

调用限制：

- 单 VM 总内存上限 32 MiB，使用 `Lua::set_memory_limit`；
- `manifest` 最大 256 KiB，单个 Lua 文件最大 1 MiB；
- `new`、`step`、`pose` 和加载 chunk 每次最多 100,000 条 VM 指令；
- hook 每 100 条指令检查一次调用局部计数，调用后由 RAII guard 恢复；
- 关闭 coroutine 后，当前线程 hook 覆盖所有可执行脚本；
- Lua table 解析限制深度 32、总 entry 8192、字符串 1 MiB；
- controller quarantine 后立即移除其 registry key，执行两次完整 GC，避免一个
  故障实例长期占住全局预算。

无限循环、内存错误、Rust callback 错误和 Lua traceback 都转换成
`ScriptError`，不能跨 FFI unwind。错误包含 operation、species、instance、
文件路径和 traceback，但不输出用户环境变量或任意文件内容。

## 10. Rust/Lua 数据契约

不启用 serde。manifest 和每帧 ABI 是固定且很小的 table，手工 reader 可以：

- 使用 `raw_get`/sequence 读取，避免触发脚本 metatable；
- 为错误保留精确路径，例如 `pose.parts.left_antenna.rotation`；
- 区分缺字段、nil、错误类型、NaN/Inf、整数溢出和越界值；
- 拒绝未知输出字段和未知 part，尽早发现拼写；
- 对 v1 可选字段在一个地方给出默认值。

核心类型保持简单：

```text
FrameInput
  dt, clock, body, world, cursor, bait, corners[4],
  sensors, feedback, features, request_corner_rest

Decision
  state, target, MotionIntent, consume_bait

MotionIntent
  direction, speed, turn_rate, acceleration, lateral_speed,
  recovery_probe_phase, intentionally_still, stop_immediately,
  cancel_recovery, allow_edge_rest, optional initial_heading

Pose
  body_offset, body_rotation, named part rotations/joint offsets
```

manifest v1、behavior v1、part 上限 64 和现有字段保持不变。启动时完成全部
manifest/atlas/source rectangle/root part/能力校验，进入帧循环后不再按字符串
搜索 part；名称预编译为稳定 index。

### Lua double 到 Rust f32

不能把 vendored Lua 改成 `LUA_FLOAT_TYPE=float`。`mlua`/`mlua-sys` 按官方
Lua ABI 把 `lua_Number` 绑定为 double；私自修改 C 编译宏而不同时生成匹配的
Rust FFI 是不受支持的 ABI，可能造成错误读栈甚至未定义行为。

确定性方案是显式边界，而不是修改 ABI：

1. Lua 保持官方 double。
2. Rust 的 `FrameInput` 数值先存为 f32，再无损提升为 f64 交给 Lua。
3. `host.random` 生成一个 f32 样本，再提升为 f64，录制的是 f32 bits。
4. Lua 输出先按 f64 检查有限、范围和结构，再只做一次 `f64 → f32`；转换后
   再检查有限，并把 `-0.0` 规范为 `0.0`。
5. MotionSolver、碰撞和渲染统一使用 f32，禁止同一字段在多层反复转换。
6. 提供 `host.f32(value)`，其语义是 `f64 → f32 → f64`。蟑螂迁移时，仅在
   原 C++ 持久 float 的更新点（timer、clock、heading accumulator）调用它，
   保持阈值转换帧；新物种不必使用。

状态、事件、RNG tag/次数和值 bits 在同一 tape 下必须精确。跨 GNU/MSVC 的
三角函数末位可能不同，因此位置、角度和 pose 使用文末容差，而不是虚假宣称
所有 libm 结果 bit-identical。

## 11. Tagged RNG

项目内实现固定算法，不依赖 `rand`：

- 一个稳定的 MT19937 engine；
- 一个在规范中写死的 `u32 → [0,1)` 映射和 f32 舍入顺序；
- master seed 通过固定 SplitMix64 派生 spawn RNG 和每实例 RNG；
- `tag` 不改变随机值，只用于检查调用顺序；
- record/replay 保存 `tag`、low/high/value 的 f32 bits；
- replay 必须校验 tag、区间、次数和 tape 完整消费。

默认 seed 来自系统时间、性能计数器和进程 ID；`--seed N` 提供可复现运行。
出生位置和 20 只的尺寸/速度差异使用独立 spawn stream，不能改变 controller
的行为流。`MotionSolver` 不消费随机数：24 方向探测是固定顺序，
`recovery_probe_phase` 和是否采用恢复方向由 Lua 决定。

迁移期先让 Rust replay 现有 C++ tape，证明状态转换和消费顺序；然后冻结
Rust-v2 生成算法的已知向量。以后升级 crate 或编译器不能悄悄改变随机序列。

## 12. 通用运动求解

`MotionSolver` 只持有：

- position、heading、actual speed；
- work area 和物种躯干 collider；
- blocked/edge dwell 和上一帧实际 displacement；
- 当前重叠、最近障碍、固定方向探测结果。

它执行：

1. 限制 `dt ≤ 0.05 s`，按 Lua 的 turn rate/acceleration 接近期望值；
2. 应用 lateral speed；
3. 只用 manifest 的旋转躯干碰撞体，腿、触须、阴影和整张 overlay 不参与
   图标碰撞；
4. 用连续 sweep/受限子步，禁止新进入静态图标；
5. 对正在拖拽且已经覆盖躯干的图标做每帧有上限的最小分离；
6. clamp 到带 10 px 边距的 Windows work area；
7. 卡住时探测 24 个方向，只把候选和反馈交给下一帧 Lua；
8. 永不把虫子 teleport 到屏幕另一侧。

求解器不能因为“卡住”而偷偷设置最小速度或随机恢复计时。只有不可穿透、
work-area clamp 和拖拽重叠分离可以覆盖 Lua 位移，这些是硬安全机制。所有
速度节奏、转身方式和脱困意图都留在 Lua。

原始 `Vec<ScreenObstacle>` 只进入 Rust solver。Lua 每帧只收到：

- overlapping、bait_blocked；
- nearest point/distance/moving；
- avoidance direction；
- static/moving urgency；
- blocked time、edge dwell、actual displacement；
- 24 方向探测归纳出的 recovery direction/clearance。

这样 20 只不会复制整个 Explorer icon table，也不会让脚本绕过碰撞。

## 13. SDL 渲染与纹理生命周期

继续使用已经验证的 Windows 提交路径：

```text
PNG RGBA
  → SDL texture
  → SurfaceCanvas<ARGB8888>
  → top-down BGRA DIB
  → UpdateLayeredWindow(AC_SRC_ALPHA)
```

PNG 在 CPU 端只解码一次，并验证尺寸、输出大小和 atlas source rectangle；
然后为每个 overlay 的 renderer 上传一份 texture。software renderer 先绘制
整套阴影，再按 layer 绘制实体，保持 body 不透明 alpha 255、现有暗度和阴影。

不启用 `unsafe_textures`。`Texture` 必须借用创建它的 `TextureCreator`，实现
采用外层 session：

```text
create overlays/canvases
create one TextureCreator for each canvas
create textures borrowing the stable creator slice
run render loop borrowing canvases mutably and textures immutably
drop textures → creators → overlays → SDL
```

每个 texture 带不可混淆的 renderer index，只能交回原 canvas。`TextureCreator`
本身不借用 canvas，但 texture 仍通过 Rust 生命周期不能超过 creator。不要用
自引用 struct、`Box::leak`、裸 `SDL_Texture*` 或手工 destroy。

SDL Window 只提供 HWND 和事件外壳；software canvas 独立持有像素。每帧提交
前按 pitch 拷贝到 top-down DIB。像素测试验证透明背景、实体 alpha 255 和边缘
预乘 alpha，避免 layered window 黑边。

## 14. Windows 平台实现

### 14.1 Layered overlay

窗口样式保持：

```text
WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE |
WS_EX_TRANSPARENT
```

配合 borderless、hidden、always-on-top 和 skip-taskbar。先完成首帧 DIB，
再 `SW_SHOWNOACTIVATE`，避免启动闪烁。窗口移动只用绝对屏幕坐标，不激活、不
抢焦点。

HWND 优先通过 `sdl2` 的 `raw-window-handle` 0.6 实现取得；只接受 Win32
handle variant。若 SDL 返回其他 backend，启动失败并报告，不回退到不透明
普通窗口。

### 14.2 Explorer 图标

Rust 逐项保持现有 Win32 算法：

- 从 `Progman` 或 `WorkerW/SHELLDLL_DefView` 查找 `SysListView32`；
- 连接 Explorer x64 process；
- 用一个远端 `RECT` 配合 `LVM_GETITEMRECT`，最多读取 2048 个图标；
- `SendMessageTimeoutW` 使用 `SMTO_ABORTIFHUNG | SMTO_BLOCK` 和 100 ms；
- 图标尺寸过滤 `8..360 × 8..300`，保留 9 px 可见间距；
- 每 120 ms 刷新；超时保留最后有效缓存，不能产生一帧无碰撞空洞；
- Explorer 重启或 HWND/PID 变化时释放旧 handle/remote memory 并重连；
- 只在桌面为前台时发布障碍，其他应用前台时保持宠物的随机运动。

所有远端内存和 process handle 使用 RAII。只申请当前功能需要的
`PROCESS_QUERY_LIMITED_INFORMATION | PROCESS_VM_OPERATION |
PROCESS_VM_READ | PROCESS_VM_WRITE`。

### 14.3 图标拖拽

按现有语义推断拖拽：

- 左键按下时命中缓存图标，记录 pointer offset；
- 移动至少 6 px 才进入 dragging；
- 拖拽障碍使用当前 cursor-offset，额外 12 px padding；
- 原位置静态矩形在拖动期间去重；
- 松开后立即请求一次图标刷新；
- 重叠时 solver 每帧只移动有限距离并优先远离图标。

测试断言：不穿过图标、不永久静止、最大单帧 displacement 受限、不从一侧
闪现到另一侧。

### 14.4 DPI、分辨率和多显示器

在 SDL 初始化前调用
`SetProcessDpiAwarenessContext(DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2)`。
之后 SDL、Monitor API、鼠标和 Explorer 矩形全部使用物理屏幕像素，允许
负坐标显示器。

选定 display 后用 `MonitorFromPoint + GetMonitorInfoW.rcWork` 得到工作区。
自动 body length 保留现有公式：

```text
scale = clamp(min(width / 1920, height / 1080), 0.60, 2.0)
body_length = round(165 * scale)
```

`--size` 仍是明确覆盖。收到显示拓扑/DPI 改变后重新读取 work area；自动尺寸
只在安全帧边界更新 overlay 和 collider，手工尺寸不变。验证 100%、150%、
200% 缩放和 1080p/1440p/4K。

### 14.5 多窗口和食物层

- 单只模式：一个 roach overlay；物种支持 bait 时再创建一个 food overlay。
- 20 只模式：20 个独立 overlay/controller/RNG；保持现有不同出生位置、
  `0.52..1.02` 尺寸和 `0.82..1.18` 速度倍率；默认不启用单宠扩展和食物。
- food overlay 也是 click-through/topmost，先提交，再用
  `SetWindowPos(food_hwnd, roach_hwnd, ..., SWP_NOACTIVATE)` 放在蟑螂正下方。
  因此食物在普通桌面和图标之上、蟑螂之下。
- 每帧校验/恢复这两个窗口的相对 Z-order，不改变其他应用焦点。
- `Ctrl+Alt+F` 在鼠标位置投放或移动食物；先在整个 work area 搜索不会被图标
  阻挡的点。`Ctrl+Alt+Q` 退出。

## 15. CLI 和资源发现

保留并扩展窄 CLI：

```text
--species ID
--species-path DIR
--asset PATH
--size N
--speed N
--display N
--count N
--seed N
--no-click-through
--frames N
--trace PATH
--help
```

两个 EXE 的唯一区别是默认 count：`cockroach_overlay.exe = 1`，
`cockroach_swarm_20.exe = 20`；显式 `--count` 仍可覆盖。parser 手写并对重复
参数、缺值、非有限 float 和范围错误给出确定消息。

默认资源根只来自 `current_exe().parent()/bugs`，从任意 cwd 启动都一致：

```text
EXE_DIR/bugs/runtime/fsm.lua
EXE_DIR/bugs/<species>/manifest.lua
```

路径安全规则：

- `--species` 只接受 ASCII `[A-Za-z0-9_-]+`；
- manifest 内文件必须是相对路径，只含 normal components；
- 拒绝 absolute、prefix、root、`.`、`..` 和空文件名；
- canonicalize 物种 root 和目标，解析 symlink 后目标必须仍在 root 内；
- 目标必须是 regular file，读入后不再允许脚本自行打开路径；
- Lua/PNG 读取有文件和解码大小上限；
- `--species-path` 是用户明确选择的完整物种目录，但仍应用内部 containment；
- `--asset` 是唯一允许在物种 root 外的显式覆盖，仍校验尺寸和 PNG 上限。

错误中显示规范化后的相关路径，但不扫描、不执行 cwd 中同名脚本。

## 16. 错误隔离

错误分三类：

1. **启动契约错误**：manifest/behavior/FSM 缺失、版本错误、PNG 不兼容、
   controller `new` 失败。创建任何可见窗口前退出非零，并显示一次 message box。
2. **实例脚本错误**：`step/pose` runtime、instruction、memory、NaN/Inf、未知
   part。只 quarantine 当前实例，返回 speed=0 的安全命令并保持最后有效 pose；
   记录一次错误，其他实例继续。
3. **平台致命错误**：SDL renderer、DIB 或 `UpdateLayeredWindow` 失败。隐藏全部
   overlay、写日志、清理 RAII 资源并退出非零。

quarantine 没有 C++/Rust 行为 fallback。否则同一物种会有两套真相。20 只中
一个脚本实例故障不能使进程 panic；VM 内存已不可恢复时允许隔离全部实例并
干净退出，但不能继续在损坏状态下渲染。

GUI 程序默认写简短日志到 `%LOCALAPPDATA%/AwesomeBug/logs/`；`--trace` 输出
测试需要的逐帧 TSV。日志实现使用标准库，不引入 logging framework。

## 17. 迁移顺序

每阶段独立提交，前一阶段的门禁通过后再进入下一阶段：

1. **冻结 oracle**  
   保留现有 C++ golden、RNG tape、状态/运动/姿态 trace 和 Windows 行为截图。
2. **建立 Rust workspace**  
   加入 contract、manual reader、species loader、Tagged RNG；Linux headless
   test 先通过。
3. **实现 mlua host**  
   单 VM、FSM 注入、32 MiB、100k hook、controller 隔离和失败测试。直接加载
   当前 `bugs/**`。
4. **移植通用 core**  
   MotionSolver、RigPlan 和 BugInstance 对 C++ oracle lockstep；所有蟑螂
   行为分支只在 Lua。
5. **实现 Windows substrate**  
   layered window、SDL safe texture session、Explorer、drag、DPI、食物和
   两个 bin。
6. **切换主程序**  
   GNU/Wine smoke 后，把 GNU 包放入现有 Windows 11 VM，完成第一轮交互测试。
7. **原生 MSVC 验证**  
   在 VM 安装固定 Rust 1.97.1 和 Visual Studio Build Tools，编译/运行同一
   commit，对比 GNU/MSVC trace。
8. **删除 oracle**  
   达到下一节门槛后删除所有 C++/CMake/Linux 应用代码和旧依赖。
9. **收口 CI、ZIP、README**  
   只描述 Rust + Lua 架构，保存最终 VM 日志和截图，再 push。

迁移期间不能为了让新测试通过而降低旧碰撞、状态、姿态或窗口层级门禁。确需
修改 golden 时，提交必须同时说明观察到的数值差异、原因和 VM 证据。

## 18. 删除 C++ 的硬门槛

以下条件全部成立后，才允许删除 oracle；删除后再次完整验证：

- 两个 Cargo 生成的 EXE 在 Windows 11 VM 可运行；
- Lua 十状态、动态速度、腿、触须和食物行为通过 lockstep/性质测试；
- Windows 图标、拖拽、边缘、DPI、Z-order 和 click-through 通过实机矩阵；
- GNU 和 MSVC 都能从 clean checkout 使用 `Cargo.lock` 构建；
- Rust-v2 golden、RNG known vectors 和 100,000 帧压力测试通过；
- ZIP 从任意 cwd 启动并完整包含 `bugs/**`。

最终清理断言：

```text
没有 src/*.cpp、src/*.h、CMakeLists.txt、cmake/、third_party/lua
没有 X11、XFixes、XRender、AT-SPI、libpng、CMake 或 C++ 构建依赖
没有 Cockroach Rust 类型或按蟑螂状态分支的宿主代码
没有 legacy fallback 或仅为 oracle 编译的 target
CI 和 release 只调用 cargo
```

`rg` 搜索和 clean build 是门禁证据，不能仅凭“主程序已经不用它”保留死代码。

## 19. 构建、CI 和 ZIP

### Linux headless

```text
cargo fmt --check
cargo clippy -p bug-runtime --all-targets --locked -- -D warnings
cargo test -p bug-runtime --locked
```

此 job 不安装 SDL/X11/Win32 依赖，也不生成 Linux app。

### Ubuntu → Windows GNU

安装 Rust target、MinGW-w64、Wine 和 SDL2 2.32.10 MinGW development 包：

```text
cargo test -p bug-runtime --target x86_64-pc-windows-gnu --locked
cargo build -p bug-windows --bins --release \
  --target x86_64-pc-windows-gnu --locked
wine cockroach_overlay.exe --help
```

Windows test binaries由 Wine 执行；交互 UI 不由 Wine 证明。GNU 产物用于本机
快速投递 VM，并作为非发布 smoke artifact。

### Windows MSVC

Windows CI 和现有 VM 使用 `x86_64-pc-windows-msvc`。Rustup 官方建议大多数
Windows 互操作优先 MSVC；MSVC/GNU ABI 的区别见：
<https://rust-lang.github.io/rustup/installation/windows.html>。

MSVC job 执行 core tests、release build、PE/resource 检查和 packaging。它生成
唯一正式产物：

```text
cockroach-overlay-windows-x64.zip
└─ windows-x64/
   ├─ cockroach_overlay.exe
   ├─ cockroach_swarm_20.exe
   ├─ SDL2.dll
   ├─ bugs/runtime/fsm.lua
   ├─ bugs/cockroach/manifest.lua
   ├─ bugs/cockroach/behavior.lua
   ├─ bugs/cockroach/cockroach_parts_atlas.png
   ├─ bugs/template/...
   ├─ README.txt
   └─ THIRD_PARTY_LICENSES.txt
```

ZIP 检查逐项比较源文件、PE x64/GUI subsystem、icon/resource、无 Lua/libpng
DLL、从临时 cwd 启动 `--frames`，并发布 SHA-256。tag release 只上传已通过
同一 workflow 的 MSVC ZIP，不重建不同源码。

## 20. Windows 11 VM 实验

沿用 `cockroach-win11`（8 vCPU、12 GiB、Windows 11 25H2）：

1. 把当前 commit、GNU ZIP、测试脚本和预期 hash 写入新的 test-tools ISO。
2. 先运行 GNU build，证明 Ubuntu 交叉产物在真实 Explorer 上工作。
3. 在 VM 的 Developer PowerShell 中使用 Rust 1.97.1 MSVC 和 VS Build Tools
   从同一 commit clean build。
4. 运行 headless trace，比较 GNU/MSVC 的状态、事件、RNG bits 和数值容差。
5. 依次执行下表人工场景；每个场景保存日志、截图或短录屏。
6. 使用 Process Explorer/PowerShell 检查窗口 Z-order、进程退出码、模块和
   文件 hash。

Wine 只能作为启动 smoke，不替代 Explorer、拖拽、DPI 和 layered window 的
VM 证据。

## 21. 功能验证矩阵

| 场景 | 自动断言 | Windows 11 证据 |
| --- | --- | --- |
| Lua 十状态 FSM | 转换帧、非法转换、RNG 次数 | 状态日志 |
| 随机快/慢/疾走 | 长期速度分布和脉冲性质 | 录屏 |
| 六足和双触须 | 九 part pose golden | 连续截图/录屏 |
| 单只默认程序 | 一个 controller/window | 桌面截图 |
| 20 只程序 | 20 个独立 controller/RNG，无共享状态 | 全屏截图、实例日志 |
| 静态图标 | 10,000 帧躯干零穿透 | 绕行录屏 |
| 腿/触须越过图标 | collider 只含 body | 近距离截图 |
| 拖拽覆盖 | 有限分离、持续进展 | 拖拽录屏 |
| 屏幕边缘夹困 | 最大 displacement、无 teleport | 边角录屏 |
| Explorer 重启/卡顿 | 缓存保留、重连 | 重启 Explorer 日志 |
| 受惊逃跑 | cursor 速度阈值、状态转换 | 快速鼠标录屏 |
| 角落潜伏/清洁 | 状态和 pose golden | 角落录屏 |
| 食物投放/移动/消费 | 事件一次、blocked 搜索 | Z-order 截图 |
| food/roach 层级 | HWND 相对顺序 | 桌面截图 |
| click-through/no-activate | focus HWND 不变 | PowerShell/录屏 |
| 1080p/1440p/4K | body 公式、work area | 尺寸日志/截图 |
| 100/150/200% DPI | 坐标同一空间、无漂移 | 设置页和桌面截图 |
| 多显示器/负坐标 | bounds/clamp 单元测试 | 移动显示器截图 |
| 任意 cwd 启动 | resource root 为 EXE_DIR | 临时目录运行日志 |
| 路径穿越/symlink | 启动拒绝 | 错误对话框 |
| Lua 语法/runtime 错误 | 明确分类、单实例隔离 | 20 只剩余实例运行 |
| 无限循环 | ≤100k hook 终止 | 错误日志 |
| 内存超限 | ≤32 MiB、key 释放、GC | 进程继续/干净退出 |
| NaN/Inf/超限 motion | contract 拒绝 | 无窗口瞬移 |
| 100,000 帧压力 | registry、VM 内存无持续增长 | headless 日志 |
| GNU/MSVC 一致性 | state/event/RNG exact，数值容差 | 两份 trace hash/报告 |
| ZIP 完整性 | 文件、PE、icon、SHA-256 | 解压后启动 |

数值门禁：

- position/target：`≤ 0.01 px`；
- speed：`≤ 0.01 px/s`；
- heading/part rotation：`≤ 1e-5 rad`；
- joint offset：`≤ 0.01 px`；
- 最大单帧拖拽分离和 work-area clamp 使用当前 C++ golden 的上限；
- 状态、事件、RNG tag/count/value bits 不使用容差。

## 22. 明确拒绝的实现

- **Rust 包一层旧 `Cockroach` C++ FFI**：会永久保留两种宿主和 C++ 行为真相。
- **Lua 只返回状态名、Rust 保存速度表**：行为仍分裂在两处。
- **每只虫子一个 Lua VM**：浪费内存，破坏 module 共享和整体预算。
- **把所有图标复制给 Lua**：扩大 ABI、分配和绕过硬碰撞的机会。
- **使用 Lua float 自定义 ABI**：`mlua` 不支持，存在 FFI 不匹配风险。
- **启用 `unsafe_textures`**：安全 lifetime 已能用 session 表达，没有理由拿掉。
- **用 serde 直接吞整个 table**：错误路径、有限数和未知字段控制不够直接。
- **加入 ECS、DI 容器、动态 trait 图或 async**：当前单线程顺序循环不需要。
- **保留 Linux 空壳或 C++“以防万一”**：与 Windows-only 和最终 Rust 目标冲突。
- **用 Wine 代替 Windows VM**：Wine 没有可作为门禁的真实 Explorer/DPI/Z-order。

最终代码的衡量标准不是抽象数量，而是：一个明显的帧循环、一个 Lua VM、
一个通用 solver、一处 Windows unsafe 边界，以及删除任何重复真相的勇气。
