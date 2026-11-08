// Task 3：网卡有线/无线/其他分类（纯 std 纯函数，无 Win32 调用）。
// 模块挂载约定：本仓库统一 `pub mod xxx;`，不使用 `#![allow(dead_code)]`；
// 本模块全是纯函数，没有 `pub unsafe fn`，故无需 `# Safety` 章节。

/// 网卡大类：有线 / 无线 / VPN / 虚拟 / 其他。Task 5 的分组标题与排序键都由它派生。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NicKind {
    Wired,
    Wireless,
    Vpn,
    Virtual,
    Other,
}

// IF_TYPE 数字取自 IANA ifTypes（iprtdb.h / ipifcons.h），与 netinfo 里的 if_type 同源。
// 公开给 netinfo：`is_virtual_if_type` 的判据与真机诊断清单都引用同一批常量，避免两处各写一份数字。
pub const IF_TYPE_ETHERNET: u32 = 6;
pub const IF_TYPE_IEEE80211: u32 = 71;
pub const IF_TYPE_WWANPP: u32 = 243;
pub const IF_TYPE_WWANPP2: u32 = 244;
// 以下四个不是「网卡类型」而是「这类接口必然不是用户要管理的物理卡」的信号。
pub const IF_TYPE_PPP: u32 = 23;
pub const IF_TYPE_SOFTWARE_LOOPBACK: u32 = 24;
pub const IF_TYPE_PROP_VIRTUAL: u32 = 53;
pub const IF_TYPE_TUNNEL: u32 = 131;

/// 按 IF_TYPE 分类；IF_TYPE 不认识时回退到驱动描述关键字。
/// `description` 传 IP_ADAPTER_ADDRESSES_LH.Description（驱动描述，通常是英文），
/// 不要传本地化的 FriendlyName，否则关键字表失效。
///
/// 判定顺序（后面的分支都会被前面短路）：
/// 蓝牙 → VPN → 虚拟 → 按 IfType 分有线/无线 → 关键字回退 → 其他。
/// VPN 必须排在虚拟前面：VPN 客户端常装成虚拟网卡（TAP-Windows 报 IfType=6，
/// Sangfor aTrust 报 53），先按虚拟判就会被吞进虚拟组。
///
/// 跨任务契约：虚拟网卡（Hyper-V、VPN、隧道）常带**物理** IfType（例如 Hyper-V
/// 虚拟以太网适配器就是 6），所以 `NicKind` 不可用来推断「这是物理卡」——
/// 过滤只能靠 `is_virtual` + `is_virtual_if_type` + 物理地址长度。
pub fn classify(if_type: u32, description: &str) -> NicKind {
    let d = description.to_ascii_lowercase();
    // 蓝牙 PAN 实测 IfType=6(Ethernet)、真正的物理介质才是 Bluetooth；若只看 IfType
    // 会误判成有线。蓝牙面板按用户裁定单列为「其他」，且优先于 IfType 判定。
    if d.contains("bluetooth") {
        return NicKind::Other;
    }
    if is_vpn(description) {
        return NicKind::Vpn;
    }
    if is_virtual(description) || is_virtual_if_type(if_type) {
        return NicKind::Virtual;
    }
    match if_type {
        IF_TYPE_ETHERNET => NicKind::Wired,
        IF_TYPE_IEEE80211 | IF_TYPE_WWANPP | IF_TYPE_WWANPP2 => NicKind::Wireless,
        _ => keyword_fallback(description),
    }
}

/// VPN 客户端/隧道适配器。与 `is_virtual` 分开是因为它们要单独成组：
/// 用户会主动连 VPN，而 Hyper-V/VMware 那类虚拟网卡只是顺带存在。
///
/// 关键字按厂商驱动描述取样（TAP-Windows、Sangfor aTrust、Cisco AnyConnect、
/// WireGuard 等），加上中文系统自带的「虚拟专用网络」。
pub fn is_vpn(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    [
        "vpn",
        "tap-windows",
        "tap-",
        "openvpn",
        "wireguard",
        "wintun",
        "anyconnect",
        "atrust",
        "sangfor",
        "pptp",
        "l2tp",
        "sstp",
        "ikev2",
        "globalprotect",
        "forticlient",
        "pulse secure",
        "softether",
        "openconnect",
        "zerotier",
        "tailscale",
        "虚拟专用网络",
    ]
    .iter()
    .any(|k| d.contains(k))
}

/// 关键字回退：先判无线再判有线（描述里两者都出现时按无线算）。
/// 关键字一律小写，比较前把描述整体 ASCII 转小写，因此大小写无关。
fn keyword_fallback(description: &str) -> NicKind {
    let d = description.to_ascii_lowercase();
    if ["wi-fi", "wifi", "wireless", "wlan", "802.11"]
        .iter()
        .any(|k| d.contains(k))
    {
        NicKind::Wireless
    } else if [
        "ethernet",
        "gbe",
        "gb e",
        "pcie gigabit",
        "family controller",
    ]
    .iter()
    .any(|k| d.contains(k))
    {
        NicKind::Wired
    } else {
        NicKind::Other
    }
}

/// 分组标题用词，与 UI 显示文本一一对应。
pub fn kind_name(k: NicKind) -> &'static str {
    match k {
        NicKind::Wired => "有线网卡",
        NicKind::Wireless => "无线网卡",
        NicKind::Vpn => "VPN",
        NicKind::Virtual => "虚拟网卡",
        NicKind::Other => "其他",
    }
}

/// 列表分组排序键：0 有线、1 无线、2 VPN、3 虚拟、4 其他。
pub fn group_order(k: NicKind) -> u8 {
    match k {
        NicKind::Wired => 0,
        NicKind::Wireless => 1,
        NicKind::Vpn => 2,
        NicKind::Virtual => 3,
        NicKind::Other => 4,
    }
}

/// 虚拟/隧道网卡：这些不是用户要管理的物理网卡。
/// VPN 也命中这张表（TAP 等），需要区分时先问 `is_vpn`。
pub fn is_virtual(description: &str) -> bool {
    let d = description.to_ascii_lowercase();
    [
        "virtual",
        "hyper-v",
        "vmware",
        "virtualbox",
        "wan miniport",
        "tap-",
        "tap-windows",
        "loopback",
        "ndiswan",
        "pptp",
        "l2tp",
        "network monitor",
        "kernel debug",
    ]
    .iter()
    .any(|k| d.contains(k))
}

/// 结构化虚拟信号：IfType 本身就是「不是物理卡」的类型编号。
///
/// 存在的理由：`is_virtual` 的关键字表是英文的，而中文 Windows 自带驱动的描述本身就是
/// 中文（「WAN 微型端口 (IP)」= PPP 23、「Microsoft IP-HTTPS 平台适配器」= 隧道 131），
/// 只靠关键字表必然漏判。netinfo 的过滤条件把它和 `is_virtual` 并联使用。
///
/// 四个编号取自 ipifcons.h：23=PPP、24=SOFTWARE_LOOPBACK、53=PROP_VIRTUAL（专有虚拟/内部）、
/// 131=TUNNEL（封装/隧道接口）。
pub fn is_virtual_if_type(if_type: u32) -> bool {
    matches!(
        if_type,
        IF_TYPE_PPP | IF_TYPE_SOFTWARE_LOOPBACK | IF_TYPE_PROP_VIRTUAL | IF_TYPE_TUNNEL
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethernet_if_type_is_wired() {
        assert_eq!(
            classify(6, "Intel(R) Ethernet Connection I219-V"),
            NicKind::Wired
        );
    }

    #[test]
    fn ieee80211_if_type_is_wireless() {
        assert_eq!(classify(71, "Intel(R) Wi-Fi 6E AX211"), NicKind::Wireless);
    }

    #[test]
    fn wwan_if_types_are_wireless() {
        assert_eq!(classify(243, "Fibocom FM150"), NicKind::Wireless);
        assert_eq!(classify(244, "Fibocom FM150"), NicKind::Wireless);
    }

    #[test]
    fn unknown_if_type_falls_back_to_description_keyword() {
        assert_eq!(
            classify(0, "Marvell AVASTAR Wireless-AC"),
            NicKind::Wireless
        );
        assert_eq!(classify(0, "Realtek PCIe GbE Family"), NicKind::Wired);
        assert_eq!(classify(0, "Some Unclassified Adapter"), NicKind::Other);
    }

    #[test]
    fn wifi_keyword_variants_all_wireless() {
        for d in [
            "ASUS Wi-Fi",
            "USB WiFi Adapter",
            "Qualcomm WLAN",
            "802.11n USB LAN",
        ] {
            assert_eq!(classify(0, d), NicKind::Wireless, "描述：{d}");
        }
    }

    #[test]
    fn virtual_detected() {
        assert!(is_virtual("Hyper-V Virtual Ethernet Adapter"));
        assert!(is_virtual("WAN Miniport (IP)"));
        assert!(is_virtual("TAP-Windows Adapter V9"));
        assert!(!is_virtual("Intel(R) Ethernet Connection I219-V"));
    }

    #[test]
    fn kind_name_and_order() {
        assert_eq!(kind_name(NicKind::Wired), "有线网卡");
        assert_eq!(kind_name(NicKind::Wireless), "无线网卡");
        assert_eq!(kind_name(NicKind::Vpn), "VPN");
        assert_eq!(kind_name(NicKind::Virtual), "虚拟网卡");
        assert_eq!(kind_name(NicKind::Other), "其他");

        // 分组顺序：有线 → 无线 → VPN → 虚拟 → 其他
        let mut kinds = [
            NicKind::Other,
            NicKind::Virtual,
            NicKind::Vpn,
            NicKind::Wireless,
            NicKind::Wired,
        ];
        kinds.sort_by_key(|k| group_order(*k));
        assert_eq!(
            kinds,
            [
                NicKind::Wired,
                NicKind::Wireless,
                NicKind::Vpn,
                NicKind::Virtual,
                NicKind::Other
            ]
        );
    }

    // ---- 以下是对简报用例的补强：每个 IF_TYPE 分支、每个关键字都要有独立判别力 ----

    /// classify 的 IF_TYPE 数字分支 + 关键字回退合并成一张表逐条核对。
    /// 每行描述都刻意只命中一个关键字，保证「删掉某个关键字」必有对应用例变红。
    #[test]
    fn classify_covers_every_if_type_and_keyword() {
        let cases: [(&str, u32, &str, NicKind); 27] = [
            // IF_TYPE 精确分支：命中时描述完全不起作用
            ("if_type=6 有线", 6, "Something Unknown", NicKind::Wired),
            (
                "if_type=71 无线",
                71,
                "Something Unknown",
                NicKind::Wireless,
            ),
            ("if_type=243 WWAN", 243, "Fibocom FM150", NicKind::Wireless),
            ("if_type=244 WWAN2", 244, "Fibocom FM150", NicKind::Wireless),
            (
                "if_type=6 压过无线描述",
                6,
                "Intel(R) Wi-Fi 6E AX211",
                NicKind::Wired,
            ),
            (
                "if_type=71 压过有线描述",
                71,
                "Realtek PCIe GbE Family",
                NicKind::Wireless,
            ),
            // 未知 IF_TYPE：逐个无线关键字
            ("0/wi-fi", 0, "ASUS Wi-Fi Card", NicKind::Wireless),
            ("0/wifi", 0, "USB WiFi Adapter", NicKind::Wireless),
            (
                "0/wireless",
                0,
                "Marvell AVASTAR Wireless-AC",
                NicKind::Wireless,
            ),
            ("0/wlan", 0, "Qualcomm WLAN", NicKind::Wireless),
            ("0/802.11", 0, "802.11n USB LAN", NicKind::Wireless),
            // 未知 IF_TYPE：逐个有线关键字
            ("0/ethernet", 0, "Intel Ethernet Connection", NicKind::Wired),
            ("0/gbe", 0, "Realtek PCIe GbE Family", NicKind::Wired),
            (
                "0/gb e",
                0,
                "Realtek Gb E Family Network Adapter",
                NicKind::Wired,
            ),
            (
                "0/pcie gigabit",
                0,
                "Intel(R) PCIe Gigabit Network Connection",
                NicKind::Wired,
            ),
            (
                "0/family controller",
                0,
                "Intel(R) I219-LM Family Controller",
                NicKind::Wired,
            ),
            // 未知 IF_TYPE 且两边关键字都不命中 → Other
            (
                "0/kernel debug",
                0,
                "Microsoft Kernel Debug Network",
                NicKind::Virtual,
            ),
            (
                "1/loopback",
                1,
                "Software Loopback Interface",
                NicKind::Virtual,
            ),
            (
                "1/真无关键字",
                1,
                "Some Unclassified Adapter",
                NicKind::Other,
            ),
            ("24/无关键字", 24, "WAN Miniport (IP)", NicKind::Virtual),
            ("53/无关键字", 53, "Unknown Device", NicKind::Virtual),
            // VPN 与虚拟网卡各有专属分支
            (
                "6/TAP 虚拟网卡但属于 VPN",
                6,
                "TAP-Windows Adapter V9",
                NicKind::Vpn,
            ),
            ("53/aTrust", 53, "Sangfor aTrust VNIC", NicKind::Vpn),
            (
                "6/Hyper-V",
                6,
                "Hyper-V Virtual Ethernet Adapter",
                NicKind::Virtual,
            ),
            // Task 4 边界：Description 为空/空白时不得 panic，也不得凭空归类
            ("0/空描述", 0, "", NicKind::Other),
            ("29/空白描述", 29, " ", NicKind::Other),
            // 蓝牙 PAN 真机 IfType=6，只有物理介质是 Bluetooth：必须优先判成其他
            (
                "6/蓝牙 PAN",
                6,
                "Bluetooth Device (Personal Area Network)",
                NicKind::Other,
            ),
        ];
        for (label, if_type, description, expected) in cases {
            assert_eq!(
                classify(if_type, description),
                expected,
                "用例 {label}：if_type={if_type}, 描述={description}"
            );
        }
    }

    /// 蓝牙优先于 IfType：真机蓝牙 PAN 报 IfType=6，只有描述里带 Bluetooth，
    /// 若先按 IfType 判就会归到有线（用户已裁定归「其他」）。
    #[test]
    fn bluetooth_wins_over_if_type() {
        for if_type in [0, 6, 71, 243] {
            assert_eq!(
                classify(if_type, "Bluetooth Device (Personal Area Network)"),
                NicKind::Other,
                "if_type={if_type} 的蓝牙适配器应归其他"
            );
        }
        assert_eq!(classify(6, "BLUETOOTH PAN"), NicKind::Other);
        // 反过来不能误伤：描述不含 bluetooth 的物理网卡照旧按 IfType 分类
        assert_eq!(classify(6, "Realtek PCIe GbE Family"), NicKind::Wired);
    }

    /// VPN 优先于虚拟与 IfType：VPN 客户端本身就是虚拟网卡（TAP 报 6、aTrust 报 53），
    /// 若先按虚拟判就会并进「虚拟网卡」组。
    #[test]
    fn vpn_wins_over_virtual_and_if_type() {
        for (if_type, d) in [
            (6, "TAP-Windows Adapter V9"),
            (6, "OpenVPN Wintun"),
            (53, "Sangfor aTrust VNIC"),
            (53, "aTrustXtun Userspace Tunnel"),
            (0, "Cisco AnyConnect Secure Mobility Virtual Adapter"),
            (0, "WireGuard Tunnel"),
            (0, "Fortinet SSL VPN Virtual Ethernet Adapter"),
        ] {
            assert_eq!(classify(if_type, d), NicKind::Vpn, "if_type={if_type}：{d}");
        }
        assert!(is_vpn("某公司虚拟专用网络适配器"));
        // 物理网卡与普通虚拟网卡不得被 VPN 关键字误伤
        assert!(!is_vpn("Hyper-V Virtual Ethernet Adapter"));
        assert!(!is_vpn("Realtek PCIe GbE Family Controller"));
        assert!(!is_vpn("Intel(R) Wi-Fi 6E AX211"));
    }

    /// 虚拟网卡单独成组：Hyper-V / VMware / 隧道接口即使报的是物理 IfType 也归虚拟。
    #[test]
    fn virtual_group_covers_hyperv_vmware_and_tunnels() {
        for (if_type, d) in [
            (6, "Hyper-V Virtual Ethernet Adapter"),
            (6, "VMware Network Adapter VMnet8"),
            (6, "VirtualBox Host-Only Network Adapter"),
            (131, "Microsoft IP-HTTPS 平台适配器"),
            (53, "某虚拟适配器"),
        ] {
            assert_eq!(
                classify(if_type, d),
                NicKind::Virtual,
                "if_type={if_type}：{d}"
            );
        }
        // 隧道/内部 IfType 即使描述完全认不出来也进虚拟组
        assert_eq!(classify(131, "Unknown Tunnel"), NicKind::Virtual);
        assert_eq!(classify(53, "Unknown Device"), NicKind::Virtual);
        // 有线/无线的判定不受影响
        assert_eq!(
            classify(6, "Realtek USB GbE Family Controller"),
            NicKind::Wired
        );
    }

    /// 回退分支里无线判定先于有线判定（简报的实现顺序）：混合关键字时必须是无线。
    #[test]
    fn wireless_keyword_takes_precedence_over_ethernet_keyword() {
        assert_eq!(
            classify(0, "Realtek Wireless Ethernet Bridge"),
            NicKind::Wireless
        );
        assert_eq!(classify(0, "802.11 Ethernet Adapter"), NicKind::Wireless);
    }

    /// 关键字匹配大小写无关（驱动描述常见全大写/混合大小写）。
    #[test]
    fn keyword_matching_is_case_insensitive() {
        // 大写/混合大小写的有线、无线关键字
        assert_eq!(classify(0, "USB WIFI ADAPTER"), NicKind::Wireless);
        assert_eq!(classify(0, "Qualcomm Wlan"), NicKind::Wireless);
        assert_eq!(
            classify(0, "REALTEK PCIE GBE FAMILY CONTROLLER"),
            NicKind::Wired,
            "全大写描述应命中 gbe 关键字"
        );
        assert_eq!(classify(0, "Intel ETHERNET Connection"), NicKind::Wired);
        // 混合大小写的虚拟关键字
        assert!(is_virtual("HYPER-V Virtual Ethernet Adapter"));
        assert!(is_virtual("TAP-WINDOWS ADAPTER V9"));
        assert!(is_virtual("WAN MINIPORT (IP)"));
        assert!(is_virtual("wAn mInIpOrT (IP)"));
    }

    /// group_order 的绝对值（Task 5 排序键直接用）。
    #[test]
    fn group_order_absolute_values() {
        assert_eq!(group_order(NicKind::Wired), 0);
        assert_eq!(group_order(NicKind::Wireless), 1);
        assert_eq!(group_order(NicKind::Vpn), 2);
        assert_eq!(group_order(NicKind::Virtual), 3);
        assert_eq!(group_order(NicKind::Other), 4);

        // 排序键契约：Task 5 用它把有线排在最前、其他垫底
        let mut kinds = [
            NicKind::Other,
            NicKind::Virtual,
            NicKind::Vpn,
            NicKind::Wireless,
            NicKind::Wired,
        ];
        kinds.sort_by_key(|k| group_order(*k));
        assert_eq!(
            kinds,
            [
                NicKind::Wired,
                NicKind::Wireless,
                NicKind::Vpn,
                NicKind::Virtual,
                NicKind::Other
            ]
        );
    }

    /// 虚拟关键字表逐条命中（第一列是被钉住的关键字）。
    /// 已知两处子串冗余：`virtualbox` 必含 `virtual`、`tap-windows` 必含 `tap-`，
    /// 删掉前者仍会被后者兜住 —— 属表内安全冗余，不是测试漏洞。
    #[test]
    fn virtual_keywords_each_detectable() {
        let hits: [(&str, &str); 13] = [
            (
                "virtual",
                "Cisco AnyConnect Secure Mobility Virtual Adapter",
            ),
            ("hyper-v", "Hyper-V Network Adapter"),
            ("vmware", "VMware Network Adapter VMnet8"),
            ("virtualbox", "VirtualBox Host-Only Network Adapter"),
            ("wan miniport", "WAN Miniport (IP)"),
            // 构造串（真实驱动名多写作 TAP-Windows…），此行只为隔离 tap- 这条关键字
            ("tap-", "TAP-Adapter (VPN)"),
            ("tap-windows", "TAP-Windows Adapter V9"),
            ("loopback", "Microsoft KM-TEST Loopback Adapter"),
            ("ndiswan", "NdisWan Adapter"),
            ("pptp", "Cisco Systems VPN Adapter (PPTP)"),
            ("l2tp", "RAS L2TP Miniport"),
            ("network monitor", "Microsoft Network Monitor Driver"),
            ("kernel debug", "Microsoft Kernel Debug Network Adapter"),
        ];
        for (keyword, description) in hits {
            assert!(
                is_virtual(description),
                "关键字「{keyword}」未命中：{description}"
            );
        }
        assert!(!is_virtual(""));
    }

    /// 物理网卡不得被误判为虚拟（Task 5 用它过滤，误杀等于少一张网卡）。
    #[test]
    fn physical_descriptions_are_not_virtual() {
        for d in [
            "Intel(R) Ethernet Connection I219-V",
            "Realtek PCIe GbE Family Controller",
            "Intel(R) Wi-Fi 6E AX211 160MHz",
            "Marvell AVASTAR Wireless-AC Network Controller",
            "Fibocom FM150",
            "Qualcomm QCA61x4a Bluetooth Adapter",
        ] {
            assert!(!is_virtual(d), "物理网卡被误判为虚拟：{d}");
        }
    }

    /// 已知盲区的机制留档（不是对正确性的背书）：`is_virtual` 的关键字表是英文的，
    /// 纯中文描述一律不命中，所以 `is_virtual("虚拟专用网络适配器")` 为 false。
    /// 分类侧另有中文 VPN 关键字与结构化 IfType 兜底，见 `classify`。
    #[test]
    fn chinese_descriptions_are_a_known_blind_spot() {
        // IF_TYPE 命中时中文描述无影响
        assert_eq!(classify(6, "以太网 网络适配器"), NicKind::Wired);
        assert_eq!(classify(71, "无线局域网适配器"), NicKind::Wireless);
        // 未知 IF_TYPE + 纯中文描述 → 关键字都不命中 → Other / 非虚拟
        assert_eq!(classify(0, "无线局域网适配器"), NicKind::Other);
        assert_eq!(classify(0, "以太网 网络适配器"), NicKind::Other);
        assert!(!is_virtual("虚拟专用网络适配器"));
        // 中文 VPN 描述单独有词条，能进 VPN 组
        assert_eq!(classify(0, "虚拟专用网络适配器"), NicKind::Vpn);
        // 中英混排时只要含 ASCII 关键字就仍算虚拟
        assert!(is_virtual("Hyper-V 虚拟以太网适配器"));
    }

    /// Task 4 的结构化虚拟信号：IfType 属于 {23 PPP, 24 环回, 53 虚拟/内部, 131 隧道} 即虚拟。
    /// 中文 Windows 自带驱动的描述是中文（如「WAN 微型端口 (IP)」），英文关键字表必然漏判，
    /// netinfo::list() 靠这个函数兜住这些卡。
    #[test]
    fn is_virtual_if_type_table() {
        let virtual_types: [u32; 4] = [23, 24, 53, 131];
        for t in virtual_types {
            assert!(is_virtual_if_type(t), "IfType={t} 应判为虚拟/环回/隧道");
        }
        // 物理与常见真实类型必须为 false，否则 Task 4 会把真网卡滤掉
        let physical_types: [u32; 11] = [0, 1, 6, 7, 9, 15, 28, 71, 125, 243, 244];
        for t in physical_types {
            assert!(!is_virtual_if_type(t), "IfType={t} 不应被判为虚拟");
        }
        // 边界：邻位数字不得被范围判断误吞
        assert!(!is_virtual_if_type(22));
        assert!(!is_virtual_if_type(25));
        assert!(!is_virtual_if_type(52));
        assert!(!is_virtual_if_type(54));
        assert!(!is_virtual_if_type(130));
        assert!(!is_virtual_if_type(132));
        assert!(!is_virtual_if_type(u32::MAX));
    }

    /// 结构化常量对外可见：netinfo 的过滤判据与真机清单都要引用同一批数字。
    #[test]
    fn if_type_constants_are_public() {
        assert_eq!(IF_TYPE_ETHERNET, 6);
        assert_eq!(IF_TYPE_IEEE80211, 71);
        assert_eq!(IF_TYPE_WWANPP, 243);
        assert_eq!(IF_TYPE_WWANPP2, 244);
        assert_eq!(IF_TYPE_PPP, 23);
        assert_eq!(IF_TYPE_SOFTWARE_LOOPBACK, 24);
        assert_eq!(IF_TYPE_PROP_VIRTUAL, 53);
        assert_eq!(IF_TYPE_TUNNEL, 131);
        // classify 走 IF_TYPE 分支时，中文虚拟描述会被判成物理类型 —— 这正是
        // netinfo 必须另用 is_virtual_if_type 过滤的原因（NicKind 不可用来推断物理卡）。
        assert_eq!(classify(6, "虚拟以太网适配器"), NicKind::Wired);
        assert!(is_virtual_if_type(24));
    }

    /// NicKind 的派生契约（简报 Interfaces 指定 Debug/PartialEq/Eq/Clone/Copy）：
    /// Task 5 排序要在闭包里按值复制 NicKind，丢了 Copy 会直接编译失败。
    #[test]
    fn nic_kind_derives_are_kept() {
        let a = NicKind::Wireless;
        let b = a; // Copy：按值移动后 a 仍可用
        assert_eq!(a, b); // PartialEq
        assert_eq!(Clone::clone(&a), b); // Clone（显式函数形式，避开 clone_on_copy）
        let all = [
            NicKind::Wired,
            NicKind::Wireless,
            NicKind::Vpn,
            NicKind::Virtual,
            NicKind::Other,
        ];
        // Debug：变体名可读（Task 7 详情/日志会直接打印）
        assert_eq!(format!("{all:?}"), "[Wired, Wireless, Vpn, Virtual, Other]");
        assert_ne!(NicKind::Wired, NicKind::Other);
        // Eq 语义：分组去重/比较按值而非按引用
        assert_eq!(all.iter().filter(|&&k| k == NicKind::Wired).count(), 1);
    }
}
