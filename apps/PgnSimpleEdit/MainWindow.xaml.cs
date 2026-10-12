using System.Collections.Specialized;
using System.Runtime.InteropServices;
using Microsoft.UI;
using Microsoft.UI.Windowing;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using PgnSimpleEdit.Services;
using PgnSimpleEdit.ViewModels;
using Windows.ApplicationModel.DataTransfer;
using Windows.Graphics;

namespace PgnSimpleEdit;

public sealed partial class MainWindow : Window
{
    private bool _closing;
    private bool _idle;

    public MainViewModel ViewModel { get; }

    public MainWindow()
    {
        ViewModel = new MainViewModel(new ShellService(this));
        InitializeComponent();

        Title = "PGN Simple Edit";
        ExtendsContentIntoTitleBar = true;
        SetTitleBar(AppTitleBar);
        AppWindow.TitleBar.ButtonForegroundColor = Colors.White;
        string icon = Path.Combine(AppContext.BaseDirectory, "Assets", "AppIcon.ico");
        if (File.Exists(icon)) AppWindow.SetIcon(icon);
        SizeToDisplay();

        ViewModel.PropertyChanged += (_, e) =>
        {
            if (e.PropertyName == nameof(MainViewModel.Tool)) SyncToolButtons();
        };
        ViewModel.Log.CollectionChanged += OnLogChanged;
        SyncToolButtons();
        // Files given on the command line, as when PGN files are opened with the app from Explorer.
        Root.Loaded += (_, _) => ViewModel.AddFiles(Environment.GetCommandLineArgs().Skip(1));
        AppWindow.Closing += (_, e) =>
        {
            if (_idle) return;
            e.Cancel = true;
            CloseWhenIdle();
        };
        Closed += (_, _) => ViewModel.Dispose();
    }

    /// <summary>Closes the window once running work has been cancelled and has cleaned up after itself,
    /// so that no half-written files are left behind.</summary>
    public async void CloseWhenIdle()
    {
        if (_closing) return;
        _closing = true;
        AppWindow.Hide();
        try
        {
            await ViewModel.ShutdownAsync();
        }
        finally
        {
            _idle = true;
            Close();
        }
    }

    [LibraryImport("user32.dll")]
    private static partial uint GetDpiForWindow(nint hwnd);

    // AppWindow works in physical pixels; size the window in DIPs and keep it inside the work area.
    private void SizeToDisplay()
    {
        double scale = GetDpiForWindow(WinRT.Interop.WindowNative.GetWindowHandle(this)) / 96.0;
        RectInt32 work = DisplayArea.GetFromWindowId(AppWindow.Id, DisplayAreaFallback.Nearest).WorkArea;
        int width = Math.Min((int)(1200 * scale), work.Width - 40);
        int height = Math.Min((int)(860 * scale), work.Height - 40);
        AppWindow.MoveAndResize(new RectInt32(work.X + (work.Width - width) / 2, work.Y + (work.Height - height) / 2, width, height));

        if (AppWindow.Presenter is OverlappedPresenter presenter)
        {
            presenter.PreferredMinimumWidth = Math.Min((int)(900 * scale), work.Width);
            presenter.PreferredMinimumHeight = Math.Min((int)(620 * scale), work.Height);
        }
    }

    // The toolbar's tool buttons work as a group of which exactly one is checked.
    private void OnToolClick(object sender, RoutedEventArgs e)
    {
        ViewModel.Tool = Enum.Parse<Tool>((string)((FrameworkElement)sender).Tag);
        // Clicking the checked button unchecks it without changing the tool, so always put the group right.
        SyncToolButtons();
    }

    private void SyncToolButtons()
    {
        InfoButton.IsChecked = ViewModel.Tool == Tool.Info;
        JoinButton.IsChecked = ViewModel.Tool == Tool.Join;
        MergeButton.IsChecked = ViewModel.Tool == Tool.Merge;
        SplitButton.IsChecked = ViewModel.Tool == Tool.Split;
        SortButton.IsChecked = ViewModel.Tool == Tool.Sort;
    }

    // Keep the newest log line in view.
    private void OnLogChanged(object? sender, NotifyCollectionChangedEventArgs e)
    {
        if (e.Action == NotifyCollectionChangedAction.Add && ViewModel.Log.Count > 0)
            LogList.ScrollIntoView(ViewModel.Log[^1]);
    }

    private void OnDragOver(object sender, DragEventArgs e)
    {
        if (!e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        e.AcceptedOperation = DataPackageOperation.Copy;
        e.DragUIOverride.Caption = "Add to the list";
    }

    private async void OnDrop(object sender, DragEventArgs e)
    {
        if (!e.DataView.Contains(StandardDataFormats.StorageItems)) return;
        DragOperationDeferral deferral = e.GetDeferral();
        try
        {
            var items = await e.DataView.GetStorageItemsAsync();
            ViewModel.AddFiles(items.Select(item => item.Path).Where(path => !string.IsNullOrEmpty(path)));
        }
        catch (Exception ex)
        {
            // An async void handler has nobody to pass an exception to; unhandled, it ends the app.
            ViewModel.ReportProblem("The dropped files could not be added", ex.Message);
        }
        finally
        {
            deferral.Complete();
        }
    }
}
