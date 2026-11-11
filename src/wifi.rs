//! WLAN 数据层：已保存配置（含明文密码）与附近扫描到的热点。
//!
//! 统一走 Windows WLAN API，不解析 netsh 的本地化文本：SSID/认证/加密是结构化字段，
//! 密码取自 `WlanGetProfile(..., WLAN_PROFILE_GET_PLAINTEXT_KEY)` 返回的配置 XML。
use std::collections::HashSet;
use std::ffi::c_void;
use std::ptr::null_mut;

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::Win32::Foundation::HANDLE;
use windows::Win32::NetworkManagement::WiFi::{
    WlanCloseHandle, WlanEnumInterfaces, WlanFreeMemory, WlanGetAvailableNetworkList,
    WlanGetProfile, WlanGetProfileList, WlanOpenHandle, WlanScan, WLAN_AVAILABLE_NETWORK,
    WLAN_AVAILABLE_NETWORK_LIST, WLAN_INTERFACE_INFO_LIST, WLAN_PROFILE_GET_PLAINTEXT_KEY,
    WLAN_PROFILE_INFO_LIST,
};

use crate::winutil::wide;

/// 一行 WLAN。已保存与附近同 SSID 时合并成一行，`signal` 只在扫描到时才有值。
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Wlan {
    pub ssid: String,
    pub saved: bool,
    pub signal: Option<u32>,
    pub security: String,
    /// `Some("")` = 开放网络（确实没有密码）；`None` = 拿不到明文（企业认证等）。
    pub password: Option<String>,
}

pub fn type_text(saved: bool) -> &'static str {
    if saved {
        "已保存"
    } else {
        "附近"
    }
}

pub fn signal_level(quality: u32) -> &'static str {
    match quality {
        80..=u32::MAX => "优",
        60..=79 => "良",
        40..=59 => "中",
        20..=39 => "弱",
        _ => "差",
    }
}

pub fn signal_text(signal: Option<u32>) -> String {
    match signal {
        Some(q) => format!("{}%（{}）", q.min(100), signal_level(q.min(100))),
        None => "—".to_string(),
    }
}

/// 信号图标点亮格数（0–5）；未知信号全灰。
pub fn signal_bars(signal: Option<u32>) -> u32 {
    match signal {
        Some(q) => (q.min(100) + 19) / 20,
        None => 0,
    }
}

pub fn password_text(w: &Wlan) -> String {
    if !w.saved {
        return "未保存，无密码".to_string();
    }
    match w.password.as_deref() {
        Some("") => "（开放网络，无密码）".to_string(),
        Some(p) => p.to_string(),
        None => "（无明文密码，可能使用企业认证或证书）".to_string(),
    }
}

/// 能否复制明文密码：已保存且有非空明文（开放网络、企业认证不行）。
pub fn password_copyable(w: &Wlan) -> bool {
    w.saved && matches!(w.password.as_deref(), Some(p) if !p.is_empty())
}

/// 密码区下方的操作提示，与密码状态一一对应。
pub fn password_hint(w: &Wlan) -> &'static str {
    if password_copyable(w) {
        "点「复制密码」，或选中后按 Ctrl+C"
    } else if !w.saved {
        "附近网络未保存，无密码"
    } else if matches!(w.password.as_deref(), Some("")) {
        "开放网络，无需密码"
    } else {
        "企业认证网络，无明文密码"
    }
}

pub fn auth_name(code: i32) -> &'static str {
    match code {
        1 => "开放",
        2 => "共享密钥",
        3 => "WPA 企业",
        4 => "WPA-PSK",
        5 => "WPA-None",
        6 => "WPA2 企业",
        7 => "WPA2-PSK",
        8 => "WPA3 企业",
        9 => "WPA3-SAE",
        10 => "OWE",
        11 => "WPA3 企业",
        _ => "其他",
    }
}

pub fn cipher_name(code: i32) -> &'static str {
    match code {
        0 => "无",
        1 | 5 | 257 => "WEP",
        2 => "TKIP",
        4 => "CCMP",
        6 => "BIP",
        8 => "GCMP",
        9 => "GCMP-256",
        10 => "CCMP-256",
        _ => "其他",
    }
}

pub fn security_text(enabled: bool, auth: i32, cipher: i32) -> String {
    if !enabled {
        return "开放（无加密）".to_string();
    }
    format!("{} / {}", auth_name(auth), cipher_name(cipher))
}

/// 取 XML 中第一个 `<tag>…</tag>` 的文本并反转义。找不到返回 `None`。
pub fn xml_field(xml: &str, tag: &str) -> Option<String> {
    let open = format!("<{tag}>");
    let close = format!("</{tag}>");
    let start = xml.find(&open)? + open.len();
    let end = xml[start..].find(&close)? + start;
    Some(xml_unescape(&xml[start..end]))
}

/// 配置 XML 里的密码是转义后的文本：`&amp;` 之类的实体必须还原，否则复制出去是错的。
pub fn xml_unescape(text: &str) -> String {
    let mut out = String::with_capacity(text.len());
    let mut rest = text;
    while let Some(pos) = rest.find('&') {
        out.push_str(&rest[..pos]);
        rest = &rest[pos..];
        let Some(semi) = rest.find(';') else {
            out.push_str(rest);
            return out;
        };
        let entity = &rest[1..semi];
        let decoded = match entity {
            "amp" => Some('&'),
            "lt" => Some('<'),
            "gt" => Some('>'),
            "quot" => Some('"'),
            "apos" => Some('\''),
            _ => {
                let ch = entity
                    .strip_prefix("#x")
                    .or_else(|| entity.strip_prefix("#X"))
                    .and_then(|h| u32::from_str_radix(h, 16).ok())
                    .or_else(|| entity.strip_prefix('#').and_then(|d| d.parse().ok()))
                    .and_then(char::from_u32);
                ch
            }
        };
        match decoded {
            Some(c) => {
                out.push(c);
                rest = &rest[semi + 1..];
            }
            // 不认识的实体原样保留，别把用户密码里的 & 吃掉
            None => {
                out.push('&');
                rest = &rest[1..];
            }
        }
    }
    out.push_str(rest);
    out
}

pub fn key_material(xml: &str) -> Option<String> {
    xml_field(xml, "keyMaterial")
}

fn profile_auth_name(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "" => "未知".to_string(),
        "open" => "开放".to_string(),
        "shared" => "共享密钥".to_string(),
        "wpa" => "WPA 企业".to_string(),
        "wpapsk" => "WPA-PSK".to_string(),
        "wpanone" => "WPA-None".to_string(),
        "wpa2" => "WPA2 企业".to_string(),
        "wpa2psk" => "WPA2-PSK".to_string(),
        "wpa3" => "WPA3 企业".to_string(),
        "wpa3sae" => "WPA3-SAE".to_string(),
        "wpa3ent" | "wpa3ent192" => "WPA3 企业".to_string(),
        "owe" => "OWE".to_string(),
        other => other.to_uppercase(),
    }
}

fn profile_cipher_name(raw: &str) -> String {
    match raw.to_ascii_lowercase().as_str() {
        "" | "none" => "无".to_string(),
        "aes" => "AES".to_string(),
        "tkip" => "TKIP".to_string(),
        "wep" => "WEP".to_string(),
        "gcmp" => "GCMP".to_string(),
        "gcmp-256" => "GCMP-256".to_string(),
        "ccmp-256" => "CCMP-256".to_string(),
        other => other.to_uppercase(),
    }
}

/// 已保存配置的认证/加密来自配置 XML，比可用列表里的默认算法更贴近用户设置。
pub fn profile_security(xml: &str) -> String {
    let auth = xml_field(xml, "authentication").unwrap_or_default();
    let enc = xml_field(xml, "encryption").unwrap_or_default();
    if auth.is_empty() && enc.is_empty() {
        return "未知".to_string();
    }
    let a = profile_auth_name(&auth);
    if enc.is_empty() || enc.eq_ignore_ascii_case("none") {
        return format!("{a}（无加密）");
    }
    format!("{a} / {}", profile_cipher_name(&enc))
}

/// 配置 XML → 明文密码。开放网络返回 `Some("")`；企业认证没有 keyMaterial，返回 `None`。
pub fn password_from_profile(xml: &str) -> Option<String> {
    let key = key_material(xml).unwrap_or_default();
    if !key.is_empty() {
        return Some(key);
    }
    let auth = xml_field(xml, "authentication").unwrap_or_default();
    if auth.eq_ignore_ascii_case("open") || auth.eq_ignore_ascii_case("owe") {
        Some(String::new())
    } else {
        None
    }
}

/// 合并已保存与附近：同 SSID 只留一行，已保存优先并补上扫描到的信号。
pub fn merge_networks(saved: Vec<Wlan>, nearby: Vec<Wlan>) -> Vec<Wlan> {
    let mut nearby_rows: Vec<Wlan> = Vec::new();
    for n in nearby {
        let Some(q) = n.signal else { continue };
        match nearby_rows.iter_mut().find(|r| r.ssid == n.ssid) {
            Some(r) => {
                if q > r.signal.unwrap_or(0) {
                    r.signal = Some(q);
                }
            }
            None => nearby_rows.push(n),
        }
    }
    let best: Vec<(String, u32)> = nearby_rows
        .iter()
        .map(|r| (r.ssid.clone(), r.signal.unwrap_or(0)))
        .collect();

    let mut rows: Vec<Wlan> = Vec::new();
    let mut saved_seen: HashSet<String> = HashSet::new();
    for mut s in saved {
        if s.ssid.is_empty() || !saved_seen.insert(s.ssid.clone()) {
            continue;
        }
        if let Some((_, q)) = best.iter().find(|(ssid, _)| ssid == &s.ssid) {
            s.signal = Some(*q);
        }
        rows.push(s);
    }
    rows.sort_by(|a, b| a.ssid.cmp(&b.ssid));

    let mut extra: Vec<Wlan> = nearby_rows
        .into_iter()
        .filter(|n| !saved_seen.contains(&n.ssid) && !n.ssid.is_empty())
        .collect();
    extra.sort_by(|a, b| {
        b.signal
            .unwrap_or(0)
            .cmp(&a.signal.unwrap_or(0))
            .then_with(|| a.ssid.cmp(&b.ssid))
    });
    rows.extend(extra);
    rows
}

fn utf16_field(buf: &[u16]) -> String {
    let len = buf.iter().position(|c| *c == 0).unwrap_or(buf.len());
    String::from_utf16_lossy(&buf[..len])
}

pub fn wlan_error_text(op: &str, code: u32) -> String {
    let hint = match code {
        5 => "（拒绝访问，需要管理员权限）",
        1062 => "（WLAN AutoConfig 服务未启动）",
        1168 => "（未找到无线网卡）",
        _ => "",
    };
    format!("{op}失败：WLAN 错误 {code}{hint}")
}

/// 扫描 + 枚举已保存配置。没有任何无线网卡时返回空列表（不是错误）。
pub fn list_networks() -> Result<Vec<Wlan>, String> {
    unsafe { list_networks_inner() }
}

/// # Safety
/// 只调用 WLAN API 并由本函数自行释放所有由系统分配的缓冲区。
unsafe fn list_networks_inner() -> Result<Vec<Wlan>, String> {
    let mut negotiated = 0u32;
    let mut handle = HANDLE::default();
    let code = WlanOpenHandle(2, None, &mut negotiated, &mut handle);
    if code != 0 {
        return Err(wlan_error_text("打开 WLAN 服务", code));
    }
    let result = collect(handle);
    let _ = WlanCloseHandle(handle, None);
    result
}

/// # Safety
/// `handle` 必须是 `WlanOpenHandle` 返回且尚未关闭的句柄。
unsafe fn collect(handle: HANDLE) -> Result<Vec<Wlan>, String> {
    let mut plist: *mut WLAN_INTERFACE_INFO_LIST = null_mut();
    let code = WlanEnumInterfaces(handle, None, &mut plist);
    if code != 0 || plist.is_null() {
        return Err(wlan_error_text("枚举无线网卡", code));
    }
    let count = (*plist).dwNumberOfItems as usize;
    let guids: Vec<GUID> = std::slice::from_raw_parts((*plist).InterfaceInfo.as_ptr(), count)
        .iter()
        .map(|i| i.InterfaceGuid)
        .collect();
    WlanFreeMemory(plist as *const c_void);

    let mut saved = Vec::new();
    let mut nearby = Vec::new();
    for guid in guids {
        // 扫描是异步的，失败（如接口未就绪）不影响读取现有列表
        let _ = WlanScan(handle, &guid, None, None, None);
        collect_available(handle, &guid, &mut nearby);
        collect_profiles(handle, &guid, &mut saved);
    }
    Ok(merge_networks(saved, nearby))
}

/// # Safety
/// `handle` 必须是有效的 WLAN 客户端句柄；`guid` 指向该句柄枚举出的接口。
unsafe fn collect_available(handle: HANDLE, guid: &GUID, out: &mut Vec<Wlan>) {
    let mut plist: *mut WLAN_AVAILABLE_NETWORK_LIST = null_mut();
    if WlanGetAvailableNetworkList(handle, guid, 0, None, &mut plist) != 0 || plist.is_null() {
        return;
    }
    let count = (*plist).dwNumberOfItems as usize;
    let items: &[WLAN_AVAILABLE_NETWORK] =
        std::slice::from_raw_parts((*plist).Network.as_ptr(), count);
    for n in items {
        let len = (n.dot11Ssid.uSSIDLength as usize).min(n.dot11Ssid.ucSSID.len());
        if len == 0 {
            continue;
        }
        let ssid = String::from_utf8_lossy(&n.dot11Ssid.ucSSID[..len]).to_string();
        out.push(Wlan {
            ssid,
            saved: false,
            signal: Some(n.wlanSignalQuality.min(100)),
            security: security_text(
                n.bSecurityEnabled.as_bool(),
                n.dot11DefaultAuthAlgorithm.0,
                n.dot11DefaultCipherAlgorithm.0,
            ),
            password: None,
        });
    }
    WlanFreeMemory(plist as *const c_void);
}

/// # Safety
/// `handle` 必须是有效的 WLAN 客户端句柄；`guid` 指向该句柄枚举出的接口。
unsafe fn collect_profiles(handle: HANDLE, guid: &GUID, out: &mut Vec<Wlan>) {
    let mut plist: *mut WLAN_PROFILE_INFO_LIST = null_mut();
    if WlanGetProfileList(handle, guid, None, &mut plist) != 0 || plist.is_null() {
        return;
    }
    let count = (*plist).dwNumberOfItems as usize;
    let names: Vec<String> = std::slice::from_raw_parts((*plist).ProfileInfo.as_ptr(), count)
        .iter()
        .map(|p| utf16_field(&p.strProfileName))
        .collect();
    WlanFreeMemory(plist as *const c_void);

    for name in names {
        if name.is_empty() {
            continue;
        }
        let Some(xml) = profile_xml(handle, guid, &name) else {
            continue;
        };
        out.push(Wlan {
            ssid: name,
            saved: true,
            signal: None,
            security: profile_security(&xml),
            password: password_from_profile(&xml),
        });
    }
}

/// # Safety
/// `handle`/`guid` 同 `collect_profiles`；`name` 必须是该接口上的配置名。
unsafe fn profile_xml(handle: HANDLE, guid: &GUID, name: &str) -> Option<String> {
    let wide_name = wide(name);
    let mut flags = WLAN_PROFILE_GET_PLAINTEXT_KEY;
    let mut xml = PWSTR::null();
    let code = WlanGetProfile(
        handle,
        guid,
        PCWSTR(wide_name.as_ptr()),
        None,
        &mut xml,
        Some(&mut flags),
        None,
    );
    if code != 0 || xml.is_null() {
        return None;
    }
    let text = xml.to_string().ok();
    WlanFreeMemory(xml.0 as *const c_void);
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn saved(ssid: &str, password: Option<&str>) -> Wlan {
        Wlan {
            ssid: ssid.into(),
            saved: true,
            signal: None,
            security: "WPA2-PSK / AES".into(),
            password: password.map(|s| s.to_string()),
        }
    }

    fn nearby(ssid: &str, quality: u32) -> Wlan {
        Wlan {
            ssid: ssid.into(),
            saved: false,
            signal: Some(quality),
            security: "WPA2-PSK / CCMP".into(),
            password: None,
        }
    }

    #[test]
    fn xml_field_extracts_and_unescapes() {
        let xml = "<WLANProfile><name>Office &amp; Lab</name><keyMaterial>p&amp;ss&lt;1&gt;</keyMaterial></WLANProfile>";
        assert_eq!(xml_field(xml, "name").as_deref(), Some("Office & Lab"));
        assert_eq!(
            key_material(xml).as_deref(),
            Some("p&ss<1>"),
            "密码里的实体必须还原"
        );
        assert_eq!(xml_field(xml, "missing"), None);
    }

    #[test]
    fn xml_unescape_handles_quotes_numeric_and_unknown_entities() {
        assert_eq!(xml_unescape("&quot;a&quot;&apos;b&apos;"), "\"a\"'b'");
        assert_eq!(xml_unescape("&#65;&#x42;"), "AB");
        // 不认识的实体原样保留，不能吞掉用户密码里的 &
        assert_eq!(xml_unescape("a&bogus;b"), "a&bogus;b");
        assert_eq!(xml_unescape("no entities"), "no entities");
        assert_eq!(xml_unescape("tail &"), "tail &");
    }

    #[test]
    fn key_material_empty_means_open_or_enterprise() {
        let open = "<authentication>open</authentication><encryption>none</encryption>";
        assert_eq!(password_from_profile(open), Some(String::new()));
        let psk = "<authentication>WPA2PSK</authentication><keyMaterial>abc</keyMaterial>";
        assert_eq!(password_from_profile(psk), Some("abc".to_string()));
        let enterprise = "<authentication>WPA2</authentication><encryption>AES</encryption>";
        assert_eq!(password_from_profile(enterprise), None);
    }

    #[test]
    fn profile_security_maps_common_settings() {
        assert_eq!(
            profile_security(
                "<authentication>WPA2PSK</authentication><encryption>AES</encryption>"
            ),
            "WPA2-PSK / AES"
        );
        assert_eq!(
            profile_security(
                "<authentication>WPAPSK</authentication><encryption>TKIP</encryption>"
            ),
            "WPA-PSK / TKIP"
        );
        assert_eq!(
            profile_security(
                "<authentication>WPA3SAE</authentication><encryption>AES</encryption>"
            ),
            "WPA3-SAE / AES"
        );
        assert_eq!(
            profile_security("<authentication>open</authentication><encryption>none</encryption>"),
            "开放（无加密）"
        );
        assert_eq!(profile_security("<nope/>"), "未知");
    }

    #[test]
    fn auth_and_cipher_names_cover_wlanapi_codes() {
        assert_eq!(auth_name(4), "WPA-PSK");
        assert_eq!(auth_name(7), "WPA2-PSK");
        assert_eq!(auth_name(10), "OWE");
        assert_eq!(auth_name(999), "其他");
        assert_eq!(cipher_name(4), "CCMP");
        assert_eq!(cipher_name(257), "WEP");
        assert_eq!(cipher_name(0), "无");
        // 关掉安全时不应出现「开放 / 无」这种双重否定
        assert_eq!(security_text(false, 1, 0), "开放（无加密）");
        assert_eq!(security_text(true, 7, 4), "WPA2-PSK / CCMP");
    }

    #[test]
    fn signal_text_clamps_and_labels() {
        assert_eq!(signal_text(Some(95)), "95%（优）");
        assert_eq!(signal_text(Some(65)), "65%（良）");
        assert_eq!(signal_text(Some(45)), "45%（中）");
        assert_eq!(signal_text(Some(25)), "25%（弱）");
        assert_eq!(signal_text(Some(5)), "5%（差）");
        assert_eq!(signal_text(Some(150)), "100%（优）");
        assert_eq!(signal_text(None), "—");
    }

    #[test]
    fn signal_bars_maps_quality_to_five_steps() {
        assert_eq!(signal_bars(None), 0);
        assert_eq!(signal_bars(Some(0)), 0);
        assert_eq!(signal_bars(Some(1)), 1);
        assert_eq!(signal_bars(Some(20)), 1);
        assert_eq!(signal_bars(Some(21)), 2);
        assert_eq!(signal_bars(Some(60)), 3);
        assert_eq!(signal_bars(Some(100)), 5);
        assert_eq!(signal_bars(Some(150)), 5);
    }

    #[test]
    fn merge_puts_saved_first_and_fills_signal() {
        let rows = merge_networks(
            vec![saved("Office", Some("secret")), saved("Home", Some("pw"))],
            vec![nearby("Office", 88), nearby("Cafe", 40), nearby("Home", 55)],
        );
        assert_eq!(
            rows.iter().map(|r| r.ssid.as_str()).collect::<Vec<_>>(),
            ["Home", "Office", "Cafe"],
            "已保存按 SSID 排序在前，附近按信号降序在后"
        );
        assert_eq!(rows[1].signal, Some(88), "已保存项要补上扫描信号");
        assert_eq!(rows[1].password.as_deref(), Some("secret"));
        assert!(!rows[2].saved);
        assert_eq!(rows[2].signal, Some(40));
    }

    #[test]
    fn merge_dedupes_repeated_ssid_and_drops_hidden() {
        let rows = merge_networks(
            vec![saved("Office", Some("pw")), saved("Office", Some("dup"))],
            vec![
                nearby("Cafe", 30),
                nearby("Cafe", 70),
                Wlan {
                    ssid: String::new(),
                    saved: false,
                    signal: Some(99),
                    security: "开放（无加密）".into(),
                    password: None,
                },
            ],
        );
        assert_eq!(rows.len(), 2, "{rows:?}");
        assert_eq!(rows[0].ssid, "Office");
        assert_eq!(rows[0].password.as_deref(), Some("pw"));
        assert_eq!(rows[1].signal, Some(70), "同一 SSID 只留最强的一行");
    }

    #[test]
    fn password_text_explains_each_state() {
        assert_eq!(password_text(&saved("A", Some("pw"))), "pw");
        assert_eq!(password_text(&saved("A", Some(""))), "（开放网络，无密码）");
        assert_eq!(
            password_text(&saved("A", None)),
            "（无明文密码，可能使用企业认证或证书）"
        );
        assert_eq!(password_text(&nearby("B", 50)), "未保存，无密码");
    }

    #[test]
    fn password_copyable_requires_saved_plaintext() {
        assert!(password_copyable(&saved("A", Some("pw"))));
        assert!(!password_copyable(&saved("A", Some(""))));
        assert!(!password_copyable(&saved("A", None)));
        assert!(!password_copyable(&nearby("B", 50)));
    }

    #[test]
    fn password_hint_guides_each_state() {
        assert_eq!(
            password_hint(&saved("A", Some("pw"))),
            "点「复制密码」，或选中后按 Ctrl+C"
        );
        assert_eq!(password_hint(&saved("A", Some(""))), "开放网络，无需密码");
        assert_eq!(password_hint(&saved("A", None)), "企业认证网络，无明文密码");
        assert_eq!(password_hint(&nearby("B", 50)), "附近网络未保存，无密码");
    }

    #[test]
    fn type_column_distinguishes_saved_from_nearby() {
        assert_eq!(type_text(true), "已保存");
        assert_eq!(type_text(false), "附近");
    }

    #[test]
    fn error_text_has_service_hint() {
        assert!(wlan_error_text("枚举无线网卡", 1062).contains("WLAN AutoConfig"));
        assert!(wlan_error_text("枚举无线网卡", 5).contains("管理员"));
        assert!(wlan_error_text("枚举无线网卡", 9999).contains("9999"));
    }

    #[test]
    fn utf16_field_stops_at_nul() {
        let mut buf = [0u16; 8];
        buf[..3].copy_from_slice(&['a' as u16, 'b' as u16, 'c' as u16]);
        assert_eq!(utf16_field(&buf), "abc");
        assert_eq!(utf16_field(&[0u16; 4]), "");
    }
}
