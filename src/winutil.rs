// 模块挂载约定（事实描述）：本仓库以 `pub mod winutil;` 挂到 crate 根，
// 且约定 `pub unsafe fn` 必写 `# Safety` 章节，不使用 `#![allow(dead_code)]`。
// 注意 `pub mod` 只是把 dead_code 的盲区收得比 blanket allow 更窄——
// 对未接线的 `pub fn` 它同样是盲区，并非「可见即不死」的保证。
use windows::core::{PSTR, PWSTR};
use windows::Win32::Foundation::GetLastError;
use windows::Win32::System::Diagnostics::Debug::{
    FORMAT_MESSAGE_FROM_SYSTEM, FORMAT_MESSAGE_IGNORE_INSERTS, FormatMessageW,
};

/// str → UTF-16 + 结尾 0。
/// 注意：返回的 Vec 就是那块缓冲区本身，调用方必须持有返回值到 Win32 API 调用结束，
/// 否则指针会随 Vec 一起被释放。
pub fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// 从空终止宽指针构造 String。
///
/// # Safety
/// `p` 必须是 NULL，或指向一段**以 0 结尾**且在本调用期间保持可读、存活的 UTF-16 缓冲区；
/// 本函数会一路向后扫描找结尾 0，缺 0 的脏内存会读越界。
pub unsafe fn wstr_from_ptr(p: *const u16) -> String {
    if p.is_null() {
        return String::new();
    }
    let mut len = 0usize;
    while unsafe { *p.add(len) } != 0 {
        len += 1;
    }
    String::from_utf16_lossy(unsafe { std::slice::from_raw_parts(p, len) })
}

/// 通用工具：把一个「类型标成 PSTR、实际装着 UTF-16 宽字符串」的指针按宽字符串解码。
///
/// 警告：**不要**用它读 windows 0.61.3 的 IP_ADAPTER_ADDRESSES_LH 字符串字段——
/// 该版本里 `DnsSuffix` / `Description` / `FriendlyName` 已声明为 `PWSTR`，
/// 应直接用 `wstr_from_ptr(x.0)`；只有 `AdapterName` 是真 ANSI，
/// 喂进本函数会按宽字符解出乱码，还可能因扫不到 2 字节的 0 而越界读。
///
/// # Safety
/// `p` 必须是 NULL，或确实指向 2 字节对齐、以 0 结尾的 UTF-16 缓冲区。
/// 若 `p` 里装的真是指针字面意义的 ANSI/UTF-8（例如 0.61.3 中的 `AdapterName`），
/// 按宽字符解读会得到乱码，还可能因为扫不到 2 字节的 0 而读越界。
pub unsafe fn wstr_from_pstr(p: PSTR) -> String {
    unsafe { wstr_from_ptr(p.0 as *const u16) }
}

/// Win32 错误码 → 系统消息文本（语言交给系统默认 UI 语言，中文系统即中文）；
/// FormatMessageW 取不到消息时回退「错误码 N」。
pub fn error_text(code: u32) -> String {
    let mut buf = [0u16; 512];
    // windows 0.61.3 的签名：FormatMessageW(flags, Option<*const c_void>, id, langid,
    // PWSTR, size, Option<*const *const i8>) -> u32
    // —— 既不是 slices::from_raw_parts 那套缓冲区写法，末参也不是 &mut [&mut ...]。
    let n = unsafe {
        FormatMessageW(
            FORMAT_MESSAGE_FROM_SYSTEM | FORMAT_MESSAGE_IGNORE_INSERTS,
            None,
            code,
            0,
            PWSTR(buf.as_mut_ptr()),
            buf.len() as u32,
            None,
        )
    };
    if n > 0 {
        // 防御性上界：n 理论上不超过 buf 大小，仍显式夹一次
        let len = (n as usize).min(buf.len());
        let s = String::from_utf16_lossy(&buf[..len]);
        let trimmed = s.trim_end_matches(['\r', '\n', '.']).to_string();
        if trimmed.is_empty() {
            format!("错误码 {code}")
        } else {
            trimmed
        }
    } else {
        format!("错误码 {code}")
    }
}

pub fn last_error_text() -> String {
    error_text(unsafe { GetLastError() }.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn wide_appends_nul() {
        let w = wide("ab");
        assert_eq!(w, vec![b'a' as u16, b'b' as u16, 0]);
    }

    #[test]
    fn wstr_from_ptr_reads_until_nul() {
        let data = wide("网卡");
        let s = unsafe { wstr_from_ptr(data.as_ptr()) };
        assert_eq!(s, "网卡");
    }

    #[test]
    fn wstr_from_ptr_null_is_empty() {
        assert_eq!(unsafe { wstr_from_ptr(std::ptr::null()) }, "");
    }

    #[test]
    fn wstr_from_pstr_decodes_utf16_bytes() {
        // IP Helper 的字段类型是 PSTR，但里面装的是 UTF-16；这里用真实的宽缓冲区验证 reinterpret 语义
        let text = "以太网";
        let data = wide(text);
        let p = PSTR(data.as_ptr().cast::<u8>().cast_mut());
        assert_eq!(unsafe { wstr_from_pstr(p) }, text);
        // 逐码元核对：读到的内容必须正好是那块宽缓冲区里结尾 0 之前的全部码元
        // （若按 PSTR 字面语义当 UTF-8 解，这里就会是乱码而不是原串）
        let decoded: Vec<u16> = unsafe { wstr_from_pstr(p) }.encode_utf16().collect();
        assert_eq!(decoded, data[..data.len() - 1]);
        // PSTR 为 NULL 时同样返回空串，不得崩溃
        assert_eq!(unsafe { wstr_from_pstr(PSTR(std::ptr::null_mut())) }, "");
    }

    #[test]
    fn error_text_for_known_code() {
        let s = error_text(5);
        assert!(!s.is_empty());
        assert!(s.chars().count() < 200);
    }

    #[test]
    fn error_text_for_unknown_code_falls_back() {
        // 该码已由探针实测：FormatMessageW 对它返回 0，必然走「错误码 N」回退分支
        const CODE: u32 = 0xE000_FFFF;
        let s = error_text(CODE);
        // 前缀断言：回退文案以「错误码」开头，报的是码值而不是别的东西
        assert!(s.starts_with("错误码"), "未走回退分支，实际得到：{s}");
        // 精确断言：整串只能是「错误码 {十进制码值}」，钉死回退文案本身
        assert_eq!(s, format!("错误码 {CODE}"));
    }
}
