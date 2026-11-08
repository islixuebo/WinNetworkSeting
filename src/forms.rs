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
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DialogBoxParamW, EndDialog, GetClientRect, GetDlgItem, GetWindowTextLengthW,
    GetWindowTextW, MessageBoxW, SendMessageW, SetWindowTextW, BS_PUSHBUTTON, DLGPROC, ES_AUTOHSCROLL,
    HMENU, MB_ICONWARNING, MB_OK, WINDOW_EX_STYLE, WINDOW_STYLE, WM_CLOSE, WM_COMMAND,
    WM_CTLCOLORSTATIC, WM_ERASEBKGND, WM_INITDIALOG, WM_SETFONT, WS_CHILD, WS_TABSTOP, WS_VISIBLE,
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
