//! 网卡详情对话框：只读展示单张网卡的 11 项信息。
//!
//! 模板（`IDD_DETAIL`）是空的，行在 `WM_INITDIALOG` 里按实际条数动态创建，
//! 这样新增/删减字段只改 `build_rows`，不用动 .rc。

use crate::adapterclass::kind_name;
use crate::netinfo::Nic;
use crate::winutil::wide;
use std::cell::RefCell;
use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, RECT, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetWindowRect, SendMessageW, SetWindowPos,
    SetWindowTextW, DLGPROC, HMENU, SWP_NOMOVE, SWP_NOZORDER, WINDOW_EX_STYLE, WM_CLOSE,
    WM_COMMAND, WM_GETFONT, WM_INITDIALOG, WM_SETFONT, WS_CHILD, WS_GROUP, WS_VISIBLE,
};

const IDD_DETAIL: u32 = 101;
/// 起始控件 ID：每行两个静态文本（键、值），所以步长是 2。
const IDC_DETAIL_ROW: u32 = 4000;

const ROW_H: i32 = 16;
const FIRST_Y: i32 = 10;
const KEY_X: i32 = 10;
const KEY_W: i32 = 90;
const VAL_X: i32 = 105;
const VAL_W: i32 = 285;
/// 客户区之外的固定开销：标题栏 + 上下边框。
const FRAME_H: i32 = 34;
const BOTTOM_PAD: i32 = 10;

thread_local! {
    static CONTENT: RefCell<(String, Vec<(&'static str, String)>)> =
        const { RefCell::new((String::new(), Vec::new())) };
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
    CONTENT.with(|c| *c.borrow_mut() = (format!("网卡详细信息 - {}", n.alias), build_rows(n)));
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
            let (title, rows) = CONTENT.with(|c| c.borrow().clone());
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
            resize_to_rows(hwnd, rows.len());
            1
        }
        WM_COMMAND => {
            // 1=IDOK、2=IDCANCEL：模板里没有按钮，这两条只由回车与 Esc 触发。
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

/// 模板给的 160 是对话框单位、与实际行数无关，按行数改成像素高度，位置不动。
unsafe fn resize_to_rows(hwnd: HWND, rows: usize) {
    let mut rect = RECT::default();
    let _ = GetWindowRect(hwnd, &mut rect);
    let _ = SetWindowPos(
        hwnd,
        None,
        rect.left,
        rect.top,
        KEY_X + VAL_X + VAL_W,
        FRAME_H + FIRST_Y + rows as i32 * ROW_H + BOTTOM_PAD,
        SWP_NOZORDER | SWP_NOMOVE,
    );
}

unsafe fn make_static(parent: HWND, x: i32, y: i32, w: i32, text: &str, id: u32) {
    let t = wide(text);
    let cls = wide("STATIC");
    let Ok(hmodule) = GetModuleHandleW(None) else {
        return;
    };
    let Ok(child) = CreateWindowExW(
        WINDOW_EX_STYLE(0),
        PCWSTR(cls.as_ptr()),
        PCWSTR(t.as_ptr()),
        WS_CHILD | WS_VISIBLE | WS_GROUP,
        x,
        y,
        w,
        ROW_H,
        Some(parent),
        Some(HMENU(id as *mut core::ffi::c_void)),
        Some(HINSTANCE(hmodule.0)),
        None,
    ) else {
        return;
    };
    // 手建的控件不继承对话框模板字体，不显式设一次会和标题栏、主窗口不一致。
    let hfont = SendMessageW(parent, WM_GETFONT, None, None);
    SendMessageW(
        child,
        WM_SETFONT,
        Some(WPARAM(hfont.0 as usize)),
        Some(LPARAM(1)),
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
