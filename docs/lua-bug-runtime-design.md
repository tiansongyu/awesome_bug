# Windows Lua 虫子运行时设计

状态：已批准，作为本轮重构的实现契约。

## 目标

本轮把项目收敛为 Windows 桌面宠物运行时，并满足以下约束：

1. 蟑螂的状态机、目标选择、速度/转向策略、威胁响应、潜伏、清洁、觅食、
   身体微动作、六足和触须姿态全部由 Lua 控制。
2. C++ 不再包含蟑螂行为分支；它只提供 Windows 平台能力、脚本宿主、
   通用运动约束、资源加载和渲染。
3. 新虫子通过一个自包含目录加入，不需要派生 C++ 类或修改主循环。
4. 现有单只、20 只、图标避障、拖拽脱离、分辨率缩放、食物和快捷键行为
   保持不变。
5. 删除可运行的 Linux 版本、X11、XFixes 和 AT-SPI 代码及依赖。非 Windows
   主机只允许构建无窗口的核心测试，不能生成 Linux 桌面应用。

## 设计原则

- **一个控制流**：每只虫子每帧只经历“采集输入 → Lua 决策 → 通用约束 →
  Lua 姿态 → 渲染”，不建立第二套隐藏状态机。
- **策略与机制分离**：Lua 决定想去哪里、如何移动和如何摆动；C++ 保证不会
  穿过图标、越出工作区或用非法数值破坏窗口。
- **数据优先于继承**：没有 `Cockroach : Bug`、行为组件树或事件总线。物种
  是一份经过验证的数据，行为扩展点是 Lua 文件。
- **窄 ABI**：Lua 只能读取一份帧输入并返回运动命令、姿态和事件。Win32、
  SDL 指针和文件句柄永远不会暴露给脚本。
- **失败显式**：缺文件、语法错误、契约版本错误在启动时失败；运行时错误只
  隔离故障实例并使其安全停下，不能跨过 C ABI 崩溃。
- **确定性可测试**：脚本不能调用 `math.random`。随机数由宿主注入，测试可
  录制并重放同一 RNG tape。

## 选择 Lua 5.4.8

运行时静态嵌入官方 Lua 5.4.8 源码，固定 SHA-256：

```text
4f18ddae154e793e46eeab727c59ef1c0c0c2b744e7b94219710d76f530629ae
```

Lua 5.5.0 是当前最新主版本，但 5.5.1 仍处于候选发布阶段。本项目只需要成熟
的 C API、表和数学库，因此选择最后一个 5.4 修复版本，换取稳定 ABI 和更广
的工具链验证范围。宿主直接使用 Lua C API，不引入 sol2 等额外绑定层。

官方来源：

- <https://www.lua.org/ftp/lua-5.4.8.tar.gz>
- <https://www.lua.org/manual/5.4/>
- <https://www.lua.org/license.html>

## 总体架构

```text
Win32 / SDL 输入、Explorer 图标、食物快捷键
                       │
                       ▼
                 BugFrameInput
                       │
                       ▼
              LuaController::step
       状态机、目标、速度、转向、行为事件
                       │
                       ▼
                  MotionIntent
                       │
                       ▼
          MotionSolver（物种无关的硬约束）
     工作区、躯干碰撞、连续移动、重叠分离、脱困
                       │
                       ▼
               LuaController::pose
       身体 bob/sway/rock、任意数量器官的姿态
                       │
                       ▼
         SpriteRig + Windows OverlayWindow
```

一个进程只有一个 `lua_State`，物种模块只加载一次。每只虫子由 Lua 工厂创建
独立 controller table，并由 C++ registry reference 持有。20 只模式不会
创建 20 个虚拟机，也不会共享可变的实例状态。

## 所有权边界

| 责任 | Lua | C++ |
| --- | --- | --- |
| 行为状态、转换顺序和计时 | 是 | 否 |
| 随机目标、角落、食物、威胁优先级 | 是 | 只提供输入 |
| 期望方向、速度脉冲、转向速率、加速度、横向疾走 | 是 | 校验并积分 |
| 六足、触须、清洁、进食及身体视觉姿态 | 是 | 只应用变换 |
| 工作区和旋转躯干碰撞 | 配置参数 | 强制约束 |
| 连续子步、重叠分离、最大单帧位移和无进展反馈 | 调整参数、读取反馈 | 通用求解 |
| Explorer 图标、拖拽、鼠标、快捷键和窗口层级 | 只读帧输入 | Win32 |
| atlas、纹理、透明窗口和绘制 | 声明数据 | SDL / Win32 |
| 随机数 | 请求带标签的样本 | 生成、录制、重放 |

`MotionSolver` 不允许包含 `wander`、`flee`、`groom` 等状态名，也不能选择行为
目标。它只执行物种无关的几何约束，因此不会形成第二套蟑螂逻辑。

## 目录和物种包

```text
bugs/
  cockroach/
    manifest.lua
    behavior.lua
    cockroach_parts_atlas.png
  template/
    README.md
    manifest.lua.example
    behavior.lua.example
src/
  bug.h / bug.cpp
  bug_species.h / bug_species.cpp
  lua_runtime.h / lua_runtime.cpp
  motion_solver.h / motion_solver.cpp
  sprite_rig.h / sprite_rig.cpp
  desktop_icons.cpp
  overlay_window.cpp
  windows_interaction.cpp
  main.cpp
tests/
  fixtures/bugs/minimal/
  golden/legacy-v1/
```

默认 `--species cockroach` 从可执行文件旁的 `bugs/cockroach` 加载。开发者可用
`--species-path PATH` 加载外部完整物种目录；原有 `--asset PATH` 继续作为
atlas 覆盖参数。所有相对路径都必须在物种根目录内，拒绝 `..` 穿越。

## `manifest.lua` 契约

manifest 只返回数据，不允许定义每帧函数：

```lua
return {
    api_version = 1,
    id = "cockroach",
    name = "Cockroach",
    behavior = "behavior.lua",
    atlas = {
        file = "cockroach_parts_atlas.png",
        width = 1536,
        height = 1024,
        reference_length = 799,
    },
    body = {
        default_length = 165,
        overlay_scale = 2.15,
        collider_half_width = 0.20,
        collider_half_length = 0.43,
        root_part = "body",
    },
    capabilities = {
        bait = true,
    },
    parts = {
        {
            name = "body",
            source = { 0, 0, 283, 799 },
            pivot = { 141.5, 399.5 },
            attachment = { 0.0, 0.0 },
            layer = 100,
        },
    },
}
```

宿主在创建窗口前验证：

- `api_version` 精确匹配；
- id、文件名和 part name 合法且唯一；
- atlas 尺寸、source rectangle、pivot、attachment 和 layer 有限；
- 恰好一个 root part，所有 source rectangle 位于 atlas 内；
- 身体尺寸、overlay 比例和碰撞体为有限正数；
- part 数量限制为 `1..64`。

## `behavior.lua` 契约

模块返回版本和工厂：

```lua
return {
    api_version = 1,
    new = function(config, host)
        local self = {}

        function self:step(frame)
            return {
                state = "wander",
                target = { x = frame.body.x, y = frame.body.y },
                motion = {
                    direction = { x = 0.0, y = -1.0 },
                    speed = 180.0,
                    turn_rate = 4.5,
                    acceleration = 680.0,
                    lateral_speed = 0.0,
                    intentionally_still = false,
                    allow_edge_rest = false,
                },
                events = {
                    consume_bait = false,
                },
            }
        end

        function self:pose(frame)
            return {
                body = { x = 0.0, y = 0.0, rotation = 0.0 },
                parts = {},
            }
        end

        return self
    end,
}
```

### 帧输入

`frame` 包含：

- `dt` 和单调行为时钟；
- `body`：位置、heading、实际 speed、body length；
- `world`：Windows 工作区；
- `cursor`：valid、位置、速度；
- `bait`：active、位置；
- `corners[4]`：宿主按物种碰撞体计算的角落位置、距离和 blocked 标志；
- `sensors`：当前躯干重叠、诱饵是否被挡和最近障碍摘要；
- `feedback`：实际位移、是否重叠、阻塞时间、边缘停留、恢复方向与剩余时间；
- `features`：是否为单只模式及宿主启用的能力；
- 测试专用 `request_corner_rest` 脉冲。

原始 Explorer 图标列表只进入 `MotionSolver`，不会在每只虫子的每一帧复制
成 Lua table。Lua 不能直接修改 C++ body。`step` 返回的所有数字必须有限，
并经过物种上限校验。`pose.parts` 通过 part name 指定 rotation 和
joint offset；未返回的 part 使用零姿态。

### 宿主函数

脚本只得到一个只读 `host` table：

- `host.random(tag, low, high)`：唯一随机源；
- `host.clamp(value, low, high)`；
- `host.wrap_angle(value)`。

tag 用于测试诊断，不参与随机结果。生产环境使用每实例独立 RNG；测试环境可
注入固定 tape，并断言 Lua 与旧实现消费次数一致。

## 沙箱和错误处理

- 只开放 base、table、string、math 和 utf8 库。
- 删除 `dofile`、`loadfile`、`load`、`collectgarbage`、
  `math.random` 和 `math.randomseed`。
- 不开放 io、os、package、debug、DLL 加载或任意 `require`。
- manifest 和 behavior 只能由宿主从已验证的物种目录读取。
- 一个 Lua VM 使用受限 allocator；默认总内存预算 32 MiB。
- 每次 `new`、`step` 和 `pose` 使用 instruction-count hook，默认单次
  100,000 条虚拟机指令。
- 每次调用前后恢复 Lua stack top；错误使用 `luaL_traceback` 保存上下文。
- 启动阶段错误导致非零退出并显示明确路径。运行阶段错误只 quarantine 当前
  实例：速度降为零、保持最后有效姿态并只报告一次，其他虫子继续运行。
- NaN、Inf、缺字段、错误类型、未知 part 或越界速度等均视为契约错误。

本轮不实现热重载。脚本在程序启动时原子加载，修改后重启程序生效。

## 通用运动约束

`MotionSolver` 保存位置、heading、speed 和碰撞恢复反馈，但不保存行为状态。
它执行：

1. 按 Lua 的 turn rate 和 acceleration 接近期望 heading / speed；
2. 应用 Lua 给出的 lateral speed；
3. 使用物种碰撞体计算旋转后的躯干范围；
4. 以受限子步连续移动，禁止新穿入静态图标；
5. 对拖拽覆盖执行每帧有上限的最小分离，禁止跨屏瞬移；
6. 记录无进展和边缘停留，并在多个方向探测通路；
7. 把恢复方向和计时反馈给下一帧 Lua。

现有浅转、陡转、移动障碍优先、10 px 工作区边距、最大分离预算及 24 方向
探测将先作为默认求解参数保留。参数来自 manifest 或 Lua motion command，
以后其他虫子可以选择不同的转弯和脱困性格。

## 数据驱动渲染

`SpriteRig` 按 manifest 的 layer 排序并绘制任意数量 part：

1. Lua `pose.body` 决定 body center 的视觉偏移和 rock；
2. 每个 part 的关节为
   `body center + rotate(attachment * bodyLength + jointOffset)`；
3. part 最终旋转为 body heading + body rock + part rotation；
4. 先用相同姿态绘制整套阴影，再绘制实体，保证阴影不会与器官脱节；
5. 蟑螂 manifest 保持实体 RGB `190`、alpha `255`，阴影 alpha `38`、
   offset `(3, 5)`，从而保持当前视觉效果。

渲染器不认识“腿”“触须”或“清洁”；这些全部是 Lua 对具名 part 的姿态。

## Windows-only 收敛

最终生产构建：

- `overlay_window.cpp` 只保留 Win32 layered window；
- `desktop_icons.cpp` 只保留 Explorer `SysListView32` 跟踪；
- `windows_interaction.cpp` 去除非 Windows stub；
- `main.cpp` 去除共享 Linux canvas 和所有平台条件分支；
- CMake 删除 X11、Xext、XFixes、XRender、pkg-config 和 AT-SPI；
- 删除 `packaging/UBUNTU-README.txt` 及 README 中的 Linux 运行说明；
- 非 Windows CMake 只生成 core/Lua 测试，绝不生成桌面程序或安装目标。

## 迁移与验证顺序

1. 在 `3f79273` 上生成 legacy golden 和带标签 RNG tape。
2. 引入通用数据结构、Lua 宿主和 manifest loader，旧 `Cockroach` 仍作为
   临时 oracle。
3. 先迁移动画并逐字段比较九个 part pose。
4. 迁移状态机和 motion intent，使用相同 RNG tape 做逐帧 lockstep。
5. 接入通用 MotionSolver，对比位置、heading、speed、状态、事件和恢复反馈。
6. 主程序切换到 `BugInstance`，再删除生产和测试中的 legacy C++ 行为。
7. 加入最小测试物种，证明框架没有依赖 cockroach part 名或状态名。
8. 删除 Linux 平台代码和文档。
9. 执行 native Release、ASan、MinGW、Wine 和 Windows 11 VM 验证。

### 必须通过的门禁

- 状态和事件逐帧精确；转换帧和 RNG draw count 精确；
- 位置/target `≤ 0.01 px`，speed `≤ 0.01 px/s`，
  heading `≤ 1e-5 rad`；
- 固定姿态网格下所有 part rotation / joint offset 在容差内；
- 旧碰撞、边缘、拖拽、食物、尺寸和长期变速性质测试全部保留；
- Lua 文件缺失、语法/运行错误、无限循环、内存超限、NaN/Inf 和实例隔离
  测试全部通过；
- 100,000 帧压力测试无 Lua stack 或内存持续增长；
- Windows ZIP 包含物种 manifest、behavior 和 atlas，且从任意 cwd 可启动；
- Windows 11 VM 验证 Explorer 图标、拖拽覆盖、食物层级、DPI/分辨率、
  单只和 20 只模式，并保存截图和测试日志。

## Windows 11 VM 验证矩阵

| 场景 | 证据 |
| --- | --- |
| 单只启动、Lua 文件从 EXE 旁加载 | 进程状态、日志、桌面截图 |
| 20 只独立 controller，无共享状态 | 截图、实例诊断日志 |
| 静态图标绕行 | 连续截图或录屏、无躯干重叠 |
| 拖拽图标覆盖和边缘夹困 | 最大单帧位移日志、恢复截图 |
| 快速鼠标逼近 | `Startled → Flee` 状态日志 |
| 角落潜伏和清洁 | 状态日志、器官连续截图 |
| 食物投放、层级、移动和消费 | Z-order 检查、消费日志 |
| 100%、150%、200% 缩放及工作区 | body 尺寸日志、截图 |
| 损坏 behavior.lua | 明确错误、无进程崩溃 |

## 明确拒绝的方案

- **把现有 `Cockroach` 包一层 Lua 开关**：C++ 仍拥有真实状态机，不满足目标。
- **Lua 只返回状态名，C++ 保存各状态速度表**：会形成两处规则来源。
- **每个物种派生 C++ 类**：新增虫子仍需重新编译，扩展边界错误。
- **每只虫子一个 Lua VM**：20 只模式浪费内存，也妨碍模块共享。
- **把 Win32/SDL userdata 暴露给 Lua**：扩大崩溃和安全边界。
- **在 Lua 中实现不可穿透碰撞**：脚本错误可能穿越图标或瞬移；硬约束应由
  宿主统一保证。
- **引入 sol2 或完整 ECS**：当前契约很窄，额外抽象只增加编译时间和调试层。
