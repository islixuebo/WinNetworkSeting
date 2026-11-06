using System.Management;
using System.Net;
using System.Net.Sockets;

namespace NicTool;

public class NicService
{
    private const string ScopePath = @"\\.\root\cimv2";

    public List<NicInfo> GetPhysicalAdapters()
    {
        var scope = new ManagementScope(ScopePath);
        scope.Connect();

        var ipConfigs = GetIpConfigs(scope);
        var defaultRouteIfIndexes = GetDefaultRouteInterfaceIndexes(scope);
        var virtualFlags = GetVirtualFlags();
        var result = new List<NicInfo>();

        using var adapters = new ManagementObjectSearcher(
            scope, new ObjectQuery("SELECT * FROM Win32_NetworkAdapter WHERE PhysicalAdapter = TRUE"));

        foreach (ManagementObject adapter in adapters.Get())
        {
            var ifaceIndex = ToInt32(Prop(adapter, "InterfaceIndex"));
            ipConfigs.TryGetValue(ifaceIndex, out var ip);

            var status = ToInt32(Prop(adapter, "NetConnectionStatus"));
            var netEnabled = Prop(adapter, "NetEnabled") as bool?;
            var isEnabled = netEnabled ?? status is 2 or 7;
            var description = Prop(adapter, "Description") as string ?? "";
            var (nicType, typeOrder) = ClassNic(ifaceIndex, description, virtualFlags);

            result.Add(new NicInfo
            {
                Name = Prop(adapter, "Name") as string ?? "(未知)",
                Description = description,
                IsEnabled = isEnabled,
                IsConnected = status == 2,
                IsDefaultRoute = ifaceIndex >= 0 && defaultRouteIfIndexes.Contains(ifaceIndex),
                Mac = Prop(adapter, "MACAddress") as string ?? "",
                Ipv4 = ip?.Ipv4 ?? "",
                SubnetMask = ip?.Mask ?? "",
                Gateway = ip?.Gateway ?? "",
                Dns = ip?.Dns ?? "",
                Ipv6 = ip?.Ipv6 ?? "",
                Speed = FormatSpeed(Prop(adapter, "Speed")),
                WmiPath = adapter.Path?.Path ?? "",
                NicType = nicType,
                TypeOrder = typeOrder,
            });
        }

        return result.OrderBy(n => n.Name, StringComparer.CurrentCultureIgnoreCase).ToList();
    }

    public void SetEnabled(NicInfo nic, bool enable)
    {
        var scope = new ManagementScope(ScopePath);
        scope.Connect();
        using var obj = new ManagementObject(scope, new ManagementPath(nic.WmiPath), null);
        var output = obj.InvokeMethod(enable ? "Enable" : "Disable", new object?[0]);
        var code = ToInt32(output);
        if (code != 0)
            throw new InvalidOperationException($"WMI 操作返回错误码 {code}");
    }

    private record IpEntry(string Ipv4, string Ipv6, string Mask, string Gateway, string Dns);

    private static Dictionary<int, IpEntry> GetIpConfigs(ManagementScope scope)
    {
        var map = new Dictionary<int, IpEntry>();
        using var configs = new ManagementObjectSearcher(
            scope, new ObjectQuery("SELECT * FROM Win32_NetworkAdapterConfiguration"));

        foreach (ManagementObject cfg in configs.Get())
        {
            var ifaceIndex = ToInt32(Prop(cfg, "InterfaceIndex"));
            if (ifaceIndex < 0) continue;
            var ips = Prop(cfg, "IPAddress") as string[] ?? [];
            var ipv4Index = Array.FindIndex(ips, s => IPAddress.TryParse(s, out var a) && a.AddressFamily == AddressFamily.InterNetwork);
            if (ipv4Index < 0) continue;

            var masks = Prop(cfg, "SubnetMask") as string[];
            var v6 = ips
                .Where(s => IPAddress.TryParse(s, out var a) && a.AddressFamily == AddressFamily.InterNetworkV6)
                .Take(2);
            map[ifaceIndex] = new IpEntry(
                ips[ipv4Index],
                string.Join(", ", v6),
                masks is not null && ipv4Index < masks.Length ? masks[ipv4Index] : "",
                First(Prop(cfg, "DefaultIPGateway") as string[]),
                DnsList(Prop(cfg, "DNSServerSearchOrder") as string[]));
        }

        return map;
    }

    private static HashSet<int> GetDefaultRouteInterfaceIndexes(ManagementScope scope)
    {
        var indexes = new HashSet<int>();
        try
        {
            using var routes = new ManagementObjectSearcher(
                scope, new ObjectQuery("SELECT InterfaceIndex FROM Win32_IP4RouteTable WHERE Destination = '0.0.0.0'"));
            foreach (ManagementObject route in routes.Get())
            {
                var i = ToInt32(Prop(route, "InterfaceIndex"));
                if (i >= 0) indexes.Add(i);
            }
        }
        catch (ManagementException) { }
        return indexes;
    }

    // 某些 WMI 实例缺少可选属性，直接索引器读取会抛 ManagementException（Not found）
    private static object? Prop(ManagementBaseObject obj, string name)
    {
        try { return obj[name]; }
        catch (ManagementException) { return null; }
    }

    private static (string Type, int Order) ClassNic(
        int ifaceIndex, string description, Dictionary<int, bool> virtualFlags)
    {
        if (virtualFlags.TryGetValue(ifaceIndex, out var isVirtual) && isVirtual)
            return ("其他", 2);
        if (description.Contains("Wi-Fi", StringComparison.OrdinalIgnoreCase)
            || description.Contains("WiFi", StringComparison.OrdinalIgnoreCase)
            || description.Contains("Wireless", StringComparison.OrdinalIgnoreCase)
            || description.Contains("WLAN", StringComparison.OrdinalIgnoreCase)
            || description.Contains("802.11", StringComparison.OrdinalIgnoreCase))
            return ("无线网卡", 1);
        return ("有线网卡", 0);
    }

    private static Dictionary<int, bool> GetVirtualFlags()
    {
        var map = new Dictionary<int, bool>();
        try
        {
            var scope = new ManagementScope(@"\\.\root\StandardCimv2");
            scope.Connect();
            using var adapters = new ManagementObjectSearcher(
                scope,
                new ObjectQuery("SELECT InterfaceIndex, Virtual FROM MSFT_NetAdapter"));
            foreach (ManagementObject o in adapters.Get())
            {
                var i = ToInt32(Prop(o, "InterfaceIndex"));
                if (i >= 0) map[i] = Prop(o, "Virtual") as bool? ?? false;
            }
        }
        catch (ManagementException) { }
        return map;
    }

    private static string First(string[]? arr) => arr is { Length: > 0 } ? arr[0] : "";

    private static string DnsList(string[]? arr) =>
        arr is { Length: > 0 } ? string.Join(", ", arr.Take(2)) : "";

    private static int ToInt32(object? value) =>
        value is null ? -1 : Convert.ToInt32(value);

    private static string FormatSpeed(object? speed)
    {
        if (speed is null) return "";
        var bytesPerSec = Convert.ToUInt64(speed);
        if (bytesPerSec == 0) return "";
        var mbps = bytesPerSec * 8.0 / 1_000_000;
        return mbps >= 1000 ? $"{mbps / 1000:G4} Gbps" : $"{mbps:G4} Mbps";
    }
}
