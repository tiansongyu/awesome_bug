# Rust + Lua 桌面虫子运行时设计

状态：已实现
目标平台：Windows 11 x64
运行时：Rust 1.97.1、Lua 5.4、SDL2、Win32

## 1. 设计目标

这套运行时把“虫子想做什么”和“操作系统允许它怎么做”分开：

- Lua 独占物种行为、状态转换、目标、速度节奏和肢体姿态。
- Rust 独占不可信脚本边界、确定性随机数、屏幕几何、硬碰撞、渲染和
  Windows 集成。
- 新物种是一个自包含目录；宿主不包含物种名、状态名或速度表。
- 单只和多只使用同一条代码路径。差别只是实例数量及功能开关。
- 生产仓库不保留项目自有 C++、CMake 或 Linux 桌面应用。

核心原则是单一真相源。行为失败时不会切换到另一套 Rust 行为；该实例会被
隔离并保持安全静止。

## 2. 组件边界

```text
Windows / Explorer / input
          │
          ▼
  bug-windows host
  ├─ display + DPI
  ├─ desktop icon snapshots
  ├─ SDL layered windows
  ├─ PNG atlas renderer
  └─ application loop
          │ strict FrameInput
          ▼
  bug-runtime
  ├─ one sandboxed Lua VM
  ├─ validated behavior/FSM source cache
  ├─ fresh sandbox environment per controller
  ├─ tagged RNG per instance
  ├─ body-only OBB solver
  └─ renderer-neutral rig plan
          │
          ▼
  bugs/<species>/
  ├─ manifest.lua
  ├─ behavior.lua
  └─ atlas.png
```

`bug-runtime` 不依赖 SDL 或 Win32，可以在无窗口环境运行完整单元、锁步和压力
测试。`bug-windows` 是唯一桌面宿主；它的两个可执行入口在非 Windows 目标直接
拒绝编译。

## 3. 仓库结构

```text
crates/
  bug-runtime/          通用契约、Lua、安全限制、运动和 rig
  bug-windows/          SDL/Win32 宿主和两个 Windows 入口
bugs/
  runtime/fsm.lua       所有物种共享的 FSM 实现
  cockroach/            当前蟑螂物种
  template/             最小可复制物种
scripts/
  build-windows-gnu.sh  Ubuntu → Windows GNU 交叉构建
  build-windows-msvc.ps1
  package-windows.ps1
  verify-windows-package.ps1
vm/windows11/           Windows 11 验证工具
```

## 4. Lua 物种契约

### manifest.lua

清单只描述数据：

- `api_version`
- `id`、`name`
- `behavior`
- atlas 文件、尺寸和参考长度
- 身体默认长度、覆盖窗口比例、OBB 半宽/半长
- 部件名、源矩形、枢轴、附着点和层级
- 可选能力，例如食物诱饵

加载器限制路径必须位于物种根目录内，拒绝软链接逃逸、未知字段、数组空洞、
重复部件、越界源矩形、非有限数和不合法标识符。

### behavior.lua

行为模块 ABI v1：

```lua
module.new(config, host) -> controller
controller:step(frame)   -> decision
controller:pose(frame)   -> pose
```

模块源码只从磁盘读取并预校验一次；创建每只虫子时，行为模块和 FSM 都在新的
sandbox environment 中重新求值，再生成独立 controller table 和
`TaggedRng`。因此即使脚本闭包捕获模块局部变量或原始 module table，也不会
跨实例污染。相同物种 id 只有在源码、路径、尺寸、能力和部件映射完全一致时
才可复用 descriptor；冲突会在启动阶段显式失败。

`host` 仅暴露：

- `host.random(tag, low, high)`
- `host.f32(value)`
- `host.clamp(value, low, high)`
- `host.wrap_angle(value)`
- 只读 `host.fsm`

Lua 没有文件、网络、进程、动态模块、调试、字节码导出或内建随机数权限。

### 每帧输入

`frame` 具有固定 schema：

- `dt`、`clock`
- 当前身体位置、朝向、速度和长度
- work area
- 鼠标位置与速度
- 食物位置
- 四个角落传感器
- 障碍物摘要
- 上一帧真实位移、受阻时间和恢复探测
- 单只/扩展行为/食物能力开关

Rust 在进入 Lua 前验证所有数值有限且在边界内。Lua 输出也通过严格手写解析器，
未知字段、缺失字段、非有限数、非法状态名或越界姿态都会隔离该 controller。

## 5. 蟑螂行为

共享 FSM 管理十个状态：

```text
wander  creep  pause  startled  flee
seek-corner  lurk  groom  seek-food  feeding
```

Lua 同时拥有：

- 动态快慢速、停顿和加速度；
- 鼠标距离与接近速度触发的受惊/逃跑；
- 角落选择、潜伏和触须清洁；
- 食物选择、靠近、进食和消费事件；
- 受阻后的恢复持续时间与目标；
- 六足交替三足步态、各腿独立相位；
- 两根触须的独立探测、清洁和进食姿态；
- 身体微摆和横向 scuttle。

Rust 不认识上述状态名。

## 6. 硬运动约束

碰撞体仅来自 manifest 中的身体 OBB。腿、触须、阴影和透明画布不参与碰撞。

每帧顺序：

1. Lua 根据传感器返回运动意图。
2. Rust 对旋转分段，防止身体旋入静态图标。
3. 连续扫掠求静态碰撞的最早时刻，防止高速穿透。
4. work area 以当前朝向的 OBB 外接范围进行硬约束。
5. 已存在的重叠按每帧有限预算分离。
6. Rust 把真实位移、受阻时间和 24 向空隙探测反馈给 Lua。
7. Lua 使用真实结果生成姿态。

如果 Explorer 在身体下方发布新图标，窗口只在身体仍重叠期间隐藏；求解器最多
逐帧移动有限距离，不会跳到屏幕另一侧。对称夹缝使用路径级 escape commitment：
允许在原重叠集合内短暂加深，但每步有界，并且连续扫掠保证不进入原本无关的
静态图标。

## 7. Explorer 图标

桌面在前台时，宿主通过 `SysListView32` 读取图标与文字的实际矩形。快照具有：

- 最多 2048 项；
- 120 ms 刷新间隔；
- 100 ms 有界消息等待；
- Explorer 重启自动重连；
- 失败时保留上一份完整快照；
- 鼠标拖拽推断，将旧矩形原子替换为移动矩形。

`LVM_GETITEMRECT` 需要 Explorer 进程中的远程 `RECT`。只有携带这个远程指针的
有界消息超时，才视为 WndProc 可能仍在 Explorer 内执行：旧的 16 字节缓冲不再
复用或释放，由 Explorer 退出时交给 Windows 回收；随后按 1、2、4、8、16、30
秒（封顶）退避，并用全新的缓冲重连。计数、跨进程读写或坐标转换失败则安全释放
并使用 500 ms 连接退避重试；普通快照仍以 120 ms 为刷新周期。这样同时避免
超时后的 use-after-free、跨进程数据竞争、60 Hz 重连风暴和一次故障造成的永久
图标缓存陈旧。

## 8. Windows 窗口与渲染

每只虫子拥有一个小型 SDL software surface 和 Win32 layered window：

- `WS_EX_LAYERED | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE`
- 默认增加 `WS_EX_TRANSPARENT`
- `UpdateLayeredWindow` 提交预乘 ARGB
- 始终置顶但不抢焦点
- 每个 renderer 只使用自己的 atlas texture

PNG 先由 Rust 解码为受尺寸/内存限制的 RGBA8，再上传到每个 renderer。贴图颜色
调制保持不透明身体，透明度只用于素材边缘和阴影。

食物使用独立 layered window。每次提交后把它放在第一只虫子窗口正下方，因此
它位于普通桌面/应用窗口之上、虫子之下。

Per-Monitor V2 在 SDL video 初始化前启用。显示拓扑或 DPI 改变时只重建 native
窗口和 renderer；Lua controller、FSM 状态及 RNG 流保持不变。

## 9. 实例、随机数和复现

一个世界拥有一个 Lua VM 和一份已验证的行为 descriptor。每个实例拥有：

- controller registry key
- 独立求值的 behavior/FSM closure
- tagged MT19937 stream
- `MotionSolver`
- `RigPlanner`
- 独立体型和速度 scale

主种子通过 SplitMix64 派生 spawn 流和实例流。每次随机调用必须携带稳定 tag。
`--seed`、`--frames` 和 `--trace` 可以复现并检查同一运行。

20 只模式先按显示器比例划分格子，再做确定性抖动和图标安全重定位；实例之间
不会共享计时器、姿态或随机调用顺序。

## 10. 资源、错误和发布

资源只相对可执行文件目录发现，启动工作目录不会影响加载。启动在显示第一个
窗口前完成 manifest、behavior 和 atlas 验证。

脚本运行错误只隔离对应实例，保留最后有效姿态并输出安全停止意图。诊断写入：

```text
%LOCALAPPDATA%\ScriptableBugOverlay\logs\latest.log
```

正式 ZIP 包含：

```text
windows-x64/
  cockroach_overlay.exe
  cockroach_swarm_20.exe
  SDL2.dll
  bugs/runtime/
  bugs/cockroach/
  bugs/template/
  README.txt
  LICENSE
  ASSET-NOTICE.md
  THIRD_PARTY_LICENSES.txt
  SHA256SUMS.txt
```

CI 在 Ubuntu 运行无窗口 Rust 检查，在 Windows 使用 MSVC 产出并验证 x64 GUI
ZIP。GNU 交叉脚本提供本地第二条构建路径。SDL 下载固定版本和 SHA-256。

## 11. 验证门槛

- Rustfmt 和 Clippy `-D warnings`
- contract/species/Lua sandbox/RNG/motion/rig 单元测试
- 2,400 帧 C++ 迁移 oracle 锁步测试
- 100,000 帧集成压力测试
- 对称图标夹缝、连续碰撞、拖拽分离和无跨屏传送回归
- 单只 360 帧、20 只 × 120 帧 Windows 11 VM trace
- VM 中验证实际 Explorer、layered alpha、图标避让、点击穿透、食物层级和热键
- ZIP 路径、逐文件 SHA、唯一 SDL DLL、PE32+ x64、GUI subsystem、icon/manifest/
  version resource 检查

锁步 fixture 保留 C++ 来源说明只是迁移证据，不构成可构建的旧实现。

## 12. 添加新物种

1. 复制 `bugs/template`。
2. 使用新的稳定 `id`。
3. 替换 atlas 并填写精确尺寸、部件、枢轴和附着点。
4. 在 `behavior.lua` 返回 ABI v1 controller。
5. 为新状态、随机 tag、姿态和长时间运行增加测试。
6. 使用 `--species <id>` 或 `--species-path <dir>` 启动。

不需要修改 Rust 主循环、派生 native class 或重新设计窗口层。
