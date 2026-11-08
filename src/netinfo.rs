// Task 4：netinfo —— 基于 IP Helper API 的网卡信息采集。
// 模块挂载约定：本仓库统一 `pub mod xxx;`，不使用 `#![allow(dead_code)]`；
// `pub unsafe fn` / 私有 `unsafe fn` 都写 `# Safety` 章节。
use crate::adapterclass::{classify, group_order, NicKind, IF_TYPE_SOFTWARE_LOOPBACK};
use crate::winutil::{error_text, wstr_from_ptr};
use std::cmp::Ordering;
use std::net::{Ipv4Addr, Ipv6Addr};
use windows::core::GUID;
use windows::Win32::Foundation::{ERROR_BUFFER_OVERFLOW, ERROR_SUCCESS};
use windows::Win32::NetworkManagement::IpHelper::{
    FreeMibTable, GetAdaptersAddresses, GetIfEntry2, GetIpForwardTable2, GetIpInterfaceEntry,
    GAA_FLAG_INCLUDE_ALL_COMPARTMENTS, GAA_FLAG_INCLUDE_ALL_INTERFACES, GAA_FLAG_INCLUDE_GATEWAYS,
    IP_ADAPTER_ADDRESSES_LH, IP_ADAPTER_DNS_SERVER_ADDRESS_XP, IP_ADAPTER_GATEWAY_ADDRESS_LH,
    IP_ADAPTER_UNICAST_ADDRESS_LH, IP_ADDRESS_PREFIX, MIB_IF_ROW2, MIB_IPFORWARD_ROW2,
    MIB_IPFORWARD_TABLE2, MIB_IPINTERFACE_ROW,
};
use windows::Win32::NetworkManagement::Ndis::{
    IfOperStatusUp, NET_IF_ADMIN_STATUS_DOWN, NET_LUID_LH,
};
use windows::Win32::Networking::WinSock::{
    AF_INET, AF_INET6, AF_UNSPEC, SOCKADDR, SOCKADDR_IN, SOCKADDR_IN6, SOCKADDR_INET,
    SOCKET_ADDRESS,
};

/// 一张物理网卡的全部可显示信息。字段与 Task 5 的列、Task 7 的详情行一一对应。
/// `Clone` 是 Task 6 的硬要求（`ui::selected_nic()` 要在持锁之外持有一份拷贝），
/// `PartialEq` 供测试与去重使用。
#[derive(Debug, Clone, PartialEq)]
pub struct Nic {
    pub if_index: u32,
    pub alias: String,
    pub description: String,
    pub kind: NicKind,
    pub mac: String,
    pub ipv4: String,
    pub mask: String,
    pub gateway: String,
    pub dns: Vec<String>,
    /// 格式化后的链路速率；驱动未上报或「速率未知」时是空串（见 `format_speed`）。
    pub speed: String,
    /// 管理状态是否启用 = `!admin_disabled(if_index)`，
    /// 即 `MIB_IF_ROW2.AdminStatus != NET_IF_ADMIN_STATUS_DOWN`（Task 6 启停就改这个）。
    pub enabled: bool,
    /// 运行状态是否 Up = `OperStatus == IfOperStatusUp`，与 `enabled` 是两件事：
    /// 启用的网卡拔掉网线仍是 `enabled=true, connected=false`。
    pub connected: bool,
    pub is_default_route: bool,
    pub metric: u32,
    /// 跃点是否由系统自动计算（`MIB_IPINTERFACE_ROW.UseAutomaticMetric`）。
    /// Task 3 先固定 true，Task 4 改成真实读取。
    pub metric_auto: bool,
    /// 注意：这是 `IP_ADAPTER_ADDRESSES_LH.NetworkGuid`，即 `NET_IF_NETWORK_GUID`
    /// ——**同一网段所有适配器共用**的机器级网络 GUID（真机上 9 条记录全同，
    /// 值 `{60179B30-...}`），**不是**网卡标识，不能用来定位某张卡。
    /// Task 6 要用 `alias`（netsh 接口名）或 `AdapterName`（接口 GUID，ANSI）；
    /// 本字段只适合在详情里当「所属网络」展示。
    pub network_guid: String,
}

/// 缓冲区不足时的重试上限：Windows 给的 `size` 理论上收敛，这里只是防止病态循环。
const MAX_BUFFER_RETRIES: u32 = 8;
/// 单条链表的遍历上限（脏 `Next` 成环时不至于死循环）。
const MAX_LIST_ROWS: u32 = 1024;
/// `GetAdaptersAddresses` 的起始缓冲大小。
const INITIAL_ADAPTER_BUFFER: u32 = 32 * 1024;

/// 链路速率格式化。
///
/// 入参单位是 **bit/s**：`IP_ADAPTER_ADDRESSES_LH.TransmitLinkSpeed` 与 WMI
/// `Win32_NetworkAdapter.Speed` 都是 bps，所以换算只除 1e6/1e9，不再乘 8。
/// 简报正文写的 `bytes_per_sec` + `* 8.0` 与简报自己的测试互相矛盾
/// （按它写会把 1 Gbps 网卡印成 `8 Gbps`）；这里按测试/真机口径实现，
/// 差异记在 task-4-report.md。`999 Mbps`/`1 Gbps` 的分界是 1000 Mbps，不是 1024。
///
/// 两种「未知」都要返回空串，交给 UI 决定显示什么：
/// - `0`：驱动没填这个字段；
/// - `u64::MAX`（`kMaxULong64` / `ULONGLONG_MAX`，见 `shared/intsafe.h`）：
///   MSDN 对 `TransmitLinkSpeed` 规定的「速率未知」哨兵值，适配器**未插网线/未关联
///   AP** 时就会取它（真机冒烟：Wi-Fi 2/4/5 三条断开态记录）。不特判会印成
///   `18446744073.71 Gbps`。真实存在的大速率（100 Gbps、1 Tbps）不受影响。
pub fn format_speed(bits_per_sec: u64) -> String {
    if bits_per_sec == 0 || bits_per_sec == u64::MAX {
        return String::new();
    }
    let mbps = bits_per_sec as f64 / 1_000_000.0;
    if mbps >= 1000.0 {
        format!("{} Gbps", trim_num(mbps / 1000.0))
    } else {
        format!("{} Mbps", trim_num(mbps))
    }
}

/// 最多三位小数，去掉多余的 0 与小数点（`1.000` → `1`、`2.500` → `2.5`）。
fn trim_num(v: f64) -> String {
    let s = format!("{v:.3}");
    s.trim_end_matches('0').trim_end_matches('.').to_string()
}

/// MAC 地址：小写、`-` 分隔，只取前 `len` 个字节。
/// `PhysicalAddress` 是 `[u8; 8]` 定长缓冲，`PhysicalAddressLength`（0.61.3 里是 u32）
/// 才是有效字节数；长度为 0 或超过缓冲区都返回空串，绝不读缓冲区外的字节。
pub fn format_mac(buf: &[u8], len: u32) -> String {
    let n = len as usize;
    if n == 0 || buf.len() < n {
        return String::new();
    }
    buf[..n]
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<Vec<_>>()
        .join("-")
}

/// 前缀长度 → 点分十进制子网掩码。越界前缀夹到 32（`u32 << 32` 会 panic）。
/// 掩码整数直接交给 `Ipv4Addr::from`（它按「高位是第一个八位组」解释），
/// 不能再 `to_be()` —— 简报那行会把 /24 印成 `0.255.255.255`。
fn prefix_to_mask(prefix: u8) -> String {
    let bits = prefix.min(32) as u32;
    let mask = if bits == 0 {
        0
    } else {
        u32::MAX << (32 - bits)
    };
    Ipv4Addr::from(mask).to_string()
}

/// GUID → `{8-4-4-4-12}` 大写形式，与注册表 `Interfaces\{GUID}`、
/// `Get-NetAdapter` 的显示形态一致（Task 6 的 netsh 命令与回滚日志按这个形态用）。
/// 0.61.2 的 `windows_core::GUID` 只有 `Debug`、没有 `Display`，只能按字段自己拼。
fn format_guid(guid: &GUID) -> String {
    format!(
        "{{{:08X}-{:04X}-{:04X}-{:02X}{:02X}-{:02X}{:02X}{:02X}{:02X}{:02X}{:02X}}}",
        guid.data1,
        guid.data2,
        guid.data3,
        guid.data4[0],
        guid.data4[1],
        guid.data4[2],
        guid.data4[3],
        guid.data4[4],
        guid.data4[5],
        guid.data4[6],
        guid.data4[7]
    )
}

/// 列表过滤口径：只滤掉「不是一张可管理网卡」的伪接口与 NDIS 重复项。
/// 虚拟网卡和 VPN 适配器不再整类隐藏 —— 它们分别进「虚拟网卡」「VPN」分组。
///
/// 仍然拦掉的：
/// - NDIS 筛选器绑定（一张卡会拖出 4 行同名重复项，见 `is_filter_binding`）；
/// - 环回（IfType=24）、WAN Miniport 系列（PPP 栈内部实现，无地址）、内核调试适配器、
///   Hyper-V 虚拟交换机扩展（是 vSwitch 的组件，不是接口）；
/// - 启用中却没有物理地址、又归不进 VPN/虚拟的幽灵接口。
///
/// `is_admin_disabled` 单独开一个口子：网卡被禁用后驱动往往不再上报物理地址
/// （真机实测 Realtek USB GbE 禁用后 `PhysicalAddressLength` 归 0），
/// 若仍按「无 MAC 即虚拟」处理，被禁用的卡就从列表里消失，用户再也无法把它启用回来。
fn is_filtered_out(
    if_type: u32,
    description: &str,
    physical_address_length: u32,
    is_admin_disabled: bool,
) -> bool {
    let d = description.to_ascii_lowercase();
    if is_filter_binding(description)
        || if_type == IF_TYPE_SOFTWARE_LOOPBACK
        || d.contains("wan miniport")
        // 中文系统里 WAN Miniport 叫「WAN 微型端口 (IP)/(PPPOE)」，是 PPP 栈的内部实现
        || d.contains("微型端口")
        || d.contains("kernel debug")
        || d.contains("virtual switch extension")
    {
        return true;
    }
    match classify(if_type, description) {
        // 两个专属分组一律展示，包括只有名称没有 MAC 的条目
        NicKind::Vpn | NicKind::Virtual => false,
        _ => physical_address_length == 0 && !is_admin_disabled,
    }
}

/// NDIS 筛选器绑定伪接口的命名约定：`<父接口描述>-<筛选器名>-<4 位十六进制实例号>`
/// （真机实测 `…Adapter #2-QoS Packet Scheduler-0000`）。
/// `GAA_FLAG_INCLUDE_ALL_INTERFACES` 会把这些绑定一起带进来，而它们的 IfType、MAC
/// 与父接口完全一样，关键字表和 IfType 都拦不住 —— 一张 WLAN 会拖出 4 行重复项。
fn is_filter_binding(description: &str) -> bool {
    match description.rsplit_once('-') {
        Some((head, instance)) => {
            instance.len() == 4
                && instance.bytes().all(|b| b.is_ascii_hexdigit())
                && !head.is_empty()
        }
        None => false,
    }
}

/// 排序：先按分组（有线 → 无线 → 其他），组内按名称忽略大小写。
/// 用稳定的 `sort_by`：同组同名（多张「以太网」）时保持 API 返回次序，避免 UI 抖动。
fn sort_nics(nics: &mut [Nic]) {
    nics.sort_by(nic_order);
}

fn nic_order(x: &Nic, y: &Nic) -> Ordering {
    group_order(x.kind)
        .cmp(&group_order(y.kind))
        .then_with(|| x.alias.to_lowercase().cmp(&y.alias.to_lowercase()))
}

/// 采集物理网卡：过滤虚拟/环回/隧道，按分组再按名称排序。
///
/// 失败时返回错误文本（`GetAdaptersAddresses` 或 IPv4 路由表读取失败），
/// 由 Task 5 显示到状态栏；单个附加字段（跃点数、管理状态）读失败只在内部降级，
/// 不牵连整张列表 —— 降级形态见 `interface_metric` / `admin_disabled` 的注释。
pub fn list() -> Result<Vec<Nic>, String> {
    let defaults = default_route_interfaces()?;
    let (buf, _retries) = fetch_adapter_table()?;
    let mut out = Vec::new();
    for a in unsafe { adapter_rows(&buf) } {
        // 0.61.3 里 Description / FriendlyName 是 PWSTR，直接 wstr_from_ptr(x.0)；
        // 真 ANSI 的只有 AdapterName（Task 6 的 netsh 才用）。
        let description = unsafe { wstr_from_ptr(a.Description.0) };
        let alias = unsafe { wstr_from_ptr(a.FriendlyName.0) };
        // 每行问一次管理状态：过滤判据要用它（禁用的卡可能不再上报 MAC），
        // 取到的结果顺手交给 build_nic，避免同一行问两次。
        // IfIndex 与 Length 共用 union，取它需要 unsafe；本循环里只取这一次。
        let if_index = unsafe { a.Anonymous1.Anonymous.IfIndex };
        let disabled = admin_disabled(if_index);
        if is_filtered_out(a.IfType, &description, a.PhysicalAddressLength, disabled) {
            continue;
        }
        out.push(unsafe { build_nic(a, &description, &alias, &defaults, disabled) });
    }
    sort_nics(&mut out);
    Ok(out)
}

/// 由单条原始记录构造 `Nic`（不做过滤，真机诊断也走这条路径，保证打印的就是实际逻辑）。
///
/// # Safety
/// `a` 必须指向 `fetch_adapter_table` 返回缓冲区内的有效节点（见 `adapter_rows`）。
unsafe fn build_nic(
    a: &IP_ADAPTER_ADDRESSES_LH,
    description: &str,
    alias: &str,
    defaults: &[u32],
    disabled: bool,
) -> Nic {
    // IfIndex 藏在 Anonymous1.Anonymous 里（与 Length 共用 union）
    let if_index = a.Anonymous1.Anonymous.IfIndex;
    let mut nic = Nic {
        if_index,
        alias: alias.to_string(),
        description: description.to_string(),
        // classify 必须吃驱动描述（通常是英文），不能吃本地化的 FriendlyName
        kind: classify(a.IfType, description),
        mac: format_mac(&a.PhysicalAddress, a.PhysicalAddressLength),
        ipv4: String::new(),
        mask: String::new(),
        gateway: String::new(),
        dns: Vec::new(),
        speed: format_speed(a.TransmitLinkSpeed),
        enabled: !disabled,
        connected: a.OperStatus == IfOperStatusUp,
        is_default_route: defaults.contains(&if_index),
        metric: interface_metric(if_index, a.Luid),
        metric_auto: true,
        network_guid: format_guid(&a.NetworkGuid),
    };
    unsafe { fill_addresses(a, &mut nic) };
    nic
}

/// 取一次 `GetAdaptersAddresses` 全量表：先按 32 KiB 试，`ERROR_BUFFER_OVERFLOW`
/// 时按 API 回写的 `size` 重新分配再调（0.61.3 的这个 API 返回裸 `u32` 错误码，
/// 不是 `Result`；而 `GetIpForwardTable2`/`GetIfEntry2` 返回 `WIN32_ERROR`）。
/// 返回 `(缓冲区, 重试次数)`；调用方必须在返回的缓冲区存活期间遍历链表。
fn fetch_adapter_table() -> Result<(Vec<u8>, u32), String> {
    let mut size: u32 = INITIAL_ADAPTER_BUFFER;
    let mut buf = vec![0u8; size as usize];
    let mut retries = 0u32;
    // INCLUDE_ALL_INTERFACES 是「禁用后仍能在列表里看到并重新启用」的前提：
    // 不带它，被禁用的接口整条不出现在返回链表中（真机实测：蓝牙网络连接禁用后消失）。
    let flags = GAA_FLAG_INCLUDE_GATEWAYS
        | GAA_FLAG_INCLUDE_ALL_COMPARTMENTS
        | GAA_FLAG_INCLUDE_ALL_INTERFACES;
    loop {
        let status = unsafe {
            GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                flags,
                None,
                Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut size,
            )
        };
        if status == ERROR_SUCCESS.0 {
            // 成功时 size 是实际用掉的字节数；为 0（异常形态）时保留原缓冲区
            if size > 0 {
                buf.truncate(size as usize);
            }
            return Ok((buf, retries));
        }
        if status == ERROR_BUFFER_OVERFLOW.0 {
            retries += 1;
            if retries > MAX_BUFFER_RETRIES {
                return Err(format!(
                    "GetAdaptersAddresses 重试 {retries} 次仍缓冲区不足（最后一次需要 {size} 字节）"
                ));
            }
            // API 已把需要的长度写进 size；保底不小于起始大小，避免 0 长度缓冲
            let need = retry_buffer_size(size) as usize;
            buf = vec![0u8; need];
            continue;
        }
        return Err(format!(
            "GetAdaptersAddresses 失败：{}（错误码 {status}）",
            error_text(status)
        ));
    }
}

/// `ERROR_BUFFER_OVERFLOW` 出参 → 下一次要分配的字节数。
///
/// Windows 写回的「需要长度」可能比起始缓冲还小（真机 22944 < 32768），甚至可能是
/// 0（异常形态）；直接按它重分配会拿到比原来还小的缓冲区，第二遍照样溢出。
/// 所以取「需要长度」与 `INITIAL_ADAPTER_BUFFER` 的较大者。
fn retry_buffer_size(requested: u32) -> u32 {
    requested.max(INITIAL_ADAPTER_BUFFER)
}

/// 遍历 `GetAdaptersAddresses` 返回的单链表。
///
/// 每个节点都先确认「整条 `IP_ADAPTER_ADDRESSES_LH` 落在缓冲区之内」才解引用，
/// 并且有 `MAX_LIST_ROWS` 上限，所以脏 `Next`（越界或成环）只会截断遍历，不会越界读。
///
/// # Safety
/// `buf` 必须是 `fetch_adapter_table` 成功返回的那块缓冲区（内容确实是适配器链表）；
/// 返回的引用只在 `buf` 存活期间有效。
unsafe fn adapter_rows(buf: &[u8]) -> Vec<&IP_ADAPTER_ADDRESSES_LH> {
    let mut out = Vec::new();
    let lo = buf.as_ptr() as usize;
    let hi = lo + buf.len();
    let mut cur = buf.as_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>();
    while !cur.is_null() && (out.len() as u32) < MAX_LIST_ROWS {
        let addr = cur as usize;
        if addr < lo || addr + std::mem::size_of::<IP_ADAPTER_ADDRESSES_LH>() > hi {
            break;
        }
        let a = unsafe { &*cur };
        out.push(a);
        cur = a.Next;
    }
    out
}

/// 沿单播 / 网关 / DNS 三条链表取值。只在 `a` 指向的缓冲区内读指针，不复制、不保留指针。
///
/// # Safety
/// `a` 必须是 `adapter_rows` 交出来的有效节点。
unsafe fn fill_addresses(a: &IP_ADAPTER_ADDRESSES_LH, nic: &mut Nic) {
    // 单播：只取**第一个** IPv4 地址（简报写法是「最后一个覆盖前面的」，
    // 会把辅助/APIPA 地址当成主地址），掩码取同一行的 OnLinkPrefixLength。
    let mut u = a.FirstUnicastAddress;
    let mut rows = 0u32;
    while !u.is_null() && rows < MAX_LIST_ROWS {
        rows += 1;
        let row: &IP_ADAPTER_UNICAST_ADDRESS_LH = unsafe { &*u };
        if nic.ipv4.is_empty() {
            if let Some(ip) = unsafe { socket_address_to_ipv4(&row.Address) } {
                nic.ipv4 = ip.to_string();
                nic.mask = prefix_to_mask(row.OnLinkPrefixLength);
            }
        }
        u = row.Next;
    }

    // 网关：第一个 IPv4 网关（本网卡上的默认路由出口）
    let mut g = a.FirstGatewayAddress;
    let mut rows = 0u32;
    while !g.is_null() && rows < MAX_LIST_ROWS {
        rows += 1;
        let row: &IP_ADAPTER_GATEWAY_ADDRESS_LH = unsafe { &*g };
        if let Some(ip) = unsafe { socket_address_to_ipv4(&row.Address) } {
            nic.gateway = ip.to_string();
            break;
        }
        g = row.Next;
    }

    // DNS：v4/v6 都收，按原次序去重
    let mut d = a.FirstDnsServerAddress;
    let mut rows = 0u32;
    while !d.is_null() && rows < MAX_LIST_ROWS {
        rows += 1;
        let row: &IP_ADAPTER_DNS_SERVER_ADDRESS_XP = unsafe { &*d };
        let s = unsafe { socket_address_to_string(&row.Address) };
        if !s.is_empty() && !nic.dns.contains(&s) {
            nic.dns.push(s);
        }
        d = row.Next;
    }
}

/// `SOCKADDR` → IPv4。地址族和声明长度**双重**校验：`iSockaddrLength` 小于
/// `SOCKADDR_IN` 时宁可不读，避免越界读；`i32` 先 widen 成 `i64` 再比较，
/// 免得负长度被 `as usize` 变成巨大下标。
///
/// # Safety
/// `addr.lpSockaddr` 必须指向至少 `addr.iSockaddrLength` 字节可读的内存。
unsafe fn socket_address_to_ipv4(addr: &SOCKET_ADDRESS) -> Option<Ipv4Addr> {
    let sa: *mut SOCKADDR = addr.lpSockaddr;
    if sa.is_null() || (addr.iSockaddrLength as i64) < std::mem::size_of::<SOCKADDR_IN>() as i64 {
        return None;
    }
    if unsafe { (*sa).sa_family } != AF_INET {
        return None;
    }
    let sin = unsafe { &*(sa.cast::<SOCKADDR_IN>()) };
    // S_addr 是网络字节序：u32::from_be 换成主机序整数后交给 Ipv4Addr::from
    Some(Ipv4Addr::from(u32::from_be(sin.sin_addr.S_un.S_addr)))
}

/// `SOCKADDR` → IPv4 或 IPv6 文本；其它地址族返回空串（同样受长度校验保护）。
///
/// # Safety
/// 同 `socket_address_to_ipv4`：`lpSockaddr` 后必须有 `iSockaddrLength` 字节可读内存。
unsafe fn socket_address_to_string(addr: &SOCKET_ADDRESS) -> String {
    if let Some(v4) = unsafe { socket_address_to_ipv4(addr) } {
        return v4.to_string();
    }
    let sa = addr.lpSockaddr;
    if sa.is_null() || (addr.iSockaddrLength as i64) < std::mem::size_of::<SOCKADDR_IN6>() as i64 {
        return String::new();
    }
    if unsafe { (*sa).sa_family } != AF_INET6 {
        return String::new();
    }
    let sin6 = unsafe { &*(sa.cast::<SOCKADDR_IN6>()) };
    Ipv6Addr::from(sin6.sin6_addr.u.Byte).to_string()
}

/// 跃点数：`GetIpInterfaceEntry` 的 `Metric`（0.61.3 的绑定里没有 `RouteMetric` 成员，
/// SDK 的 netioapi.h 同样只有 `ULONG Metric`，两者是同一个字段）。
/// 读失败回 0（Task 7 把 0 显示成「自动/未知」），不让单张网卡的附加字段拖垮整张列表。
fn interface_metric(if_index: u32, luid: NET_LUID_LH) -> u32 {
    interface_metric_entry(if_index, luid).unwrap_or_default()
}

/// `interface_metric` 的可诊断版本：把真实返回码/错误文本原样交出去，供真机冒烟测试打印。
///
/// # Safety 说明：`Family` + `InterfaceLuid` + `InterfaceIndex` 按要求填好再调用
/// （LUID 与 Index 都给，API 优先用非零 LUID）。
fn interface_metric_entry(if_index: u32, luid: NET_LUID_LH) -> Result<u32, String> {
    let mut row = MIB_IPINTERFACE_ROW {
        Family: AF_INET,
        InterfaceLuid: luid,
        InterfaceIndex: if_index,
        ..Default::default()
    };
    let status = unsafe { GetIpInterfaceEntry(&mut row) };
    if status == ERROR_SUCCESS {
        Ok(row.Metric)
    } else {
        Err(format!(
            "GetIpInterfaceEntry(ifindex={if_index})：{}（错误码 {}）",
            error_text(status.0),
            status.0
        ))
    }
}

/// 管理员禁用状态：用 `MIB_IF_ROW2.AdminStatus`（`NET_IF_ADMIN_STATUS`：1=Up、2=Down、
/// 3=Testing，见 ifdef.h）—— 0.61.3 的绑定确实暴露了这个字段，因此不用简报里
/// 「猜 `Attributes` 的 bit2」那种写法。失败时把错误文本交出去，便于真机核对。
fn admin_status(if_index: u32) -> Result<i32, String> {
    // InterfaceIndex 给了、InterfaceLuid 保持全零 → API 按 Index 查
    let mut row = MIB_IF_ROW2 {
        InterfaceIndex: if_index,
        ..Default::default()
    };
    let status = unsafe { GetIfEntry2(&mut row) };
    if status == ERROR_SUCCESS {
        Ok(row.AdminStatus.0)
    } else {
        Err(format!(
            "GetIfEntry2(ifindex={if_index})：{}（错误码 {}）",
            error_text(status.0),
            status.0
        ))
    }
}

/// `AdminStatus == NET_IF_ADMIN_STATUS_DOWN` 才算被管理员禁用。
/// 读不到时按「未禁用」处理：宁可显示成可用，也不要在 UI 上谎报禁用
/// （Task 6 的启停会以重新采集的结果为准）。极性与 `netsh interface show interface`
/// 的比对表见 task-4-report.md。
fn admin_disabled(if_index: u32) -> bool {
    matches!(admin_status(if_index), Ok(code) if code == NET_IF_ADMIN_STATUS_DOWN.0)
}

/// 默认路由判定：只有目的前缀长度 0（`0.0.0.0/0`）才算默认路由。
/// 环回/链路本地/子网路由的前缀长度都大于 0，不能因为它们是路由条目就把网卡标成默认出口。
fn is_default_route_prefix(prefix_length: u8) -> bool {
    prefix_length == 0
}

/// 默认路由（0.0.0.0/0）落在哪些 IfIndex 上。
/// `GetIpForwardTable2` 由系统分配表内存，用完必须 `FreeMibTable`
/// （0.61.3 没有 `FreeIpForwardTable`，新版表统一走 `FreeMibTable`）。
fn default_route_interfaces() -> Result<Vec<u32>, String> {
    let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
    let status = unsafe { GetIpForwardTable2(AF_INET, &mut table) };
    if status != ERROR_SUCCESS || table.is_null() {
        return Err(format!(
            "GetIpForwardTable2 失败：{}（错误码 {}）",
            error_text(status.0),
            status.0
        ));
    }
    let mut ifaces = Vec::new();
    unsafe {
        // Table 是 [MIB_IPFORWARD_ROW2; 1] 的柔性数组尾巴，真实行数看 NumEntries
        let header = &*table;
        let rows = std::slice::from_raw_parts(header.Table.as_ptr(), header.NumEntries as usize);
        for row in rows {
            let row: &MIB_IPFORWARD_ROW2 = row;
            if is_default_route_prefix(row.DestinationPrefix.PrefixLength) {
                ifaces.push(row.InterfaceIndex);
            }
        }
        FreeMibTable(table.cast::<core::ffi::c_void>());
    }
    Ok(ifaces)
}

/// 一条路由。字段就是路由页的列，顺序也按列来，避免 UI 里再拼一次。
#[derive(Debug, Clone, PartialEq)]
pub struct Route {
    pub destination: String,
    pub next_hop: String,
    pub if_index: u32,
    pub metric: u32,
    pub protocol: String,
    pub origin: String,
    pub loopback: bool,
    pub immortal: bool,
}

/// `MIB_IPPROTO_*` 中实际会在本机出现的取值。未收录的返回空串，由 `protocol_text` 兜底。
pub fn protocol_name(code: i32) -> &'static str {
    match code {
        2 => "本地",
        3 => "手动",
        8 => "RIP",
        13 => "OSPF",
        14 => "BGP",
        19 => "DHCP",
        10002 => "自动静态",
        10006 => "静态",
        10007 => "静态(非DOD)",
        _ => "",
    }
}

/// 协议列文字：未知协议印成「协议 N」而不是留空，否则表格里会出现无名行。
pub fn protocol_text(code: i32) -> String {
    let n = protocol_name(code);
    if n.is_empty() {
        format!("协议 {code}")
    } else {
        n.to_string()
    }
}

/// `NL_ROUTE_ORIGIN` 已文档化的取值。
pub fn origin_name(code: i32) -> &'static str {
    match code {
        0 => "手动",
        1 => "系统",
        2 => "DHCP",
        3 => "路由通告",
        4 => "6to4",
        _ => "",
    }
}

pub fn origin_text(code: i32) -> String {
    let n = origin_name(code);
    if n.is_empty() {
        format!("来源 {code}")
    } else {
        n.to_string()
    }
}

/// `SOCKADDR_INET` → 文本。union 的三个分支都是 POD，先读 `si_family` 再读对应分支，
/// 不会触及未初始化内存；其它地址族返回空串。
pub fn inet_text(addr: &SOCKADDR_INET) -> String {
    let family = unsafe { addr.si_family };
    if family == AF_INET {
        let raw = unsafe { addr.Ipv4.sin_addr.S_un.S_addr };
        Ipv4Addr::from(u32::from_be(raw)).to_string()
    } else if family == AF_INET6 {
        Ipv6Addr::from(unsafe { addr.Ipv6.sin6_addr.u.Byte }).to_string()
    } else {
        String::new()
    }
}

pub fn prefix_text(prefix: &IP_ADDRESS_PREFIX) -> String {
    let dest = inet_text(&prefix.Prefix);
    if dest.is_empty() {
        String::new()
    } else {
        format!("{dest}/{}", prefix.PrefixLength)
    }
}

/// 全量路由表（IPv4 + IPv6）。`GetIpForwardTable2` 只接受单地址族，两次调用后合并，
/// 顺序统一交给 `sort_routes`。
pub fn list_routes() -> Result<Vec<Route>, String> {
    let mut out = Vec::new();
    for family in [AF_INET, AF_INET6] {
        let mut table: *mut MIB_IPFORWARD_TABLE2 = std::ptr::null_mut();
        let status = unsafe { GetIpForwardTable2(family, &mut table) };
        if status != ERROR_SUCCESS || table.is_null() {
            return Err(format!(
                "{family:?} 路由表读取失败：{}（错误码 {}）",
                error_text(status.0),
                status.0
            ));
        }
        unsafe {
            // Table 是 [MIB_IPFORWARD_ROW2; 1] 的柔性数组尾巴，真实行数看 NumEntries
            let header = &*table;
            let rows =
                std::slice::from_raw_parts(header.Table.as_ptr(), header.NumEntries as usize);
            for row in rows {
                let row: &MIB_IPFORWARD_ROW2 = row;
                out.push(Route {
                    destination: prefix_text(&row.DestinationPrefix),
                    next_hop: inet_text(&row.NextHop),
                    if_index: row.InterfaceIndex,
                    metric: row.Metric,
                    protocol: protocol_text(row.Protocol.0),
                    origin: origin_text(row.Origin.0),
                    loopback: row.Loopback,
                    immortal: row.Immortal,
                });
            }
            FreeMibTable(table.cast::<core::ffi::c_void>());
        }
    }
    sort_routes(&mut out);
    Ok(out)
}

/// 接口升序 → 跃点升序 → 默认路由优先 → 目的前缀字典序。
pub fn sort_routes(routes: &mut [Route]) {
    routes.sort_by(|a, b| {
        a.if_index
            .cmp(&b.if_index)
            .then(a.metric.cmp(&b.metric))
            .then_with(|| default_first(&a.destination).cmp(&default_first(&b.destination)))
            .then(a.destination.cmp(&b.destination))
    });
}

/// 默认路由排在所属接口的最前面：`0.0.0.0/0`、`::/0` 都得排 0。
fn default_first(dest: &str) -> u8 {
    if dest.ends_with("/0") {
        0
    } else {
        1
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::adapterclass::NicKind;
    use windows::Win32::Networking::WinSock::{
        IN6_ADDR, IN6_ADDR_0, IN_ADDR, IN_ADDR_0, SOCKADDR_IN6_0,
    };

    // ---------------- format_speed ----------------

    #[test]
    fn speed_gbps() {
        assert_eq!(format_speed(1_000_000_000), "1 Gbps");
    }

    #[test]
    fn speed_mbps() {
        assert_eq!(format_speed(100_000_000), "100 Mbps");
    }

    #[test]
    fn speed_unknown_when_zero() {
        assert_eq!(format_speed(0), "");
    }

    /// 口径逐个钉住：入参是**比特每秒**（TransmitLinkSpeed 的单位就是 bps），
    /// 换算用十进制 1e6/1e9，Gbps 门槛是 1000 Mbps 而不是 1024。
    #[test]
    fn speed_boundary_table() {
        let cases: [(&str, u64, &str); 11] = [
            ("1 bps 直接归零", 1, "0 Mbps"),
            ("999 Kbps", 999_000, "0.999 Mbps"),
            ("1 Mbps 起点", 1_000_000, "1 Mbps"),
            ("10 Mbps", 10_000_000, "10 Mbps"),
            ("100 Mbps", 100_000_000, "100 Mbps"),
            ("128 Mbps", 128_000_000, "128 Mbps"),
            ("999 Mbps 上限", 999_000_000, "999 Mbps"),
            // 999.999999 Mbps 按 {:.3} 进位成 1000 Mbps：与 WPF 的 G4 口径同样结果
            ("999.999999 Mbps 进位", 999_999_999, "1000 Mbps"),
            ("刚好 1 Gbps", 1_000_000_000, "1 Gbps"),
            ("2.5 Gbps", 2_500_000_000, "2.5 Gbps"),
            ("10 Gbps", 10_000_000_000, "10 Gbps"),
        ];
        for (label, bps, expected) in cases {
            assert_eq!(format_speed(bps), expected, "用例：{label}（{bps} bps）");
        }
        // 只保留 3 位小数：1.048576 Gbps → 1.049
        assert_eq!(format_speed(1_048_576_000), "1.049 Gbps");
    }

    /// 真机冒烟发现：Wi-Fi 适配器未连接时驱动把 `TransmitLinkSpeed` 置为
    /// `kMaxULong64`（0xFFFFFFFFFFFFFFFF，见 intsafe.h `ULONGLONG_MAX`），
    /// 不特判就会印出 `18446744073.71 Gbps`。同时必须证明这个特判不会误伤
    /// 真实存在的大速率（100 Gbps / 1 Tbps 仍要照常格式化）。
    #[test]
    fn speed_unknown_sentinel_is_not_printed() {
        assert_eq!(format_speed(u64::MAX), "");
        assert_eq!(format_speed(100_000_000_000), "100 Gbps");
        assert_eq!(format_speed(1_000_000_000_000), "1000 Gbps");
    }

    // ---------------- 缓冲区重试 / 默认路由判定（纯逻辑，可在 CI 钉住） ----------------

    #[test]
    fn retry_buffer_size_never_shrinks_below_initial() {
        // 真机探针：1 字节调用时 Windows 写回 22944，比起始缓冲还小
        assert_eq!(retry_buffer_size(22944), INITIAL_ADAPTER_BUFFER);
        // 异常出参（0）也不能退化成 1 字节缓冲
        assert_eq!(retry_buffer_size(0), INITIAL_ADAPTER_BUFFER);
        assert_eq!(
            retry_buffer_size(INITIAL_ADAPTER_BUFFER),
            INITIAL_ADAPTER_BUFFER
        );
        // 真正的大需求必须照用，否则会反复溢出
        assert_eq!(retry_buffer_size(1 << 20), 1 << 20);
    }

    #[test]
    fn default_route_needs_zero_prefix() {
        assert!(is_default_route_prefix(0), "0.0.0.0/0 才是默认路由");
        assert!(!is_default_route_prefix(1));
        assert!(!is_default_route_prefix(24));
        assert!(!is_default_route_prefix(u8::MAX));
    }

    // ---------------- format_mac ----------------
    #[test]
    fn mac_empty_when_absent() {
        assert_eq!(format_mac(&[], 0), "");
        assert_eq!(format_mac(&[0xAA, 0xBB], 0), "");
    }

    #[test]
    fn mac_dash_separated_lowercase() {
        assert_eq!(
            format_mac(&[0xaa, 0xbb, 0xcc, 0x01, 0x02, 0x03], 6),
            "aa-bb-cc-01-02-03"
        );
    }

    /// 长度不足 / 超界 / 只取前 len 个字节：钉死「PhysicalAddress 是 8 字节定长缓冲，
    /// 只有前 PhysicalAddressLength 个有效」这条约定。
    #[test]
    fn mac_length_is_respected() {
        assert_eq!(
            format_mac(&[0x01], 4),
            "",
            "声明长度超过缓冲区时必须返回空串"
        );
        assert_eq!(format_mac(&[0xAA; 8], 8), "aa-aa-aa-aa-aa-aa-aa-aa");
        assert_eq!(
            format_mac(&[0xAB, 0x0C, 0xF0, 0x01, 0x02, 0x03, 0xFF, 0xFF], 6),
            "ab-0c-f0-01-02-03",
            "尾部填充字节不得进入 MAC"
        );
        // 单字节补零、字母小写
        assert_eq!(
            format_mac(&[0x0A, 0xFF, 0x00, 0x1B, 0x2C, 0x3D], 6),
            "0a-ff-00-1b-2c-3d"
        );
    }

    // ---------------- prefix_to_mask ----------------

    /// 前缀长度 → 子网掩码：简报写法（`Ipv4Addr::from(bits.to_be())`）会把掩码字节序翻反，
    /// 这里逐个值钉住正确形态。
    #[test]
    fn prefix_to_mask_table() {
        let cases: [(u8, &str); 10] = [
            (0, "0.0.0.0"),
            (1, "128.0.0.0"),
            (8, "255.0.0.0"),
            (16, "255.255.0.0"),
            (22, "255.255.252.0"),
            (23, "255.255.254.0"),
            (24, "255.255.255.0"),
            (25, "255.255.255.128"),
            (30, "255.255.255.252"),
            (32, "255.255.255.255"),
        ];
        for (prefix, expected) in cases {
            assert_eq!(prefix_to_mask(prefix), expected, "前缀 /{prefix}");
        }
        // 越界前缀必须夹到 32，不得 panic（u32 << 32 会溢出）
        assert_eq!(prefix_to_mask(33), "255.255.255.255");
        assert_eq!(prefix_to_mask(u8::MAX), "255.255.255.255");
    }

    /// 钉住 std 的字节序约定，免得 prefix_to_mask / 地址解码在平台语义上想当然。
    #[test]
    fn ipv4_from_u32_is_big_endian_value() {
        assert_eq!(Ipv4Addr::from(0x7F00_0001).to_string(), "127.0.0.1");
        assert_eq!(Ipv4Addr::from(0xFF00_0000).to_string(), "255.0.0.0");
    }

    // ---------------- format_guid ----------------

    #[test]
    fn guid_registry_format() {
        let g = GUID::from_values(
            0x1234_5678,
            0xABCD,
            0xEF01,
            [0x01, 0x02, 0x03, 0x04, 0x05, 0x06, 0x07, 0x08],
        );
        assert_eq!(format_guid(&g), "{12345678-ABCD-EF01-0102-030405060708}");
        // 零 GUID 也照实输出，不做「空即隐藏」的处理
        assert_eq!(
            format_guid(&GUID::zeroed()),
            "{00000000-0000-0000-0000-000000000000}"
        );
    }

    // ---------------- 过滤判据 ----------------

    /// 过滤口径：伪接口/NDIS 重复项拦掉；虚拟网卡与 VPN 适配器保留（各有分组）。
    /// 下面全部按「启用中」（第 4 参 false）取样，禁用带来的 MAC 缺失例外单独测。
    #[test]
    fn filter_rule_table() {
        // 物理网卡必须留下
        assert!(!is_filtered_out(
            6,
            "Realtek PCIe GbE Family Controller",
            6,
            false
        ));
        assert!(!is_filtered_out(
            71,
            "Intel(R) Wi-Fi 6E AX211 160MHz",
            6,
            false
        ));
        assert!(
            !is_filtered_out(6, "以太网 网络适配器", 6, false),
            "本地化中文描述不得被误杀"
        );
        // 虚拟网卡、VPN 适配器不再隐藏：分别进「虚拟网卡」「VPN」分组
        assert!(
            !is_filtered_out(6, "Hyper-V Virtual Ethernet Adapter", 6, false),
            "Hyper-V 虚拟网卡应保留（进虚拟组）"
        );
        assert!(
            !is_filtered_out(53, "Sangfor aTrust VNIC", 6, false),
            "VPN 适配器应保留（进 VPN 组）"
        );
        assert!(
            !is_filtered_out(6, "TAP-Windows Adapter V9", 6, false),
            "TAP 是 VPN 适配器，应保留"
        );
        // 无 MAC 的虚拟/VPN 条目也要保留（vSwitch 组件、隧道口常没有 MAC）
        assert!(!is_filtered_out(
            6,
            "Hyper-V Virtual Ethernet Adapter",
            0,
            false
        ));
        assert!(!is_filtered_out(
            53,
            "aTrustXtun Userspace Tunnel",
            0,
            false
        ));
        // 伪接口与 NDIS 重复项继续拦掉
        assert!(
            is_filtered_out(24, "Software Loopback Interface 1", 0, false),
            "环回=24 必须过滤"
        );
        assert!(
            is_filtered_out(23, "WAN Miniport (PPPOE)", 0, false),
            "WAN Miniport 是 PPP 栈内部实现"
        );
        assert!(
            is_filtered_out(23, "WAN 微型端口 (IP)", 0, false),
            "中文 WAN 微型端口同样要滤掉"
        );
        assert!(is_filtered_out(
            6,
            "Microsoft Kernel Debug Network Adapter",
            0,
            false
        ));
        assert!(is_filtered_out(
            6,
            "Hyper-V Virtual Switch Extension Adapter",
            0,
            false
        ));
        assert!(is_filtered_out(
            6,
            "Hyper-V Virtual Ethernet Adapter-QoS Packet Scheduler-0000",
            6,
            false
        ));
        // 启用中却没有物理地址、也认不出是虚拟/VPN → 按幽灵接口滤掉
        assert!(
            is_filtered_out(6, "Some Physical Adapter", 0, false),
            "PhysicalAddressLength=0 且未禁用必须滤掉"
        );
        // 已知残留风险（如实留档）：纯中文虚拟名 + 物理 IfType + 有 MAC 时会留在「其他」
        assert!(!is_filtered_out(6, "虚拟以太网适配器", 6, false));
        assert!(is_filtered_out(6, "虚拟以太网适配器", 0, false));
    }

    /// 禁用中的物理网卡常常不再上报 MAC（真机实测：Realtek USB GbE 禁用后 maclen 归 0）。
    /// 只按 MAC 判虚拟就会把「正等着被重新启用」的那张卡整条藏掉。
    #[test]
    fn admin_disabled_adapter_survives_missing_mac() {
        assert!(!is_filtered_out(
            6,
            "Realtek USB GbE Family Controller",
            0,
            true
        ));
        // 启用状态下的无 MAC 仍按虚拟处理
        assert!(is_filtered_out(
            6,
            "Realtek USB GbE Family Controller",
            0,
            false
        ));
        // 伪接口即使被禁用也不会回到列表
        assert!(is_filtered_out(
            24,
            "Software Loopback Interface 1",
            0,
            true
        ));
        assert!(is_filtered_out(6, "WAN Miniport (IP)", 0, true));
    }

    /// 筛选器绑定必须整类拦掉：一张 WLAN 会拖出 4 行同名重复项。
    #[test]
    fn filter_bindings_are_hidden() {
        let binding = "Qualcomm FastConnect 7800 Network Adapter #2-QoS Packet Scheduler-0000";
        assert!(is_filtered_out(71, binding, 6, false));
        // 父接口自己也被禁用时，绑定同样不能漏
        assert!(is_filter_binding(
            "Realtek USB GbE Family Controller-WFP Native MAC Layer LightWeight Filter-0000"
        ));
        assert!(is_filter_binding(
            "WAN Miniport (IP)-QoS Packet Scheduler-0000"
        ));
    }

    /// 真实适配器描述里也有连字符，命名约定不能把它们误杀。
    #[test]
    fn real_adapter_names_are_not_filter_bindings() {
        for name in [
            "Realtek PCIe GbE Family Controller",
            "TAP-Windows Adapter V9",
            "Intel(R) Wi-Fi 6E AX211 160MHz",
            "Bluetooth Device (Personal Area Network)",
            "ASIX USB to Gigabit Ethernet Family Adapter",
            "Marvell AVASTAR Wireless-USB Network Adapter",
        ] {
            assert!(!is_filter_binding(name), "{name} 被误判成筛选器绑定");
        }
    }

    // ---------------- 排序 ----------------

    fn test_nic(if_index: u32, alias: &str, kind: NicKind) -> Nic {
        Nic {
            if_index,
            alias: alias.to_string(),
            description: "desc".to_string(),
            kind,
            mac: String::new(),
            ipv4: String::new(),
            mask: String::new(),
            gateway: String::new(),
            dns: Vec::new(),
            speed: String::new(),
            enabled: true,
            connected: false,
            is_default_route: false,
            metric: 0,
            metric_auto: true,
            network_guid: String::new(),
        }
    }

    /// 分组次序（有线 → 无线 → VPN → 虚拟 → 其他）压过名称次序，组内按名称小写比较。
    #[test]
    fn sort_groups_before_names() {
        let mut v = vec![
            test_nic(1, "alpha-other", NicKind::Other),
            test_nic(4, "delta-virtual", NicKind::Virtual),
            test_nic(5, "eps-vpn", NicKind::Vpn),
            test_nic(2, "zeta-wired", NicKind::Wired),
            test_nic(3, "mid-wireless", NicKind::Wireless),
        ];
        sort_nics(&mut v);
        assert_eq!(
            v.iter().map(|n| n.alias.as_str()).collect::<Vec<_>>(),
            vec![
                "zeta-wired",
                "mid-wireless",
                "eps-vpn",
                "delta-virtual",
                "alpha-other"
            ]
        );
        assert_eq!(
            v.iter().map(|n| n.if_index).collect::<Vec<_>>(),
            vec![2, 3, 5, 4, 1]
        );
    }

    /// 组内比较必须忽略大小写：B/a 这对值在「按原始字节比较」时结果相反。
    #[test]
    fn sort_is_case_insensitive_within_group() {
        let mut v = vec![
            test_nic(1, "B", NicKind::Wired),
            test_nic(2, "a", NicKind::Wired),
        ];
        sort_nics(&mut v);
        assert_eq!(
            v.iter().map(|n| n.alias.as_str()).collect::<Vec<_>>(),
            vec!["a", "B"]
        );
    }

    /// 分组与名称都相等时保持输入次序（sort_by 稳定性）：Task 5 的重复「以太网」依赖它。
    #[test]
    fn sort_is_stable_for_equal_keys() {
        let mut v = vec![
            test_nic(11, "以太网", NicKind::Wired),
            test_nic(12, "以太网", NicKind::Wired),
            test_nic(13, "以太网", NicKind::Wired),
        ];
        sort_nics(&mut v);
        assert_eq!(
            v.iter().map(|n| n.if_index).collect::<Vec<_>>(),
            vec![11, 12, 13]
        );
        // 只有大小写差异时也算「相等」，同样保持原次序
        let mut mixed = vec![
            test_nic(21, "Eth", NicKind::Wired),
            test_nic(22, "eth", NicKind::Wired),
        ];
        sort_nics(&mut mixed);
        assert_eq!(
            mixed.iter().map(|n| n.if_index).collect::<Vec<_>>(),
            vec![21, 22]
        );
    }

    /// Nic 的派生契约：Task 6 的 `selected_nic()` 要 `.cloned()`，Task 7/5 要 `{:?}`。
    #[test]
    fn nic_is_clone_and_debug() {
        let n = test_nic(7, "以太网", NicKind::Wireless);
        let c = n.clone();
        assert_eq!(c.if_index, 7);
        assert_eq!(c.alias, "以太网");
        assert_eq!(c.kind, NicKind::Wireless);
        let text = format!("{n:?}");
        assert!(
            text.contains("alias") && text.contains("network_guid"),
            "{text}"
        );
    }

    // ---------------- sockaddr 解码 ----------------

    /// IPv4 的 sockaddr：内存里就是这 4 个字节（网络字节序），Windows 也这么放
    fn v4_addr(ip: [u8; 4]) -> SOCKADDR_IN {
        SOCKADDR_IN {
            sin_family: AF_INET,
            sin_port: 0,
            sin_addr: IN_ADDR {
                S_un: IN_ADDR_0 {
                    S_addr: u32::from_ne_bytes(ip),
                },
            },
            sin_zero: [0; 8],
        }
    }

    fn v6_addr(octets: [u8; 16]) -> SOCKADDR_IN6 {
        SOCKADDR_IN6 {
            sin6_family: AF_INET6,
            sin6_port: 0,
            sin6_flowinfo: 0,
            sin6_addr: IN6_ADDR {
                u: IN6_ADDR_0 { Byte: octets },
            },
            Anonymous: SOCKADDR_IN6_0 { sin6_scope_id: 0 },
        }
    }

    /// 把**已在原地**的 sockaddr 包成 SOCKET_ADDRESS。
    /// 调用方必须保证 `sa` 在使用返回值的整段时间里不被移动/借用，否则指针悬垂。
    fn sock_of<T>(sa: &mut T, len: i32) -> SOCKET_ADDRESS {
        SOCKET_ADDRESS {
            lpSockaddr: sa as *mut T as *mut SOCKADDR,
            iSockaddrLength: len,
        }
    }

    #[test]
    fn sockaddr_v4_decodes_network_order() {
        let mut sa = v4_addr([192, 168, 1, 10]);
        let addr = sock_of(&mut sa, std::mem::size_of::<SOCKADDR_IN>() as i32);
        assert_eq!(
            unsafe { socket_address_to_ipv4(&addr) },
            Some(Ipv4Addr::new(192, 168, 1, 10))
        );
        assert_eq!(unsafe { socket_address_to_string(&addr) }, "192.168.1.10");
        // 少转一次字节序就会得到 10.1.168.192，这条断言专门盯字节序
        assert_ne!(unsafe { socket_address_to_string(&addr) }, "10.1.168.192");
    }

    #[test]
    fn sockaddr_v6_decodes_octets() {
        let mut sa = v6_addr([
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01,
        ]);
        let addr = sock_of(&mut sa, std::mem::size_of::<SOCKADDR_IN6>() as i32);
        assert_eq!(unsafe { socket_address_to_string(&addr) }, "2001:db8::1");
        assert_eq!(
            unsafe { socket_address_to_ipv4(&addr) },
            None,
            "IPv6 不得被当成 IPv4"
        );
    }

    /// 内存安全：声明长度小于目标结构体时必须拒绝解引用，而不是越界读。
    #[test]
    fn sockaddr_truncated_length_is_rejected() {
        let mut sa = v4_addr([10, 0, 0, 5]);
        let mut addr = sock_of(&mut sa, std::mem::size_of::<SOCKADDR_IN>() as i32);
        addr.iSockaddrLength = 8; // 谎报：只有 8 字节，不够 SOCKADDR_IN
        assert_eq!(unsafe { socket_address_to_ipv4(&addr) }, None);
        assert_eq!(unsafe { socket_address_to_string(&addr) }, "");
        addr.iSockaddrLength = 0;
        assert_eq!(unsafe { socket_address_to_ipv4(&addr) }, None);
        addr.iSockaddrLength = -1;
        assert_eq!(
            unsafe { socket_address_to_string(&addr) },
            "",
            "负长度不得变成超大 usize"
        );

        // IPv6 同理：长度够 SOCKADDR_IN 但不够 SOCKADDR_IN6 时不得按 v6 读
        let mut sa6 = v6_addr([0xfe, 0x80, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x01]);
        let mut short6 = sock_of(&mut sa6, std::mem::size_of::<SOCKADDR_IN6>() as i32);
        short6.iSockaddrLength = 20;
        assert_eq!(unsafe { socket_address_to_string(&short6) }, "");
        short6.iSockaddrLength = std::mem::size_of::<SOCKADDR_IN6>() as i32;
        assert_eq!(unsafe { socket_address_to_string(&short6) }, "fe80::1");
    }

    #[test]
    fn sockaddr_null_and_unknown_family_are_empty() {
        let null_addr = SOCKET_ADDRESS {
            lpSockaddr: std::ptr::null_mut(),
            iSockaddrLength: 16,
        };
        assert_eq!(unsafe { socket_address_to_ipv4(&null_addr) }, None);
        assert_eq!(unsafe { socket_address_to_string(&null_addr) }, "");

        let mut sa = SOCKADDR {
            sa_family: AF_UNSPEC,
            sa_data: [0; 14],
        };
        let unspec = sock_of(&mut sa, std::mem::size_of::<SOCKADDR>() as i32);
        assert_eq!(unsafe { socket_address_to_ipv4(&unspec) }, None);
        assert_eq!(unsafe { socket_address_to_string(&unspec) }, "");
    }

    // ---------------- fill_addresses（栈上假链表）----------------

    /// 三条链表的取数规则都能在 CI 里验证：第一个 IPv4 主地址 + 同行的前缀长度、
    /// 网关跳过非 IPv4 行、DNS 保序去重、Next 走完即停。
    #[test]
    fn fill_addresses_walks_the_three_lists() {
        let mut sa_main = v4_addr([10, 0, 0, 5]);
        let mut sa_extra = v4_addr([169, 254, 1, 1]);
        let mut u2 = IP_ADAPTER_UNICAST_ADDRESS_LH {
            Address: sock_of(&mut sa_extra, std::mem::size_of::<SOCKADDR_IN>() as i32),
            OnLinkPrefixLength: 16,
            ..Default::default()
        };
        let mut u1 = IP_ADAPTER_UNICAST_ADDRESS_LH {
            Address: sock_of(&mut sa_main, std::mem::size_of::<SOCKADDR_IN>() as i32),
            OnLinkPrefixLength: 24,
            Next: &mut u2,
            ..Default::default()
        };

        // 网关：第一行是 IPv6（必须跳过），第二行才是 IPv4
        let mut sa_gw_v6 = v6_addr([
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x02,
        ]);
        let mut sa_gw_v4 = v4_addr([10, 0, 0, 1]);
        let mut g2 = IP_ADAPTER_GATEWAY_ADDRESS_LH {
            Address: sock_of(&mut sa_gw_v4, std::mem::size_of::<SOCKADDR_IN>() as i32),
            ..Default::default()
        };
        let mut g1 = IP_ADAPTER_GATEWAY_ADDRESS_LH {
            Address: sock_of(&mut sa_gw_v6, std::mem::size_of::<SOCKADDR_IN6>() as i32),
            Next: &mut g2,
            ..Default::default()
        };

        // DNS：重复项 + 一个不认识的地址族都要被处理掉
        let mut sa_dns1 = v4_addr([223, 5, 5, 5]);
        let mut sa_dns2 = v4_addr([223, 5, 5, 5]);
        let mut sa_dns3 = v6_addr([
            0x20, 0x01, 0x0d, 0xb8, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0, 0x00, 0x01,
        ]);
        let mut sa_dns4 = SOCKADDR {
            sa_family: AF_UNSPEC,
            sa_data: [0; 14],
        };
        let mut d4 = IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
            Address: sock_of(&mut sa_dns4, std::mem::size_of::<SOCKADDR>() as i32),
            ..Default::default()
        };
        let mut d3 = IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
            Address: sock_of(&mut sa_dns3, std::mem::size_of::<SOCKADDR_IN6>() as i32),
            Next: &mut d4,
            ..Default::default()
        };
        let mut d2 = IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
            Address: sock_of(&mut sa_dns2, std::mem::size_of::<SOCKADDR_IN>() as i32),
            Next: &mut d3,
            ..Default::default()
        };
        let mut d1 = IP_ADAPTER_DNS_SERVER_ADDRESS_XP {
            Address: sock_of(&mut sa_dns1, std::mem::size_of::<SOCKADDR_IN>() as i32),
            Next: &mut d2,
            ..Default::default()
        };

        let a = IP_ADAPTER_ADDRESSES_LH {
            FirstUnicastAddress: &mut u1,
            FirstGatewayAddress: &mut g1,
            FirstDnsServerAddress: &mut d1,
            ..Default::default()
        };

        let mut nic = test_nic(1, "以太网", NicKind::Wired);
        unsafe { fill_addresses(&a, &mut nic) };

        assert_eq!(
            nic.ipv4, "10.0.0.5",
            "必须取第一个 IPv4，而不是链表尾部的 APIPA 地址"
        );
        assert_eq!(
            nic.mask, "255.255.255.0",
            "掩码必须取同一行的 /24，不是第二行的 /16"
        );
        assert_eq!(nic.gateway, "10.0.0.1", "网关要跳过 IPv6 行取第一个 IPv4");
        assert_eq!(
            nic.dns,
            vec!["223.5.5.5".to_string(), "2001:db8::1".to_string()]
        );
        // 链表走完（d4 之后 Next 为空）必须停止，不得崩溃或重复
        assert_eq!(nic.dns.len(), 2);
    }

    /// 空链表（虚拟/未配置网卡常见）必须留下空字段而不是崩溃。
    #[test]
    fn fill_addresses_with_no_rows_keeps_fields_empty() {
        let a = IP_ADAPTER_ADDRESSES_LH::default();
        let mut nic = test_nic(2, "以太网 2", NicKind::Other);
        unsafe { fill_addresses(&a, &mut nic) };
        assert_eq!(nic.ipv4, "");
        assert_eq!(nic.mask, "");
        assert_eq!(nic.gateway, "");
        assert!(nic.dns.is_empty());
    }

    // ---------------- 真机冒烟 ----------------

    /// 需要管理员与真实网卡，默认跳过，**非 CI 可验证**。手动跑：
    /// `cargo test -- --ignored --nocapture`
    /// 打印内容同时供 Task 4 报告与真机关键字表裁定使用。
    #[test]
    #[ignore]
    fn real_adapters_have_at_least_one_entry() {
        use crate::adapterclass::{is_virtual, is_virtual_if_type};

        // ---- 1) GetAdaptersAddresses 的真实返回形态 ----
        println!("\n=== GetAdaptersAddresses 形态探针 ===");
        unsafe {
            let mut tiny: u32 = 1;
            let mut buf = vec![0u8; 1];
            let code = GetAdaptersAddresses(
                AF_UNSPEC.0 as u32,
                GAA_FLAG_INCLUDE_GATEWAYS | GAA_FLAG_INCLUDE_ALL_COMPARTMENTS,
                None,
                Some(buf.as_mut_ptr().cast::<IP_ADAPTER_ADDRESSES_LH>()),
                &mut tiny,
            );
            println!(
                "1 字节缓冲调用 → 返回码 {code}（{}），出参需求长度 {tiny}",
                error_text(code)
            );
            println!(
                "ERROR_SUCCESS={} / ERROR_BUFFER_OVERFLOW={}",
                ERROR_SUCCESS.0, ERROR_BUFFER_OVERFLOW.0
            );
        }
        let (buf, retries) = fetch_adapter_table().expect("fetch_adapter_table 失败");
        println!(
            "fetch_adapter_table 缓冲重试次数 = {retries}，缓冲区 {} 字节",
            buf.len()
        );

        // ---- 2) 每张网卡（含被过滤掉的）的完整清单 ----
        println!("\n=== 原始网卡清单（未过滤）===");
        let defaults = default_route_interfaces().expect("default_route_interfaces 失败");
        println!("默认路由接口 IfIndex = {defaults:?}");
        /// 一条原始记录的转储：`skip` 之前几个字段是「采集输入」，`nic` 是「采集输出」，
        /// 两份都留下，第 4 节才能按行对齐打印。
        struct RowDump {
            if_index: u32,
            luid: NET_LUID_LH,
            adapter_name: String,
            if_type: u32,
            mac_len: u32,
            entry_len: u32,
            skip: bool,
            description: String,
            alias: String,
            nic: Nic,
        }

        let mut rows: Vec<RowDump> = Vec::new();
        unsafe {
            for a in adapter_rows(&buf) {
                let description = wstr_from_ptr(a.Description.0);
                let alias = wstr_from_ptr(a.FriendlyName.0);
                // AdapterName 是真正的 ANSI（接口 GUID 字符串）：Task 6 除 netsh 别名外的另一条路
                let adapter_name = if a.AdapterName.is_null() {
                    String::new()
                } else {
                    String::from_utf8_lossy(a.AdapterName.as_bytes()).into_owned()
                };
                let disabled = admin_disabled(a.Anonymous1.Anonymous.IfIndex);
                let skip =
                    is_filtered_out(a.IfType, &description, a.PhysicalAddressLength, disabled);
                let nic = build_nic(a, &description, &alias, &defaults, disabled);
                rows.push(RowDump {
                    if_index: a.Anonymous1.Anonymous.IfIndex,
                    luid: a.Luid,
                    adapter_name,
                    if_type: a.IfType,
                    mac_len: a.PhysicalAddressLength,
                    entry_len: a.Anonymous1.Anonymous.Length,
                    skip,
                    description,
                    alias,
                    nic,
                });
            }
        }
        let kept = rows.iter().filter(|r| !r.skip).count();
        for (i, r) in rows.iter().enumerate() {
            println!(
                "#{i:<3} {} ifindex={:<4} iftype={:<4} maclen={:<2} entrylen={:<4} kind={:?} virt={} iftypevirt={}",
                if r.skip { "过滤" } else { "保留" },
                r.if_index,
                r.if_type,
                r.mac_len,
                r.entry_len,
                r.nic.kind,
                is_virtual(&r.description),
                is_virtual_if_type(r.if_type),
            );
            println!("     description={}", r.description);
            println!("     alias={} adapter_name={}", r.alias, r.adapter_name);
            println!(
                "     mac={} speed={} en={} conn={} metric={} gw={} ip={}/{} dns={:?}",
                r.nic.mac,
                r.nic.speed,
                r.nic.enabled,
                r.nic.connected,
                r.nic.metric,
                r.nic.gateway,
                r.nic.ipv4,
                r.nic.mask,
                r.nic.dns
            );
            println!(
                "     guid={} defrt={}",
                r.nic.network_guid, r.nic.is_default_route
            );
        }
        println!("共 {} 条原始条目，保留 {kept} 条", rows.len());

        // ---- 3) list() 的实际结果 ----
        println!("\n=== list() 结果（已过滤 + 已排序）===");
        let nics = list().expect("list() 失败");
        for n in &nics {
            println!(
                "{:?} | {} | {} | {} | mac={} ip={}/{} gw={} dns={:?} metric={} en={} conn={} defrt={} guid={}",
                n.kind,
                n.alias,
                n.description,
                n.speed,
                n.mac,
                n.ipv4,
                n.mask,
                n.gateway,
                n.dns,
                n.metric,
                n.enabled,
                n.connected,
                n.is_default_route,
                n.network_guid
            );
        }
        assert!(
            !nics.is_empty(),
            "未采集到任何物理网卡，检查权限或 API 调用"
        );

        // ---- 4) 禁用状态 + 两个 Entry API 的真实返回形态 ----
        // 对照命令（只读，不改任何网卡状态）：netsh interface show interface
        println!("\n=== 管理状态 / GetIfEntry2 / GetIpInterfaceEntry 返回形态 ===");
        println!(
            "ifindex disabled  admin_status             metric                     过滤     alias"
        );
        for r in &rows {
            println!(
                "{:<7} {:<9} {:<24} {:<26} {:<6} {} ({})",
                r.if_index,
                admin_disabled(r.if_index),
                format!("{:?}", admin_status(r.if_index)),
                format!("{:?}", interface_metric_entry(r.if_index, r.luid)),
                if r.skip { "是" } else { "否" },
                r.alias,
                r.adapter_name
            );
        }
    }

    // ---------------- Route ----------------

    #[test]
    fn protocol_names_cover_what_windows_actually_reports() {
        assert_eq!(protocol_name(2), "本地"); // MIB_IPPROTO_LOCAL
        assert_eq!(protocol_name(3), "手动"); // MIB_IPPROTO_NETMGMT
        assert_eq!(protocol_name(19), "DHCP"); // MIB_IPPROTO_DHCP
        assert_eq!(protocol_name(10006), "静态"); // MIB_IPPROTO_NT_STATIC
        assert_eq!(protocol_name(10002), "自动静态");
        assert_eq!(protocol_name(13), "OSPF");
        // 未收录的协议不能显示空串，否则表格里会出现无名行
        assert_eq!(protocol_text(9999), "协议 9999");
    }

    #[test]
    fn origin_names_cover_all_documented_values() {
        assert_eq!(origin_name(0), "手动");
        assert_eq!(origin_name(1), "系统");
        assert_eq!(origin_name(2), "DHCP");
        assert_eq!(origin_name(3), "路由通告");
        assert_eq!(origin_name(4), "6to4");
        assert_eq!(origin_text(99), "来源 99");
    }

    /// 用 union 的 Ipv4 分支填一个地址；`S_addr` 是内存里的网络序字节。
    fn prefix_v4(bytes: [u8; 4], len: u8) -> IP_ADDRESS_PREFIX {
        let mut p = IP_ADDRESS_PREFIX::default();
        // 写 union 字段（Copy 类型）不需要 unsafe；读才需要
        p.Prefix.Ipv4.sin_family = AF_INET;
        p.Prefix.Ipv4.sin_addr.S_un.S_addr = u32::from_ne_bytes(bytes);
        p.PrefixLength = len;
        p
    }

    #[test]
    fn ipv4_prefix_text_uses_prefix_length() {
        assert_eq!(prefix_text(&prefix_v4([10, 0, 0, 0], 24)), "10.0.0.0/24");
    }

    /// 默认路由必须印成 0.0.0.0/0 —— 路由页要靠它识别默认出口。
    #[test]
    fn default_route_prefix_text() {
        assert_eq!(prefix_text(&prefix_v4([0, 0, 0, 0], 0)), "0.0.0.0/0");
    }

    /// 排序：接口升序 → 跃点升序 → 目的前缀字典序；同接口内默认路由排最前。
    #[test]
    fn routes_sort_by_interface_metric_then_destination() {
        let mk = |if_index: u32, dest: &str, metric: u32| Route {
            destination: dest.into(),
            next_hop: "0.0.0.0".into(),
            if_index,
            metric,
            protocol: "本地".into(),
            origin: "系统".into(),
            loopback: false,
            immortal: false,
        };
        let mut v = vec![
            mk(19, "10.0.0.0/24", 256),
            mk(19, "0.0.0.0/0", 25),
            mk(3, "fe80::/64", 256),
        ];
        sort_routes(&mut v);
        assert_eq!(
            v.iter()
                .map(|r| (r.if_index, r.destination.as_str()))
                .collect::<Vec<_>>(),
            vec![(3, "fe80::/64"), (19, "0.0.0.0/0"), (19, "10.0.0.0/24")]
        );
    }

    /// 真机路由表冒烟：至少要有回环与默认路由，且每行 destination 都能解析成 a.b.c.d/nn。
    #[test]
    #[ignore]
    fn real_route_table_has_loopback_and_default() {
        let routes = list_routes().expect("读取路由表失败");
        assert!(!routes.is_empty());
        assert!(routes.iter().any(|r| r.loopback), "缺回环路由");
        assert!(
            routes.iter().any(|r| r.destination.ends_with("/0")),
            "缺默认路由"
        );
        for r in &routes {
            let (addr, len) = r.destination.rsplit_once('/').expect("缺前缀长度");
            assert!(addr.parse::<std::net::IpAddr>().is_ok(), "坏地址 {addr}");
            assert!(len.parse::<u8>().is_ok(), "坏前缀 {len}");
        }
        println!("路由 {}/IPv4+IPv6", routes.len());
    }
}
