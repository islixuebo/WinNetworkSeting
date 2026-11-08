// Task 6：网卡启用/禁用。设计指定走 netsh 子进程（瞬时开销，不影响常驻内存）；
// 本机实测 netsh 的 interface 上下文只认「已与路由器一起注册」的 3 个接口名，
// 列表里的 5 条有 4 条（WLAN 2/4/5、蓝牙网络连接）按名字点不到，故补一条按 ifIndex 的 CIM 回退。
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};

/// netsh/powershell 都是控制台程序：GUI 进程没有控制台，CreateProcess 会给子进程
/// 新建一个控制台窗口，界面上就是「点启用/禁用闪一下黑窗」。CREATE_NO_WINDOW 让它
/// 在没有窗口的控制台里跑，stdout/stderr 管道不受影响。
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// name=<alias> 作为单个参数元素传给 netsh：Rust 不经 shell 分词，别名含空格也安全。
pub fn netsh_args_for(alias: &str, enable: bool) -> Vec<String> {
    vec![
        "interface".into(),
        "set".into(),
        "interface".into(),
        format!("name={alias}"),
        if enable { "enable" } else { "disable" }.into(),
    ]
}

/// CIM 通道参数。用 `-InterfaceIndex` 而不是名字：ifIndex 是采集时拿到的结构化标识，
/// 不受本地化别名与 netsh 名单影响。`-NonInteractive` 是硬要求 —— UI 线程同步等待子进程，
/// 任何确认提示都会把窗口冻住。
pub fn netadapter_args_for(if_index: u32, enable: bool) -> Vec<String> {
    let verb = if enable { "Enable" } else { "Disable" };
    ps_args(&format!(
        "{verb}-NetAdapter -InterfaceIndex {if_index} -Confirm:$False"
    ))
}

/// PowerShell 单命令参数：前置 UTF-8 输出编码，否则中文 Windows 下的错误行是 GBK，
/// `failure_text` 会判为不可解码而丢掉唯一的原因线索。
fn ps_args(command: &str) -> Vec<String> {
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-Command".into(),
        format!("[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; {command}"),
    ]
}

/// 手动跃点：写数值的同时必须关掉「自动」，否则 Windows 会继续按链路速率算。
/// 走 CIM 而不是 `SetIpInterfaceEntry`：后者只改运行时值，重启回退。
pub fn set_metric_args(if_index: u32, metric: u32) -> Vec<String> {
    ps_args(&format!(
        "Set-NetIPInterface -InterfaceIndex {if_index} -InterfaceMetric {metric} -AutomaticMetric Disabled -Confirm:$False"
    ))
}

/// 恢复自动：只把开关打开，数值交给系统。
pub fn set_metric_auto_args(if_index: u32) -> Vec<String> {
    ps_args(&format!(
        "Set-NetIPInterface -InterfaceIndex {if_index} -AutomaticMetric Enabled -Confirm:$False"
    ))
}

/// 改名同样按 ifIndex 寻址。名字走单引号字面量：中文、空格、`$` 都不会被 PowerShell 展开；
/// 名字里的单引号按 PowerShell 规则翻倍转义。
pub fn rename_args(if_index: u32, name: &str) -> Vec<String> {
    let quoted = name.replace('\'', "''");
    ps_args(&format!(
        "Rename-NetAdapter -InterfaceIndex {if_index} -NewName '{quoted}'"
    ))
}

/// `None` = 自动。两条路径都写进注册表，刷新后 `GetIpInterfaceEntry` 能读回。
pub fn apply_metric(if_index: u32, metric: Option<u32>) -> Result<(), String> {
    match metric {
        Some(m) => run_netadapter(&set_metric_args(if_index, m)),
        None => run_netadapter(&set_metric_auto_args(if_index)),
    }
}

pub fn run_netsh(args: &[String]) -> Result<(), String> {
    run("netsh", args)
}

pub fn run_netadapter(args: &[String]) -> Result<(), String> {
    run("powershell", args)
}

fn run(program: &str, args: &[String]) -> Result<(), String> {
    let out = Command::new(program)
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("启动 {program} 失败：{e}"))?;
    if out.status.success() {
        return Ok(());
    }
    Err(failure_text(
        program,
        out.status.code(),
        &out.stdout,
        &out.stderr,
    ))
}

/// 失败文案：退出码一定有，可读的末行不一定有（netsh 输出跟随系统 ANSI 代码页）。
pub fn failure_text(program: &str, code: Option<i32>, stdout: &[u8], stderr: &[u8]) -> String {
    let detail = last_line(stderr).or_else(|| last_line(stdout));
    let reason = match detail {
        Some(d) => d,
        None => {
            return match code {
                Some(c) => format!("{program} 退出码 {c}"),
                None => format!("{program} 失败（未取到退出码）"),
            }
        }
    };
    match code {
        Some(c) => format!("{program} 退出码 {c}：{reason}"),
        None => format!("{program} 失败：{reason}"),
    }
}

/// 只接受能按 UTF-8 解码的输出。GBK 中文硬解码会得到乱码或替换符，不如只报退出码。
fn last_line(bytes: &[u8]) -> Option<String> {
    let text = std::str::from_utf8(bytes).ok()?;
    text.lines()
        .rev()
        .map(str::trim)
        .find(|line| !line.is_empty())
        .map(str::to_string)
}

/// 先走设计指定的 netsh；netsh 够不着的接口再按 ifIndex 回退到 CIM。
/// 两条路都失败时把两段原因一起返回，便于用户判断是被组策略拦了还是接口真的不存在。
pub fn set_enabled(if_index: u32, alias: &str, enable: bool) -> Result<(), String> {
    match run_netsh(&netsh_args_for(alias, enable)) {
        Ok(()) => Ok(()),
        Err(primary) => run_netadapter(&netadapter_args_for(if_index, enable))
            .map_err(|fallback| format!("{primary}；按 ifIndex 回退也失败：{fallback}")),
    }
}

/// 跑一条 PowerShell 并取回 stdout（CIM 读取通道）。输出必须能按 UTF-8 解码，
/// 否则宁可报错也不把乱码当结果。
pub fn ps_capture(args: &[String]) -> Result<String, String> {
    let out = Command::new("powershell")
        .args(args)
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output()
        .map_err(|e| format!("启动 powershell 失败：{e}"))?;
    if !out.status.success() {
        return Err(failure_text(
            "powershell",
            out.status.code(),
            &out.stdout,
            &out.stderr,
        ));
    }
    String::from_utf8(out.stdout).map_err(|_| "powershell 输出不是 UTF-8".to_string())
}

pub fn dhcp_read_args(if_index: u32) -> Vec<String> {
    ps_args(&format!(
        "(Get-NetIPInterface -InterfaceIndex {if_index} -AddressFamily IPv4).Dhcp.ToString()"
    ))
}

pub fn parse_dhcp_flag(text: &str) -> Option<bool> {
    match text.trim() {
        "Enabled" => Some(true),
        "Disabled" => Some(false),
        _ => None,
    }
}

pub fn dhcp_enabled(if_index: u32) -> Result<bool, String> {
    let text = ps_capture(&dhcp_read_args(if_index))?;
    parse_dhcp_flag(&text).ok_or_else(|| format!("无法识别 DHCP 状态：{}", text.trim()))
}

pub fn set_dhcp_args(if_index: u32, enable: bool) -> Vec<String> {
    let state = if enable { "Enabled" } else { "Disabled" };
    ps_args(&format!(
        "Set-NetIPInterface -InterfaceIndex {if_index} -Dhcp {state} -Confirm:$False"
    ))
}

/// 静态 IP：先关 DHCP，再清掉旧地址，最后写新地址。`gw` 为空时整段省略 ——
/// 传 `-DefaultGateway ''` 会被 PowerShell 当成空参数报错。
pub fn set_static_ip_args(if_index: u32, ip: &str, prefix: u8, gw: &str) -> Vec<String> {
    let mut cmd = format!(
        "Set-NetIPInterface -InterfaceIndex {if_index} -Dhcp Disabled -Confirm:$False; \
         Remove-NetIPAddress -InterfaceIndex {if_index} -AddressFamily IPv4 -Confirm:$False -ErrorAction SilentlyContinue; \
         New-NetIPAddress -InterfaceIndex {if_index} -IPAddress {ip} -PrefixLength {prefix}"
    );
    if !gw.is_empty() {
        cmd.push_str(&format!(" -DefaultGateway {gw}"));
    }
    cmd.push_str(" -Confirm:$False");
    ps_args(&cmd)
}

pub fn set_dns_args(if_index: u32, servers: &[String]) -> Vec<String> {
    if servers.is_empty() {
        return ps_args(&format!(
            "Set-DnsClientServerAddress -InterfaceIndex {if_index} -ResetServerAddresses"
        ));
    }
    ps_args(&format!(
        "Set-DnsClientServerAddress -InterfaceIndex {if_index} -ServerAddresses {}",
        servers.join(",")
    ))
}

pub fn release_dhcp_args(if_index: u32) -> Vec<String> {
    ps_args(&format!(
        "Invoke-CimMethod -InputObject (Get-NetIPInterface -InterfaceIndex {if_index} -AddressFamily IPv4) -MethodName ReleaseDHCPLease"
    ))
}

pub fn renew_dhcp_args(if_index: u32) -> Vec<String> {
    ps_args(&format!(
        "Invoke-CimMethod -InputObject (Get-NetIPInterface -InterfaceIndex {if_index} -AddressFamily IPv4) -MethodName RenewDHCPLease"
    ))
}

#[derive(Clone)]
pub struct IpConfig {
    pub if_index: u32,
    pub use_dhcp: bool,
    pub ip: String,
    pub mask: u8,
    pub gw: String,
    pub dns: Vec<String>,
}

/// 按顺序执行：DHCP/静态 → DNS。前一步失败就停，避免「IP 改了 DNS 没改」的半截状态。
pub fn apply_ip_config(cfg: &IpConfig) -> Result<(), String> {
    let step = if cfg.use_dhcp {
        set_dhcp_args(cfg.if_index, true)
    } else {
        set_static_ip_args(cfg.if_index, &cfg.ip, cfg.mask, &cfg.gw)
    };
    run_netadapter(&step)?;
    run_netadapter(&set_dns_args(cfg.if_index, &cfg.dns))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn enable_args_shape() {
        assert_eq!(
            netsh_args_for("以太网", true),
            vec!["interface", "set", "interface", "name=以太网", "enable"]
        );
    }

    #[test]
    fn disable_args_shape() {
        assert_eq!(netsh_args_for("WLAN", false)[4], "disable");
    }

    #[test]
    fn alias_with_spaces_stays_one_arg() {
        assert_eq!(netsh_args_for("Ethernet 2", true)[3], "name=Ethernet 2");
    }

    /// 回退通道必须按 ifIndex 寻址，不用名字。
    #[test]
    fn netadapter_args_address_by_index() {
        let a = netadapter_args_for(15, false);
        assert!(
            a[3].ends_with("Disable-NetAdapter -InterfaceIndex 15 -Confirm:$False"),
            "{}",
            a[3]
        );
        let b = netadapter_args_for(13, true);
        assert!(
            b[3].ends_with("Enable-NetAdapter -InterfaceIndex 13 -Confirm:$False"),
            "{}",
            b[3]
        );
    }

    /// -NonInteractive 是硬要求：UI 线程同步等待子进程，任何提示都会把窗口冻住。
    #[test]
    fn netadapter_args_never_prompt() {
        let a = netadapter_args_for(1, true);
        assert_eq!(&a[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        assert!(a[3].contains("-Confirm:$False"));
    }

    #[test]
    fn netadapter_command_forces_utf8_output() {
        assert!(
            netadapter_args_for(1, true)[3].starts_with("[Console]::OutputEncoding="),
            "错误行若不能按 UTF-8 解码，回退失败原因会被整条丢掉"
        );
    }

    #[test]
    fn metric_args_turn_automatic_off() {
        let a = set_metric_args(19, 300);
        assert_eq!(&a[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        assert!(
            a[3].ends_with("Set-NetIPInterface -InterfaceIndex 19 -InterfaceMetric 300 -AutomaticMetric Disabled -Confirm:$False"),
            "{}",
            a[3]
        );
        assert!(a[3].contains("-Confirm:$False"), "写入配置不得弹确认");
    }

    /// 「自动」必须同时让 Windows 重新计算，只去掉数值是不够的。
    #[test]
    fn auto_metric_args_only_touch_automatic_flag() {
        let a = set_metric_auto_args(19);
        assert!(
            a[3].ends_with("Set-NetIPInterface -InterfaceIndex 19 -AutomaticMetric Enabled -Confirm:$False"),
            "{}",
            a[3]
        );
        assert!(a[3].contains("-Confirm:$False"), "写入配置不得弹确认");
        assert!(!a[3].contains("InterfaceMetric 3"), "自动模式不应写死数值");
    }

    #[test]
    fn rename_args_address_by_index() {
        let a = rename_args(19, "办公网");
        assert_eq!(&a[..3], ["-NoProfile", "-NonInteractive", "-Command"]);
        assert!(
            a[3].ends_with("Rename-NetAdapter -InterfaceIndex 19 -NewName '办公网'"),
            "{}",
            a[3]
        );
    }

    #[test]
    fn dhcp_flag_parser_understands_cim_output() {
        // Get-NetIPInterface 的 Dhcp 是枚举，命令行里印成 Enabled/Disabled
        assert_eq!(parse_dhcp_flag("Enabled\r\n"), Some(true));
        assert_eq!(parse_dhcp_flag("Disabled\n"), Some(false));
        assert_eq!(parse_dhcp_flag(""), None);
        assert_eq!(parse_dhcp_flag("garbage"), None);
    }

    #[test]
    fn static_ip_command_sets_address_and_gateway() {
        let a = set_static_ip_args(19, "10.0.0.5", 24, "10.0.0.1");
        assert!(
            a[3].ends_with("Set-NetIPInterface -InterfaceIndex 19 -Dhcp Disabled -Confirm:$False; Remove-NetIPAddress -InterfaceIndex 19 -AddressFamily IPv4 -Confirm:$False -ErrorAction SilentlyContinue; New-NetIPAddress -InterfaceIndex 19 -IPAddress 10.0.0.5 -PrefixLength 24 -DefaultGateway 10.0.0.1 -Confirm:$False"),
            "{}",
            a[3]
        );
    }

    /// 网关留空时必须整段省略 —— 传 `-DefaultGateway ''` 会被 PowerShell 当成空参数报错。
    #[test]
    fn static_ip_without_gateway_omits_flag() {
        let a = set_static_ip_args(19, "10.0.0.5", 24, "");
        assert!(!a[3].contains("-DefaultGateway"), "{}", a[3]);
        assert!(a[3].contains("-PrefixLength 24"));
    }

    #[test]
    fn dns_command_sends_all_servers_or_resets() {
        let two = set_dns_args(19, &["1.1.1.1".into(), "8.8.8.8".into()]);
        assert!(
            two[3].ends_with("Set-DnsClientServerAddress -InterfaceIndex 19 -ServerAddresses 1.1.1.1,8.8.8.8"),
            "{}",
            two[3]
        );
        let none = set_dns_args(19, &[]);
        assert!(none[3].contains("-ResetServerAddresses"), "{}", none[3]);
    }

    #[test]
    fn dhcp_release_and_renew_are_ifindex_scoped() {
        assert!(release_dhcp_args(19)[3].ends_with("Invoke-CimMethod -InputObject (Get-NetIPInterface -InterfaceIndex 19 -AddressFamily IPv4) -MethodName ReleaseDHCPLease"));
        assert!(renew_dhcp_args(19)[3].ends_with("Invoke-CimMethod -InputObject (Get-NetIPInterface -InterfaceIndex 19 -AddressFamily IPv4) -MethodName RenewDHCPLease"));
    }

    #[test]
    fn failure_text_keeps_exit_code() {
        let s = failure_text("netsh", Some(1), b"", b"");
        assert!(s.contains("netsh"), "{s}");
        assert!(s.contains("退出码 1"), "{s}");
    }

    /// netsh 在中文 Windows 输出 GBK，不是合法 UTF-8 → 只能报退出码，不能贴乱码。
    #[test]
    fn failure_text_drops_non_utf8_output() {
        let gbk = [0xb4, 0xed, 0xce, 0xf3]; // 「错误」的 GBK 字节
        let s = failure_text("netsh", Some(2), &gbk, &[]);
        assert!(!s.contains('\u{fffd}'), "{s}");
        assert!(s.contains("退出码 2"), "{s}");
    }

    #[test]
    fn failure_text_appends_last_non_empty_line() {
        let s = failure_text("powershell", Some(1), b"line1\n\n  last line  \n", b"");
        assert!(s.contains("last line"), "{s}");
        assert!(!s.contains("line1"), "{s}");
        // 前后空白必须裁掉，否则状态栏/弹窗里会出现缩进错位的错误行
        assert!(s.ends_with("：last line"), "{s}");
    }

    #[test]
    fn failure_text_prefers_stderr() {
        let s = failure_text("netsh", Some(3), b"stdout line\n", b"stderr line\n");
        assert!(s.contains("stderr line"), "{s}");
        assert!(!s.contains("stdout line"), "{s}");
    }

    /// 尾随空行不能被当成错误行（netsh 常在末尾留空行），否则错误行只剩一个冒号。
    #[test]
    fn failure_text_ignores_trailing_blank_lines() {
        assert_eq!(
            failure_text("netsh", Some(1), b"real\n\n   \n", b""),
            "netsh 退出码 1：real"
        );
        assert_eq!(
            failure_text("netsh", Some(1), b"\n \n", b""),
            "netsh 退出码 1"
        );
    }

    #[test]
    fn failure_text_without_exit_code() {
        assert!(failure_text("netsh", None, b"boom\n", b"").contains("失败：boom"));
        assert!(
            failure_text("netsh", None, b"", b"").contains("未取到退出码"),
            "两条线索都丢了也必须留一句能看懂的话"
        );
    }
}
