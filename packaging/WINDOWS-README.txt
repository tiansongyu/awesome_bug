Scriptable Bug Overlay（Windows x64，Rust + Lua）
=================================================

启动
----

cockroach_overlay.exe
  单只桌面宠物。启用角落潜伏、触须清洁、鼠标惊吓和食物诱饵。

cockroach_swarm_20.exe
  默认生成 20 只位置、体型和运动相位不同的蟑螂。

请保持两个 EXE、SDL2.dll 和 bugs 文件夹的相对位置不变。程序从 EXE
旁边发现 Lua 行为脚本和贴图，因此可以从任意工作目录启动。

主要机制
--------

* Lua 独占状态机、目标选择、动态快慢速、受惊逃跑、潜伏、清洁、觅食、
  六足交替三足步态和两根独立触须的姿态。
* Rust 宿主管理受限 Lua 运行时、屏幕边界、躯干碰撞、渲染和 Win32 交互。
* 蟑螂尺寸按显示器物理像素动态缩放；默认速度倍率为 3。
* 桌面位于前台时，仅使用躯干碰撞体绕开图标和文字区域。腿和触须不参与
  碰撞。拖拽图标时会主动远离，受困时会探测空隙脱离，不跨屏瞬移。
* 食物位于普通桌面窗口之上、蟑螂之下。
* Ctrl+Alt+F：在鼠标位置投放或移动食物（单只模式）。
* Ctrl+Alt+Q：退出。

常用参数
--------

  --species ID          使用 bugs/ 下的物种包（默认 cockroach）
  --species-path DIR    使用一个明确的物种包目录
  --asset PATH          覆盖物种贴图
  --size N              固定身体长度，100..520 像素（默认自动）
  --speed N             速度倍率，0.25..3（默认 3）
  --display N           显示器序号（默认 0）
  --count N             虫子数量，1..50
  --seed N              固定随机种子，便于复现
  --no-click-through    让覆盖窗口接收鼠标输入
  --frames N            N 帧后退出（测试）
  --trace PATH          写出确定性帧轨迹
  --help                显示完整帮助

示例
----

  cockroach_overlay.exe --size 200
  cockroach_swarm_20.exe --count 20 --seed 42
  cockroach_overlay.exe --species-path D:\bugs\my_beetle

扩展新虫子
----------

复制 bugs/template，修改 manifest.lua、behavior.lua 和 atlas.png。宿主不包含
物种名或蟑螂专用状态；新物种通过同一份严格契约接入。模板的 README.md
给出了最小结构说明。

许可与完整性
------------

代码许可见 LICENSE，蟑螂美术的授权边界见 ASSET-NOTICE.md，第三方软件
许可见 THIRD_PARTY_LICENSES.txt。
SHA256SUMS.txt 可用于校验 ZIP 内每一个文件。
