//! 配置类小对话框：改名 / 网卡配置 / 添加路由。
//! 控件全部运行时创建（与 detail.rs 同套路），模板只提供空壳与标题栏。
use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{GlobalFree, HANDLE, HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateSolidBrush, DeleteObject, FillRect, GetSysColorBrush, SetBkMode, SetTextColor,
    COLOR_WINDOW, HGDIOBJ, TRANSPARENT,
};
use windows::Win32::System::DataExchange::{
    CloseClipboard, EmptyClipboard, OpenClipboard, SetClipboardData,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::CF_UNICODETEXT;
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetClientRect, GetDlgItem, GetWindowTextLengthW,
    GetWindowTextW, MessageBoxW, SendMessageW, SetWindowTextW, BS_AUTOCHECKBOX, BS_GROUPBOX,
    BS_PUSHBUTTON, CBS_DROPDOWNLIST, DLGPROC, ES_AUTOHSCROLL, ES_READONLY, HMENU, MB_ICONWARNING,
    MB_OK, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORBTN, WM_CTLCOLORSTATIC,
    WM_ERASEBKGND, WM_INITDIALOG, WM_SETFONT, WS_CHILD, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

use crate::netinfo::Nic;
use crate::theme;
use crate::ui::ui_font;
use crate::wifi::Wlan;
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
    add_edit(
        hwnd,
        edit_x,
        g2_y + 112,
        edit_w,
        &cfg.ip.dns.join(","),
        CFG_DNS,
    );
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

// ---------------- 添加路由对话框 ----------------

const IDD_ROUTE: u32 = 104;
const ROUTE_COMBO: u32 = 4200;
const ROUTE_PREFIX: u32 = 4201;
const ROUTE_HOP: u32 = 4202;
const ROUTE_METRIC: u32 = 4203;

const CB_ADDSTRING: u32 = 0x0143;
const CB_GETCURSEL: u32 = 0x0147;
const CB_SETCURSEL: u32 = 0x014E;
const CB_GETITEMDATA: u32 = 0x0150;
const CB_SETITEMDATA: u32 = 0x0151;

#[derive(Clone)]
pub struct RouteDraft {
    pub if_index: u32,
    pub prefix: String,
    pub next_hop: String,
    pub metric: u32,
}

/// 路由表单校验；ifIndex 由对话框按下拉选择覆盖（抽出来便于单测）。
pub fn parse_route_fields(
    prefix: &str,
    next_hop: &str,
    metric: &str,
) -> Result<RouteDraft, String> {
    let prefix = crate::validate::route_prefix(prefix)?;
    let hop = crate::validate::parse_ipv4(next_hop)
        .ok_or_else(|| "下一跳不是合法的 IPv4 地址".to_string())?;
    let metric: u32 = metric
        .trim()
        .parse()
        .map_err(|_| "跃点必须是正整数".to_string())?;
    Ok(RouteDraft {
        if_index: 0,
        prefix,
        next_hop: hop.to_string(),
        metric,
    })
}

type RouteState = (Vec<(String, u32)>, Option<RouteDraft>);

thread_local! {
    static ROUTE: RefCell<RouteState> = const { RefCell::new((Vec::new(), None)) };
}

/// 弹出「添加路由」；取消返回 `None`。
pub fn route_dialog(parent: HWND, nics: &[Nic]) -> Option<RouteDraft> {
    let entries: Vec<(String, u32)> = nics
        .iter()
        .map(|n| (format!("{} ({})", n.alias, n.if_index), n.if_index))
        .collect();
    ROUTE.with(|r| *r.borrow_mut() = (entries, None));
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return None;
        };
        let dlg: DLGPROC = Some(route_proc);
        let r = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            crate::ui::resource_id(IDD_ROUTE),
            Some(parent),
            dlg,
            LPARAM(0),
        );
        if r == 0 {
            None
        } else {
            ROUTE.with(|x| x.borrow().1.clone())
        }
    }
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框。
unsafe fn add_combo(parent: HWND, x: i32, y: i32, w: i32, h: i32, id: u32) -> HWND {
    create(
        parent,
        "COMBOBOX",
        "",
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WS_VSCROLL | WINDOW_STYLE(CBS_DROPDOWNLIST as u32),
        x,
        y,
        w,
        h,
        id,
    )
}

unsafe fn build_route_controls(hwnd: HWND) {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let cx = (rc.right - rc.left).max(360);
    let cy = (rc.bottom - rc.top).max(220);
    let margin = 12;
    let edit_x = margin + 76;
    let edit_w = (cx - edit_x - margin).max(150);

    add_label(hwnd, margin, 18, 64, "接口");
    let combo = add_combo(hwnd, edit_x, 16, edit_w, 200, ROUTE_COMBO);
    let entries = ROUTE.with(|r| r.borrow().0.clone());
    for (label, if_index) in entries.iter() {
        let t = wide(label);
        let idx =
            SendMessageW(combo, CB_ADDSTRING, None, Some(LPARAM(t.as_ptr() as isize))).0 as i32;
        if idx >= 0 {
            SendMessageW(
                combo,
                CB_SETITEMDATA,
                Some(WPARAM(idx as usize)),
                Some(LPARAM(*if_index as isize)),
            );
        }
    }
    if !entries.is_empty() {
        SendMessageW(combo, CB_SETCURSEL, Some(WPARAM(0)), None);
    }

    add_label(hwnd, margin, 56, 64, "目标前缀");
    add_edit(hwnd, edit_x, 52, edit_w, "", ROUTE_PREFIX);
    add_label(hwnd, margin, 88, 64, "下一跳");
    add_edit(hwnd, edit_x, 84, edit_w, "", ROUTE_HOP);
    add_label(hwnd, margin, 120, 64, "跃点");
    add_edit(hwnd, edit_x, 116, 120, "0", ROUTE_METRIC);

    let buttons_y = cy - 40;
    add_button(hwnd, cx - margin - 68 - 8 - 68, buttons_y, 68, "确定", 1);
    add_button(hwnd, cx - margin - 68, buttons_y, 68, "取消", 2);
}

unsafe fn accept_route(hwnd: HWND) -> isize {
    let mut errors = Vec::new();
    let combo = GetDlgItem(Some(hwnd), ROUTE_COMBO as i32).unwrap_or_default();
    let sel = SendMessageW(combo, CB_GETCURSEL, None, None).0 as i32;
    let if_index = if sel < 0 {
        errors.push("请选择接口".to_string());
        0
    } else {
        SendMessageW(combo, CB_GETITEMDATA, Some(WPARAM(sel as usize)), None).0 as u32
    };
    let mut draft = match parse_route_fields(
        &read_edit(hwnd, ROUTE_PREFIX),
        &read_edit(hwnd, ROUTE_HOP),
        &read_edit(hwnd, ROUTE_METRIC),
    ) {
        Ok(d) => Some(d),
        Err(e) => {
            errors.push(e);
            None
        }
    };
    if !errors.is_empty() {
        show_errors(hwnd, &errors);
        return 1;
    }
    if let Some(d) = draft.as_mut() {
        d.if_index = if_index;
    }
    ROUTE.with(|r| r.borrow_mut().1 = draft);
    let _ = EndDialog(hwnd, 1);
    1
}

unsafe extern "system" fn route_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _l: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            build_route_controls(hwnd);
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
            match id {
                1 => accept_route(hwnd),
                2 => {
                    let _ = EndDialog(hwnd, 0);
                    1
                }
                _ => 0,
            }
        }
        WM_CLOSE => {
            let _ = EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}

// ---------------- Wi-Fi 详情对话框 ----------------

const IDD_WLAN: u32 = 105;
const WLAN_PASSWORD: u32 = 4300;
const WLAN_COPY: u32 = 4301;
const WLAN_HINT: u32 = 4302;

thread_local! {
    static WLAN: RefCell<Option<Wlan>> = const { RefCell::new(None) };
}

/// 弹出 Wi-Fi 详情：已保存网络显示密码并可复制，附近网络只显示概况。
pub fn wlan_detail_dialog(parent: HWND, w: &Wlan) {
    WLAN.with(|s| *s.borrow_mut() = Some(w.clone()));
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return;
        };
        let dlg: DLGPROC = Some(wlan_proc);
        let _ = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            crate::ui::resource_id(IDD_WLAN),
            Some(parent),
            dlg,
            LPARAM(0),
        );
    }
}

/// # Safety
/// `parent` 必须是当前线程上存活、已创建的对话框；`id` 在同一个对话框内不能重复。
unsafe fn add_readonly_edit(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) -> HWND {
    create(
        parent,
        "EDIT",
        text,
        WS_CHILD | WS_VISIBLE | WS_TABSTOP | WINDOW_STYLE((ES_AUTOHSCROLL | ES_READONLY) as u32),
        x,
        y,
        w,
        22,
        id,
    )
}

/// 写剪贴板：调用方保证 `text` 是用户明确要复制的密码。
unsafe fn copy_to_clipboard(hwnd: HWND, text: &str) -> bool {
    if OpenClipboard(Some(hwnd)).is_err() {
        return false;
    }
    let mut ok = false;
    if EmptyClipboard().is_ok() {
        let w = wide(text);
        let bytes = w.len() * std::mem::size_of::<u16>();
        if let Ok(h) = GlobalAlloc(GMEM_MOVEABLE, bytes) {
            let p = GlobalLock(h);
            if p.is_null() {
                let _ = GlobalFree(Some(h));
            } else {
                std::ptr::copy_nonoverlapping(w.as_ptr() as *const u8, p as *mut u8, bytes);
                let _ = GlobalUnlock(h);
                // 成功后内存归剪贴板所有，不能再 GlobalFree
                if SetClipboardData(CF_UNICODETEXT.0 as u32, Some(HANDLE(h.0))).is_ok() {
                    ok = true;
                } else {
                    let _ = GlobalFree(Some(h));
                }
            }
        }
    }
    let _ = CloseClipboard();
    ok
}

unsafe fn build_wlan_controls(hwnd: HWND, w: &Wlan) {
    let mut rc = RECT::default();
    let _ = GetClientRect(hwnd, &mut rc);
    let cx = (rc.right - rc.left).max(360);
    let cy = (rc.bottom - rc.top).max(230);
    let margin = 14;
    let label_w = 72;
    let value_x = margin + label_w;
    let value_w = (cx - value_x - margin).max(180);

    add_label(
        hwnd,
        margin,
        12,
        cx - margin * 2,
        &format!("SSID：{}", w.ssid),
    );
    add_label(hwnd, margin, 40, label_w, "类型");
    add_label(hwnd, value_x, 40, value_w, crate::wifi::type_text(w.saved));
    add_label(hwnd, margin, 64, label_w, "信号");
    add_label(
        hwnd,
        value_x,
        64,
        value_w,
        &crate::wifi::signal_text(w.signal),
    );
    add_label(hwnd, margin, 88, label_w, "加密方式");
    add_label(hwnd, value_x, 88, value_w, &w.security);
    add_label(hwnd, margin, 118, label_w, "密码");
    add_readonly_edit(
        hwnd,
        value_x,
        114,
        value_w,
        &crate::wifi::password_text(w),
        WLAN_PASSWORD,
    );

    let copyable = w.saved && matches!(w.password.as_deref(), Some(p) if !p.is_empty());
    let copy = add_button(hwnd, value_x, 146, 90, "复制密码", WLAN_COPY);
    if !copyable {
        let _ = EnableWindow(copy, false);
    }
    create(
        hwnd,
        "STATIC",
        "",
        WS_CHILD | WS_VISIBLE,
        value_x + 100,
        150,
        (value_w - 100).max(0),
        18,
        WLAN_HINT,
    );
    add_button(hwnd, cx - margin - 90, cy - 38, 90, "关闭", 2);
}

unsafe extern "system" fn wlan_proc(hwnd: HWND, msg: u32, wparam: WPARAM, _l: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            let entry = WLAN.with(|s| s.borrow().clone());
            match entry {
                Some(w) => build_wlan_controls(hwnd, &w),
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
            if id == WLAN_COPY {
                let text = WLAN.with(|s| {
                    s.borrow()
                        .as_ref()
                        .and_then(|w| w.password.clone())
                        .unwrap_or_default()
                });
                let hint = if !text.is_empty() && copy_to_clipboard(hwnd, &text) {
                    "已复制到剪贴板"
                } else {
                    "复制失败，可选中密码框按 Ctrl+C"
                };
                edit_text(hwnd, WLAN_HINT, hint);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn route_fields_normalize_and_validate() {
        let d = parse_route_fields(" 10.20.0.9/24 ", "10.20.0.1", " 50 ").unwrap();
        assert_eq!(d.prefix, "10.20.0.0/24");
        assert_eq!(d.next_hop, "10.20.0.1");
        assert_eq!(d.metric, 50);
        // 默认路由也是合法输入（删除侧才做断网告警）
        assert_eq!(
            parse_route_fields("0.0.0.0/0", "10.0.0.1", "0")
                .unwrap()
                .prefix,
            "0.0.0.0/0"
        );
        assert!(
            parse_route_fields("10.20.0.0", "10.20.0.1", "5").is_err(),
            "缺前缀长度"
        );
        assert!(
            parse_route_fields("10.20.0.0/24", "no", "5").is_err(),
            "下一跳非法"
        );
        assert!(
            parse_route_fields("10.20.0.0/24", "10.20.0.1", "x").is_err(),
            "跃点非数字"
        );
    }

    #[test]
    fn metric_field_parses_auto_and_numbers() {
        assert_eq!(parse_metric_field("", true), Ok(None));
        assert_eq!(parse_metric_field("300", false), Ok(Some(300)));
        assert_eq!(parse_metric_field(" 1 ", false), Ok(Some(1)));
        assert!(parse_metric_field("0", false).is_err(), "跃点 0 由系统保留");
        assert!(parse_metric_field("abc", false).is_err());
        assert!(
            parse_metric_field("999999999", false).is_err(),
            "超出 u32 要报错"
        );
    }
}
