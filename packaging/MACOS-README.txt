Awesome Bug for macOS
=====================

系统要求：macOS 11 或更高版本。

运行：
  Cockroach Overlay.app     单只桌面宠物
  Cockroach Swarm 20.app    默认 20 只

快捷键：
  Control+Option+F          在鼠标位置投放或移动食物（单只模式）
  Control+Option+Q          退出

首次运行：
  本项目当前未使用 Apple Developer ID 签名和公证。若 Gatekeeper 阻止启动，
  请在 Finder 中按住 Control 点击应用，选择“打开”，再确认一次。

注意：
  - 应用不显示 Dock 图标，退出请使用 Control+Option+Q。
  - macOS 没有公开、稳定的 Finder 桌面图标矩形接口，因此此版本不会对
    Finder 桌面图标做碰撞；屏幕边界、鼠标惊吓、Lua 行为和其余碰撞规则正常。
  - 日志位于 ~/Library/Logs/ScriptableBugOverlay/latest.log。

命令行参数：
  可直接运行 .app/Contents/MacOS/ 下的可执行文件并传入 --help 查看。

第三方软件许可见 THIRD_PARTY_LICENSES.txt；美术资源说明见 ASSET-NOTICE.md。
