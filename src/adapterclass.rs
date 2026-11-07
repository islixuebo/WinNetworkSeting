// Task 3：网卡有线/无线/其他分类（纯 std 纯函数，无 Win32 调用）。
// 模块挂载约定：本仓库统一 `pub mod xxx;`，不使用 `#![allow(dead_code)]`；
// 本模块全是纯函数，没有 `pub unsafe fn`，故无需 `# Safety` 章节。

/// 网卡大类：有线 / 无线 / 其他。Task 5 的分组标题与排序键都由它派生。
#[derive(Debug, PartialEq, Eq, Clone, Copy)]
pub enum NicKind {
    Wired,
    Wireless,
    Other,
}

// IF_TYPE 数字取自 IANA ifTypes（iprtdb.h / ipifcons.h），与 netinfo 里的 if_type 同源。
const IF_TYPE_ETHERNET: u32 = 6;
const IF_TYPE_IEEE80211: u32 = 71;
const IF_TYPE_WWANPP: u32 = 243;
const IF_TYPE_WWANPP2: u32 = 244;

/// 按 IF_TYPE 分类；IF_TYPE 不认识时回退到驱动描述关键字。
/// `description` 传 IP_ADAPTER_ADDRESSES_LH.Description（驱动描述，通常是英文），
/// 不要传本地化的 FriendlyName，否则关键字表失效。
pub fn classify(if_type: u32, description: &str) -> NicKind {
    match if_type {
        IF_TYPE_ETHERNET => NicKind::Wired,
        IF_TYPE_IEEE80211 | IF_TYPE_WWANPP | IF_TYPE_WWANPP2 => NicKind::Wireless,
        _ => keyword_fallback(description),
    }
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
    } else if ["ethernet", "gbe", "gb e", "pcie gigabit", "family controller"]
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
        NicKind::Other => "其他",
    }
}

/// 列表分组排序键：0 有线、1 无线、2 其他。
pub fn group_order(k: NicKind) -> u8 {
    match k {
        NicKind::Wired => 0,
        NicKind::Wireless => 1,
        NicKind::Other => 2,
    }
}

/// 虚拟/隧道网卡：这些不是用户要管理的物理网卡
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn ethernet_if_type_is_wired() {
        assert_eq!(classify(6, "Intel(R) Ethernet Connection I219-V"), NicKind::Wired);
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
        assert_eq!(classify(0, "Marvell AVASTAR Wireless-AC"), NicKind::Wireless);
        assert_eq!(classify(0, "Realtek PCIe GbE Family"), NicKind::Wired);
        assert_eq!(classify(0, "Microsoft Kernel Debug Network"), NicKind::Other);
    }

    #[test]
    fn wifi_keyword_variants_all_wireless() {
        for d in ["ASUS Wi-Fi", "USB WiFi Adapter", "Qualcomm WLAN", "802.11n USB LAN"] {
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
        assert_eq!(kind_name(NicKind::Other), "其他");
        assert!(group_order(NicKind::Wired) < group_order(NicKind::Wireless));
        assert!(group_order(NicKind::Wireless) < group_order(NicKind::Other));
    }

    // ---- 以下是对简报用例的补强：每个 IF_TYPE 分支、每个关键字都要有独立判别力 ----

    /// classify 的 IF_TYPE 数字分支 + 关键字回退合并成一张表逐条核对。
    /// 每行描述都刻意只命中一个关键字，保证「删掉某个关键字」必有对应用例变红。
    #[test]
    fn classify_covers_every_if_type_and_keyword() {
        let cases: [(&str, u32, &str, NicKind); 22] = [
            // IF_TYPE 精确分支：命中时描述完全不起作用
            ("if_type=6 有线", 6, "Something Unknown", NicKind::Wired),
            ("if_type=71 无线", 71, "Something Unknown", NicKind::Wireless),
            ("if_type=243 WWAN", 243, "Fibocom FM150", NicKind::Wireless),
            ("if_type=244 WWAN2", 244, "Fibocom FM150", NicKind::Wireless),
            ("if_type=6 压过无线描述", 6, "Intel(R) Wi-Fi 6E AX211", NicKind::Wired),
            ("if_type=71 压过有线描述", 71, "Realtek PCIe GbE Family", NicKind::Wireless),
            // 未知 IF_TYPE：逐个无线关键字
            ("0/wi-fi", 0, "ASUS Wi-Fi Card", NicKind::Wireless),
            ("0/wifi", 0, "USB WiFi Adapter", NicKind::Wireless),
            ("0/wireless", 0, "Marvell AVASTAR Wireless-AC", NicKind::Wireless),
            ("0/wlan", 0, "Qualcomm WLAN", NicKind::Wireless),
            ("0/802.11", 0, "802.11n USB LAN", NicKind::Wireless),
            // 未知 IF_TYPE：逐个有线关键字
            ("0/ethernet", 0, "Intel Ethernet Connection", NicKind::Wired),
            ("0/gbe", 0, "Realtek PCIe GbE Family", NicKind::Wired),
            ("0/gb e", 0, "Realtek Gb E Family Network Adapter", NicKind::Wired),
            ("0/pcie gigabit", 0, "Intel(R) PCIe Gigabit Network Connection", NicKind::Wired),
            ("0/family controller", 0, "Intel(R) I219-LM Family Controller", NicKind::Wired),
            // 未知 IF_TYPE 且两边关键字都不命中 → Other
            ("0/无关键字", 0, "Microsoft Kernel Debug Network", NicKind::Other),
            ("1/无关键字", 1, "Software Loopback Interface", NicKind::Other),
            ("24/无关键字", 24, "WAN Miniport (IP)", NicKind::Other),
            ("53/无关键字", 53, "Unknown Device", NicKind::Other),
            // Task 4 边界：Description 为空/空白时不得 panic，也不得凭空归类
            ("0/空描述", 0, "", NicKind::Other),
            ("29/空白描述", 29, " ", NicKind::Other),
        ];
        for (label, if_type, description, expected) in cases {
            assert_eq!(
                classify(if_type, description),
                expected,
                "用例 {label}：if_type={if_type}, 描述={description}"
            );
        }
    }

    /// 回退分支里无线判定先于有线判定（简报的实现顺序）：混合关键字时必须是无线。
    #[test]
    fn wireless_keyword_takes_precedence_over_ethernet_keyword() {
        assert_eq!(classify(0, "Realtek Wireless Ethernet Bridge"), NicKind::Wireless);
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

    /// group_order 的绝对值（简报 Interfaces 要求 0/1/2，Task 5 排序键直接用）。
    #[test]
    fn group_order_absolute_values() {
        assert_eq!(group_order(NicKind::Wired), 0);
        assert_eq!(group_order(NicKind::Wireless), 1);
        assert_eq!(group_order(NicKind::Other), 2);

        // 排序键契约：Task 5 用它把有线排在最前、其他垫底
        let mut kinds = [NicKind::Other, NicKind::Wireless, NicKind::Wired];
        kinds.sort_by_key(|k| group_order(*k));
        assert_eq!(kinds, [NicKind::Wired, NicKind::Wireless, NicKind::Other]);
    }

    /// 虚拟关键字表逐条命中（第一列是被钉住的关键字）。
    /// 已知两处子串冗余：`virtualbox` 必含 `virtual`、`tap-windows` 必含 `tap-`，
    /// 删掉前者仍会被后者兜住 —— 属表内安全冗余，不是测试漏洞。
    #[test]
    fn virtual_keywords_each_detectable() {
        let hits: [(&str, &str); 13] = [
            ("virtual", "Cisco AnyConnect Secure Mobility Virtual Adapter"),
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

    /// 中文描述的行为留档：关键字表是英文的，本地化中文名不参与判定。
    #[test]
    fn chinese_descriptions_follow_documented_behavior() {
        // IF_TYPE 命中时中文描述无影响
        assert_eq!(classify(6, "以太网 网络适配器"), NicKind::Wired);
        assert_eq!(classify(71, "无线局域网适配器"), NicKind::Wireless);
        // 未知 IF_TYPE + 纯中文描述 → 关键字都不命中 → Other / 非虚拟
        assert_eq!(classify(0, "无线局域网适配器"), NicKind::Other);
        assert_eq!(classify(0, "以太网 网络适配器"), NicKind::Other);
        assert!(!is_virtual("虚拟专用网络适配器"));
        // 中英混排时只要含 ASCII 关键字就仍算虚拟
        assert!(is_virtual("Hyper-V 虚拟以太网适配器"));
    }

    /// NicKind 的派生契约（简报 Interfaces 指定 Debug/PartialEq/Eq/Clone/Copy）：
    /// Task 5 排序要在闭包里按值复制 NicKind，丢了 Copy 会直接编译失败。
    #[test]
    fn nic_kind_derives_are_kept() {
        let a = NicKind::Wireless;
        let b = a; // Copy：按值移动后 a 仍可用
        assert_eq!(a, b); // PartialEq
        assert_eq!(Clone::clone(&a), b); // Clone（显式函数形式，避开 clone_on_copy）
        let all = [NicKind::Wired, NicKind::Wireless, NicKind::Other];
        // Debug：变体名可读（Task 7 详情/日志会直接打印）
        assert_eq!(format!("{all:?}"), "[Wired, Wireless, Other]");
        assert_ne!(NicKind::Wired, NicKind::Other);
        // Eq 语义：分组去重/比较按值而非按引用
        assert_eq!(all.iter().filter(|&&k| k == NicKind::Wired).count(), 1);
    }
}
