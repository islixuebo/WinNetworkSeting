#![windows_subsystem = "windows"]

pub mod adapterclass;
pub mod netinfo;
pub mod ui;
pub mod winutil;

use windows::core::PCWSTR;
use windows::Win32::Foundation::{HINSTANCE, LPARAM};
use windows::Win32::System::Diagnostics::Debug::OutputDebugStringW;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::WindowsAndMessaging::{DialogBoxParamW, DLGPROC};

/// 失败诊断走 OutputDebugStringW（src/ 内禁止 unwrap/expect/panic，失败只让外部可见）。
fn report_failure(message: &str) {
    let utf16 = winutil::wide(message);
    unsafe { OutputDebugStringW(PCWSTR(utf16.as_ptr())) };
}

fn main() {
    unsafe {
        if !ui::init_common_controls() {
            report_failure(
                "网卡管理工具：通用控件初始化失败（Common Controls v6 依赖未链接？请用 --release 产物验收）",
            );
            std::process::exit(1);
        }

        // 取模块句柄失败时为 NULL；此时 DialogBoxParamW 返回 -1，走下面的失败分支
        let hmodule = GetModuleHandleW(None).unwrap_or_default();
        let dlg: DLGPROC = Some(ui::main_dlgproc);
        let result = DialogBoxParamW(
            Some(HINSTANCE(hmodule.0)),
            ui::resource_id(ui::IDD_MAIN),
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