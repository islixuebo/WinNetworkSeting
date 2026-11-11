// Task 6：网卡启用/禁用。设计指定走 netsh 子进程（瞬时开销，不影响常驻内存）；
// 本机实测 netsh 的 interface 上下文只认「已与路由器一起注册」的 3 个接口名，
// 列表里的 5 条有 4 条（WLAN 2/4/5、蓝牙网络连接）按名字点不到，故补一条按 ifIndex 的 CIM 回退。
use std::os::windows::process::CommandExt;
use std::process::{Command, Stdio};
use windows::Win32::Globalization::{GetOEMCP, MultiByteToWideChar, MULTI_BYTE_TO_WIDE_CHAR_FLAGS};

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

/// 重置网络的三步。前两步是 netsh（会改 Winsock/协议栈），第三步清 DNS 缓存。
pub fn reset_commands() -> Vec<Vec<String>> {
    vec![
        vec!["winsock".into(), "reset".into()],
        vec!["int".into(), "ip".into(), "reset".into()],
        ps_args("Clear-DnsClientCache"),
    ]
}

/// 依次执行；失败立即停，已成功的步骤不回滚（回滚本身会再次改动网络栈）。
pub fn reset_network() -> Result<(), String> {
    let cmds = reset_commands();
    for (i, args) in cmds.iter().enumerate() {
        let r = if i == 2 {
            run_netadapter(args)
        } else {
            run_netsh(args)
        };
        if let Err(e) = r {
            return Err(format!("第 {} 步失败：{e}", i + 1));
        }
    }
    Ok(())
}

pub fn flush_dns_args() -> Vec<String> {
    ps_args("Clear-DnsClientCache")
}

pub fn flush_dns() -> Result<(), String> {
    run_netadapter(&flush_dns_args())
}

pub fn new_route_args(if_index: u32, prefix: &str, next_hop: &str, metric: u32) -> Vec<String> {
    ps_args(&format!(
        "New-NetRoute -InterfaceIndex {if_index} -DestinationPrefix {prefix} -NextHop {next_hop} -RouteMetric {metric} -Confirm:$False"
    ))
}

pub fn remove_route_args(if_index: u32, prefix: &str, next_hop: &str) -> Vec<String> {
    ps_args(&format!(
        "Remove-NetRoute -InterfaceIndex {if_index} -DestinationPrefix {prefix} -NextHop {next_hop} -Confirm:$False"
    ))
}

pub fn add_route(if_index: u32, prefix: &str, next_hop: &str, metric: u32) -> Result<(), String> {
    run_netadapter(&new_route_args(if_index, prefix, next_hop, metric))
}

pub fn remove_route(if_index: u32, prefix: &str, next_hop: &str) -> Result<(), String> {
    run_netadapter(&remove_route_args(if_index, prefix, next_hop))
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

#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum TestKind {
    Ping,
    Tracert,
    Nslookup,
    IpConfig,
}

pub fn test_kind_name(kind: TestKind) -> &'static str {
    match kind {
        TestKind::Ping => "Ping",
        TestKind::Tracert => "Tracert",
        TestKind::Nslookup => "Nslookup",
        TestKind::IpConfig => "Ipconfig /all",
    }
}

/// 全部用系统自带的控制台工具：没有 PowerShell 启动开销，输出也是用户熟悉的样子。
pub fn test_program(kind: TestKind) -> &'static str {
    match kind {
        TestKind::Ping => "ping",
        TestKind::Tracert => "tracert",
        TestKind::Nslookup => "nslookup",
        TestKind::IpConfig => "ipconfig",
    }
}

pub fn test_args(kind: TestKind, target: &str) -> Vec<String> {
    match kind {
        TestKind::Ping => vec!["-n".into(), "4".into(), target.into()],
        TestKind::Tracert => vec![
            "-h".into(),
            "30".into(),
            "-w".into(),
            "500".into(),
            target.into(),
        ],
        TestKind::Nslookup => vec![target.into()],
        TestKind::IpConfig => vec!["/all".into()],
    }
}

/// 控制台工具按 OEM 码页输出字节；`codepage` 显式传入，便于用固定码页做单测。
pub fn decode_codepage(codepage: u32, bytes: &[u8]) -> String {
    if bytes.is_empty() {
        return String::new();
    }
    let mut buf = vec![0u16; bytes.len() + 1];
    let n = unsafe {
        MultiByteToWideChar(
            codepage,
            MULTI_BYTE_TO_WIDE_CHAR_FLAGS(0),
            bytes,
            Some(&mut buf),
        )
    };
    if n <= 0 {
        return String::new();
    }
    String::from_utf16_lossy(&buf[..n as usize])
}

pub fn decode_oem(bytes: &[u8]) -> String {
    decode_codepage(unsafe { GetOEMCP() }, bytes)
}

/// 测试输出：解码后的全文 + 退出码。退出码非零不等于失败（ping 不通就是 1），
/// 所以 UI 必须把输出原样显示，不能只显示一句「失败」。
pub struct TestResult {
    pub text: String,
    pub code: Option<i32>,
}

impl TestResult {
    pub fn summary(&self) -> String {
        match self.code {
            Some(0) => "完成".to_string(),
            Some(c) => format!("结束（退出码 {c}）"),
            None => "结束（未取到退出码）".to_string(),
        }
    }
}

pub fn run_test(kind: TestKind, target: &str) -> TestResult {
    let out = Command::new(test_program(kind))
        .args(test_args(kind, target))
        .creation_flags(CREATE_NO_WINDOW)
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .output();
    match out {
        Ok(o) => {
            let mut text = decode_oem(&o.stdout);
            let err = decode_oem(&o.stderr);
            if !err.trim().is_empty() {
                text.push_str(&err);
            }
            TestResult {
                text,
                code: o.status.code(),
            }
        }
        Err(e) => TestResult {
            text: format!("启动 {} 失败：{e}", test_program(kind)),
            code: None,
        },
    }
}

/// 预置目标：默认网关排第一（诊断第一跳），其余是回环与常用公共 DNS/域名。
pub fn test_presets(gateway: &str) -> Vec<String> {
    let mut v = Vec::new();
    let g = gateway.trim();
    if !g.is_empty() {
        v.push(g.to_string());
    }
    for s in ["127.0.0.1", "114.114.114.114", "223.5.5.5", "8.8.8.8", "www.baidu.com"] {
        if !v.iter().any(|x| x == s) {
            v.push(s.to_string());
        }
    }
    v
}

/// 把控制台原始输出翻译成人话：结论 + 可能原因 + 下一步定位建议。
pub fn explain_test(kind: TestKind, text: &str) -> String {
    match kind {
        TestKind::Ping => explain_ping(text),
        TestKind::Tracert => explain_tracert(text),
        TestKind::Nslookup => explain_nslookup(text),
        TestKind::IpConfig => explain_ipconfig(text),
    }
}

fn contains_any(text: &str, markers: &[&str]) -> bool {
    markers.iter().any(|m| text.contains(m))
}

/// 「(25% 丢失)」/「(25% loss)」里 % 前的数字。
fn loss_percent(text: &str) -> Option<u32> {
    let pos = text.find('%')?;
    let digits: String = text[..pos]
        .chars()
        .rev()
        .take_while(|c| c.is_ascii_digit())
        .collect::<Vec<_>>()
        .into_iter()
        .rev()
        .collect();
    digits.parse().ok()
}

fn number_after(text: &str, markers: &[&str]) -> Option<u32> {
    for m in markers {
        if let Some(pos) = text.find(m) {
            let digits: String = text[pos + m.len()..]
                .trim_start()
                .chars()
                .take_while(|c| c.is_ascii_digit())
                .collect();
            if let Ok(n) = digits.parse() {
                return Some(n);
            }
        }
    }
    None
}

fn latency_tier(ms: u32) -> &'static str {
    match ms {
        0..=1 => "延迟极低（本机或同网段设备）。",
        2..=10 => "局域网水平，优秀。",
        11..=50 => "良好。",
        51..=150 => "一般，跨地域访问的正常水平。",
        _ => "偏高，可能是跨洲链路、拥塞或无线链路质量差。",
    }
}

fn explain_ping(text: &str) -> String {
    if contains_any(text, &["找不到主机", "could not find host"]) {
        return "域名解析失败：DNS 没能把这个名字翻译成 IP。先检查域名拼写；再用 Nslookup 或直接 Ping 公共 IP（如 223.5.5.5）区分是 DNS 问题还是网络问题。".into();
    }
    // 「无法访问目标主机」的回复会被 ping 计为已接收，必须先于 0% 丢失判断。
    if contains_any(text, &["无法访问目标主机", "Destination host unreachable"]) {
        return "目标不可达：本机或沿途路由器没有去往目标的路由。检查默认网关与本机 IP 配置（是否 169.254.x.x）。".into();
    }
    match loss_percent(text) {
        Some(0) => {
            let mut s = "连通正常：0% 丢失，目标可达。".to_string();
            if let Some(avg) = number_after(text, &["平均 = ", "Average = "]) {
                s.push_str(&format!(" 平均延迟 {avg}ms，{}", latency_tier(avg)));
            }
            s
        }
        Some(100) => "全部丢包，目标不可达。可能原因：目标关机、防火墙禁 Ping、本机断网。逐跳定位：先 Ping 127.0.0.1（协议栈），再 Ping 默认网关（本机网络），最后 Ping 公共 IP（外网）。".into(),
        Some(p) => format!("丢包 {p}%：链路不稳定。常见于 Wi-Fi 信号差/干扰、网线接触不良、沿途拥塞或目标限速。先 Ping 默认网关：网关也丢包则是本机链路问题，网关正常则问题在外网路径。"),
        None => "未能从输出里解析出统计信息，请查看上方原始输出。".into(),
    }
}

fn explain_tracert(text: &str) -> String {
    let mut hop_count = 0u32;
    let mut first_star = None;
    for line in text.lines() {
        if line.trim_start().starts_with(|c: char| c.is_ascii_digit()) {
            hop_count += 1;
            if first_star.is_none() && line.matches('*').count() >= 3 {
                first_star = Some(hop_count);
            }
        }
    }
    let complete = contains_any(text, &["跟踪完成", "Trace complete"]);
    if let Some(n) = first_star {
        if complete {
            return format!("已到达目标，共 {hop_count} 跳。第 {n} 跳无回应但后续继续：该节点只是不回应 ICMP，转发正常，一般不是故障。");
        }
        if n == 1 {
            return "第一跳（默认网关）就无回应：问题在本机到网关之间。检查网线/Wi-Fi 连接、网关设备是否正常工作。".into();
        }
        return format!("从第 {n} 跳开始无回应，故障可能在该节点或其上联链路；第 {} 跳是最后有回应的位置。", n - 1);
    }
    if complete {
        return format!("已到达目标，共 {hop_count} 跳，路径全程有回应。");
    }
    "未到达目标（达到最大跳数或中断）。查看上方最后几跳，定位停在哪一段。".into()
}

fn explain_nslookup(text: &str) -> String {
    if contains_any(text, &["timed out", "请求超时", "超时"]) {
        return "DNS 服务器无响应：检查本机 DNS 配置，或换用公共 DNS（223.5.5.5 / 114.114.114.114）再试。".into();
    }
    if contains_any(text, &["Non-existent domain", "找不到", "can't find"]) {
        return "域名不存在（NXDOMAIN）：DNS 服务器正常但查不到这个名字，检查拼写或确认域名已注册。".into();
    }
    if contains_any(text, &["名称:", "Name:"]) {
        return "解析正常：域名已解析到上方地址。".into();
    }
    "未能判定解析结果，请查看上方原始输出。".into()
}

fn explain_ipconfig(text: &str) -> String {
    if text.contains("169.254.") {
        return "出现 169.254.x.x 自动私有地址：DHCP 获取失败。检查网线/Wi-Fi 是否连上、DHCP 服务是否正常，或在网卡配置里改用静态 IP。".into();
    }
    "重点看三项：IPv4 地址、默认网关、DNS 服务器是否齐全；网关为空或 169.254 开头都意味着上不了外网。".into()
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
            a[3].ends_with(
                "Set-NetIPInterface -InterfaceIndex 19 -AutomaticMetric Enabled -Confirm:$False"
            ),
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
            two[3].ends_with(
                "Set-DnsClientServerAddress -InterfaceIndex 19 -ServerAddresses 1.1.1.1,8.8.8.8"
            ),
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
    fn new_route_uses_prefix_and_next_hop() {
        let a = new_route_args(19, "10.20.0.0/24", "10.20.0.1", 50);
        assert!(
            a[3].ends_with("New-NetRoute -InterfaceIndex 19 -DestinationPrefix 10.20.0.0/24 -NextHop 10.20.0.1 -RouteMetric 50 -Confirm:$False"),
            "{}",
            a[3]
        );
    }

    /// 删除必须同时带目的前缀和下一跳：只给前缀会删掉同前缀的多条路由。
    #[test]
    fn remove_route_scopes_by_prefix_and_hop() {
        let a = remove_route_args(19, "10.20.0.0/24", "10.20.0.1");
        assert!(
            a[3].ends_with("Remove-NetRoute -InterfaceIndex 19 -DestinationPrefix 10.20.0.0/24 -NextHop 10.20.0.1 -Confirm:$False"),
            "{}",
            a[3]
        );
    }

    #[test]
    fn reset_runs_three_steps_in_order() {
        let cmds = reset_commands();
        assert_eq!(cmds.len(), 3);
        assert_eq!(cmds[0], vec!["winsock".to_string(), "reset".to_string()]);
        assert_eq!(
            cmds[1],
            vec!["int".to_string(), "ip".to_string(), "reset".to_string()]
        );
        assert!(cmds[2][3].contains("Clear-DnsClientCache"));
    }

    /// 阶段1 的工具条按钮「刷新 DNS 缓存」一直没接线，本任务补上。
    #[test]
    fn flush_dns_args_clear_cache() {
        let a = flush_dns_args();
        assert!(a[3].ends_with("Clear-DnsClientCache"), "{}", a[3]);
    }

    #[test]
    fn failure_text_without_exit_code() {
        assert!(failure_text("netsh", None, b"boom\n", b"").contains("失败：boom"));
        assert!(
            failure_text("netsh", None, b"", b"").contains("未取到退出码"),
            "两条线索都丢了也必须留一句能看懂的话"
        );
    }

    #[test]
    fn test_command_shapes() {
        assert_eq!(test_program(TestKind::Ping), "ping");
        assert_eq!(
            test_args(TestKind::Ping, "10.0.0.1"),
            vec!["-n", "4", "10.0.0.1"]
        );
        assert_eq!(
            test_args(TestKind::Tracert, "10.0.0.1"),
            vec!["-h", "30", "-w", "500", "10.0.0.1"]
        );
        assert_eq!(
            test_args(TestKind::Nslookup, "example.com"),
            vec!["example.com"]
        );
        // ipconfig 不吃目标参数
        assert_eq!(test_args(TestKind::IpConfig, "ignored"), vec!["/all"]);
        assert_eq!(test_kind_name(TestKind::Ping), "Ping");
    }

    /// 中文 Windows 的控制台工具输出是 GBK。按 OEM 码页解码，
    /// 按 UTF-8 硬解会得到替换符 —— 这条测试钉的是「按码页解」这件事本身。
    #[test]
    fn oem_decode_keeps_chinese_output() {
        // 「无法访问目标主机.」的 GBK 字节
        let gbk = [
            0xce, 0xde, 0xb7, 0xa8, 0xb7, 0xc3, 0xce, 0xca, 0xc4, 0xbf, 0xb1, 0xea, 0xd6, 0xf7,
            0xbb, 0xfa, 0x2e,
        ];
        assert_eq!(decode_codepage(936, &gbk), "无法访问目标主机.");
        // 「请求超时.」
        let gbk2 = [0xc7, 0xeb, 0xc7, 0xf3, 0xb3, 0xac, 0xca, 0xb1, 0x2e];
        assert_eq!(decode_codepage(936, &gbk2), "请求超时.");
        // 空输入不 panic，返回空串
        assert_eq!(decode_codepage(936, &[]), "");
    }

    #[test]
    fn test_result_carries_exit_code_even_without_output() {
        let r = TestResult {
            text: String::new(),
            code: Some(1),
        };
        assert_eq!(r.summary(), "结束（退出码 1）");
    }

    #[test]
    #[ignore]
    fn real_ping_loopback_works() {
        let r = run_test(TestKind::Ping, "127.0.0.1");
        assert_eq!(r.code, Some(0), "{}", r.text);
        assert!(r.text.contains("127.0.0.1"));
    }

    const PING_OK_ZH: &str = "正在 Ping 127.0.0.1 具有 32 字节的数据:\n\
        来自 127.0.0.1 的回复: 字节=32 时间<1ms TTL=128\n\n\
        127.0.0.1 的 Ping 统计信息:\n    \
        数据包: 已发送 = 4，已接收 = 4，丢失 = 0 (0% 丢失)，\n\
        往返行程的估计时间(以毫秒为单位):\n    \
        最短 = 0ms，最长 = 0ms，平均 = 0ms\n";

    #[test]
    fn explain_ping_all_received_is_normal() {
        let s = explain_test(TestKind::Ping, PING_OK_ZH);
        assert!(s.contains("正常"), "{s}");
        assert!(s.contains("0%"), "{s}");
    }

    #[test]
    fn explain_ping_reports_latency() {
        let text = PING_OK_ZH.replace("平均 = 0ms", "平均 = 120ms");
        let s = explain_test(TestKind::Ping, &text);
        assert!(s.contains("120"), "{s}");
        assert!(s.contains("延迟"), "{s}");
    }

    #[test]
    fn explain_ping_partial_loss_blames_link() {
        let text = PING_OK_ZH.replace("(0% 丢失)", "(25% 丢失)");
        let s = explain_test(TestKind::Ping, &text);
        assert!(s.contains("丢包"), "{s}");
        assert!(s.contains("不稳定"), "{s}");
    }

    #[test]
    fn explain_ping_total_loss_suggests_hop_by_hop() {
        let text = PING_OK_ZH.replace("(0% 丢失)", "(100% 丢失)");
        let s = explain_test(TestKind::Ping, &text);
        assert!(s.contains("不可达"), "{s}");
        assert!(s.contains("网关"), "{s}");
    }

    #[test]
    fn explain_ping_dns_failure() {
        let s = explain_test(
            TestKind::Ping,
            "Ping 请求找不到主机 bad.example。请检查该名称并尝试重新解析。",
        );
        assert!(s.contains("解析"), "{s}");
        assert!(s.contains("DNS"), "{s}");
    }

    /// 「无法访问目标主机」的回复会被 ping 计为已接收（0% 丢失），
    /// 解说必须先认这句，不能掉进「0% 丢失 = 正常」的分支。
    #[test]
    fn explain_ping_unreachable_beats_zero_loss() {
        let text = format!(
            "来自 10.0.0.5 的回复: 无法访问目标主机。\n{}",
            PING_OK_ZH
        );
        let s = explain_test(TestKind::Ping, &text);
        assert!(!s.contains("正常"), "{s}");
        assert!(s.contains("网关") || s.contains("路由"), "{s}");
    }

    #[test]
    fn explain_ping_english_loss() {
        let s = explain_test(
            TestKind::Ping,
            "Packets: Sent = 4, Received = 0, Lost = 4 (100% loss),",
        );
        assert!(s.contains("不可达"), "{s}");
    }

    #[test]
    fn explain_tracert_complete_star_is_not_fault() {
        let text = "  1    <1 毫秒    <1 毫秒    <1 毫秒  10.0.0.1\n  \
            2     *        *        *     请求超时。\n  \
            3    12 ms    11 ms    12 ms  93.184.216.34\n\
            跟踪完成。\n";
        let s = explain_test(TestKind::Tracert, text);
        assert!(s.contains("到达"), "{s}");
        assert!(s.contains("ICMP"), "{s}");
    }

    #[test]
    fn explain_tracert_first_hop_timeout_blames_gateway() {
        let text = "  1     *        *        *     请求超时。\n  \
            2     *        *        *     请求超时。\n";
        let s = explain_test(TestKind::Tracert, text);
        assert!(s.contains("网关"), "{s}");
    }

    #[test]
    fn explain_tracert_stops_midway_points_at_hop() {
        let text = "  1    <1 毫秒    <1 毫秒  10.0.0.1\n  \
            2    10 ms    10 ms  10.1.0.1\n  \
            3     *        *        *     请求超时。\n  \
            4     *        *        *     请求超时。\n";
        let s = explain_test(TestKind::Tracert, text);
        assert!(s.contains("3"), "{s}");
    }

    #[test]
    fn explain_nslookup_timeout() {
        let s = explain_test(
            TestKind::Nslookup,
            "DNS request timed out.\ntimeout was 2 seconds.\n请求超时。",
        );
        assert!(s.contains("DNS"), "{s}");
    }

    #[test]
    fn explain_nslookup_nxdomain() {
        let s = explain_test(
            TestKind::Nslookup,
            "*** public1.alidns.com 找不到 bad.example: Non-existent domain",
        );
        assert!(s.contains("不存在"), "{s}");
    }

    #[test]
    fn explain_nslookup_ok() {
        let s = explain_test(
            TestKind::Nslookup,
            "服务器:  public1.alidns.com\nAddress:  223.5.5.5\n\n\
            名称:    www.baidu.com\nAddresses:  110.242.68.3\n",
        );
        assert!(s.contains("正常"), "{s}");
    }

    #[test]
    fn explain_ipconfig_apipa_warns_dhcp() {
        let s = explain_test(
            TestKind::IpConfig,
            "IPv4 地址 . . . . . . . . . . . . : 169.254.12.34( preferred)",
        );
        assert!(s.contains("DHCP"), "{s}");
    }

    #[test]
    fn presets_gateway_first_and_fixed_list() {
        let p = test_presets("10.0.0.1");
        assert_eq!(p[0], "10.0.0.1");
        assert!(p.contains(&"223.5.5.5".to_string()));
        assert!(p.contains(&"www.baidu.com".to_string()));
        let no_gw = test_presets("");
        assert!(!no_gw.iter().any(|s| s.is_empty()));
        assert_eq!(no_gw[0], "127.0.0.1");
    }
}
