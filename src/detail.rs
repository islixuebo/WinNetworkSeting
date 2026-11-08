//! 网卡详情对话框：只读展示单张网卡的 11 项信息。
//!
//! 模板（`IDD_DETAIL`）是空的，行在 `WM_INITDIALOG` 里按实际条数动态创建，
//! 这样新增/删减字段只改 `build_rows`，不用动 .rc。
//! 浅色精修：白底、灰标签 / 深色值、状态值带语义色、Segoe UI 大行距、底部关闭按钮；
//! 窗口高度按「客户区期望 + 实测非客户区」算，不再猜标题栏像素，避免末行被裁。

use crate::adapterclass::kind_name;
use crate::netinfo::Nic;
use crate::theme;
use crate::winutil::wide;
use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    GetStockObject, SetBkColor, SetTextColor, HBRUSH, HDC, WHITE_BRUSH,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetClientRect, GetDlgCtrlID, GetWindowRect,
    SendMessageW, SetWindowLongPtrW, SetWindowPos, SetWindowTextW, BS_PUSHBUTTON, DLGPROC,
    DWLP_MSGRESULT, HMENU, SWP_NOMOVE, SWP_NOZORDER, WINDOW_EX_STYLE, WINDOW_LONG_PTR_INDEX,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_CTLCOLORDLG, WM_CTLCOLORSTATIC, WM_INITDIALOG,
    WM_SETFONT, WS_CHILD, WS_GROUP, WS_VISIBLE,
};

const IDD_DETAIL: u32 = 101;
/// 起始控件 ID：每行两个静态文本（键、值），所以步长是 2。
const IDC_DETAIL_ROW: u32 = 4000;
/// 状态行的值控件 ID（第 1 行的值），用于上语义色。
const IDC_STATUS_VALUE: u32 = IDC_DETAIL_ROW + 3;

const ROW_H: i32 = 26;
const FIRST_Y: i32 = 16;
const KEY_X: i32 = 16;
const KEY_W: i32 = 100;
const VAL_X: i32 = KEY_X + KEY_W + 12;
const VAL_W: i32 = 340;
const RIGHT_PAD: i32 = 16;
const BOTTOM_PAD: i32 = 16;
const BUTTON_GAP: i32 = 12;
const BUTTON_H: i32 = 26;
const BUTTON_W: i32 = 80;

type Rows = Vec<(&'static str, String)>;
type Content = (String, Rows, COLORREF);

thread_local! {
    static CONTENT: RefCell<Content> =
        const { RefCell::new((String::new(), Vec::new(), COLORREF(0))) };
}

pub fn build_rows(n: &Nic) -> Vec<(&'static str, String)> {
    let dash = |s: &str| {
        if s.is_empty() {
            "—".to_string()
        } else {
            s.to_string()
        }
    };
    vec![
        ("类型", kind_name(n.kind).to_string()),
        ("状态", status_line(n)),
        ("描述", dash(&n.description)),
        ("IPv4", dash(&n.ipv4)),
        ("子网掩码", dash(&n.mask)),
        ("网关", dash(&n.gateway)),
        ("DNS", dash(&n.dns.join(", "))),
        ("MAC", dash(&n.mac)),
        ("速率", dash(&n.speed)),
        (
            "接口跃点数",
            if n.metric == 0 {
                "自动/未知".to_string()
            } else {
                n.metric.to_string()
            },
        ),
        (
            "默认路由",
            if n.is_default_route { "是" } else { "否" }.to_string(),
        ),
    ]
}

fn status_line(n: &Nic) -> String {
    if !n.enabled {
        "已禁用".into()
    } else if n.connected {
        "已启用·已连接".into()
    } else {
        "已启用·未连接".into()
    }
}

/// 弹出模态详情对话框。`parent` 是主对话框。
pub fn show(parent: HWND, n: &Nic) {
    // 模板里没有名称行，标题栏带上 alias，否则用户分不清看的是哪张卡。
    let status_fg = theme::status_colors(n.enabled, n.connected).0;
    CONTENT.with(|c| {
        *c.borrow_mut() = (
            format!("网卡详细信息 - {}", n.alias),
            build_rows(n),
            status_fg,
        )
    });
    unsafe {
        let Ok(hmodule) = GetModuleHandleW(None) else {
            return;
        };
        let dlg: DLGPROC = Some(dlgproc);
        let _ = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            crate::ui::resource_id(IDD_DETAIL),
            Some(parent),
            dlg,
            LPARAM(0),
        );
    }
}

unsafe extern "system" fn dlgproc(hwnd: HWND, msg: u32, wparam: WPARAM, lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            let (title, rows, _) = CONTENT.with(|c| c.borrow().clone());
            let t = wide(&title);
            let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
            for (i, (key, value)) in rows.iter().enumerate() {
                let y = FIRST_Y + i as i32 * ROW_H;
                make_static(hwnd, KEY_X, y, KEY_W, key, IDC_DETAIL_ROW + i as u32 * 2);
                make_static(
                    hwnd,
                    VAL_X,
                    y,
                    VAL_W,
                    value,
                    IDC_DETAIL_ROW + i as u32 * 2 + 1,
                );
            }
            let client_w = VAL_X + VAL_W + RIGHT_PAD;
            let btn_y = FIRST_Y + rows.len() as i32 * ROW_H + BUTTON_GAP;
            make_button(hwnd, client_w - RIGHT_PAD - BUTTON_W, btn_y, "关闭", 2);
            resize_to_rows(hwnd, rows.len());
            1
        }
        WM_CTLCOLORDLG => {
            let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
            SetBkColor(hdc, theme::BG);
            set_msg_result(hwnd, white_brush().0 as isize);
            1
        }
        WM_CTLCOLORSTATIC => {
            let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
            let child = HWND(lparam.0 as *mut core::ffi::c_void);
            let id = GetDlgCtrlID(child) as u32;
            let status_fg = CONTENT.with(|c| c.borrow().2);
            let off = id.wrapping_sub(IDC_DETAIL_ROW);
            let color = if id == IDC_STATUS_VALUE {
                status_fg
            } else if off.is_multiple_of(2) {
                theme::TEXT_DIM
            } else {
                theme::TEXT
            };
            SetBkColor(hdc, theme::BG);
            SetTextColor(hdc, color);
            set_msg_result(hwnd, white_brush().0 as isize);
            1
        }
        WM_COMMAND => {
            // 1=IDOK、2=IDCANCEL：回车 / Esc / 关闭按钮都走这里。
            let id = (wparam.0 & 0xFFFF) as u32;
            if id == 1 || id == 2 {
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

/// 对话框过程的返回值只是 BOOL；WM_CTLCOLOR* 要返回的画刷必须写进 DWLP_MSGRESULT。
unsafe fn set_msg_result(hwnd: HWND, value: isize) {
    SetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(DWLP_MSGRESULT as i32), value);
}

fn white_brush() -> HBRUSH {
    unsafe { HBRUSH(GetStockObject(WHITE_BRUSH).0) }
}

/// 客户区期望尺寸 + 实测非客户区（标题栏/边框，随 DPI 与主题变化），不再猜固定像素。
unsafe fn resize_to_rows(hwnd: HWND, rows: usize) {
    let mut wr = RECT::default();
    let mut cr = RECT::default();
    if GetWindowRect(hwnd, &mut wr).is_err() || GetClientRect(hwnd, &mut cr).is_err() {
        return;
    }
    let nc_w = (wr.right - wr.left) - (cr.right - cr.left);
    let nc_h = (wr.bottom - wr.top) - (cr.bottom - cr.top);
    let client_w = VAL_X + VAL_W + RIGHT_PAD;
    let client_h = FIRST_Y + rows as i32 * ROW_H + BUTTON_GAP + BUTTON_H + BOTTOM_PAD;
    let _ = SetWindowPos(
        hwnd,
        None,
        wr.left,
        wr.top,
        client_w + nc_w,
        client_h + nc_h,
        SWP_NOZORDER | SWP_NOMOVE,
    );
}

unsafe fn create_child(
    parent: HWND,
    class: &str,
    style: WINDOW_STYLE,
    bounds: RECT,
    text: &str,
    id: u32,
) {
    let t = wide(text);
    let cls = wide(class);
    let Ok(hmodule) = GetModuleHandleW(None) else {
        return;
    };
    let Ok(child) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(cls.as_ptr()),
        PCWSTR(t.as_ptr()),
        style,
        bounds.left,
        bounds.top,
        bounds.right - bounds.left,
        bounds.bottom - bounds.top,
        Some(parent),
        Some(HMENU(id as *mut core::ffi::c_void)),
        Some(HINSTANCE(hmodule.0)),
        None,
    ) else {
        return;
    };
    // 手建的控件不继承对话框模板字体，统一用主列表同款 Segoe UI。
    SendMessageW(
        child,
        WM_SETFONT,
        Some(WPARAM(crate::ui::ui_font().0 as usize)),
        Some(LPARAM(1)),
    );
}

unsafe fn make_static(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) {
    create_child(
        parent,
        "STATIC",
        WS_CHILD | WS_VISIBLE | WS_GROUP,
        RECT {
            left: x,
            top: y,
            right: x + w,
            bottom: y + ROW_H,
        },
        text,
        id,
    );
}

unsafe fn make_button(parent: HWND, x: i32, y: i32, text: &str, id: u32) {
    create_child(
        parent,
        "BUTTON",
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        RECT {
            left: x,
            top: y,
            right: x + BUTTON_W,
            bottom: y + BUTTON_H,
        },
        text,
        id,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rows_include_core_fields_in_order() {
        let n = sample();
        let keys: Vec<&str> = build_rows(&n).iter().map(|(k, _)| *k).collect();
        assert_eq!(
            keys,
            vec![
                "类型",
                "状态",
                "描述",
                "IPv4",
                "子网掩码",
                "网关",
                "DNS",
                "MAC",
                "速率",
                "接口跃点数",
                "默认路由"
            ]
        );
    }

    #[test]
    fn empty_values_show_placeholder() {
        let mut n = sample();
        n.ipv4 = String::new();
        n.dns.clear();
        let rows = build_rows(&n);
        assert_eq!(rows[3].1, "—");
        assert_eq!(rows[6].1, "—");
    }

    #[test]
    fn status_and_metric_text() {
        let mut n = sample();
        n.enabled = false;
        assert_eq!(build_rows(&n)[1].1, "已禁用");
        n.enabled = true;
        n.connected = true;
        assert_eq!(build_rows(&n)[1].1, "已启用·已连接");
        n.metric = 0;
        assert_eq!(build_rows(&n)[9].1, "自动/未知");
        n.metric = 25;
        assert_eq!(build_rows(&n)[9].1, "25");
    }

    #[test]
    fn enabled_but_unplugged_is_not_reported_as_disabled() {
        let mut n = sample();
        n.connected = false;
        assert_eq!(build_rows(&n)[1].1, "已启用·未连接");
    }

    #[test]
    fn dns_list_is_joined_with_comma_space() {
        // 详情里 DNS 可能有多条，分隔符要和列表、WPF 版一致。
        let rows = build_rows(&sample());
        assert_eq!(rows[6].1, "223.5.5.5, 114.114.114.114");
    }

    #[test]
    fn values_are_single_line_and_never_empty() {
        let rows = build_rows(&sample());
        assert!(rows.iter().all(|(_, v)| !v.is_empty() && !v.contains('\n')));
    }

    /// 状态值控件 ID 必须落在第 1 行的「值」上，否则语义色会染错行。
    #[test]
    fn status_value_id_is_row1_value() {
        let row: u32 = 1;
        assert_eq!(IDC_STATUS_VALUE, IDC_DETAIL_ROW + row * 2 + 1);
    }

    fn sample() -> crate::netinfo::Nic {
        crate::netinfo::Nic {
            if_index: 11,
            alias: "以太网".into(),
            description: "Realtek Gaming 2.5GbE".into(),
            kind: crate::adapterclass::NicKind::Wired,
            mac: "aa-bb-cc-dd-ee-ff".into(),
            ipv4: "10.0.0.5".into(),
            mask: "255.255.255.0".into(),
            gateway: "10.0.0.1".into(),
            dns: vec!["223.5.5.5".into(), "114.114.114.114".into()],
            speed: "2.5 Gbps".into(),
            enabled: true,
            connected: true,
            is_default_route: true,
            metric: 20,
            network_guid: "{...}".into(),
        }
    }
}
