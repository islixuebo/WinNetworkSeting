using System.ComponentModel;
using System.Windows;
using System.Windows.Controls;
using System.Windows.Data;
using System.Windows.Input;

namespace NicTool;

public partial class MainWindow : Wpf.Ui.Controls.FluentWindow
{
    private readonly NicService _service = new();

    public MainWindow()
    {
        InitializeComponent();
    }

    private void OnLoaded(object sender, RoutedEventArgs e) => Refresh();

    private void OnRefresh(object sender, RoutedEventArgs e) => Refresh();

    private void Refresh()
    {
        try
        {
            var items = _service.GetPhysicalAdapters();
            var view = new ListCollectionView(items);
            view.GroupDescriptions.Add(new PropertyGroupDescription(nameof(NicInfo.NicType)));
            view.SortDescriptions.Add(new SortDescription(nameof(NicInfo.TypeOrder), ListSortDirection.Ascending));
            view.SortDescriptions.Add(new SortDescription(nameof(NicInfo.Name), ListSortDirection.Ascending));
            NicList.ItemsSource = view;
            StatusText.Text = $"共 {items.Count} 个物理网卡";
        }
        catch (Exception ex)
        {
            MessageBox.Show(this, $"读取网卡信息失败：{ex.Message}", "错误",
                MessageBoxButton.OK, MessageBoxImage.Error);
        }
    }

    private void OnListRightClick(object sender, MouseButtonEventArgs e)
    {
        if (ItemsControl.ContainerFromElement(NicList, e.OriginalSource as DependencyObject)
            is ListViewItem { DataContext: NicInfo nic })
            NicList.SelectedItem = nic;
    }

    private void OnSelectionChanged(object sender, SelectionChangedEventArgs e)
    {
        var nic = NicList.SelectedItem as NicInfo;
        EnableButton.IsEnabled = nic is { IsEnabled: false };
        DisableButton.IsEnabled = nic is { IsEnabled: true };
    }

    private void OnDetails(object sender, RoutedEventArgs e)
    {
        if (NicList.SelectedItem is NicInfo nic)
            new NicDetailWindow(nic) { Owner = this }.Show();
    }

    private async void OnEnable(object sender, RoutedEventArgs e)
    {
        if (NicList.SelectedItem is NicInfo nic)
            await SetState(nic, enable: true);
    }

    private async void OnDisable(object sender, RoutedEventArgs e)
    {
        if (NicList.SelectedItem is not NicInfo nic) return;

        if (nic.HasConflictRisk)
        {
            var answer = MessageBox.Show(this,
                $"「{nic.Name}」是当前默认路由所在的网卡，禁用会立即断网。\n\n确定要禁用吗？",
                "警告", MessageBoxButton.YesNo, MessageBoxImage.Warning);
            if (answer != MessageBoxResult.Yes) return;
        }

        await SetState(nic, enable: false);
    }

    private async Task SetState(NicInfo nic, bool enable)
    {
        EnableButton.IsEnabled = DisableButton.IsEnabled = RefreshButton.IsEnabled = false;
        StatusText.Text = $"正在{(enable ? "启用" : "禁用")}「{nic.Name}」…";
        try
        {
            await Task.Run(() => _service.SetEnabled(nic, enable));
            StatusText.Text = $"已{(enable ? "启用" : "禁用")}「{nic.Name}」";
        }
        catch (Exception ex)
        {
            MessageBox.Show(this,
                $"{(enable ? "启用" : "禁用")}「{nic.Name}」失败：{ex.Message}",
                "错误", MessageBoxButton.OK, MessageBoxImage.Error);
            StatusText.Text = "操作失败";
        }
        finally
        {
            RefreshButton.IsEnabled = true;
            Refresh();
        }
    }
}
