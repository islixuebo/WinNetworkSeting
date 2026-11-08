# NicTool（Rust / 原生 Win32）

Windows 网卡管理工具。Rust + 原生 Win32 对话框，无 WebView、无运行时托管堆，
阶段1 目标：常驻内存 < 10MB、exe < 2MB、启动 < 0.3s、空闲 CPU ≈ 0。

WPF 版（同仓库上一代实现）留在外层目录作对照，本目录是独立仓库。

## 功能

### 阶段1

- 网卡列表，按有线 / 无线 / VPN / 虚拟网卡 / 其他分组；整行可点、整行高亮
- **禁用中的网卡同样在列表里**（状态列显示「已禁用」），筛选器绑定伪接口不显示
- VPN 客户端适配器（TAP-Windows、Sangfor aTrust、WireGuard、AnyConnect 等）单独成组；
  Hyper-V / VMware / VirtualBox 与隧道接口归「虚拟网卡」
- 状态列着色：已连接蓝、未连接绿、已禁用灰
- 启用 / 禁用（按钮与右键菜单）；禁用默认路由网卡前先告警断网风险
- 网卡详细信息对话框：类型/状态/描述/IPv4/掩码/网关/DNS/MAC/速率/接口跃点数/默认路由
- 刷新；空闲时零轮询、零定时器，所有刷新由用户触发

### 阶段2

- 左侧导航三页：网卡配置 / 系统路由表 / 网络测试
- 网卡配置页：列表 + 启用/禁用/配置…/详细/刷新/刷新 DNS 缓存/重置网络
- 网卡配置对话框：改名、跃点（自动/手动）、IP/DNS（DHCP↔静态）、DHCP 释放/续期
- 系统路由表页：IPv4+IPv6 全量路由，接口名映射；添加/删除持久路由（删默认路由前告警）
- 网络测试页：Ping / Tracert / Nslookup / Ipconfig，工作线程执行、输出原样回显

阶段3：Profile 切换（兼容 WPF 版 `profiles.json`）

## 构建

前置：Rust 工具链（`aarch64-pc-windows-msvc`）+ VS 2022 Build Tools 的 C++ 工作负载（含 ARM64）。
`build.ps1` 会把 `vcvarsall.bat` 的环境（`cl`/`link` 与 `INCLUDE`）导入当前会话 ——
`INCLUDE` 缺失时 `rc.exe` 编译资源会报 RC1015，这一步不能省。

    powershell -ExecutionPolicy Bypass -File build.ps1
    powershell -File check_metrics.ps1

产物：`target\aarch64-pc-windows-msvc\release\nictool.exe`。
exe 带 `requireAdministrator` manifest，双击或 `Start-Process` 都会走 UAC。
构建前需关闭正在运行的窗口，否则链接步骤无法替换被锁住的 exe。

## 测试

    cargo test                 # 单元测试（不要加 --release，见下）
    cargo test -- --ignored    # 真机采集冒烟，需要管理员

`build.rs` 只在 release 形态链接 `app.res`：debug/test 产物一旦带上 requireAdministrator，
非提权 shell 里连测试可执行文件都无法启动（os error 740），单测会全线跑不起来。

## 指标验收

`check_metrics.ps1` 输出体积、启动耗时、空闲工作集、空闲私有内存、空闲 CPU 增量，
任一项超阈值以退出码 2 结束。必须在**已提权**的 PowerShell 里跑：
否则 UAC 弹窗的等待时间会被算进启动耗时，脚本也会直接拒绝执行。

内存口径已定：按**私有内存 <10MB** 判定（进程独占的提交量）。工作集含可被系统换出的共享页
（comctl/uxtheme 等），本机实测比私有量高一个数量级，脚本里只作参考项打印，不参与达标判定。

## 实现说明

- 网卡数据：`GetAdaptersAddresses`（带 `GAA_FLAG_INCLUDE_ALL_INTERFACES`，否则禁用中的接口整条不出现）
  + `GetIfEntry2`（管理/运行状态）+ `GetIpForwardTable2`/`GetIpInterfaceEntry`（默认路由与跃点数）
- 列表：`SysListView32` + `LVN_GETDISPINFO` 之前的静态填充 + `NM_CUSTOMDRAW` 状态色；
  整行选中是**扩展样式**，只能在运行时 `LVM_SETEXTENDEDLISTVIEWSTYLE` 设置
- 启停：`netsh interface set interface "<alias>"`，失败按 ifIndex 回退 `Get-NetAdapter | Enable/Disable-NetAdapter`
- 配置写入统一走 `powershell -NoProfile -NonInteractive -Command`（CIM cmdlet，按 ifIndex 寻址）；
  跃点写入必须同时设 `-AutomaticMetric Disabled`，只写数值会被系统重新算掉
- 控制台工具（ping/tracert/nslookup/ipconfig）的输出按 OEM 代码页解码
  （`GetOEMCP` + `MultiByteToWideChar`），按 UTF-8 硬解会得到替换符
- 依赖只有 `windows` crate；图标为 `res/app.ico`（16/24/32/48/256），运行时 `WM_SETICON` 挂标题栏
