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

运行：

```bash
./vm/windows11/create-vm.sh
./vm/windows11/open-console.sh
```

安装使用 `Autounattend.xml` 自动完成。测试账户：

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
/home/ubuntu/VirtualMachines/cockroach-win11
```

本配置只用于本机程序测试。Windows 通用安装密钥只负责选择 Pro
版本，不会激活 Windows；请根据自己的许可情况完成激活。
