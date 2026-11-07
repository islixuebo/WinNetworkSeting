// Task 6：网卡启用/禁用。设计指定走 netsh 子进程（瞬时开销，不影响常驻内存）；
// 本机实测 netsh 的 interface 上下文只认「已与路由器一起注册」的 3 个接口名，
// 列表里的 5 条有 4 条（WLAN 2/4/5、蓝牙网络连接）按名字点不到，故补一条按 ifIndex 的 CIM 回退。
use std::process::{Command, Stdio};

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
/// 任何确认提示都会把窗口冻住。前置 UTF-8 输出编码，否则中文 Windows 下的错误行是 GBK，
/// `failure_text` 会判为不可解码而丢掉唯一的原因线索。
pub fn netadapter_args_for(if_index: u32, enable: bool) -> Vec<String> {
    let verb = if enable { "Enable" } else { "Disable" };
    vec![
        "-NoProfile".into(),
        "-NonInteractive".into(),
        "-Command".into(),
        format!(
            "[Console]::OutputEncoding=[System.Text.Encoding]::UTF8; \
             {verb}-NetAdapter -InterfaceIndex {if_index} -Confirm:$False"
        ),
    ]
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
