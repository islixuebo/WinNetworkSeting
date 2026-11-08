//! 配置类小对话框：改名 / 网卡配置 / 添加路由。
//! 控件全部运行时创建（与 detail.rs 同套路），模板只提供空壳与标题栏。
use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetSysColorBrush, SetBkMode, SetTextColor, COLOR_WINDOW,
    HGDIOBJ, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetClientRect, GetDlgItem, GetWindowTextLengthW,
    GetWindowTextW, MessageBoxW, SendMessageW, SetWindowTextW, BS_AUTOCHECKBOX, BS_GROUPBOX,
    BS_PUSHBUTTON, DLGPROC, ES_AUTOHSCROLL, HMENU, MB_ICONWARNING, MB_OK, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC, WM_ERASEBKGND,
    WM_INITDIALOG, WM_SETFONT, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
};

use crate::netinfo::Nic;
use crate::theme;
use crate::ui::ui_font;
use crate::winutil::wide;

pub const IDC_FORM_TEXT: u32 = 4000;
pub const IDC_FORM_TEXT2: u32 = 4001;
pub const IDC_FORM_TEXT3: u32 = 4002;
pub const IDC_FORM_TEXT4: u32 = 4003;
pub const IDC_FORM_TEXT5: u32 = 4004;

// 与 res/resource.h 一致。对话框模板只用数字 ID，Rust 侧按模块各持一份（同 detail.rs 的 IDD_DETAIL）。
const IDD_RENAME: u32 = 102;

#[allow(clippy::too_many_arguments)]
unsafe fn create(
    parent: HWND,
    class: &str,
    text: &str,
    style: WINDOW_STYLE,
    x: i32,
    y: i32,
    w: i32,
    h: i32,
    id: u32,
) -> HWND {
    let cls = wide(class);
    let t = wide(text);
    let Ok(hmodule) = GetModuleHandleW(None) else {
        return HWND::default();
    };
    let hwnd = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(cls.as_ptr()),
        PCWSTR(t.as_ptr()),
        style,
        x,
        y,
        w,
        h,
        Some(parent),
        if id == 0 {
            None
        } else {
            Some(HMENU(id as usize as *mut core::ffi::c_void))
        },
        Some(HINSTANCE(hmodule.0)),
        None,
    )
    .unwrap_or_default();
    if !hwnd.is_invalid() {
        SendMessageW(
            hwnd,
            WM_SETFONT,
            Some(WPARAM(ui_font().0 as usize)),
            Some(LPARAM(1)),
        );
    }
    hwnd
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框。
pub unsafe fn add_label(parent: HWND, x: i32, y: i32, w: i32, text: &str) -> HWND {
    create(
        parent,
        "STATIC",
        text,
        WS_CHILD | WS_VISIBLE,
        x,
        y,
        w,
        18,
        0,
    )
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框；`id` 在同一个对话框内不能重复。
pub unsafe fn add_edit(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) -> HWND {
    create(
        parent,
        "EDIT",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(ES_AUTOHSCROLL as u32),
        x,
        y,
        w,
        22,
        id,
    )
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框；`id` 在同一个对话框内不能重复。
pub unsafe fn add_button(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) -> HWND {
    create(
        parent,
        "BUTTON",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        x,
        y,
        w,
        26,
        id,
    )
}

/// # Safety
/// `hwnd` 必须是一个存活对话框；`id` 对应的控件不存在时返回空串。
pub unsafe fn read_edit(hwnd: HWND, id: u32) -> String {
    let h = GetDlgItem(Some(hwnd), id as i32).unwrap_or_default();
    if h.is_invalid() {
        return String::new();
    }
    let n = GetWindowTextLengthW(h);
    if n <= 0 {
        return String::new();
    }
    let mut buf = vec![0u16; n as usize + 1];
    let got = GetWindowTextW(h, &mut buf);
    String::from_utf16_lossy(&buf[..got as usize])
}

/// # Safety
/// `hwnd` 必须是一个存活对话框；`id` 对应的控件不存在时静默忽略。
pub unsafe fn edit_text(hwnd: HWND, id: u32, text: &str) {
    if let Ok(h) = GetDlgItem(Some(hwnd), id as i32) {
        let t = wide(text);
        let _ = SetWindowTextW(h, PCWSTR(t.as_ptr()));
    }
}

/// 配置类对话框统一白底：擦除阶段直接填白，控件区的 STATIC 用系统窗口底色 + 透明文字背景，
/// 既不闪灰也不会出现灰底白块拼贴。
fn fill_background(hwnd: HWND, wparam: WPARAM) {
    unsafe {
        let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
        let mut rc = RECT::default();
        if GetClientRect(hwnd, &mut rc).is_ok() {
            let brush = CreateSolidBrush(theme::BG);
            FillRect(hdc, &rc, brush);
            let _ = DeleteObject(HGDIOBJ(brush.0));
        }
    }
}

thread_local! {
    static RENAME: RefCell<(String, String)> = const { RefCell::new((String::new(), String::new())) };
    static CFG: RefCell<(u32, Option<NicConfigEdit>)> = const { RefCell::new((0, None)) };
}

/// 弹出改名框；返回用户确认后的新名字（未改动或取消都返回 `None`）。
pub fn rename_dialog(parent: HWND, nic: &Nic) -> Option<String> {
    RENAME.with(|r| *r.borrow_mut() = (nic.alias.clone(), String::new()));
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return None;
        };
        let dlg: DLGPROC = Some(rename_proc);
        let r = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            crate::ui::resource_id(IDD_RENAME),
            Some(parent),
            dlg,
            LPARAM(0),
        );
        if r == 0 {
            None
        } else {
            RENAME.with(|x| {
                let name = x.borrow().1.clone();
                if name.is_empty() {
                    None
                } else {
                    Some(name)
                }
            })
        }
    }
}

unsafe extern "system" fn rename_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _l: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            let current = RENAME.with(|x| x.borrow().0.clone());
            add_label(hwnd, 16, 18, 360, "新的网卡名称：");
            add_edit(hwnd, 16, 40, 360, &current, IDC_FORM_TEXT);
            add_button(hwnd, 232, 80, 68, "确定", 1);
            add_button(hwnd, 308, 80, 68, "取消", 2);
            1
        }
        WM_ERASEBKGND => {
            fill_background(hwnd, wparam);
            1
        }
        WM_CTLCOLORSTATIC => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, theme::TEXT);
            GetSysColorBrush(COLOR_WINDOW).0 as isize
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u32;
            if id == 1 {
                let raw = read_edit(hwnd, IDC_FORM_TEXT);
                match crate::validate::adapter_name(&raw) {
                    Ok(name) => {
                        let current = RENAME.with(|x| x.borrow().0.clone());
                        // 没改动就当取消，省掉一次无意义的 netsh/CIM 往返
                        let changed = if name == current { String::new() } else { name };
                        let confirm = !changed.is_empty();
                        RENAME.with(|x| x.borrow_mut().1 = changed);
                        let _ = EndDialog(hwnd, if confirm { 1 } else { 0 });
                    }
                    Err(e) => {
                        // 子对话框里没有状态栏，错误只走 MessageBox
                        let body = wide(&e);
                        let title = wide("名称无效");
                        MessageBoxW(
                            Some(hwnd),
                            PCWSTR(body.as_ptr()),
                            PCWSTR(title.as_ptr()),
                            MB_OK | MB_ICONWARNING,
                        );
                    }
                }
                return 1;
            }
            if id == 2 {
                let _ = EndDialog(hwnd, 0);
                return 1;
            }
            0
        }
        WM_CLOSE => {
            let _ = EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}

// ---------------- 网卡配置对话框（名称 + 跃点 + IP/DNS） ----------------

// 与 res/resource.h 一致
const IDD_NIC_CFG: u32 = 103;
const CFG_NAME: u32 = 4100;
const CFG_METRIC_AUTO: u32 = 4101;
const CFG_METRIC: u32 = 4102;
const CFG_DHCP: u32 = 4103;
const CFG_IP: u32 = 4104;
const CFG_MASK: u32 = 4105;
const CFG_GW: u32 = 4106;
const CFG_DNS: u32 = 4107;

const BM_GETCHECK: u32 = 0x00F0;
const BM_SETCHECK: u32 = 0x00F1;
const BST_CHECKED: usize = 1;

#[derive(Clone)]
pub struct NicConfigEdit {
    pub name: String,
    pub metric: Option<u32>,
    pub ip: crate::netctl::IpConfig,
}

/// 跃点输入：自动勾选时忽略输入框。
pub fn parse_metric_field(text: &str, automatic: bool) -> Result<Option<u32>, String> {
    if automatic {
        return Ok(None);
    }
    let t = text.trim();
    let v: u32 = t.parse().map_err(|_| "跃点必须是正整数".to_string())?;
    if v == 0 {
        return Err("跃点不能为 0（0 由系统保留）".into());
    }
    if v > 9999 {
        return Err("跃点不能超过 9999".into());
    }
    Ok(Some(v))
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框。
unsafe fn add_check(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) -> HWND {
    create(
        parent,
        "BUTTON",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE(BS_AUTOCHECKBOX as u32),
        x,
        y,
        w,
        20,
        id,
    )
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框。
unsafe fn add_group(parent: HWND, x: i32, y: i32, w: i32, h: i32, text: &str) -> HWND {
    create(
        parent,
        "BUTTON",
        text,
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_GROUPBOX as u32),
        x,
        y,
        w,
        h,
        0,
    )
}

unsafe fn is_checked(hwnd: HWND, id: u32) -> bool {
    let Ok(h) = GetDlgItem(Some(hwnd), id as i32) else {
        return false;
    };
    SendMessageW(h, BM_GETCHECK, None, None).0 as usize == BST_CHECKED
}

unsafe fn set_checked(hwnd: HWND, id: u32, on: bool) {
    if let Ok(h) = GetDlgItem(Some(hwnd), id as i32) {
        let state = if on { BST_CHECKED } else { 0 };
        SendMessageW(h, BM_SETCHECK, Some(WPARAM(state)), None);
    }
}

unsafe fn enable_ip_fields(hwnd: HWND, enabled: bool) {
    for id in [CFG_IP, CFG_MASK, CFG_GW, CFG_DNS] {
        if let Ok(h) = GetDlgItem(Some(hwnd), id as i32) {
            let _ = EnableWindow(h, enabled);
        }
    }
}

/// 弹出网卡配置框；返回用户确认后的完整修改（取消返回 `None`）。
pub fn nic_config_dialog(parent: HWND, nic: &Nic) -> Option<NicConfigEdit> {
    let use_dhcp = crate::netctl::dhcp_enabled(nic.if_index).unwrap_or(false);
    let mask = crate::validate::parse_mask(&nic.mask).unwrap_or(24);
    let prefill = NicConfigEdit {
        name: nic.alias.clone(),
        metric: if nic.metric_auto {
            None
        } else {
            Some(nic.metric)
        },
        ip: crate::netctl::IpConfig {
            if_index: nic.if_index,
            use_dhcp,
            ip: nic.ipv4.clone(),
            mask,
            gw: nic.gateway.clone(),
            dns: nic.dns.clone(),
        },
    };
    CFG.with(|c| *c.borrow_mut() = (nic.if_index, Some(prefill)));
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return None;
        };
        let dlg: DLGPROC = Some(nic_cfg_proc);
        let r = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            crate::ui::resource_id(IDD_NIC_CFG),
            Some(parent),
            dlg,
            LPARAM(0),
        );
        if r == 0 {
            None
        } else {
            CFG.with(|c| c.borrow().1.clone())
        }
    }
}

unsafe fn build_cfg_controls(hwnd: HWND, cfg: &NicConfigEdit) {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let cx = (rc.right - rc.left).max(360);
    let cy = (rc.bottom - rc.top).max(260);
    let margin = 12;
    let edit_x = margin + 68;
    let edit_w = (cx - edit_x - margin).max(120);

    let g1_y = 8;
    add_group(hwnd, margin, g1_y, cx - margin * 2, 94, "名称与跃点");
    add_label(hwnd, 24, g1_y + 24, 48, "名称");
    add_edit(hwnd, edit_x, g1_y + 20, edit_w, &cfg.name, CFG_NAME);
    add_check(hwnd, 24, g1_y + 56, 88, "自动跃点", CFG_METRIC_AUTO);
    add_label(hwnd, 120, g1_y + 60, 40, "跃点");
    let metric_text = cfg.metric.map(|m| m.to_string()).unwrap_or_default();
    add_edit(hwnd, 160, g1_y + 54, 80, &metric_text, CFG_METRIC);
    set_checked(hwnd, CFG_METRIC_AUTO, cfg.metric.is_none());

    let g2_y = g1_y + 102;
    let buttons_y = cy - 40;
    let g2_h = (buttons_y - 10 - g2_y).max(150);
    add_group(hwnd, margin, g2_y, cx - margin * 2, g2_h, "IP 设置");
    add_check(hwnd, 24, g2_y + 20, 160, "自动获取 IP（DHCP）", CFG_DHCP);
    set_checked(hwnd, CFG_DHCP, cfg.ip.use_dhcp);
    add_label(hwnd, 24, g2_y + 56, 48, "IP 地址");
    add_edit(hwnd, edit_x, g2_y + 52, 140, &cfg.ip.ip, CFG_IP);
    add_label(hwnd, 236, g2_y + 56, 40, "掩码");
    add_edit(hwnd, 280, g2_y + 52, 70, &cfg.ip.mask.to_string(), CFG_MASK);
    add_label(hwnd, 24, g2_y + 86, 48, "网关");
    add_edit(hwnd, edit_x, g2_y + 82, 140, &cfg.ip.gw, CFG_GW);
    add_label(hwnd, 24, g2_y + 116, 48, "DNS");
    add_edit(hwnd, edit_x, g2_y + 112, edit_w, &cfg.ip.dns.join(","), CFG_DNS);
    enable_ip_fields(hwnd, !cfg.ip.use_dhcp);

    add_button(hwnd, cx - margin - 68 - 8 - 68, buttons_y, 68, "确定", 1);
    add_button(hwnd, cx - margin - 68, buttons_y, 68, "取消", 2);
}

unsafe fn show_errors(hwnd: HWND, errors: &[String]) {
    let body = wide(&errors.join("\n"));
    let title = wide("输入有误");
    MessageBoxW(
        Some(hwnd),
        PCWSTR(body.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_OK | MB_ICONWARNING,
    );
}

/// 收集并校验表单；通过时写入 `CFG` 并结束对话框。
unsafe fn accept_cfg(hwnd: HWND) -> isize {
    let mut errors = Vec::new();
    let name = match crate::validate::adapter_name(&read_edit(hwnd, CFG_NAME)) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            String::new()
        }
    };
    let auto = is_checked(hwnd, CFG_METRIC_AUTO);
    let metric = match parse_metric_field(&read_edit(hwnd, CFG_METRIC), auto) {
        Ok(v) => v,
        Err(e) => {
            errors.push(e);
            None
        }
    };
    let use_dhcp = is_checked(hwnd, CFG_DHCP);
    let (ip, mask, gw, dns) = if use_dhcp {
        (String::new(), 0u8, String::new(), Vec::new())
    } else {
        let ip = match crate::validate::parse_ipv4(&read_edit(hwnd, CFG_IP)) {
            Some(a) => a.to_string(),
            None => {
                errors.push("IP 地址不合法".into());
                String::new()
            }
        };
        let mask = match crate::validate::parse_mask(&read_edit(hwnd, CFG_MASK)) {
            Some(m) => m,
            None => {
                errors.push("子网掩码不合法".into());
                0
            }
        };
        let gw = read_edit(hwnd, CFG_GW).trim().to_string();
        if !gw.is_empty() && crate::validate::parse_ipv4(&gw).is_none() {
            errors.push(format!("网关 {gw} 不是合法的 IPv4 地址"));
        }
        let dns = match crate::validate::dns_servers(&read_edit(hwnd, CFG_DNS)) {
            Ok(v) => v,
            Err(e) => {
                errors.push(e);
                Vec::new()
            }
        };
        (ip, mask, gw, dns)
    };
    if !errors.is_empty() {
        show_errors(hwnd, &errors);
        return 1;
    }
    CFG.with(|c| {
        let mut b = c.borrow_mut();
        let if_index = b.0;
        b.1 = Some(NicConfigEdit {
            name,
            metric,
            ip: crate::netctl::IpConfig {
                if_index,
                use_dhcp,
                ip,
                mask,
                gw,
                dns,
            },
        });
    });
    let _ = EndDialog(hwnd, 1);
    1
}

unsafe extern "system" fn nic_cfg_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _l: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            let prefill = CFG.with(|c| c.borrow().1.clone());
            match prefill {
                Some(cfg) => build_cfg_controls(hwnd, &cfg),
                None => {
                    let _ = EndDialog(hwnd, 0);
                }
            }
            1
        }
        WM_ERASEBKGND => {
            fill_background(hwnd, wparam);
            1
        }
        WM_CTLCOLORSTATIC | WM_CTLCOLORBTN => {
            let hdc = windows::Win32::Graphics::Gdi::HDC(wparam.0 as *mut core::ffi::c_void);
            SetBkMode(hdc, TRANSPARENT);
            SetTextColor(hdc, theme::TEXT);
            GetSysColorBrush(COLOR_WINDOW).0 as isize
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u32;
            let code = (wparam.0 >> 16) as u32;
            if id == CFG_DHCP && code == 0 {
                let on = is_checked(hwnd, CFG_DHCP);
                enable_ip_fields(hwnd, !on);
                return 1;
            }
            if id == 1 {
                return accept_cfg(hwnd);
            }
            if id == 2 {
                let _ = EndDialog(hwnd, 0);
                return 1;
            }
            0
        }
        WM_CLOSE => {
            let _ = EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn metric_field_parses_auto_and_numbers() {
        assert_eq!(parse_metric_field("", true), Ok(None));
        assert_eq!(parse_metric_field("300", false), Ok(Some(300)));
        assert_eq!(parse_metric_field(" 1 ", false), Ok(Some(1)));
        assert!(parse_metric_field("0", false).is_err(), "跃点 0 由系统保留");
        assert!(parse_metric_field("abc", false).is_err());
        assert!(parse_metric_field("999999999", false).is_err(), "超出 u32 要报错");
    }
}
