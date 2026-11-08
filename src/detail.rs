//! 网卡详情对话框：只读展示单张网卡的 11 项信息。
//!
//! 模板（`IDD_DETAIL`）是空的，内容在 `WM_PAINT` 里整块自绘（与主列表同一套浅色语言）：
//! 白底 + 隔行浅灰、左列灰标签 / 右列深色值、状态行用圆角胶囊、底部真实「关闭」按钮。
//! 不用一堆 STATIC + WM_CTLCOLOR*（实测那套会留下灰底白块拼贴），自绘保证每个像素可控。
//! 窗口高度按「客户区期望 + 实测非客户区」算，不再猜标题栏像素，避免末行被裁。

use crate::adapterclass::kind_name;
use crate::netinfo::Nic;
use crate::theme;
use crate::winutil::wide;
use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    BeginPaint, CreateSolidBrush, DeleteObject, DrawTextW, EndPaint, FillRect, SelectObject,
    SetBkMode, SetTextColor, DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, HDC,
    HGDIOBJ, PAINTSTRUCT, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetClientRect, GetWindowRect, SetWindowPos,
    SetWindowTextW, BS_PUSHBUTTON, DLGPROC, HMENU, SWP_NOMOVE, SWP_NOZORDER, WINDOW_EX_STYLE,
    WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_ERASEBKGND, WM_INITDIALOG, WM_PAINT, WS_CHILD,
    WS_VISIBLE,
};

const IDD_DETAIL: u32 = 101;
/// 「状态」在 build_rows 里的行号，自绘时给这一行的值上胶囊。
const STATUS_ROW: usize = 1;

const PAD_X: i32 = 20;
const LABEL_W: i32 = 110;
const LABEL_GAP: i32 = 16;
const VAL_X: i32 = PAD_X + LABEL_W + LABEL_GAP;
const CLIENT_W: i32 = 520;
const VAL_W: i32 = CLIENT_W - VAL_X - PAD_X;
const ROW_H: i32 = 30;
const FIRST_Y: i32 = 12;
const BUTTON_GAP: i32 = 14;
const BUTTON_H: i32 = 28;
const BUTTON_W: i32 = 88;
const BOTTOM_PAD: i32 = 16;

type Rows = Vec<(&'static str, String)>;
/// (标题, 行, 状态前景色, 状态胶囊底色)
type Content = (String, Rows, COLORREF, COLORREF);

thread_local! {
    static CONTENT: RefCell<Content> =
        const { RefCell::new((String::new(), Vec::new(), COLORREF(0), COLORREF(0))) };
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
    let (fg, pill_bg) = theme::status_colors(n.enabled, n.connected);
    CONTENT.with(|c| {
        *c.borrow_mut() = (
            format!("网卡详细信息 - {}", n.alias),
            build_rows(n),
            fg,
            pill_bg,
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

unsafe extern "system" fn dlgproc(hwnd: HWND, msg: u32, wparam: WPARAM, _lparam: LPARAM) -> isize {
    match msg {
        WM_INITDIALOG => {
            crate::ui::set_title_icons(hwnd);
            let (title, rows, _, _) = CONTENT.with(|c| c.borrow().clone());
            let t = wide(&title);
            let _ = SetWindowTextW(hwnd, PCWSTR(t.as_ptr()));
            let btn_y = FIRST_Y + rows.len() as i32 * ROW_H + BUTTON_GAP;
            make_button(hwnd, CLIENT_W - PAD_X - BUTTON_W, btn_y, "关闭", 2);
            resize_to_rows(hwnd, rows.len());
            1
        }
        // 自己整块画白底，跳过默认灰底擦除，避免闪灰。
        WM_ERASEBKGND => 1,
        WM_PAINT => {
            paint(hwnd);
            0
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

/// 整块自绘：白底 + 隔行浅灰 + 左标签右值 + 状态胶囊。
unsafe fn paint(hwnd: HWND) {
    let mut ps = PAINTSTRUCT::default();
    let hdc = BeginPaint(hwnd, &mut ps);
    if hdc.0.is_null() {
        return;
    }
    let mut cr = RECT::default();
    let _ = GetClientRect(hwnd, &mut cr);

    let base = CreateSolidBrush(theme::BG);
    FillRect(hdc, &cr, base);
    let _ = DeleteObject(HGDIOBJ(base.0));

    let (_, rows, fg, pill_bg) = CONTENT.with(|c| c.borrow().clone());
    let old_font = SelectObject(hdc, HGDIOBJ(crate::ui::ui_font().0));
    SetBkMode(hdc, TRANSPARENT);

    for (i, (key, value)) in rows.iter().enumerate() {
        let top = FIRST_Y + i as i32 * ROW_H;
        let bg = theme::row_bg(false, i);
        let band = RECT {
            left: 0,
            top,
            right: cr.right,
            bottom: top + ROW_H,
        };
        let brush = CreateSolidBrush(bg);
        FillRect(hdc, &band, brush);
        let _ = DeleteObject(HGDIOBJ(brush.0));

        SetTextColor(hdc, theme::TEXT_DIM);
        let mut lr = RECT {
            left: PAD_X,
            top,
            right: PAD_X + LABEL_W,
            bottom: top + ROW_H,
        };
        draw_line(hdc, key, &mut lr);

        if i == STATUS_ROW {
            let cell = RECT {
                left: VAL_X - 6,
                top,
                right: VAL_X + VAL_W,
                bottom: top + ROW_H,
            };
            crate::ui::draw_status_pill(hdc, &cell, bg, fg, pill_bg, value);
        } else {
            SetTextColor(hdc, theme::TEXT);
            let mut vr = RECT {
                left: VAL_X,
                top,
                right: VAL_X + VAL_W,
                bottom: top + ROW_H,
            };
            draw_line(hdc, value, &mut vr);
        }
    }

    SelectObject(hdc, old_font);
    let _ = EndPaint(hwnd, &ps);
}

unsafe fn draw_line(hdc: HDC, text: &str, r: &mut RECT) {
    let mut buf: Vec<u16> = text.encode_utf16().collect();
    DrawTextW(
        hdc,
        &mut buf,
        r,
        DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX | DT_END_ELLIPSIS,
    );
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
    let client_h = FIRST_Y + rows as i32 * ROW_H + BUTTON_GAP + BUTTON_H + BOTTOM_PAD;
    let _ = SetWindowPos(
        hwnd,
        None,
        wr.left,
        wr.top,
        CLIENT_W + nc_w,
        client_h + nc_h,
        SWP_NOZORDER | SWP_NOMOVE,
    );
}

unsafe fn make_button(parent: HWND, x: i32, y: i32, text: &str, id: u32) {
    let t = wide(text);
    let cls = wide("BUTTON");
    let Ok(hmodule) = GetModuleHandleW(None) else {
        return;
    };
    // 按钮不单独设字体：继承对话框默认 chrome 字体，与主窗口按钮保持一致；
    // 正文（自绘行）才用 ui_font，形成「正文 17 / chrome 默认」两级统一字阶。
    let _ = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(cls.as_ptr()),
        PCWSTR(t.as_ptr()),
        WS_CHILD | WS_VISIBLE | WINDOW_STYLE(BS_PUSHBUTTON as u32),
        x,
        y,
        BUTTON_W,
        BUTTON_H,
        Some(parent),
        Some(HMENU(id as *mut core::ffi::c_void)),
        Some(HINSTANCE(hmodule.0)),
        None,
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

    /// 胶囊必须落在「状态」行，否则语义色会染错行。
    #[test]
    fn status_row_index_matches_status_key() {
        assert_eq!(build_rows(&sample())[STATUS_ROW].0, "状态");
    }

    /// 值列必须放得下最宽的标签且不越出右留白（编译期布局不变量）。
    #[test]
    fn value_column_stays_inside_client() {
        const {
            assert!(VAL_X > PAD_X + LABEL_W);
            assert!(VAL_X + VAL_W + PAD_X == CLIENT_W);
        }
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
            metric_auto: true,
            network_guid: "{...}".into(),
        }
    }
}
