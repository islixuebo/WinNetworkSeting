//! 输入校验：全部纯函数，UI 与 netctl 共用。
use std::net::Ipv4Addr;

pub fn parse_ipv4(s: &str) -> Option<Ipv4Addr> {
    // std 的解析允许前导零等宽松写法，这里额外要求每段都是十进制数字，避免 "0x10.1.1.1"
    let t = s.trim();
    if t.is_empty() || !t.split('.').all(|p| !p.is_empty() && p.chars().all(|c| c.is_ascii_digit()))
    {
        return None;
    }
    // 每段必须是十进制写法：std 会接受 "0x10"，所以先按数字过滤再交给 std
    t.parse().ok()
}

pub fn parse_mask(s: &str) -> Option<u8> {
    let t = s.trim();
    if t.is_empty() {
        return None;
    }
    if let Some(rest) = t.strip_prefix('/') {
        return rest.trim().parse::<u8>().ok().filter(|n| *n <= 32);
    }
    if !t.contains('.') {
        return t.parse::<u8>().ok().filter(|n| *n <= 32);
    }
    let bits = u32::from(parse_ipv4(t)?);
    let ones = bits.leading_ones();
    // 连续 1 才是合法掩码：取反后必须是「全 0」或「低 N 位全 1」的形状。
    // ones == 32 时整段都是 1，左移 32 位在 debug 下会 panic，先短路。
    if ones == 32 || bits << ones == 0 {
        Some(ones as u8)
    } else {
        None
    }
}

/// 网卡显示名：非空、≤128 字符、不含 Windows 拒绝的文件名式字符。
pub fn adapter_name(s: &str) -> Result<String, String> {
    let t = s.trim();
    if t.is_empty() {
        return Err("名称不能为空".into());
    }
    if t.chars().count() > 128 {
        return Err("名称不能超过 128 个字符".into());
    }
    if let Some(c) = t.chars().find(|c| "\\/:*?\"<>|".contains(*c)) {
        return Err(format!("名称不能包含字符 {c}"));
    }
    Ok(t.to_string())
}

/// DNS 列表：逗号分隔，逐项校验；空输入表示清空。
pub fn dns_servers(s: &str) -> Result<Vec<String>, String> {
    let mut out = Vec::new();
    for part in s.split([',', '，', ' ', '\t', ';']) {
        let p = part.trim();
        if p.is_empty() {
            continue;
        }
        match parse_ipv4(p) {
            Some(a) => out.push(a.to_string()),
            None => return Err(format!("{p} 不是合法的 IPv4 地址")),
        }
    }
    Ok(out)
}

/// 目的前缀：必须是 `地址/长度`，地址会被规范化（10.0.0.5/24 → 10.0.0.0/24）。
pub fn route_prefix(s: &str) -> Result<String, String> {
    let t = s.trim();
    let (addr, len) = t
        .split_once('/')
        .ok_or_else(|| "格式应为 地址/前缀长度，例如 10.0.0.0/24".to_string())?;
    let a = parse_ipv4(addr).ok_or_else(|| format!("{addr} 不是合法的 IPv4 地址"))?;
    let n: u8 = len
        .trim()
        .parse()
        .ok()
        .filter(|n| *n <= 32)
        .ok_or_else(|| "前缀长度应在 0~32 之间".to_string())?;
    let bits = u32::from(a);
    let mask = if n == 0 { 0 } else { u32::MAX << (32 - n) };
    Ok(format!("{}/{}", Ipv4Addr::from(bits & mask), n))
}

pub fn is_default_prefix(s: &str) -> bool {
    matches!(s.trim().split_once('/'), Some(("0.0.0.0", "0")))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ipv4_accepts_only_canonical_dotted_quads() {
        assert_eq!(
            parse_ipv4("192.168.1.10").map(|a| a.to_string()).as_deref(),
            Some("192.168.1.10")
        );
        assert_eq!(
            parse_ipv4(" 10.0.0.2 ").map(|a| a.to_string()).as_deref(),
            Some("10.0.0.2")
        );
        for bad in ["", "192.168.1", "192.168.1.256", "1.2.3.4.5", "abc", "1.2.3.-4"] {
            assert!(parse_ipv4(bad).is_none(), "不该接受 {bad:?}");
        }
    }

    #[test]
    fn mask_accepts_dotted_and_prefix_forms() {
        assert_eq!(parse_mask("255.255.255.0"), Some(24));
        assert_eq!(parse_mask("255.255.0.0"), Some(16));
        assert_eq!(parse_mask("255.255.255.255"), Some(32));
        assert_eq!(parse_mask("24"), Some(24));
        assert_eq!(parse_mask("/24"), Some(24));
        // 非连续掩码是配置错误，必须拒绝而不是猜
        assert_eq!(parse_mask("255.0.255.0"), None);
        assert_eq!(parse_mask("33"), None);
        assert_eq!(parse_mask(""), None);
    }

    #[test]
    fn adapter_name_trims_and_rejects_hostile_input() {
        assert_eq!(adapter_name("  办公网  ").as_deref(), Ok("办公网"));
        for bad in [
            "", "   ", "a/b", "a\\b", "a:b", "a*b", "a?b", "a\"b", "a<b", "a>b", "a|b",
        ] {
            assert!(adapter_name(bad).is_err(), "不该接受 {bad:?}");
        }
        // 长度上限按 Windows 显示名留足余量
        let long = "网".repeat(129);
        assert!(adapter_name(&long).is_err());
    }

    #[test]
    fn dns_list_splits_on_commas_and_validates_each() {
        assert_eq!(
            dns_servers("1.1.1.1, 8.8.8.8").as_deref(),
            Ok(&["1.1.1.1".to_string(), "8.8.8.8".to_string()][..])
        );
        assert_eq!(dns_servers("").as_deref(), Ok(&[][..]), "空串 = 清空 DNS");
        assert!(dns_servers("1.1.1.1, nope").is_err());
        // 最多两个备用服务器是 Windows 界面的通行值，多了不拦，但要能全量下发
        assert_eq!(dns_servers("1.1.1.1,8.8.8.8,9.9.9.9").map(|v| v.len()), Ok(3));
    }

    #[test]
    fn route_prefix_normalizes_and_detects_default() {
        assert_eq!(route_prefix("10.0.0.0/24").as_deref(), Ok("10.0.0.0/24"));
        assert_eq!(
            route_prefix(" 192.168.2.0/24 ").as_deref(),
            Ok("192.168.2.0/24")
        );
        assert!(route_prefix("10.0.0.0/33").is_err());
        assert!(route_prefix("10.0.0.0").is_err(), "缺前缀长度要报错");
        assert!(route_prefix("nope/24").is_err());
        assert!(is_default_prefix("0.0.0.0/0"));
        assert!(!is_default_prefix("0.0.0.1/0"), "只有全零地址才算默认路由");
    }
}
