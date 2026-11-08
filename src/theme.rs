//! 浅色主题调色板与行/状态配色。全部纯函数，便于单测；不持有任何 GDI 对象。
//!
//! COLORREF 是 `0x00BBGGRR`（BGR 顺序），`rgb` 显式拼以免依赖绑定里的 RGB 宏。

use windows::Win32::Foundation::COLORREF;

pub const fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((b as u32) << 16 | (g as u32) << 8 | r as u32)
}

pub const BG: COLORREF = rgb(255, 255, 255);
pub const ZEBRA: COLORREF = rgb(247, 249, 252);
pub const TEXT: COLORREF = rgb(32, 36, 42);
pub const TEXT_DIM: COLORREF = rgb(96, 104, 116);
pub const SELECT_BG: COLORREF = rgb(224, 236, 252);
pub const SELECT_TEXT: COLORREF = rgb(20, 40, 80);

/// 行背景：选中优先，其次隔行浅灰，否则纯白。
pub fn row_bg(selected: bool, row: usize) -> COLORREF {
    if selected {
        SELECT_BG
    } else if row % 2 == 1 {
        ZEBRA
    } else {
        BG
    }
}

pub fn row_text(selected: bool) -> COLORREF {
    if selected {
        SELECT_TEXT
    } else {
        TEXT
    }
}

/// 状态胶囊的 (前景, 底色)：灰=已禁用、蓝=已启用未连接、绿=已连接。
pub fn status_colors(enabled: bool, connected: bool) -> (COLORREF, COLORREF) {
    if !enabled {
        (rgb(128, 132, 140), rgb(238, 240, 243))
    } else if !connected {
        (rgb(26, 95, 180), rgb(228, 238, 251))
    } else {
        (rgb(27, 122, 61), rgb(230, 244, 234))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn colorref_is_bgr_order() {
        assert_eq!(rgb(0x1A, 0x5F, 0xB4).0, 0x00B4_5F1A);
        assert_eq!(rgb(0xFF, 0, 0).0, 0x0000_00FF);
        assert_eq!(rgb(0, 0, 0xFF).0, 0x00FF_0000);
    }

    #[test]
    fn zebra_alternates_and_selection_overrides() {
        assert_eq!(row_bg(false, 0), BG);
        assert_eq!(row_bg(false, 1), ZEBRA);
        assert_eq!(row_bg(false, 2), BG);
        assert_eq!(row_bg(true, 1), SELECT_BG);
        assert_eq!(row_bg(true, 0), SELECT_BG);
        assert_ne!(BG, ZEBRA);
    }

    #[test]
    fn selected_row_uses_darker_text() {
        assert_eq!(row_text(false), TEXT);
        assert_eq!(row_text(true), SELECT_TEXT);
    }

    #[test]
    fn status_colors_are_distinct_per_state() {
        let disabled = status_colors(false, false);
        let idle = status_colors(true, false);
        let live = status_colors(true, true);
        assert_ne!(disabled.0, idle.0);
        assert_ne!(idle.0, live.0);
        // 禁用态即使 connected=true 也必须是灰（enabled 优先）
        assert_eq!(status_colors(false, true), disabled);
    }
}
