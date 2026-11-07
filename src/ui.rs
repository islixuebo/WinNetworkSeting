// Task 5：主窗口 —— 分组 ListView、状态栏、状态列着色、缩放跟随。
// 资源 ID 的单一来源在本模块（res/resource.h 是 rc 侧的单一来源，两边数值必须一致）。
use crate::adapterclass::{kind_name, NicKind};
use crate::netctl;
use crate::netinfo::{self, Nic};
use crate::winutil::wide;
use std::sync::{Mutex, OnceLock};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::ScreenToClient;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT,
    CDRF_NEWFONT, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, ICC_BAR_CLASSES,
    ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW, LVGF_GROUPID,
    LVGF_HEADER, LVGROUP, LVIF_GROUPID, LVIF_TEXT, LVITEMW, LVNI_SELECTED, LVM_DELETEALLITEMS,
    LVM_ENABLEGROUPVIEW, LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTGROUP, LVM_INSERTITEMW,
    LVM_REMOVEALLGROUPS, LVM_SETITEMTEXTW, LVN_ITEMCHANGED, NMHDR, NM_CUSTOMDRAW, NMLVCUSTOMDRAW,
    SB_SETPARTS, SB_SETTEXTW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetClientRect, GetCursorPos,
    GetDlgItem, GetWindowRect, IDYES, MessageBoxW, MoveWindow, SendMessageW, TrackPopupMenu,
    MB_ICONERROR, MB_ICONWARNING, MB_OK, MB_YESNO, MESSAGEBOX_RESULT, MF_GRAYED, MF_STRING,
    TPM_LEFTALIGN, TPM_TOPALIGN, WM_CLOSE, WM_COMMAND, WM_CONTEXTMENU, WM_INITDIALOG, WM_NOTIFY,
    WM_SIZE,
};

pub const IDD_MAIN: u32 = 100;
pub const IDC_LIST: u32 = 1000;
pub const IDC_ENABLE: u32 = 1001;
pub const IDC_DISABLE: u32 = 1002;
pub const IDC_REFRESH: u32 = 1003;
pub const IDC_FLUSH_DNS: u32 = 1004;
pub const IDC_STATUSBAR: u32 = 3000;
pub const IDM_ENABLE: u32 = 2000;
pub const IDM_DISABLE: u32 = 2001;
pub const IDM_DETAIL: u32 = 2002;

pub const GRP_WIRED: i32 = 0;
pub const GRP_WIRELESS: i32 = 1;
pub const GRP_OTHER: i32 = 2;

/// 数字资源 ID 的 MAKEINTRESOURCE 等价写法。
pub fn resource_id(id: u32) -> PCWSTR {
    PCWSTR(id as *const u16)
}

pub struct AppState {
    pub nics: Vec<Nic>,
    pub err: Option<String>,
}

/// 单例状态。对话框是单线程模态循环，但 custom-draw 回调与按钮事件都要读同一份数据，
/// 用 Mutex 而不是 `static mut`（后者在 edition 2024 下是硬错误）。
pub fn state() -> &'static Mutex<AppState> {
    static S: OnceLock<Mutex<AppState>> = OnceLock::new();
    S.get_or_init(|| {
        Mutex::new(AppState {
            nics: Vec::new(),
            err: None,
        })
    })
}

fn lock_state() -> std::sync::MutexGuard<'static, AppState> {
    // 中毒只可能来自别处 panic；本进程 panic=abort，取回内层数据继续渲染比整窗空白好
    state().lock().unwrap_or_else(|e| e.into_inner())
}

/// # Safety
/// `hwnd` 必须是存活的主对话框窗口；控件不存在时返回无效句柄而非崩溃。
pub unsafe fn list_view(hwnd: HWND) -> HWND {
    GetDlgItem(Some(hwnd), IDC_LIST as i32).unwrap_or_default()
}

unsafe fn status_bar(hwnd: HWND) -> HWND {
    GetDlgItem(Some(hwnd), IDC_STATUSBAR as i32).unwrap_or_default()
}

fn group_of(kind: NicKind) -> i32 {
    match kind {
        NicKind::Wired => GRP_WIRED,
        NicKind::Wireless => GRP_WIRELESS,
        NicKind::Other => GRP_OTHER,
    }
}

/// 建立三个分组。分组视图默认是关的，必须先 LVM_ENABLEGROUPVIEW，
/// 否则插了组也不显示标题（简报 Step 2 的「若标题不出现」正是这个原因）。
///
/// # Safety
/// `hwnd` 必须是主对话框窗口，且其 IDC_LIST 控件已创建。
pub unsafe fn ensure_groups(hwnd: HWND) {
    let hl = list_view(hwnd);
    if hl.is_invalid() {
        return;
    }
    let _ = SendMessageW(hl, LVM_ENABLEGROUPVIEW, Some(WPARAM(1)), None);
    for (id, kind) in [
        (GRP_WIRED, NicKind::Wired),
        (GRP_WIRELESS, NicKind::Wireless),
        (GRP_OTHER, NicKind::Other),
    ] {
        let w = wide(kind_name(kind));
        let mut g = LVGROUP {
            cbSize: std::mem::size_of::<LVGROUP>() as u32,
            mask: LVGF_HEADER | LVGF_GROUPID,
            iGroupId: id,
            pszHeader: PWSTR(w.as_ptr() as *mut u16),
            cchHeader: w.len() as i32,
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_INSERTGROUP,
            Some(WPARAM(id as usize)),
            Some(LPARAM(&mut g as *mut LVGROUP as isize)),
        );
    }
}

/// # Safety
/// 同 `ensure_groups`。
pub unsafe fn set_columns(hwnd: HWND) {
    let hl = list_view(hwnd);
    if hl.is_invalid() {
        return;
    }
    for (i, (name, width)) in [("状态", 110), ("名称", 210), ("IPv4", 120), ("网关", 120)]
        .iter()
        .enumerate()
    {
        let w = wide(name);
        let mut c = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: *width,
            pszText: PWSTR(w.as_ptr() as *mut u16),
            cchTextMax: w.len() as i32,
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(i)),
            Some(LPARAM(&mut c as *mut LVCOLUMNW as isize)),
        );
    }
}

/// # Safety
/// `hwnd` 是主对话框；`nics` 与当前 ListView 行序一一对应（custom-draw 按行号回查颜色）。
pub unsafe fn populate(hwnd: HWND, nics: &[Nic]) {
    let hl = list_view(hwnd);
    if hl.is_invalid() {
        return;
    }
    let _ = SendMessageW(hl, LVM_REMOVEALLGROUPS, None, None);
    let _ = SendMessageW(hl, LVM_DELETEALLITEMS, None, None);
    ensure_groups(hwnd);

    for (row, n) in nics.iter().enumerate() {
        let status = status_text(n);
        let wstat = wide(&status);
        let mut it = LVITEMW {
            mask: LVIF_TEXT | LVIF_GROUPID,
            iItem: row as i32,
            iSubItem: 0,
            pszText: PWSTR(wstat.as_ptr() as *mut u16),
            cchTextMax: wstat.len() as i32,
            // 0.61.3 的 LVITEMW 同时有 iGroupId(v5 语义) 与 iGroup(v6 语义)，
            // 两个都填，避免 comctl32 版本差异导致分组失效
            iGroupId: group_of(n.kind),
            iGroup: group_of(n.kind),
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_INSERTITEMW,
            None,
            Some(LPARAM(&mut it as *mut LVITEMW as isize)),
        );

        let name = wide(&n.alias);
        let ip = wide(&n.ipv4);
        let gw = wide(&n.gateway);
        for (sub_item, text) in [(1, &name), (2, &ip), (3, &gw)] {
            let mut sub = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: sub_item,
                pszText: PWSTR(text.as_ptr() as *mut u16),
                cchTextMax: text.len() as i32,
                ..Default::default()
            };
            let _ = SendMessageW(
                hl,
                LVM_SETITEMTEXTW,
                Some(WPARAM(row)),
                Some(LPARAM(&mut sub as *mut LVITEMW as isize)),
            );
        }
    }
}

pub fn status_text(n: &Nic) -> String {
    if !n.enabled {
        "已禁用".to_string()
    } else if n.connected {
        "已连接".to_string()
    } else {
        "已启用".to_string()
    }
}

/// # Safety
/// `hwnd` 是主对话框。状态栏控件不存在时静默跳过。
pub unsafe fn init_status_bar(hwnd: HWND) {
    let sb = status_bar(hwnd);
    if sb.is_invalid() {
        return;
    }
    // 模板里状态栏初始宽度为 0，comctl32 不会自建分区；
    // 不显式设分区的话 SB_SETTEXTW(0) 静默失败，状态栏永远一片空白。
    // -1 表示该分区右缘贴到窗口右缘，缩放时不用重设。
    let mut parts = [-1i32];
    let _ = SendMessageW(
        sb,
        SB_SETPARTS,
        Some(WPARAM(1)),
        Some(LPARAM(parts.as_mut_ptr() as isize)),
    );
}

/// # Safety
/// `hwnd` 是主对话框。状态栏控件不存在时静默跳过（不让状态文案拖垮列表）。
pub unsafe fn set_status(hwnd: HWND, text: &str) {
    let sb = status_bar(hwnd);
    if sb.is_invalid() {
        return;
    }
    let w = wide(text);
    let _ = SendMessageW(sb, SB_SETTEXTW, Some(WPARAM(0)), Some(LPARAM(w.as_ptr() as isize)));
}

/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn refresh_list(hwnd: HWND) {
    // 顺序要紧：先 populate 再写 state。populate 里发的每条 ListView 消息都可能回调进
    // dlgproc（LVN_ITEMCHANGED → sync_buttons → selected_nic 又要拿锁），
    // 持锁跨 SendMessage 会在同一线程上自死锁。
    match netinfo::list() {
        Ok(nics) => {
            let text = format!("共 {} 个物理网卡", nics.len());
            let snapshot = nics.clone();
            populate(hwnd, &snapshot);
            *lock_state() = AppState { nics, err: None };
            set_status(hwnd, &text);
        }
        Err(e) => {
            populate(hwnd, &[]);
            *lock_state() = AppState {
                nics: Vec::new(),
                err: Some(e.clone()),
            };
            set_status(hwnd, &format!("读取网卡信息失败：{e}"));
        }
    }
}

/// 当前选中行号。分组视图下 LVM_GETNEXTITEM 返回的是插入序（= state().nics 下标），
/// 不是显示序，所以可以直接用来回查数据。
///
/// # Safety
/// `hl` 是主对话框的 ListView 控件。
pub unsafe fn selected_index(hl: HWND) -> Option<usize> {
    if hl.is_invalid() {
        return None;
    }
    let i = SendMessageW(
        hl,
        LVM_GETNEXTITEM,
        Some(WPARAM(usize::MAX)),
        Some(LPARAM(LVNI_SELECTED as isize)),
    )
    .0;
    if i < 0 {
        None
    } else {
        Some(i as usize)
    }
}

/// 返回拷贝而不是引用：调用方要在不持锁的情况下跑完 netsh / 弹窗。
///
/// # Safety
/// `hl` 是主对话框的 ListView 控件。
pub unsafe fn selected_nic(hl: HWND) -> Option<Nic> {
    let idx = selected_index(hl)?;
    let guard = lock_state();
    guard.nics.get(idx).cloned()
}

/// 按选中行的状态同步「启用/禁用」按钮可用态；未选中时两个都置灰。
///
/// # Safety
/// `hwnd` 是主对话框，`hl` 是其 ListView 控件。
pub unsafe fn sync_buttons(hwnd: HWND, hl: HWND) {
    let (has_sel, en_state) = match selected_nic(hl) {
        Some(n) => (true, n.enabled),
        None => (false, true),
    };
    let h_en = GetDlgItem(Some(hwnd), IDC_ENABLE as i32).unwrap_or_default();
    let h_dis = GetDlgItem(Some(hwnd), IDC_DISABLE as i32).unwrap_or_default();
    if !h_en.is_invalid() {
        let _ = EnableWindow(h_en, has_sel && !en_state);
    }
    if !h_dis.is_invalid() {
        let _ = EnableWindow(h_dis, has_sel && en_state);
    }
}

/// 在光标处弹出启用/禁用菜单。不带 TPM_RETURNCMD：选中项由系统以 WM_COMMAND
/// 投给 owner 对话框，与按钮走同一条分发路径。
///
/// # Safety
/// `hwnd` 是主对话框，`hl` 是其 ListView 控件。
pub unsafe fn show_popup_menu(hwnd: HWND, hl: HWND) {
    let Ok(hmenu) = CreatePopupMenu() else {
        set_status(hwnd, "创建右键菜单失败");
        return;
    };
    let (has_sel, en_state) = match selected_nic(hl) {
        Some(n) => (true, n.enabled),
        None => (false, true),
    };
    let menus = [
        (IDM_ENABLE, "启用", has_sel && !en_state),
        (IDM_DISABLE, "禁用", has_sel && en_state),
    ];
    for (id, label, usable) in menus {
        let flags = if usable { MF_STRING } else { MF_STRING | MF_GRAYED };
        let text = wide(label);
        let _ = AppendMenuW(hmenu, flags, id as usize, PCWSTR(text.as_ptr()));
    }
    let mut pt = POINT::default();
    let _ = GetCursorPos(&mut pt);
    let _ = TrackPopupMenu(
        hmenu,
        TPM_TOPALIGN | TPM_LEFTALIGN,
        pt.x,
        pt.y,
        Some(0),
        hwnd,
        None,
    );
    let _ = DestroyMenu(hmenu);
}

/// 禁用会不会切断当前默认路由 —— 需要用户确认的唯一条件。
/// 已禁用的卡再点一次「禁用」不会断网，不必打扰用户。
pub fn needs_disable_confirm(n: &Nic) -> bool {
    n.is_default_route && n.enabled
}

/// 确认框答复 → 是否继续执行。单独拆出来是因为弹窗本身没法在单测里点，
/// 而这一行的方向反了就是「点是」不干活、点「否」反而断网。
fn disable_approved(r: MESSAGEBOX_RESULT) -> bool {
    r == IDYES
}

/// 禁用默认路由网卡前必须确认；返回 false 表示用户取消。
/// 非默认路由、或本来就已禁用的网卡不会断网，不打扰用户。
pub fn confirm_disable_default_route(n: &Nic) -> bool {
    if !needs_disable_confirm(n) {
        return true;
    }
    unsafe {
        let title = wide("警告");
        let body = wide(&format!(
            "「{}」是当前默认路由所在的网卡，禁用会立即断网。\n\n确定要禁用吗？",
            n.alias
        ));
        let r = MessageBoxW(None, PCWSTR(body.as_ptr()), PCWSTR(title.as_ptr()), MB_YESNO | MB_ICONWARNING);
        disable_approved(r)
    }
}

/// 按钮与右键菜单共用的执行体：确认 → 调用 netctl → 反馈 → 重采集。
///
/// # Safety
/// `hwnd` 是主对话框，`hl` 是其 ListView 控件。
pub unsafe fn on_toggle(hwnd: HWND, hl: HWND, enable: bool) {
    let Some(n) = selected_nic(hl) else {
        set_status(hwnd, "未选择网卡");
        return;
    };
    if !enable && !confirm_disable_default_route(&n) {
        return;
    }
    let verb = if enable { "启用" } else { "禁用" };
    set_status(hwnd, &format!("正在{verb}「{}」…", n.alias));
    if let Err(e) = netctl::set_enabled(n.if_index, &n.alias, enable) {
        let title = wide("错误");
        let body = wide(&format!("{verb}「{}」失败：{e}", n.alias));
        let _ = MessageBoxW(Some(hwnd), PCWSTR(body.as_ptr()), PCWSTR(title.as_ptr()), MB_OK | MB_ICONERROR);
        set_status(hwnd, &format!("{verb}「{}」失败", n.alias));
    } else {
        set_status(hwnd, &format!("已{verb}「{}」", n.alias));
    }
    refresh_list(hwnd);
    sync_buttons(hwnd, hl);
}

/// COLORREF 是 0x00BBGGRR（不是 RGB 顺序），这里显式拼以免依赖绑定里是否存在 RGB 宏。
fn rgb(r: u8, g: u8, b: u8) -> COLORREF {
    COLORREF((b as u32) << 16 | (g as u32) << 8 | r as u32)
}

/// 状态列着色：灰=已禁用、蓝=已启用未连接、绿=已连接（与 WPF 版语义一致）。
/// 三级请求：整表 → 逐项 → 逐项每列；只有最后一级的第 0 列改文字色。
///
/// # Safety
/// `nmh` 必须指向本次 WM_NOTIFY 的真实 NMHDR（调用方从 lParam 转出）。
pub unsafe fn on_list_notify(hwnd: HWND, nmh: &mut NMHDR) -> Option<isize> {
    if nmh.hwndFrom != list_view(hwnd) || nmh.code != NM_CUSTOMDRAW {
        return None;
    }
    let cd = &mut *(nmh as *mut NMHDR as *mut NMLVCUSTOMDRAW);
    // NMCUSTOMDRAW_DRAW_STAGE 是裸 u32 新类型且未实现 BitOr，
    // ITEMPREPAINT|SUBITEM 这个组合阶段只能在整数域里拼
    let stage = cd.nmcd.dwDrawStage.0;

    if stage == CDDS_PREPAINT.0 {
        // CDRF_NOTIFYITEMDRAW 才会换来 ITEMPREPAINT 通知；
        // 简报这里写的 CDRF_NEWFONT 不请求下一级，颜色永远不会生效
        return Some(CDRF_NOTIFYITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 {
        return Some(CDRF_NOTIFYSUBITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
        if cd.iSubItem == 0 {
            let color = {
                let guard = lock_state();
                match guard.nics.get(cd.nmcd.dwItemSpec) {
                    Some(n) if !n.enabled => rgb(0x80, 0x80, 0x80),
                    Some(n) if !n.connected => rgb(0x1A, 0x5F, 0xB4),
                    Some(_) => rgb(0x1B, 0x7A, 0x3D),
                    None => return Some(CDRF_DODEFAULT as isize),
                }
            };
            cd.clrText = color;
            // 改了颜色必须回 CDRF_NEWFONT，控件才会采用新值
            return Some(CDRF_NEWFONT as isize);
        }
        return Some(CDRF_DODEFAULT as isize);
    }
    None
}

/// 窗口缩放时让列表与状态栏跟随。
/// 列表顶边不写死像素：从「刷新」按钮的实际底边推出来，这样任意 DPI 下都不会压到按钮。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn layout(hwnd: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let mut top = 24;
    let btn = GetDlgItem(Some(hwnd), IDC_REFRESH as i32).unwrap_or_default();
    if !btn.is_invalid() {
        let mut br = RECT::default();
        if GetWindowRect(btn, &mut br).is_ok() {
            let mut pt = POINT {
                x: br.left,
                y: br.bottom,
            };
            if ScreenToClient(hwnd, &mut pt).as_bool() {
                top = pt.y + 6;
            }
        }
    }

    let sb = status_bar(hwnd);
    let mut sb_height = 0;
    if !sb.is_invalid() {
        // 状态栏自己处理 WM_SIZE：贴底、宽度跟随父窗口
        let _ = SendMessageW(sb, WM_SIZE, None, None);
        let mut sr = RECT::default();
        if GetWindowRect(sb, &mut sr).is_ok() {
            sb_height = sr.bottom - sr.top;
        }
    }

    let hl = list_view(hwnd);
    if hl.is_invalid() {
        return;
    }
    let margin = 5;
    let width = (rc.right - rc.left - margin * 2).max(0);
    let height = (rc.bottom - rc.top - top - sb_height - margin).max(0);
    let _ = MoveWindow(hl, margin, top, width, height, true);
}

/// 通用控件初始化（ListView 分组与状态栏都依赖 comctl32 v6）。
/// 失败时返回 false，由 main 走可见的失败路径。
pub fn init_common_controls() -> bool {
    let icc = INITCOMMONCONTROLSEX {
        dwSize: std::mem::size_of::<INITCOMMONCONTROLSEX>() as u32,
        dwICC: ICC_LISTVIEW_CLASSES | ICC_BAR_CLASSES,
    };
    unsafe { InitCommonControlsEx(&icc).as_bool() }
}

/// 主对话框过程。windows 0.61.3 的 DLGPROC 返回 `isize`（该版本没有 INTARG 别名）。
///
/// # Safety
/// 由系统按对话框过程约定调用；`lparam` 在 WM_NOTIFY 下必须指向有效 NMHDR。
pub unsafe extern "system" fn main_dlgproc(
    hwnd: HWND,
    msg: u32,
    wparam: WPARAM,
    lparam: LPARAM,
) -> isize {
    match msg {
        WM_INITDIALOG => {
            set_columns(hwnd);
            ensure_groups(hwnd);
            init_status_bar(hwnd);
            refresh_list(hwnd);
            layout(hwnd);
            sync_buttons(hwnd, list_view(hwnd));
            1
        }
        WM_COMMAND => {
            let hl = list_view(hwnd);
            match (wparam.0 & 0xFFFF) as u32 {
                IDC_REFRESH => {
                    refresh_list(hwnd);
                    sync_buttons(hwnd, hl);
                }
                IDC_ENABLE | IDM_ENABLE => on_toggle(hwnd, hl, true),
                IDC_DISABLE | IDM_DISABLE => on_toggle(hwnd, hl, false),
                // 刷新 DNS 缓存、详细信息由 Task 7/阶段2 接线
                _ => return 0,
            }
            1
        }
        WM_CONTEXTMENU => {
            show_popup_menu(hwnd, list_view(hwnd));
            1
        }
        WM_NOTIFY => {
            if lparam.0 == 0 {
                return 0;
            }
            let nmh = &mut *(lparam.0 as *mut NMHDR);
            if nmh.code == LVN_ITEMCHANGED && nmh.hwndFrom == list_view(hwnd) {
                sync_buttons(hwnd, nmh.hwndFrom);
                return 0;
            }
            on_list_notify(hwnd, nmh).unwrap_or(0)
        }
        WM_SIZE => {
            layout(hwnd);
            0
        }
        WM_CLOSE => {
            let _ = windows::Win32::UI::WindowsAndMessaging::EndDialog(hwnd, 0);
            1
        }
        _ => 0,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn nic(enabled: bool, connected: bool, kind: NicKind, alias: &str) -> Nic {
        Nic {
            if_index: 1,
            alias: alias.to_string(),
            description: String::new(),
            kind,
            mac: String::new(),
            ipv4: String::new(),
            mask: String::new(),
            gateway: String::new(),
            dns: Vec::new(),
            speed: String::new(),
            enabled,
            connected,
            is_default_route: false,
            metric: 0,
            network_guid: String::new(),
        }
    }

    #[test]
    fn status_text_three_states() {
        assert_eq!(status_text(&nic(false, true, NicKind::Wired, "a")), "已禁用");
        assert_eq!(status_text(&nic(false, false, NicKind::Wired, "a")), "已禁用");
        assert_eq!(status_text(&nic(true, true, NicKind::Wired, "a")), "已连接");
        assert_eq!(status_text(&nic(true, false, NicKind::Wired, "a")), "已启用");
    }

    /// 禁用优先于连接：被禁用的卡即使 OperStatus 残留 Up 也必须显示「已禁用」
    #[test]
    fn disabled_wins_over_connected() {
        let n = nic(false, true, NicKind::Wireless, "WLAN");
        assert_eq!(status_text(&n), "已禁用");
        assert_ne!(status_text(&n), "已连接");
    }

    /// 确认门只在「禁用会真的断网」时成立：非默认路由不会断、已禁用的卡再禁也不会断。
    #[test]
    fn only_default_route_disable_needs_confirm() {
        let routed = Nic {
            is_default_route: true,
            ..nic(true, true, NicKind::Wired, "以太网")
        };
        assert!(needs_disable_confirm(&routed));
        let routed_off = Nic {
            is_default_route: true,
            ..nic(false, false, NicKind::Wired, "以太网")
        };
        assert!(!needs_disable_confirm(&routed_off));
        assert!(!needs_disable_confirm(&nic(true, true, NicKind::Wireless, "WLAN")));
    }

    /// 未触发确认门时不得弹窗，直接放行（这条同时锁住「取消语义」：true = 继续执行）。
    #[test]
    fn confirm_passes_through_without_dialog() {
        assert!(confirm_disable_default_route(&nic(
            true,
            true,
            NicKind::Wireless,
            "WLAN"
        )));
        assert!(confirm_disable_default_route(&nic(
            false,
            false,
            NicKind::Wired,
            "以太网"
        )));
    }

    /// 「是」= 继续执行，「否」= 取消。方向写反会让确认框变成一个陷阱。
    #[test]
    fn yes_approves_and_no_cancels() {
        use windows::Win32::UI::WindowsAndMessaging::{IDCANCEL, IDNO};
        assert!(disable_approved(IDYES));
        assert!(!disable_approved(IDNO));
        assert!(!disable_approved(IDCANCEL));
    }

    #[test]
    fn group_ids_match_group_order() {
        use crate::adapterclass::group_order;
        for k in [NicKind::Wired, NicKind::Wireless, NicKind::Other] {
            assert_eq!(group_of(k), group_order(k) as i32);
        }
        assert_eq!(group_of(NicKind::Wired), 0);
        assert_eq!(group_of(NicKind::Wireless), 1);
        assert_eq!(group_of(NicKind::Other), 2);
    }

    /// COLORREF 是 0x00BBGGRR：拼错顺序会让蓝/绿互换，真机上很难一眼看出
    #[test]
    fn colorref_is_bgr_order() {
        assert_eq!(rgb(0x1A, 0x5F, 0xB4).0, 0x00B4_5F1A);
        assert_eq!(rgb(0x1B, 0x7A, 0x3D).0, 0x003D_7A1B);
        assert_eq!(rgb(0x80, 0x80, 0x80).0, 0x0080_8080);
        assert_eq!(rgb(0xFF, 0, 0).0, 0x0000_00FF);
        assert_eq!(rgb(0, 0, 0xFF).0, 0x00FF_0000);
    }

    /// 资源 ID 必须与 res/resource.h 逐字一致（rc 侧改了这边没改会静默错位）
    #[test]
    fn resource_ids_match_header() {
        let header = include_str!("../res/resource.h");
        for (name, value) in [
            ("IDD_MAIN", IDD_MAIN),
            ("IDC_LIST", IDC_LIST),
            ("IDC_ENABLE", IDC_ENABLE),
            ("IDC_DISABLE", IDC_DISABLE),
            ("IDC_REFRESH", IDC_REFRESH),
            ("IDC_FLUSH_DNS", IDC_FLUSH_DNS),
            ("IDM_ENABLE", IDM_ENABLE),
            ("IDM_DISABLE", IDM_DISABLE),
            ("IDM_DETAIL", IDM_DETAIL),
            ("IDC_STATUSBAR", IDC_STATUSBAR),
        ] {
            let needle = format!("#define {name}");
            let line = header
                .lines()
                .find(|l| l.starts_with(&needle))
                .unwrap_or_else(|| panic!("resource.h 缺少 {name}"));
            let declared: u32 = line
                .split_whitespace()
                .nth(2)
                .and_then(|v| v.parse().ok())
                .unwrap_or_else(|| panic!("{name} 的值不是数字：{line}"));
            assert_eq!(declared, value, "{name} 与 resource.h 不一致");
        }
    }

    #[test]
    fn resource_id_is_makeintresource() {
        assert_eq!(resource_id(100).0 as usize, 100);
        assert_eq!(resource_id(0).0 as usize, 0);
    }
}