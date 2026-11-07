#![windows_subsystem = "windows"]

pub mod adapterclass;
pub mod winutil;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, HWND, LPARAM, WPARAM};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, INITCOMMONCONTROLSEX, ICC_LISTVIEW_CLASSES,
};
use windows::Win32::UI::WindowsAndMessaging::{
    DialogBoxParamW, DLGPROC, EndDialog, WM_CLOSE, WM_INITDIALOG,
};

// 与 res/resource.h 保持一致；资源 ID 一律用 u32（后续任务的 IDC_*/IDM_* 常量同此类型）
pub const IDD_MAIN: u32 = 100;

/// 数字资源 ID 的 MAKEINTRESOURCE 等价写法
pub fn resource_id(id: u32) -> PCWSTR {
    PCWSTR(id as *const u16)
}

/// 失败诊断走 OutputDebugStringW（src/ 内禁止 unwrap/expect/panic，失败只让外部可见）。
fn report_failure(message: &str) {
    let utf16 = winutil::wide(message);
    unsafe { OutputDebugStringW(PCWSTR(utf16.as_ptr())) };
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
        if !InitCommonControlsEx(&icc).as_bool() {
            report_failure(
                "网卡管理工具：通用控件初始化失败（Common Controls v6 依赖未链接？请用 --release 产物验收）",
            );
            std::process::exit(1);
        }

        // 取模块句柄失败时为 NULL；此时 DialogBoxParamW 返回 -1，走下面的失败分支
        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let dlg: DLGPROC = Some(main_dlgproc);
        let result = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            resource_id(IDD_MAIN),
            None,
            dlg,
            LPARAM(0),
        );
        // -1 是「对话框创建失败」的唯一信号（正常关闭时返回 EndDialog 传的 0），不能吞掉
        if result == -1 {
            report_failure("网卡管理工具：主对话框创建失败（资源未链接？请用 --release 产物验收）");
            std::process::exit(1);
        }
    }
}
