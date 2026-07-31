# Windows 11 蟑螂桌宠测试虚拟机

虚拟机名称：`cockroach-win11`

- Windows 11 Pro 25H2 简体中文
- 8 vCPU
- 12 GiB 内存
- 100 GiB QCOW2 动态磁盘
- KVM 硬件加速
- UEFI Secure Boot
- TPM 2.0
- 用户模式 NAT 网络

## 第一次安装

先生成并校验最终 Windows ZIP，再把 Windows 11 x64 ISO 路径传给创建脚本：

```bash
./scripts/build-windows.sh
WINDOWS_ISO=/path/to/Win11_x64.iso ./vm/windows11/create-vm.sh
./vm/windows11/open-console.sh
```

创建脚本只从 `dist/cockroach-overlay-windows-x64.zip` 取测试程序，不读取旧的
展开构建目录。安装使用 `Autounattend.xml` 自动完成。测试账户：

```text
用户名：tester
密码：RoachTest!25H2
```

进入桌面后，程序位于：

```text
C:\CockroachOverlay
```

桌面上会生成单只和 20 只版本的快捷方式。

## 后续使用

启动并打开窗口：

```bash
./vm/windows11/start-vm.sh
```

正常关机：

```bash
./vm/windows11/stop-vm.sh
```

虚拟机数据保存在：

```text
$HOME/VirtualMachines/cockroach-win11
```

可通过 `VM_NAME`、`VM_STATE_DIR` 和 `LIBVIRT_URI` 覆盖默认值。

要给现有虚拟机挂载刚构建的精确 ZIP 内容，可先运行：

```bash
./vm/windows11/make-test-iso.sh
```

测试 ISO 中的 `run-rust-smoke.cmd` 执行单只和 20 只的有界 trace 检查。
`run-interaction-probe.cmd` 用于 1280×800 测试桌面：它把左下方测试图标拖到
默认蟑螂附近并短暂停留，便于观察移动图标避让、有限脱困和点击穿透；坐标也可
作为 PowerShell 参数覆盖。`run-bait-trace.cmd` 会投放食物并验证 1,800 帧内
同时出现 `seek-food`、`feeding`，且没有 controller quarantine。
`run-single-live.cmd` 和 `run-swarm-live.cmd` 会先清理旧测试进程，再以固定种子
启动唯一一轮持续交互测试，避免重复窗口影响观察。

本配置只用于本机程序测试。Windows 通用安装密钥只负责选择 Pro
版本，不会激活 Windows；请根据自己的许可情况完成激活。
