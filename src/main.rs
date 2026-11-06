#![windows_subsystem = "windows"]

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HWND, HINSTANCE, LPARAM, WPARAM};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_LISTVIEW_CLASSES,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DialogBoxParamW, DLGPROC, EndDialog, WM_CLOSE, WM_INITDIALOG,
};

// 与 res/resource.h 保持一致
const IDD_MAIN: usize = 100;

/// Task 2 落地后删除此临时函数，改用 winutil::wide
#[allow(dead_code)]
fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 数字资源 ID 的 MAKEINTRESOURCE 等价写法
fn resource_id(id: usize) -> PCWSTR {
    PCWSTR(id as *const u16)
}

/// 对话框过程。windows 0.61.3 的 DLGPROC 返回 `isize`
/// （即 WIN32 的 INT_PTR；该版本绑定里还没有 INTARG 这个类型别名）
unsafe extern "system" fn main_dlgproc(
    hwnd: HWND,
    msg: u32,
    _wparam: WPARAM,
    _lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => 1,
        WM_CLOSE => {
            let _ = EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}

fn main() {
    unsafe {
        let icc = INITCOMMONCONTROLSEX {
            dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
            dwICC: ICC_LISTVIEW_CLASSES,
        };
        let _ = InitCommonControlsEx(&icc);

        // 取模块句柄失败时为 NULL，DialogBoxParamW 收到 NULL 会自然返回 -1，不会 panic
        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let dlg: DLGPROC = Some(main_dlgproc);
        let _ = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            resource_id(IDD_MAIN),
            None,
            dlg,
            LPARAM(0),
        );
    }
}
