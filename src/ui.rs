// Task 5：主窗口 —— 分组 ListView、状态栏、状态列着色、缩放跟随。
// 资源 ID 的单一来源在本模块（res/resource.h 是 rc 侧的单一来源，两边数值必须一致）。
use crate::adapterclass::{kind_name, NicKind};
use crate::netctl::{self, TestKind, TestResult};
use crate::netinfo::{self, Nic, Route};
use crate::theme;
use crate::winutil::wide;
use std::sync::atomic::{AtomicBool, AtomicU32, Ordering};
use std::sync::{Mutex, OnceLock};
use windows::core::{PCWSTR, PWSTR};
use windows::Win32::Foundation::{COLORREF, HINSTANCE, HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{
    CreateFontW, CreatePen, CreateSolidBrush, DeleteObject, DrawTextW, Ellipse, FillRect,
    InvalidateRect, RoundRect, ScreenToClient, SelectObject, SetBkMode, SetTextColor,
    CLEARTYPE_QUALITY, CLIP_DEFAULT_PRECIS, DEFAULT_CHARSET, DT_CALCRECT, DT_CENTER,
    DT_END_ELLIPSIS, DT_LEFT, DT_NOPREFIX, DT_SINGLELINE, DT_VCENTER, FW_NORMAL, HDC, HFONT,
    HGDIOBJ, OUT_DEFAULT_PRECIS, PS_SOLID, TRANSPARENT,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::ProcessStatus::EmptyWorkingSet;
use windows::Win32::System::Threading::GetCurrentProcess;
use windows::Win32::UI::Controls::{
    InitCommonControlsEx, CDDS_ITEMPREPAINT, CDDS_PREPAINT, CDDS_SUBITEM, CDRF_DODEFAULT,
    CDRF_NEWFONT, CDRF_NOTIFYITEMDRAW, CDRF_NOTIFYSUBITEMDRAW, CDRF_SKIPDEFAULT, DRAWITEMSTRUCT,
    ICC_BAR_CLASSES, ICC_LISTVIEW_CLASSES, INITCOMMONCONTROLSEX, LVCF_TEXT, LVCF_WIDTH, LVCOLUMNW,
    LVGF_GROUPID, LVGF_HEADER, LVGROUP, LVIF_GROUPID, LVIF_PARAM, LVIF_TEXT, LVITEMW,
    LVM_DELETEALLITEMS, LVM_ENABLEGROUPVIEW, LVM_GETITEMCOUNT, LVM_GETITEMTEXTW, LVM_GETITEMW,
    LVM_GETNEXTITEM, LVM_INSERTCOLUMNW, LVM_INSERTGROUP, LVM_INSERTITEMW, LVM_REMOVEALLGROUPS,
    LVM_SETCOLUMNWIDTH, LVM_SETEXTENDEDLISTVIEWSTYLE, LVM_SETITEMTEXTW, LVNI_SELECTED,
    LVN_ITEMCHANGED, LVS_EX_FULLROWSELECT, NMHDR, NMLVCUSTOMDRAW, NM_CUSTOMDRAW, SB_SETPARTS,
    SB_SETTEXTW,
};
use windows::Win32::UI::Input::KeyboardAndMouse::EnableWindow;
use windows::Win32::UI::WindowsAndMessaging::{
    AppendMenuW, CreatePopupMenu, DestroyMenu, GetClientRect, GetCursorPos, GetDlgItem,
    GetSystemMetrics, GetWindowRect, GetWindowTextW, LoadImageW, MessageBoxW, MoveWindow,
    PostMessageW, SendMessageW, SetWindowLongPtrW, ShowWindow, TrackPopupMenu, CB_ADDSTRING,
    CB_GETCURSEL, CB_SETCURSEL, CBN_SELCHANGE, DWLP_MSGRESULT, ICON_BIG, ICON_SMALL, IDYES,
    IMAGE_ICON, LR_SHARED, MB_ICONERROR, MB_ICONQUESTION, MB_ICONWARNING, MB_OK, MB_YESNO,
    MESSAGEBOX_RESULT, MF_GRAYED, MF_STRING, SC_MINIMIZE, SM_CXICON, SM_CXSMICON, SW_HIDE,
    SW_SHOW, TPM_LEFTALIGN, TPM_TOPALIGN, WINDOW_LONG_PTR_INDEX, WM_APP, WM_CLOSE, WM_COMMAND,
    WM_CONTEXTMENU, WM_DRAWITEM, WM_ERASEBKGND, WM_INITDIALOG, WM_NOTIFY, WM_SETFONT, WM_SETICON,
    WM_SIZE, WM_SYSCOMMAND,
};

pub const IDD_MAIN: u32 = 100;
pub const IDI_APP: u32 = 200;
pub const IDC_LIST: u32 = 1000;
pub const IDC_ENABLE: u32 = 1001;
pub const IDC_DISABLE: u32 = 1002;
pub const IDC_REFRESH: u32 = 1003;
pub const IDC_FLUSH_DNS: u32 = 1004;
pub const IDC_DETAIL: u32 = 1005;
pub const IDC_RESET: u32 = 1006;
pub const IDC_CONFIG: u32 = 1007;
pub const IDC_ROUTE_LIST: u32 = 1010;
pub const IDC_ROUTE_REFRESH: u32 = 1011;
pub const IDC_ROUTE_ADD: u32 = 1012;
pub const IDC_ROUTE_DEL: u32 = 1013;
pub const IDC_TEST_LABEL: u32 = 1020;
pub const IDC_TEST_TARGET: u32 = 1021;
pub const IDC_TEST_KIND: u32 = 1022;
pub const IDC_TEST_RUN: u32 = 1023;
pub const IDC_TEST_OUT: u32 = 1024;
pub const IDC_STATUSBAR: u32 = 3000;
pub const IDC_NAV_NIC: u32 = 3100;
pub const IDC_NAV_ROUTE: u32 = 3101;
pub const IDC_NAV_TEST: u32 = 3102;
pub const IDM_ENABLE: u32 = 2000;
pub const IDM_DISABLE: u32 = 2001;
pub const IDM_DETAIL: u32 = 2002;
pub const IDM_RENAME: u32 = 2003;
pub const IDM_RELEASE: u32 = 2004;
pub const IDM_RENEW: u32 = 2005;

/// 左侧竖排导航项，顺序即 `res/app.rc` 中自上而下的顺序。
pub const NAV_IDS: &[u32] = &[IDC_NAV_NIC, IDC_NAV_ROUTE, IDC_NAV_TEST];

/// 三页导航。每页只显示自己的控件，页与页的控件集合两两不相交。
#[derive(Copy, Clone, PartialEq, Eq, Debug)]
pub enum Page {
    Nic,
    Route,
    Test,
}

pub fn nav_of_page(p: Page) -> u32 {
    match p {
        Page::Nic => IDC_NAV_NIC,
        Page::Route => IDC_NAV_ROUTE,
        Page::Test => IDC_NAV_TEST,
    }
}

pub fn page_of_nav(id: u32) -> Option<Page> {
    match id {
        IDC_NAV_NIC => Some(Page::Nic),
        IDC_NAV_ROUTE => Some(Page::Route),
        IDC_NAV_TEST => Some(Page::Test),
        _ => None,
    }
}

/// 该页工具条控件：一行横排在列表上方。
pub fn page_toolbar_ids(p: Page) -> &'static [u32] {
    match p {
        Page::Nic => &[
            IDC_ENABLE,
            IDC_DISABLE,
            IDC_CONFIG,
            IDC_DETAIL,
            IDC_REFRESH,
            IDC_FLUSH_DNS,
            IDC_RESET,
        ],
        Page::Route => &[IDC_ROUTE_REFRESH, IDC_ROUTE_ADD, IDC_ROUTE_DEL],
        Page::Test => &[IDC_TEST_LABEL, IDC_TEST_TARGET, IDC_TEST_KIND, IDC_TEST_RUN],
    }
}

/// 该页吃满剩余空间的大块控件（列表 / 输出框）。
pub fn page_fill_ids(p: Page) -> &'static [u32] {
    match p {
        Page::Nic => &[IDC_LIST],
        Page::Route => &[IDC_ROUTE_LIST],
        Page::Test => &[IDC_TEST_OUT],
    }
}

/// 该页的全部控件（切换页面时统一显示/隐藏）。
pub fn page_ids(p: Page) -> &'static [u32] {
    match p {
        Page::Nic => &[
            IDC_ENABLE,
            IDC_DISABLE,
            IDC_CONFIG,
            IDC_DETAIL,
            IDC_REFRESH,
            IDC_FLUSH_DNS,
            IDC_RESET,
            IDC_LIST,
        ],
        Page::Route => &[
            IDC_ROUTE_REFRESH,
            IDC_ROUTE_ADD,
            IDC_ROUTE_DEL,
            IDC_ROUTE_LIST,
        ],
        Page::Test => &[
            IDC_TEST_LABEL,
            IDC_TEST_TARGET,
            IDC_TEST_KIND,
            IDC_TEST_RUN,
            IDC_TEST_OUT,
        ],
    }
}

/// 当前页面。对话框只有一条 UI 线程，AtomicU32 足够，也省掉一个 Mutex。
static CURRENT_PAGE: AtomicU32 = AtomicU32::new(0);

pub fn current_page() -> Page {
    match CURRENT_PAGE.load(Ordering::Relaxed) {
        1 => Page::Route,
        2 => Page::Test,
        _ => Page::Nic,
    }
}

pub fn set_current_page(p: Page) {
    CURRENT_PAGE.store(
        match p {
            Page::Nic => 0,
            Page::Route => 1,
            Page::Test => 2,
        },
        Ordering::Relaxed,
    );
}

pub const GRP_WIRED: i32 = 0;
pub const GRP_WIRELESS: i32 = 1;
pub const GRP_VPN: i32 = 2;
pub const GRP_VIRTUAL: i32 = 3;
pub const GRP_OTHER: i32 = 4;

// 列表列宽（px）。IPv4/网关按 15 字符（169.254.xxx.xxx）留量；描述列在布局时吃掉剩余宽度，
// 固定像素在窄窗口里会被视口直接裁掉，连省略号都看不到。
const COL_NAME: i32 = 150;
const COL_STATUS: i32 = 104;
const COL_IP: i32 = 150;
const COL_GW: i32 = 148;
const COL_DESC_MIN: i32 = 160;
const COL_DESC_INDEX: i32 = 4;

/// 列表与窗口边缘、列表与左侧菜单列之间的间距（px）。
const LIST_MARGIN: i32 = 5;

/// 描述列宽度：列表客户区减去四列固定宽度。
pub fn desc_column_width(list_client_w: i32) -> i32 {
    // 额外留 2px 给主题边框与取整，否则算得刚好时会冒出横向滚动条
    (list_client_w - (COL_NAME + COL_STATUS + COL_IP + COL_GW) - 2).max(COL_DESC_MIN)
}

/// 菜单列右边界：取最宽按钮，且不小于 `fallback`。
/// 一个都没量到时（控件尚未创建/查询失败）必须退回 fallback，否则列表会盖住整列按钮。
pub fn nav_right(edges: &[i32], fallback: i32) -> i32 {
    edges
        .iter()
        .copied()
        .max()
        .unwrap_or(fallback)
        .max(fallback)
}

/// 列表矩形 (x, y, w, h)：导航列右侧、工具条下方、状态栏上方。宽高夹到 0 ——
/// MoveWindow 收到负值不会报错，只会画出一个不可见的控件，看起来像「窗口拉小后列表消失了」。
pub fn list_bounds(
    client_w: i32,
    client_h: i32,
    nav_right: i32,
    top: i32,
    sb_height: i32,
    margin: i32,
) -> (i32, i32, i32, i32) {
    let x = nav_right + margin;
    let y = top;
    (
        x,
        y,
        (client_w - x - margin).max(0),
        (client_h - y - sb_height - margin).max(0),
    )
}

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
        NicKind::Vpn => GRP_VPN,
        NicKind::Virtual => GRP_VIRTUAL,
        NicKind::Other => GRP_OTHER,
    }
}

/// 建立分组。分组视图默认是关的，必须先 LVM_ENABLEGROUPVIEW，
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
        (GRP_VPN, NicKind::Vpn),
        (GRP_VIRTUAL, NicKind::Virtual),
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
    for (i, (name, width)) in [
        ("名称", COL_NAME),
        ("状态", COL_STATUS),
        ("IPv4", COL_IP),
        ("网关", COL_GW),
        ("描述", COL_DESC_MIN),
    ]
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
        let name = wide(&n.alias);
        let mut it = LVITEMW {
            mask: LVIF_TEXT | LVIF_GROUPID,
            iItem: row as i32,
            iSubItem: 0,
            pszText: PWSTR(name.as_ptr() as *mut u16),
            cchTextMax: name.len() as i32,
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

        let desc = wide(&n.description);
        let ip = wide(&n.ipv4);
        let gw = wide(&n.gateway);
        for (sub_item, text) in [(1, &wstat), (2, &ip), (3, &gw), (4, &desc)] {
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

/// 整行高亮、整行可点。这条是 ListView 的**扩展**样式：rc 模板里连常量都没有
/// （`LVS_FULLROWSELECT` 在 rc 的预处理里未定义，实测 RC2104），只能在这里发消息。
///
/// # Safety
/// `hl` 是主对话框的 ListView 控件。
pub unsafe fn enable_full_row_select(hl: HWND) {
    if hl.is_invalid() {
        return;
    }
    let _ = SendMessageW(
        hl,
        LVM_SETEXTENDEDLISTVIEWSTYLE,
        Some(WPARAM(0)),
        Some(LPARAM(LVS_EX_FULLROWSELECT as isize)),
    );
}

/// 标题栏图标。对话框模板没有图标字段（只有顶层窗口类的 `hIcon` 才有），
/// 所以 .rc 里的 `IDI_APP ICON` 必须在运行时用 `WM_SETICON` 挂上去。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn set_title_icons(hwnd: HWND) {
    let Ok(hmodule) = GetModuleHandleW(None) else {
        return;
    };
    let hmodule: HINSTANCE = hmodule.into();
    // LR_SHARED 拿的是系统缓存的图标，不能 DestroyIcon。
    for (kind, size) in [
        (ICON_SMALL, GetSystemMetrics(SM_CXSMICON)),
        (ICON_BIG, GetSystemMetrics(SM_CXICON)),
    ] {
        let Ok(hicon) = LoadImageW(
            Some(hmodule),
            resource_id(IDI_APP),
            IMAGE_ICON,
            size,
            size,
            LR_SHARED,
        ) else {
            continue;
        };
        SendMessageW(
            hwnd,
            WM_SETICON,
            Some(WPARAM(kind as usize)),
            Some(LPARAM(hicon.0 as isize)),
        );
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
    let _ = SendMessageW(
        sb,
        SB_SETTEXTW,
        Some(WPARAM(0)),
        Some(LPARAM(w.as_ptr() as isize)),
    );
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

/// 路由列宽：固定列按内容给足，接口列吃剩余宽度（窄窗口下沉到下限，交给横向滚动条）。
pub fn route_column_widths(list_client_w: i32) -> [i32; 6] {
    const DEST: i32 = 170; // 192.168.100.0/24 + 余量
    const HOP: i32 = 130; // 255.255.255.255
    const METRIC: i32 = 60;
    const PROTO: i32 = 70;
    const ORIGIN: i32 = 70;
    let iface = (list_client_w - DEST - HOP - METRIC - PROTO - ORIGIN - 2).max(80);
    [DEST, HOP, iface, METRIC, PROTO, ORIGIN]
}

/// # Safety
/// `hwnd` 是主对话框，且 `IDC_ROUTE_LIST` 已创建。
pub unsafe fn set_route_columns(hwnd: HWND) {
    let hl = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32).unwrap_or_default();
    if hl.is_invalid() {
        return;
    }
    for (i, title) in ["目的地址", "下一跳", "接口", "跃点", "协议", "来源"]
        .iter()
        .enumerate()
    {
        let mut t = wide(title);
        let mut item = LVCOLUMNW {
            mask: LVCF_TEXT | LVCF_WIDTH,
            cx: 100,
            pszText: PWSTR(t.as_mut_ptr()),
            cchTextMax: t.len() as i32,
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_INSERTCOLUMNW,
            Some(WPARAM(i)),
            Some(LPARAM((&mut item) as *mut _ as isize)),
        );
    }
    fit_route_columns(hl);
    enable_full_row_select(hl);
}

/// # Safety
/// `hl` 是路由页的 ListView。
unsafe fn fit_route_columns(hl: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hl, &mut rc).is_err() {
        return;
    }
    for (i, w) in route_column_widths(rc.right - rc.left)
        .into_iter()
        .enumerate()
    {
        let _ = SendMessageW(
            hl,
            LVM_SETCOLUMNWIDTH,
            Some(WPARAM(i)),
            Some(LPARAM(w as isize)),
        );
    }
}

/// 接口列文字：优先网卡的本地化名称，查不到时退回 IfIndex。
pub fn route_interface_text(route: &Route, nics: &[Nic]) -> String {
    nics.iter()
        .find(|n| n.if_index == route.if_index)
        .map(|n| n.alias.clone())
        .unwrap_or_else(|| format!("IfIndex {}", route.if_index))
}

/// 填表：先删全部行再插入。路由条目数很少（本机 20~60 行），不需要虚拟列表。
///
/// # Safety
/// `hwnd` 是主对话框，且 `IDC_ROUTE_LIST` 已创建。
pub unsafe fn populate_routes(hwnd: HWND, routes: &[Route]) {
    let hl = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32).unwrap_or_default();
    if hl.is_invalid() {
        return;
    }
    let _ = SendMessageW(hl, LVM_DELETEALLITEMS, None, None);
    let nics = lock_state().nics.clone();
    for (row, r) in routes.iter().enumerate() {
        let cols = [
            r.destination.clone(),
            r.next_hop.clone(),
            route_interface_text(r, &nics),
            r.metric.to_string(),
            r.protocol.clone(),
            r.origin.clone(),
        ];
        let first = wide(&cols[0]);
        // lParam 存 if_index：删除时按接口寻址，不用「接口」列的文字反查
        let mut it = LVITEMW {
            mask: LVIF_TEXT | LVIF_PARAM,
            iItem: row as i32,
            iSubItem: 0,
            pszText: PWSTR(first.as_ptr() as *mut u16),
            cchTextMax: first.len() as i32,
            lParam: LPARAM(r.if_index as isize),
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_INSERTITEMW,
            None,
            Some(LPARAM(&mut it as *mut LVITEMW as isize)),
        );
        for (sub, text) in cols.iter().enumerate().skip(1) {
            let t = wide(text);
            let mut item = LVITEMW {
                mask: LVIF_TEXT,
                iItem: row as i32,
                iSubItem: sub as i32,
                pszText: PWSTR(t.as_ptr() as *mut u16),
                cchTextMax: t.len() as i32,
                ..Default::default()
            };
            let _ = SendMessageW(
                hl,
                LVM_SETITEMTEXTW,
                Some(WPARAM(row)),
                Some(LPARAM(&mut item as *mut LVITEMW as isize)),
            );
        }
    }
    sync_route_buttons(hwnd, hl);
}

/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn routes_are_empty(hwnd: HWND) -> bool {
    let hl = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32).unwrap_or_default();
    hl.is_invalid() || SendMessageW(hl, LVM_GETITEMCOUNT, None, None).0 == 0
}

/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn refresh_routes(hwnd: HWND) {
    match netinfo::list_routes() {
        Ok(routes) => {
            populate_routes(hwnd, &routes);
            set_status(hwnd, &route_status_text(routes.len()));
        }
        Err(e) => {
            populate_routes(hwnd, &[]);
            set_status(hwnd, &format!("读取路由表失败：{e}"));
        }
    }
}

pub fn route_status_text(n: usize) -> String {
    match n {
        0 => "没有可用路由".to_string(),
        1 => "共 1 条路由".to_string(),
        n => format!("共 {n} 条路由"),
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
    // 配置对话框作用于选中的那张卡；未选中时置灰，避免点了没反应
    if let Ok(h_cfg) = GetDlgItem(Some(hwnd), IDC_CONFIG as i32) {
        let _ = EnableWindow(h_cfg, has_sel);
    }
}

/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn open_detail(hwnd: HWND) {
    match selected_nic(list_view(hwnd)) {
        Some(n) => crate::detail::show(hwnd, &n),
        None => set_status(hwnd, "未选择网卡"),
    }
}

/// 改名：对话框拿名字 → 二次确认 → CIM 改名 → 刷新。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_rename(hwnd: HWND) {
    let Some(nic) = selected_nic(list_view(hwnd)) else {
        set_status(hwnd, "未选择网卡");
        return;
    };
    let Some(name) = crate::forms::rename_dialog(hwnd, &nic) else {
        return;
    };
    let question = wide(&format!("把「{}」改名为「{name}」？", nic.alias));
    let title = wide("确认改名");
    let yes = MessageBoxW(
        Some(hwnd),
        PCWSTR(question.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_YESNO | MB_ICONQUESTION,
    ) == IDYES;
    if !yes {
        return;
    }
    let args = netctl::rename_args(nic.if_index, &name);
    match netctl::run_netadapter(&args) {
        Ok(()) => {
            set_status(hwnd, &format!("已改名为 {name}"));
            refresh_list(hwnd);
            sync_buttons(hwnd, list_view(hwnd));
        }
        Err(e) => {
            set_status(hwnd, &format!("改名失败：{e}"));
            let body = wide(&e);
            let title = wide("改名失败");
            let _ = MessageBoxW(
                Some(hwnd),
                PCWSTR(body.as_ptr()),
                PCWSTR(title.as_ptr()),
                MB_OK | MB_ICONERROR,
            );
        }
    }
}

/// 改默认路由网卡 IP 的告警文案；非默认路由返回空串。
pub fn nic_config_confirm(n: &Nic) -> &'static str {
    if n.is_default_route {
        "这张网卡承载默认路由，改动 IP/网关会立刻断网。\n继续后需重新配置网关才能恢复上网。"
    } else {
        ""
    }
}

/// 「配置...」：名称 + 跃点 + IP/DNS 一次改完。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_config(hwnd: HWND) {
    let Some(nic) = selected_nic(list_view(hwnd)) else {
        set_status(hwnd, "未选择网卡");
        return;
    };
    let warn = nic_config_confirm(&nic);
    if !warn.is_empty() {
        let body = wide(warn);
        let title = wide("断网风险");
        let yes = MessageBoxW(
            Some(hwnd),
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        ) == IDYES;
        if !yes {
            return;
        }
    }
    let Some(edit) = crate::forms::nic_config_dialog(hwnd, &nic) else {
        return;
    };
    if edit.name != nic.alias {
        if let Err(e) = netctl::run_netadapter(&netctl::rename_args(nic.if_index, &edit.name)) {
            set_status(hwnd, &format!("改名失败：{e}"));
            return;
        }
    }
    // 跃点：只有真的变了才写。`None` = 自动，与 `metric_auto` 一起比较。
    let metric_changed = match edit.metric {
        None => !nic.metric_auto,
        Some(m) => nic.metric_auto || m != nic.metric,
    };
    if metric_changed {
        if let Err(e) = netctl::apply_metric(nic.if_index, edit.metric) {
            set_status(hwnd, &format!("设置跃点失败：{e}"));
            return;
        }
    }
    // IP/DNS：同样只写变化项；否则每次点「确定」都会把网卡重启一遍。
    let dhcp_now = netctl::dhcp_enabled(nic.if_index).unwrap_or(false);
    let ip_changed = if edit.ip.use_dhcp {
        !dhcp_now
    } else {
        dhcp_now
            || edit.ip.ip != nic.ipv4
            || edit.ip.gw != nic.gateway
            || crate::validate::parse_mask(&nic.mask) != Some(edit.ip.mask)
            || edit.ip.dns != nic.dns
    };
    if ip_changed {
        if let Err(e) = netctl::apply_ip_config(&edit.ip) {
            set_status(hwnd, &format!("设置 IP/DNS 失败：{e}"));
            return;
        }
    }
    set_status(hwnd, "配置已应用");
    refresh_list(hwnd);
    sync_buttons(hwnd, list_view(hwnd));
}

/// 释放 / 续期 DHCP 租约（右键菜单）。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_dhcp_action(hwnd: HWND, renew: bool) {
    let Some(nic) = selected_nic(list_view(hwnd)) else {
        set_status(hwnd, "未选择网卡");
        return;
    };
    let args = if renew {
        netctl::renew_dhcp_args(nic.if_index)
    } else {
        netctl::release_dhcp_args(nic.if_index)
    };
    match netctl::run_netadapter(&args) {
        Ok(()) => {
            set_status(
                hwnd,
                if renew {
                    "已请求续期 DHCP 租约"
                } else {
                    "已释放 DHCP 租约"
                },
            );
            refresh_list(hwnd);
            sync_buttons(hwnd, list_view(hwnd));
        }
        Err(e) => set_status(hwnd, &format!("DHCP 操作失败：{e}")),
    }
}

pub fn reset_warning() -> &'static str {
    "将重置 Winsock 与 TCP/IP 协议栈，并清空 DNS 缓存。\n\n\
     这会删除所有网络适配器上的自定义设置（静态 IP、DNS、路由），\
     且 Winsock 重置需要重启电脑才完全生效。\n\n确定继续？"
}

/// 「重置网络」：破坏性操作，必须二次确认。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_reset(hwnd: HWND) {
    let body = wide(reset_warning());
    let title = wide("重置网络");
    if MessageBoxW(
        Some(hwnd),
        PCWSTR(body.as_ptr()),
        PCWSTR(title.as_ptr()),
        MB_YESNO | MB_ICONWARNING,
    ) != IDYES
    {
        return;
    }
    match netctl::reset_network() {
        Ok(()) => {
            set_status(hwnd, "网络已重置，请重启电脑使 Winsock 重置完全生效");
            refresh_list(hwnd);
            sync_buttons(hwnd, list_view(hwnd));
        }
        Err(e) => set_status(hwnd, &format!("重置网络失败：{e}")),
    }
}

/// 「刷新 DNS 缓存」：步骤无破坏性、可重复执行，不做二次确认。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_flush_dns(hwnd: HWND) {
    match netctl::flush_dns() {
        Ok(()) => set_status(hwnd, "DNS 缓存已刷新"),
        Err(e) => set_status(hwnd, &format!("刷新 DNS 缓存失败：{e}")),
    }
}

/// 目标参数校验：不经过 shell，所以不存在注入；但仍拒绝空白与元字符，
/// 免得把错参数喂给 ping 之后只得到一句看不懂的用法提示。
pub fn valid_target(kind: TestKind, target: &str) -> Result<(), String> {
    if kind == TestKind::IpConfig {
        return Ok(());
    }
    let t = target.trim();
    if t.is_empty() {
        return Err("请输入目标地址或域名".into());
    }
    if t.len() > 255 {
        return Err("目标过长".into());
    }
    if t.chars().any(|c| c.is_whitespace() || "&|;<>\"'".contains(c)) {
        return Err("目标不能包含空格或特殊字符".into());
    }
    Ok(())
}

pub fn test_report(kind: TestKind, target: &str, r: &TestResult) -> String {
    let head = if kind == TestKind::IpConfig {
        format!("=== {} === {}", netctl::test_kind_name(kind), r.summary())
    } else {
        format!(
            "=== {} {} === {}",
            netctl::test_kind_name(kind),
            target,
            r.summary()
        )
    };
    format!("{head}\r\n{}\r\n", r.text)
}

static TEST_BUSY: AtomicBool = AtomicBool::new(false);
pub const WM_TEST_DONE: u32 = WM_APP + 1;

/// 在工作线程跑测试，完成后 PostMessage 回填。UI 线程一秒都不能等：
/// tracert 单次就能跑十几秒。
///
/// # Safety
/// `hwnd` 是主对话框，且在下一次 `WM_TEST_DONE` 之前不能销毁（模态框这一条天然成立）。
pub unsafe fn on_test_run(hwnd: HWND) {
    if TEST_BUSY.swap(true, Ordering::SeqCst) {
        set_status(hwnd, "上一次测试还在进行");
        return;
    }
    let kind = current_test_kind(hwnd);
    let target = crate::forms::read_edit(hwnd, IDC_TEST_TARGET);
    if let Err(e) = valid_target(kind, &target) {
        TEST_BUSY.store(false, Ordering::SeqCst);
        set_status(hwnd, &e);
        return;
    }
    crate::forms::edit_text(
        hwnd,
        IDC_TEST_OUT,
        &format!("正在执行 {} …\r\n", netctl::test_kind_name(kind)),
    );
    set_control_enabled(hwnd, IDC_TEST_RUN, false);

    let h = hwnd.0 as usize;
    let t = target.trim().to_string();
    std::thread::spawn(move || {
        let result = netctl::run_test(kind, &t);
        let report = test_report(kind, &t, &result);
        let payload = Box::into_raw(Box::new(report));
        unsafe {
            let _ = PostMessageW(
                Some(HWND(h as *mut core::ffi::c_void)),
                WM_TEST_DONE,
                WPARAM(0),
                LPARAM(payload as isize),
            );
        }
    });
}

/// # Safety
/// `hwnd` 是主对话框，且测试页下拉框已创建。
pub unsafe fn current_test_kind(hwnd: HWND) -> TestKind {
    let h = GetDlgItem(Some(hwnd), IDC_TEST_KIND as i32).unwrap_or_default();
    let sel = SendMessageW(h, CB_GETCURSEL, None, None).0 as usize;
    match sel {
        1 => TestKind::Tracert,
        2 => TestKind::Nslookup,
        3 => TestKind::IpConfig,
        _ => TestKind::Ping,
    }
}

/// # Safety
/// `hwnd` 是主对话框；`id` 对应的控件不存在时静默忽略。
pub unsafe fn set_control_enabled(hwnd: HWND, id: u32, on: bool) {
    if let Ok(h) = GetDlgItem(Some(hwnd), id as i32) {
        let _ = EnableWindow(h, on);
    }
}

unsafe fn init_test_kind(hwnd: HWND) {
    let h = GetDlgItem(Some(hwnd), IDC_TEST_KIND as i32).unwrap_or_default();
    if h.is_invalid() {
        return;
    }
    for kind in [
        TestKind::Ping,
        TestKind::Tracert,
        TestKind::Nslookup,
        TestKind::IpConfig,
    ] {
        let t = wide(netctl::test_kind_name(kind));
        let _ = SendMessageW(h, CB_ADDSTRING, None, Some(LPARAM(t.as_ptr() as isize)));
    }
    let _ = SendMessageW(h, CB_SETCURSEL, Some(WPARAM(0)), None);
}

/// 选到 Ipconfig 时目标框无意义，直接禁用。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_test_kind_changed(hwnd: HWND) {
    set_control_enabled(hwnd, IDC_TEST_TARGET, current_test_kind(hwnd) != TestKind::IpConfig);
}

/// 删除默认路由前的断网告警；其它路由返回空串。
pub fn route_delete_warning(prefix: &str) -> &'static str {
    if crate::validate::is_default_prefix(prefix) {
        "这是默认路由（0.0.0.0/0），删除后本机将立即断网。\n确定删除？"
    } else {
        ""
    }
}

/// 读路由列表当前选中行：目标、下一跳、ifIndex（lParam 里存的，不按名字反查）。
unsafe fn selected_route(hl: HWND) -> Option<(String, String, u32)> {
    let idx = SendMessageW(
        hl,
        LVM_GETNEXTITEM,
        Some(WPARAM(usize::MAX)),
        Some(LPARAM(LVNI_SELECTED as isize)),
    )
    .0 as i32;
    if idx < 0 {
        return None;
    }
    let cell = |sub: i32| -> String {
        let mut text = [0u16; 128];
        let mut item = LVITEMW {
            iSubItem: sub,
            pszText: PWSTR(text.as_mut_ptr()),
            cchTextMax: text.len() as i32,
            ..Default::default()
        };
        let _ = SendMessageW(
            hl,
            LVM_GETITEMTEXTW,
            Some(WPARAM(idx as usize)),
            Some(LPARAM(&mut item as *mut LVITEMW as isize)),
        );
        crate::winutil::wstr_from_ptr(text.as_ptr())
    };
    let mut item = LVITEMW {
        iItem: idx,
        mask: LVIF_PARAM,
        ..Default::default()
    };
    let _ = SendMessageW(
        hl,
        LVM_GETITEMW,
        Some(WPARAM(0)),
        Some(LPARAM(&mut item as *mut LVITEMW as isize)),
    );
    Some((cell(0), cell(1), item.lParam.0 as u32))
}

/// 路由页的「删除路由」按选中态启停。
///
/// # Safety
/// `hv` 是路由 ListView（`IDC_ROUTE_LIST`）。
pub unsafe fn sync_route_buttons(hwnd: HWND, hv: HWND) {
    let selected = SendMessageW(
        hv,
        LVM_GETNEXTITEM,
        Some(WPARAM(usize::MAX)),
        Some(LPARAM(LVNI_SELECTED as isize)),
    )
    .0 as i32
        >= 0;
    set_control_enabled(hwnd, IDC_ROUTE_DEL, selected);
}

/// 删除选中路由；默认路由要额外确认。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_route_delete(hwnd: HWND) {
    let hl = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32).unwrap_or_default();
    let Some((prefix, hop, if_index)) = selected_route(hl) else {
        set_status(hwnd, "未选择路由");
        return;
    };
    let warn = route_delete_warning(&prefix);
    let (text, title) = if warn.is_empty() {
        (format!("删除路由 {prefix} → {hop}？"), "删除路由")
    } else {
        (warn.to_string(), "断网风险")
    };
    let body = wide(&text);
    let t = wide(title);
    if MessageBoxW(
        Some(hwnd),
        PCWSTR(body.as_ptr()),
        PCWSTR(t.as_ptr()),
        MB_YESNO | MB_ICONWARNING,
    ) != IDYES
    {
        return;
    }
    match netctl::remove_route(if_index, &prefix, &hop) {
        Ok(()) => {
            set_status(hwnd, &format!("已删除路由 {prefix}"));
            refresh_routes(hwnd);
            sync_route_buttons(hwnd, hl);
        }
        Err(e) => set_status(hwnd, &format!("删除路由失败：{e}")),
    }
}

/// 「添加路由」：填表后按 ifIndex 写持久路由。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn on_route_add(hwnd: HWND) {
    let nics = lock_state().nics.clone();
    if nics.is_empty() {
        set_status(hwnd, "暂无可用网卡：先刷新网卡列表");
        return;
    }
    let Some(draft) = crate::forms::route_dialog(hwnd, &nics) else {
        return;
    };
    match netctl::add_route(draft.if_index, &draft.prefix, &draft.next_hop, draft.metric) {
        Ok(()) => {
            set_status(hwnd, &format!("已添加路由 {}", draft.prefix));
            refresh_routes(hwnd);
        }
        Err(e) => set_status(hwnd, &format!("添加路由失败：{e}")),
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
        (IDM_DETAIL, "详细信息", has_sel),
        (IDM_RENAME, "重命名…", has_sel),
        (IDM_RELEASE, "释放 DHCP 租约", has_sel),
        (IDM_RENEW, "续期 DHCP 租约", has_sel),
    ];
    for (id, label, usable) in menus {
        let flags = if usable {
            MF_STRING
        } else {
            MF_STRING | MF_GRAYED
        };
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
        let r = MessageBoxW(
            None,
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_YESNO | MB_ICONWARNING,
        );
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
        let _ = MessageBoxW(
            Some(hwnd),
            PCWSTR(body.as_ptr()),
            PCWSTR(title.as_ptr()),
            MB_OK | MB_ICONERROR,
        );
        set_status(hwnd, &format!("{verb}「{}」失败", n.alias));
    } else {
        set_status(hwnd, &format!("已{verb}「{}」", n.alias));
    }
    refresh_list(hwnd);
    sync_buttons(hwnd, hl);
}

/// 列表字体：Segoe UI，进程内只建一次（OnceLock 存裸句柄值以满足 Sync）。
/// 负高度按字符高度取整，比默认 MS Shell Dlg 略大，行高随之增高、更易读。
pub fn ui_font() -> HFONT {
    static FONT: OnceLock<usize> = OnceLock::new();
    let raw = *FONT.get_or_init(|| unsafe {
        let face = wide("Segoe UI");
        CreateFontW(
            -17,
            0,
            0,
            0,
            FW_NORMAL.0 as i32,
            0,
            0,
            0,
            DEFAULT_CHARSET,
            OUT_DEFAULT_PRECIS,
            CLIP_DEFAULT_PRECIS,
            CLEARTYPE_QUALITY,
            0,
            PCWSTR(face.as_ptr()),
        )
        .0 as usize
    });
    HFONT(raw as *mut std::ffi::c_void)
}

/// 最小化时收缩工作集：把本进程占用的物理页交还系统，降低任务管理器里的可见占用。
/// 私有提交量（验收口径）不受影响；纯展示层优化，事件驱动、无常驻轮询。
fn trim_working_set() {
    unsafe {
        let _ = EmptyWorkingSet(GetCurrentProcess());
    }
}

/// 导航项整块自绘：底色 + 左侧强调条 + 居中文字。
///
/// # Safety
/// `dis` 来自 WM_DRAWITEM，`hDC` 与 `rcItem` 由系统填好。
unsafe fn draw_nav_item(dis: &DRAWITEMSTRUCT) {
    let selected = page_of_nav(dis.CtlID) == Some(current_page());
    let (bg, fg, accent) = theme::nav_colors(selected);
    let brush = CreateSolidBrush(bg);
    FillRect(dis.hDC, &dis.rcItem, brush);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    if selected {
        let mut bar = dis.rcItem;
        bar.right = bar.left + 3;
        let b = CreateSolidBrush(accent);
        FillRect(dis.hDC, &bar, b);
        let _ = DeleteObject(HGDIOBJ(b.0));
    }
    let mut buf = [0u16; 64];
    let n = GetWindowTextW(dis.hwndItem, &mut buf);
    let text = String::from_utf16_lossy(&buf[..n as usize]);
    let mut t = wide(&text);
    let len = t.len() - 1;
    let mut r = dis.rcItem;
    let _ = SetBkMode(dis.hDC, TRANSPARENT);
    let _ = SetTextColor(dis.hDC, fg);
    DrawTextW(
        dis.hDC,
        &mut t[..len],
        &mut r,
        DT_CENTER | DT_VCENTER | DT_SINGLELINE,
    );
}

/// 自绘状态胶囊：圆角底色 + 同色圆点 + 文字。返回 CDRF_SKIPDEFAULT 后本格背景须自己填。
///
/// # Safety
/// `hdc` 必须是 custom-draw 通知里有效的设备上下文；`cell` 是该格矩形。
pub unsafe fn draw_status_pill(
    hdc: HDC,
    cell: &RECT,
    row_bg: COLORREF,
    fg: COLORREF,
    pill_bg: COLORREF,
    text: &str,
) {
    // 单元格背景（隔行浅灰 / 选中浅蓝）
    let bg_brush = CreateSolidBrush(row_bg);
    FillRect(hdc, cell, bg_brush);
    let _ = DeleteObject(HGDIOBJ(bg_brush.0));

    SetBkMode(hdc, TRANSPARENT);
    let mut buf: Vec<u16> = text.encode_utf16().collect();

    // 量文字尺寸（DT_CALCRECT 只算不画）
    let mut m = RECT::default();
    DrawTextW(
        hdc,
        &mut buf,
        &mut m,
        DT_CALCRECT | DT_SINGLELINE | DT_NOPREFIX,
    );
    let text_w = (m.right - m.left).max(0);
    let text_h = (m.bottom - m.top).max(0);

    // 胶囊几何：左边距 + 圆点 + 间距 + 文字 + 右留白；垂直居中于单元格
    let pad_x = 8;
    let dot_d = 6;
    let gap = 6;
    let cell_h = cell.bottom - cell.top;
    let pill_h = (cell_h - 6).max(text_h + 4).min(cell_h);
    let pill_w = pad_x + dot_d + gap + text_w + pad_x;
    let left = cell.left + 6;
    let top = cell.top + ((cell_h - pill_h) / 2).max(0);
    let right = left + pill_w;
    let bottom = top + pill_h;

    // 圆角底：笔与刷同色，边框不可见，呈柔和胶囊
    let brush = CreateSolidBrush(pill_bg);
    let pen = CreatePen(PS_SOLID, 1, pill_bg);
    let old_b = SelectObject(hdc, HGDIOBJ(brush.0));
    let old_p = SelectObject(hdc, HGDIOBJ(pen.0));
    let _ = RoundRect(hdc, left, top, right, bottom, pill_h, pill_h);
    SelectObject(hdc, old_b);
    SelectObject(hdc, old_p);
    let _ = DeleteObject(HGDIOBJ(brush.0));
    let _ = DeleteObject(HGDIOBJ(pen.0));

    // 前景圆点
    let dot_brush = CreateSolidBrush(fg);
    let dot_pen = CreatePen(PS_SOLID, 1, fg);
    let old_b = SelectObject(hdc, HGDIOBJ(dot_brush.0));
    let old_p = SelectObject(hdc, HGDIOBJ(dot_pen.0));
    let cy = top + pill_h / 2;
    let dl = left + pad_x;
    let _ = Ellipse(hdc, dl, cy - dot_d / 2, dl + dot_d, cy + dot_d / 2);
    SelectObject(hdc, old_b);
    SelectObject(hdc, old_p);
    let _ = DeleteObject(HGDIOBJ(dot_brush.0));
    let _ = DeleteObject(HGDIOBJ(dot_pen.0));

    // 文字
    SetTextColor(hdc, fg);
    let mut tr = RECT {
        left: dl + dot_d + gap,
        top,
        right: right - pad_x,
        bottom,
    };
    DrawTextW(
        hdc,
        &mut buf,
        &mut tr,
        DT_SINGLELINE | DT_VCENTER | DT_LEFT | DT_NOPREFIX | DT_END_ELLIPSIS,
    );
}

/// 浅色精修：隔行浅灰底、选中整行柔和浅蓝、名称深色 / IPv4·网关浅灰，
/// 状态列自绘圆角胶囊（灰=已禁用、蓝=已启用未连接、绿=已连接）。
/// 三级请求：整表 → 逐项 → 逐项每列；第 0 列自绘并 SKIPDEFAULT，其余列改色回 NEWFONT。
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
        // CDRF_NOTIFYITEMDRAW 才会换来 ITEMPREPAINT 通知
        return Some(CDRF_NOTIFYITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 {
        return Some(CDRF_NOTIFYSUBITEMDRAW as isize);
    }
    if stage == CDDS_ITEMPREPAINT.0 | CDDS_SUBITEM.0 {
        let row = cd.nmcd.dwItemSpec;
        // 不信任 uItemState（实测分组报表下它对每行都置 CDIS_SELECTED，导致整表像全选）；
        // 用 LVM_GETNEXTITEM 的真实选中行回查，未选中时返回 None。
        let selected = selected_index(list_view(hwnd)) == Some(row);
        let bg = theme::row_bg(selected, row);
        if cd.iSubItem == 1 {
            let (fg, pill_bg, text) = {
                let guard = lock_state();
                match guard.nics.get(row) {
                    Some(n) => {
                        let (fg, pbg) = theme::status_colors(n.enabled, n.connected);
                        (fg, pbg, status_text(n))
                    }
                    None => return Some(CDRF_DODEFAULT as isize),
                }
            };
            draw_status_pill(cd.nmcd.hdc, &cd.nmcd.rc, bg, fg, pill_bg, &text);
            return Some(CDRF_SKIPDEFAULT as isize);
        }
        // 名称列深色突出，IPv4/网关浅灰次要；选中行统一深蓝文字
        cd.clrText = if selected {
            theme::SELECT_TEXT
        } else if cd.iSubItem == 0 {
            theme::TEXT
        } else {
            theme::TEXT_DIM
        };
        cd.clrTextBk = bg;
        return Some(CDRF_NEWFONT as isize);
    }
    None
}

/// 只显示目标页面的控件。模板不负责可见性，这里是唯一真相。
///
/// # Safety
/// `hwnd` 是主对话框；缺失的控件会被 `GetDlgItem` 过滤掉，不会崩。
pub unsafe fn apply_page(hwnd: HWND, page: Page) {
    for p in [Page::Nic, Page::Route, Page::Test] {
        let show = if p == page { SW_SHOW } else { SW_HIDE };
        for id in page_ids(p) {
            if let Ok(h) = GetDlgItem(Some(hwnd), *id as i32) {
                let _ = ShowWindow(h, show);
            }
        }
    }
    set_current_page(page);
    // 首次切到路由页才采集，避免每次点击都跑一遍 IP Helper
    if page == Page::Route {
        if routes_are_empty(hwnd) {
            refresh_routes(hwnd);
        }
        if let Ok(hv) = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32) {
            sync_route_buttons(hwnd, hv);
        }
    }
    for id in NAV_IDS {
        if let Ok(h) = GetDlgItem(Some(hwnd), *id as i32) {
            let _ = InvalidateRect(Some(h), None, true);
        }
    }
    layout(hwnd);
}

/// 工具条底边 = 该页所有工具条控件客户区底边的最大值 + 6px。
/// 取最大值而不是「第一个控件」：测试页第一个控件是静态标签，比同行按钮矮。
/// 一个都量不到时返回 `None`，由调用方退回模板顶边 —— 绝不把列表挪到控件上面。
///
/// # Safety
/// `hwnd` 是主对话框。
unsafe fn toolbar_bottom(hwnd: HWND, page: Page) -> Option<i32> {
    let mut bottom: Option<i32> = None;
    for id in page_toolbar_ids(page) {
        let Ok(ctl) = GetDlgItem(Some(hwnd), *id as i32) else {
            continue;
        };
        let mut r = RECT::default();
        if GetWindowRect(ctl, &mut r).is_err() {
            continue;
        }
        let mut pt = POINT {
            x: r.left,
            y: r.bottom,
        };
        if !ScreenToClient(hwnd, &mut pt).as_bool() {
            continue;
        }
        bottom = Some(bottom.map_or(pt.y, |b: i32| b.max(pt.y)));
    }
    bottom.map(|y| y + 6)
}

/// 窗口缩放时让大块控件跟随：导航列右侧、本页工具条下方、状态栏上方。
/// 导航列宽不写死像素：从导航按钮的实际右边界量出来，任意 DPI 下都贴合。
///
/// # Safety
/// `hwnd` 是主对话框。
pub unsafe fn layout(hwnd: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hwnd, &mut rc).is_err() {
        return;
    }
    let page = current_page();

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

    let nav = menu_right_edge(hwnd, list_view(hwnd));
    let top = toolbar_bottom(hwnd, page).unwrap_or(24);
    let (x, y, w, h) = list_bounds(
        rc.right - rc.left,
        rc.bottom - rc.top,
        nav,
        top,
        sb_height,
        LIST_MARGIN,
    );
    for id in page_fill_ids(page) {
        if let Ok(hctl) = GetDlgItem(Some(hwnd), *id as i32) {
            let _ = MoveWindow(hctl, x, y, w, h, true);
        }
    }
    if page == Page::Nic {
        fit_description_column(list_view(hwnd));
    } else if page == Page::Route {
        if let Ok(hl) = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32) {
            fit_route_columns(hl);
        }
    }
}

/// 左侧菜单列的右边界（客户区 px）。量不到任何按钮时退回列表当前的左边距，
/// 也就是「让它待在模板位置」，而不是把列表挪到 x=margin 盖住按钮。
///
/// # Safety
/// `hwnd` 是主对话框，`hl` 是其列表控件。
unsafe fn menu_right_edge(hwnd: HWND, hl: HWND) -> i32 {
    let mut edges: Vec<i32> = Vec::with_capacity(NAV_IDS.len());
    for id in NAV_IDS {
        let btn = GetDlgItem(Some(hwnd), *id as i32).unwrap_or_default();
        if btn.is_invalid() {
            continue;
        }
        let mut br = RECT::default();
        if GetWindowRect(btn, &mut br).is_ok() {
            let mut pt = POINT {
                x: br.right,
                y: br.top,
            };
            if ScreenToClient(hwnd, &mut pt).as_bool() {
                edges.push(pt.x);
            }
        }
    }
    let mut fallback = 0;
    let mut lr = RECT::default();
    if GetWindowRect(hl, &mut lr).is_ok() {
        let mut pt = POINT {
            x: lr.left,
            y: lr.top,
        };
        if ScreenToClient(hwnd, &mut pt).as_bool() {
            fallback = (pt.x - LIST_MARGIN).max(0);
        }
    }
    nav_right(&edges, fallback)
}

/// 描述列跟着窗口宽度伸缩（窗口可拖拽），避免固定宽度被视口硬裁。
///
/// # Safety
/// `hl` 是主列表 ListView。
unsafe fn fit_description_column(hl: HWND) {
    let mut rc = RECT::default();
    if GetClientRect(hl, &mut rc).is_err() {
        return;
    }
    let w = desc_column_width(rc.right - rc.left);
    let _ = SendMessageW(
        hl,
        LVM_SETCOLUMNWIDTH,
        Some(WPARAM(COL_DESC_INDEX as usize)),
        Some(LPARAM(w as isize)),
    );
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
            set_title_icons(hwnd);
            let hl = list_view(hwnd);
            enable_full_row_select(hl);
            if !hl.is_invalid() {
                SendMessageW(
                    hl,
                    WM_SETFONT,
                    Some(WPARAM(ui_font().0 as usize)),
                    Some(LPARAM(1)),
                );
            }
            set_columns(hwnd);
            set_route_columns(hwnd);
            ensure_groups(hwnd);
            init_status_bar(hwnd);
            init_test_kind(hwnd);
            refresh_list(hwnd);
            sync_buttons(hwnd, list_view(hwnd));
            apply_page(hwnd, Page::Nic);
            1
        }
        WM_DRAWITEM => {
            let dis = &*(lparam.0 as *const DRAWITEMSTRUCT);
            if NAV_IDS.contains(&dis.CtlID) {
                draw_nav_item(dis);
                1
            } else {
                0
            }
        }
        // 主窗口与详情统一白底：直接在擦除阶段填白（WM_CTLCOLORDLG 在对话框过程里被忽略过）。
        WM_ERASEBKGND => {
            let hdc = HDC(wparam.0 as *mut core::ffi::c_void);
            let mut rc = RECT::default();
            if GetClientRect(hwnd, &mut rc).is_ok() {
                let brush = CreateSolidBrush(theme::BG);
                FillRect(hdc, &rc, brush);
                let _ = DeleteObject(HGDIOBJ(brush.0));
            }
            1
        }
        WM_COMMAND => {
            let id = (wparam.0 & 0xFFFF) as u32;
            let code = ((wparam.0 >> 16) & 0xFFFF) as u32;
            if let Some(page) = page_of_nav(id) {
                apply_page(hwnd, page);
                return 1;
            }
            let hl = list_view(hwnd);
            match id {
                IDC_REFRESH => {
                    refresh_list(hwnd);
                    sync_buttons(hwnd, hl);
                }
                IDC_ENABLE | IDM_ENABLE => on_toggle(hwnd, hl, true),
                IDC_DISABLE | IDM_DISABLE => on_toggle(hwnd, hl, false),
                IDC_DETAIL | IDM_DETAIL => open_detail(hwnd),
                IDC_CONFIG => on_config(hwnd),
                IDM_RENAME => on_rename(hwnd),
                IDM_RELEASE => on_dhcp_action(hwnd, false),
                IDM_RENEW => on_dhcp_action(hwnd, true),
                IDC_FLUSH_DNS => on_flush_dns(hwnd),
                IDC_RESET => on_reset(hwnd),
                IDC_ROUTE_REFRESH => refresh_routes(hwnd),
                IDC_ROUTE_ADD => on_route_add(hwnd),
                IDC_ROUTE_DEL => on_route_delete(hwnd),
                IDC_TEST_RUN => on_test_run(hwnd),
                IDC_TEST_KIND if code == CBN_SELCHANGE => on_test_kind_changed(hwnd),
                _ => return 0,
            }
            1
        }
        WM_TEST_DONE => {
            // 所有权移交：Box 在工作线程里 into_raw，这里唯一一次 from_raw。
            let report = *Box::from_raw(lparam.0 as *mut String);
            TEST_BUSY.store(false, Ordering::SeqCst);
            set_control_enabled(hwnd, IDC_TEST_RUN, true);
            crate::forms::edit_text(hwnd, IDC_TEST_OUT, &report);
            set_status(hwnd, "测试完成");
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
            if nmh.code == LVN_ITEMCHANGED {
                if let Ok(hv) = GetDlgItem(Some(hwnd), IDC_ROUTE_LIST as i32) {
                    if nmh.hwndFrom == hv {
                        sync_route_buttons(hwnd, hv);
                        return 0;
                    }
                }
            }
            match on_list_notify(hwnd, nmh) {
                Some(result) => {
                    // 对话框过程的返回值只是「是否处理」BOOL；custom-draw 的 CDRF 结果
                    // 必须写进 DWLP_MSGRESULT，否则永远被当成 0(DODEFAULT)，自绘/着色全失效。
                    SetWindowLongPtrW(hwnd, WINDOW_LONG_PTR_INDEX(DWLP_MSGRESULT as i32), result);
                    1
                }
                None => 0,
            }
        }
        WM_SIZE => {
            layout(hwnd);
            0
        }
        WM_SYSCOMMAND => {
            // 低 4 位另有用途，按文档掩掉再比；返回 0 让系统继续默认最小化
            if (wparam.0 & 0xFFF0) == SC_MINIMIZE as usize {
                trim_working_set();
            }
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
            metric_auto: true,
            network_guid: String::new(),
        }
    }

    /// 路由页测试用的最小网卡。
    fn test_nic(if_index: u32, alias: &str, kind: NicKind) -> Nic {
        Nic {
            if_index,
            alias: alias.into(),
            description: String::new(),
            kind,
            mac: String::new(),
            ipv4: String::new(),
            mask: String::new(),
            gateway: String::new(),
            dns: Vec::new(),
            speed: String::new(),
            enabled: true,
            connected: true,
            is_default_route: false,
            metric: 0,
            metric_auto: true,
            network_guid: String::new(),
        }
    }

    #[test]
    fn status_text_three_states() {
        assert_eq!(
            status_text(&nic(false, true, NicKind::Wired, "a")),
            "已禁用"
        );
        assert_eq!(
            status_text(&nic(false, false, NicKind::Wired, "a")),
            "已禁用"
        );
        assert_eq!(status_text(&nic(true, true, NicKind::Wired, "a")), "已连接");
        assert_eq!(
            status_text(&nic(true, false, NicKind::Wired, "a")),
            "已启用"
        );
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
        assert!(!needs_disable_confirm(&nic(
            true,
            true,
            NicKind::Wireless,
            "WLAN"
        )));
    }

    /// 重置网络的告警必须说清「要重启」—— 不说清的提示等于没提示。
    #[test]
    fn reset_warning_mentions_reboot() {
        assert!(reset_warning().contains("重启"));
    }

    #[test]
    fn deleting_default_route_warns_about_disconnect() {
        assert!(route_delete_warning("0.0.0.0/0").contains("断网"));
        assert_eq!(route_delete_warning("10.20.0.0/24"), "");
    }

    /// 改 IP 的断网告警只在默认路由网卡上出现，否则每次点「确定」都白弹一次。
    #[test]
    fn ip_change_warns_only_for_default_route() {
        let mut n = test_nic(19, "WLAN", NicKind::Wireless);
        n.is_default_route = true;
        assert!(nic_config_confirm(&n).contains("断网"));
        n.is_default_route = false;
        assert_eq!(nic_config_confirm(&n), "");
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
        for k in [
            NicKind::Wired,
            NicKind::Wireless,
            NicKind::Vpn,
            NicKind::Virtual,
            NicKind::Other,
        ] {
            assert_eq!(group_of(k), group_order(k) as i32);
        }
        assert_eq!(group_of(NicKind::Wired), 0);
        assert_eq!(group_of(NicKind::Wireless), 1);
        assert_eq!(group_of(NicKind::Vpn), 2);
        assert_eq!(group_of(NicKind::Virtual), 3);
        assert_eq!(group_of(NicKind::Other), 4);
    }

    /// 列宽契约：IPv4/网关 必须装得下 15 字符地址，描述列吃满剩余宽度且不低于下限。
    #[test]
    fn column_widths_fit_address_length_and_fill_remainder() {
        // 15 字符地址（169.254.206.73 / 192.168.100.200）实测约 130px，留 20px 余量
        const {
            assert!(COL_IP >= 140, "IPv4 列宽不足以显示完整地址");
            assert!(COL_GW >= 140, "网关列宽不足以显示完整地址");
        }
        // 默认窗口列表客户区约 722px：描述列应把剩余宽度全部吃掉
        assert_eq!(desc_column_width(722), 722 - (150 + 104 + 150 + 148) - 2);
        // 窗口拉大 → 描述列变宽（线性跟随）
        assert_eq!(
            desc_column_width(922) - desc_column_width(722),
            200,
            "描述列应随窗口宽度线性变化"
        );
        // 窗口极窄时不低于下限，此时横向滚动条兜底
        assert_eq!(desc_column_width(300), COL_DESC_MIN);
        assert_eq!(desc_column_width(0), COL_DESC_MIN);
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
            ("IDC_DETAIL", IDC_DETAIL),
            ("IDC_RESET", IDC_RESET),
            ("IDC_CONFIG", IDC_CONFIG),
            ("IDC_ROUTE_LIST", IDC_ROUTE_LIST),
            ("IDC_ROUTE_REFRESH", IDC_ROUTE_REFRESH),
            ("IDC_ROUTE_ADD", IDC_ROUTE_ADD),
            ("IDC_ROUTE_DEL", IDC_ROUTE_DEL),
            ("IDC_TEST_LABEL", IDC_TEST_LABEL),
            ("IDC_TEST_TARGET", IDC_TEST_TARGET),
            ("IDC_TEST_KIND", IDC_TEST_KIND),
            ("IDC_TEST_RUN", IDC_TEST_RUN),
            ("IDC_TEST_OUT", IDC_TEST_OUT),
            ("IDC_NAV_NIC", IDC_NAV_NIC),
            ("IDC_NAV_ROUTE", IDC_NAV_ROUTE),
            ("IDC_NAV_TEST", IDC_NAV_TEST),
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

    /// 模板里的一个控件矩形（DLU）。
    #[derive(Copy, Clone)]
    struct TRect {
        x: i32,
        y: i32,
        w: i32,
        h: i32,
    }

    /// IDD_MAIN 模板解析结果。
    struct Template {
        buttons: Vec<(u32, TRect)>,
        list: TRect,
        width: i32,
    }

    /// 模板里所有按钮的 (ID, 符号名)。加按钮必须同步登记，否则解析时直接 panic。
    const BUTTON_NAMES: &[(u32, &str)] = &[
        (IDC_NAV_NIC, "IDC_NAV_NIC"),
        (IDC_NAV_ROUTE, "IDC_NAV_ROUTE"),
        (IDC_NAV_TEST, "IDC_NAV_TEST"),
        (IDC_ENABLE, "IDC_ENABLE"),
        (IDC_DISABLE, "IDC_DISABLE"),
        (IDC_CONFIG, "IDC_CONFIG"),
        (IDC_DETAIL, "IDC_DETAIL"),
        (IDC_REFRESH, "IDC_REFRESH"),
        (IDC_FLUSH_DNS, "IDC_FLUSH_DNS"),
        (IDC_RESET, "IDC_RESET"),
        (IDC_ROUTE_REFRESH, "IDC_ROUTE_REFRESH"),
        (IDC_ROUTE_ADD, "IDC_ROUTE_ADD"),
        (IDC_ROUTE_DEL, "IDC_ROUTE_DEL"),
        (IDC_TEST_RUN, "IDC_TEST_RUN"),
    ];

    /// 控件 ID → .rc 里的符号名。
    fn id_name(id: u32) -> &'static str {
        const OTHER: &[(u32, &str)] = &[
            (IDC_LIST, "IDC_LIST"),
            (IDC_ROUTE_LIST, "IDC_ROUTE_LIST"),
            (IDC_TEST_LABEL, "IDC_TEST_LABEL"),
            (IDC_TEST_TARGET, "IDC_TEST_TARGET"),
            (IDC_TEST_KIND, "IDC_TEST_KIND"),
            (IDC_TEST_OUT, "IDC_TEST_OUT"),
        ];
        BUTTON_NAMES
            .iter()
            .chain(OTHER)
            .find(|(i, _)| *i == id)
            .map(|(_, name)| *name)
            .unwrap_or_else(|| panic!("未登记的控件 {id}"))
    }

    /// 解析 IDD_MAIN 模板。
    ///
    /// 位置有两套真相 —— `.rc` 模板和 `layout()` 的运行时 MoveWindow。`.rc` 写歪了
    /// 不会被任何 Rust 代码感知（运行时会被纠正成「看起来对」的布局），模板改回横向一排
    /// 也照样跑得起来，所以这里直接拿模板文本当契约来钉。
    fn main_dialog_template() -> Template {
        let mut buttons = Vec::new();
        let mut list = TRect {
            x: -1,
            y: -1,
            w: -1,
            h: -1,
        };
        let mut width = 0;
        for stmt in dialog_statements("IDD_MAIN") {
            if stmt.starts_with("IDD_MAIN DIALOGEX") {
                // IDD_MAIN DIALOGEX 0, 0, 620, 380
                width = numbers(&stmt)[2];
            } else if stmt.starts_with("PUSHBUTTON") {
                let id = BUTTON_NAMES
                    .iter()
                    .find(|(_, name)| stmt.contains(name))
                    .map(|(id, _)| *id)
                    .unwrap_or_else(|| panic!("按钮不在 BUTTON_NAMES 里：{stmt}"));
                buttons.push((id, geometry(&stmt)));
            } else if stmt.contains("IDC_LIST") {
                list = geometry(&stmt);
            }
        }
        Template {
            buttons,
            list,
            width,
        }
    }

    /// 语句末尾的四个数字就是 x, y, w, h（标签里出现数字也不会串位）。
    fn geometry(statement: &str) -> TRect {
        let nums = numbers(statement);
        let n = nums.len();
        assert!(n >= 4, "语句里没有几何数字：{statement}");
        TRect {
            x: nums[n - 4],
            y: nums[n - 3],
            w: nums[n - 2],
            h: nums[n - 1],
        }
    }

    /// 把 `name` 这个 DIALOGEX 块拆成逻辑语句（含模板头行）。rc 里一条控件语句可以跨行写
    /// （以逗号结尾），逐行读会把几何数字落到下一条「语句」里。
    fn dialog_statements(name: &str) -> Vec<String> {
        let rc = include_str!("../res/app.rc");
        let mut out = Vec::new();
        let mut current = String::new();
        let mut inside = false;
        for line in rc.lines() {
            let t = line.trim();
            if !inside {
                inside = t.starts_with(name);
                if !inside {
                    continue;
                }
            }
            if t == "END" {
                break;
            }
            if !current.is_empty() {
                current.push(' ');
            }
            current.push_str(t);
            if !t.ends_with(',') {
                out.push(std::mem::take(&mut current));
            }
        }
        assert!(!out.is_empty(), "{name} 模板没解析出任何语句");
        out
    }

    fn numbers(text: &str) -> Vec<i32> {
        text.replace(',', " ")
            .split_whitespace()
            .filter_map(|w| w.parse().ok())
            .collect()
    }

    /// 三个页面必须两两不相交，且每个 ID 都在 .rc 模板里真实存在。
    #[test]
    fn page_ids_are_disjoint_and_present_in_template() {
        use std::collections::HashSet;
        let rc = include_str!("../res/app.rc");
        let mut seen = HashSet::new();
        for p in [Page::Nic, Page::Route, Page::Test] {
            for id in page_ids(p) {
                assert!(seen.insert(*id), "控件 {id} 被两个页面同时声明");
            }
            for id in page_ids(p) {
                assert!(
                    rc.contains(id_name(*id)),
                    "页面 {:?} 的控件 {id} 不在 res/app.rc 里",
                    p
                );
            }
        }
        // 导航项不属于任何页面（永远可见）
        for id in NAV_IDS {
            assert!(!seen.contains(id), "导航项 {id} 不能混进页面控件");
        }
    }

    /// 大块区域控件必须包含在该页面的全部控件里，否则切换页面会留下没被隐藏的空框。
    #[test]
    fn fill_controls_belong_to_their_page() {
        for p in [Page::Nic, Page::Route, Page::Test] {
            for id in page_fill_ids(p) {
                assert!(page_ids(p).contains(id), "{id} 不在页面 {:?} 的全集里", p);
            }
        }
        assert_eq!(page_fill_ids(Page::Nic), &[IDC_LIST]);
        assert_eq!(page_fill_ids(Page::Route), &[IDC_ROUTE_LIST]);
        assert_eq!(page_fill_ids(Page::Test), &[IDC_TEST_OUT]);
    }

    #[test]
    fn nav_ids_round_trip() {
        assert_eq!(page_of_nav(nav_of_page(Page::Nic)), Some(Page::Nic));
        assert_eq!(page_of_nav(nav_of_page(Page::Route)), Some(Page::Route));
        assert_eq!(page_of_nav(nav_of_page(Page::Test)), Some(Page::Test));
        assert_eq!(page_of_nav(IDC_LIST), None);
        // 三个导航项互不相同，否则点一个会选中两个
        let ids: Vec<u32> = [Page::Nic, Page::Route, Page::Test]
            .into_iter()
            .map(nav_of_page)
            .collect();
        let uniq: std::collections::HashSet<u32> = ids.iter().copied().collect();
        assert_eq!(uniq.len(), 3);
    }

    /// 导航是左侧一竖列，工具条是右侧一横行 —— 两者都不许混。
    #[test]
    fn nav_is_vertical_and_toolbar_is_horizontal() {
        let t = main_dialog_template();
        let nav: Vec<TRect> = t
            .buttons
            .iter()
            .filter(|(id, _)| NAV_IDS.contains(id))
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(nav.len(), 3);
        for i in 1..nav.len() {
            let r = nav[i];
            assert_eq!(r.x, nav[0].x, "导航项必须同列");
            assert!(
                r.y >= nav[i - 1].y + nav[i - 1].h,
                "导航项垂直重叠：y {} 压在 {} 上",
                r.y,
                nav[i - 1].y
            );
        }
        let toolbar: Vec<TRect> = t
            .buttons
            .iter()
            .filter(|(id, _)| page_toolbar_ids(Page::Nic).contains(id))
            .map(|(_, r)| *r)
            .collect();
        assert_eq!(toolbar.len(), page_toolbar_ids(Page::Nic).len());
        for r in &toolbar[1..] {
            assert_eq!(r.y, toolbar[0].y, "工具条必须同一行");
            assert!(r.x > toolbar[0].x, "工具条必须自左向右");
        }
        // 工具条整体在导航列右侧
        let nav_right = nav.iter().map(|r| r.x + r.w).max().unwrap();
        assert!(toolbar.iter().all(|r| r.x >= nav_right));
        // 导航列不占右半窗，且列表初始矩形也在它右侧
        assert!(nav_right < t.width / 2, "导航列宽 {nav_right} 超过半窗");
        assert!(
            toolbar.iter().all(|r| r.x + r.w <= t.width),
            "工具条超出窗口"
        );
        assert!(t.list.x >= nav_right, "列表初始位置压在导航列上");
    }

    /// 菜单列右边界取最宽按钮；一个控件都没量到时退回兜底宽度，不能让列表盖住菜单。
    #[test]
    fn nav_right_uses_widest_button_and_never_shrinks_below_fallback() {
        assert_eq!(nav_right(&[80, 96, 74], 60), 96);
        assert_eq!(nav_right(&[70], 100), 100);
        assert_eq!(nav_right(&[], 100), 100);
    }

    #[test]
    fn route_columns_fit_and_fill() {
        let w = route_column_widths(900);
        assert_eq!(w.len(), 6);
        // 目的 / 下一跳 / 接口 / 跃点 / 协议 / 来源
        assert!(w[0] >= 150, "目的列装不下 192.168.100.0/24");
        assert!(w[1] >= 110);
        assert!(w[3] >= 60, "跃点列装不下 4 位数");
        assert_eq!(
            w.iter().sum::<i32>() + 2,
            900,
            "六列宽度之和应吃满视口（留 2px 边框）"
        );
        // 窄窗口下不低于各自下限，交给横向滚动条
        let narrow = route_column_widths(300);
        assert!(narrow.iter().all(|c| *c > 0));
    }

    #[test]
    fn route_interface_shows_alias_or_index() {
        let route = Route {
            destination: "0.0.0.0/0".into(),
            next_hop: "192.168.1.1".into(),
            if_index: 19,
            metric: 25,
            protocol: "DHCP".into(),
            origin: "DHCP".into(),
            loopback: false,
            immortal: false,
        };
        let nic = test_nic(19, "WLAN", NicKind::Wireless);
        assert_eq!(route_interface_text(&route, &[nic]), "WLAN");
        assert_eq!(route_interface_text(&route, &[]), "IfIndex 19");
    }

    /// 列表矩形：导航右侧、工具条下方、状态栏上方；极小窗口夹到 0，负宽高会让控件直接不画。
    #[test]
    fn list_sits_right_of_menu_and_above_statusbar() {
        assert_eq!(list_bounds(900, 600, 140, 40, 22, 5), (145, 40, 750, 533));
        assert_eq!(list_bounds(100, 30, 140, 40, 22, 5), (145, 40, 0, 0));
    }

    #[test]
    fn target_validation_requires_input_except_for_ipconfig() {
        assert!(valid_target(TestKind::Ping, "10.0.0.1").is_ok());
        assert!(valid_target(TestKind::Ping, "  ").is_err());
        assert!(valid_target(TestKind::Ping, "10.0.0.1 & del x").is_err(), "不许带 shell 元字符");
        assert!(valid_target(TestKind::Nslookup, "example.com").is_ok());
        assert!(valid_target(TestKind::IpConfig, "").is_ok());
    }

    #[test]
    fn route_status_counts_rows() {
        assert_eq!(route_status_text(0), "没有可用路由");
        assert_eq!(route_status_text(1), "共 1 条路由");
        assert_eq!(route_status_text(58), "共 58 条路由");
    }

    /// 报告头必须包含命令与结果摘要，否则用户不知道这段输出来自哪次测试。
    #[test]
    fn report_header_has_kind_target_and_summary() {
        let r = TestResult { text: "Pinging 10.0.0.1".into(), code: Some(0) };
        let s = test_report(TestKind::Ping, "10.0.0.1", &r);
        assert!(s.contains("Ping 10.0.0.1"), "{s}");
        assert!(s.contains("完成"), "{s}");
        assert!(s.contains("Pinging 10.0.0.1"), "{s}");
    }
}
