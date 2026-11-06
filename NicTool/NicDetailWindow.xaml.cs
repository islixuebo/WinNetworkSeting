using System.Windows;

namespace NicTool;

public partial class NicDetailWindow : Wpf.Ui.Controls.FluentWindow
{
    public NicDetailWindow(NicInfo nic)
    {
        InitializeComponent();
        TitleText.Text = nic.Name;
        Rows.ItemsSource = new[]
        {
            Row("类型", nic.NicType),
            Row("状态", nic.StatusText),
            Row("当前活跃连接", nic.IsDefaultRoute ? "是" : "否"),
            Row("描述", nic.Description),
            Row("IPv4", nic.Ipv4),
            Row("IPv6", nic.Ipv6),
            Row("子网掩码", nic.SubnetMask),
            Row("网关", nic.Gateway),
            Row("DNS", nic.Dns),
            Row("MAC", nic.Mac),
            Row("速率", nic.Speed),
        };
    }

    private static KeyValuePair<string, string> Row(string key, string value) =>
        new(key, string.IsNullOrWhiteSpace(value) ? "—" : value);
}
