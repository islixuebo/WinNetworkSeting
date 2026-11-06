namespace NicTool;

public class NicInfo
{
    public required string Name { get; init; }
    public required string Description { get; init; }
    public bool IsEnabled { get; init; }
    public bool IsConnected { get; init; }
    public bool IsDefaultRoute { get; init; }
    public string Mac { get; init; } = "";
    public string Ipv4 { get; init; } = "";
    public string SubnetMask { get; init; } = "";
    public string Gateway { get; init; } = "";
    public string Dns { get; init; } = "";
    public string Ipv6 { get; init; } = "";
    public string Speed { get; init; } = "";
    public string WmiPath { get; init; } = "";
    public required string NicType { get; init; }
    public required int TypeOrder { get; init; }

    public string StatusText =>
        !IsEnabled ? "已禁用"
        : IsConnected ? "已启用 · 已连接"
        : "已启用 · 未连接";

    public bool HasConflictRisk => IsEnabled && IsDefaultRoute;

    public bool CanEnable => !IsEnabled;
    public bool CanDisable => IsEnabled;
}
