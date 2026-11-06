// 本模块是阶段1 后续任务（netinfo / netctl / ui / detail）的公共地基，
// 故在 main.rs 里以 `pub mod winutil;` 挂到 crate 根：从 crate 根可见的 pub 项
// 不会被 dead_code 判成「未使用」，Task 3+ 逐个接入时不必再挂 allow 属性。
// 代价是 clippy 的 missing_safety_doc 会要求给 unsafe fn 写 `# Safety` 章节——
// 这本来就是裸指针 API 该有的调用约定，写在文档里比写在 allow 里有用。
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

/// IP_ADAPTER_ADDRESSES_LH 的字符串字段在 windows crate 中声明为 PSTR，
/// 实际内容是 UTF-16（结构体为 Unicode 版本），需要按宽字符串解读。
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
        let s = String::from_utf16_lossy(&buf[..n as usize]);
        s.trim_end_matches(['\r', '\n', '.']).to_string()
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
        let p = windows::core::PSTR(data.as_ptr().cast::<u8>().cast_mut());
        assert_eq!(unsafe { wstr_from_pstr(p) }, text);
        // 逐码元核对：读到的内容必须正好是那块宽缓冲区里结尾 0 之前的全部码元
        // （若按 PSTR 字面语义当 UTF-8 解，这里就会是乱码而不是原串）
        let decoded: Vec<u16> = unsafe { wstr_from_pstr(p) }.encode_utf16().collect();
        assert_eq!(decoded, data[..data.len() - 1]);
        // PSTR 为 NULL 时同样返回空串，不得崩溃
        assert_eq!(unsafe { wstr_from_pstr(windows::core::PSTR(std::ptr::null_mut())) }, "");
    }

    #[test]
    fn error_text_for_known_code() {
        let s = error_text(5);
        assert!(!s.is_empty());
        assert!(s.chars().count() < 200);
    }

    #[test]
    fn error_text_for_unknown_code_still_returns() {
        let s = error_text(0xE000_FFFF);
        assert!(s.contains("错误码") || !s.is_empty());
    }
}
