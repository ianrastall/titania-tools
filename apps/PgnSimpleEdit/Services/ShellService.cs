using System.Diagnostics;
using Microsoft.UI.Xaml;
using Microsoft.UI.Xaml.Controls;
using Microsoft.Windows.Storage.Pickers;

namespace PgnSimpleEdit.Services;

public sealed class ShellService(Window window) : IShellService
{
    public async Task<IReadOnlyList<string>> PickPgnFilesAsync()
    {
        var picker = new FileOpenPicker(window.AppWindow.Id);
        picker.FileTypeFilter.Add(".pgn");
        picker.FileTypeFilter.Add("*");
        var picked = await picker.PickMultipleFilesAsync();
        return [.. picked.Select(file => file.Path)];
    }

    public async Task<string?> PickSaveFileAsync(string suggestedName)
    {
        var picker = new FileSavePicker(window.AppWindow.Id)
        {
            SuggestedFileName = suggestedName,
            DefaultFileExtension = ".pgn",
        };
        picker.FileTypeChoices.Add("PGN chess games", [".pgn"]);
        return (await picker.PickSaveFileAsync())?.Path;
    }

    public async Task<string?> PickFolderAsync()
    {
        var picker = new FolderPicker(window.AppWindow.Id);
        return (await picker.PickSingleFolderAsync())?.Path;
    }

    public async Task ShowMessageAsync(string title, string message)
    {
        if (CreateDialog(title, message) is not { } dialog) return;
        dialog.CloseButtonText = "OK";
        await dialog.ShowAsync();
    }

    public async Task<bool> ConfirmAsync(string title, string message, string confirmText)
    {
        if (CreateDialog(title, message) is not { } dialog) return false;
        dialog.PrimaryButtonText = confirmText;
        dialog.CloseButtonText = "Cancel";
        dialog.DefaultButton = ContentDialogButton.Close;
        return await dialog.ShowAsync() == ContentDialogResult.Primary;
    }

    private ContentDialog? CreateDialog(string title, string message)
    {
        if (window.Content?.XamlRoot is not { } root) return null;
        return new ContentDialog
        {
            Title = title,
            Content = new TextBlock { Text = message, TextWrapping = TextWrapping.Wrap, IsTextSelectionEnabled = true },
            XamlRoot = root,
            RequestedTheme = ElementTheme.Dark,
        };
    }

    public void ShowInExplorer(string path)
    {
        string arguments = Directory.Exists(path) ? $"\"{path}\"" : $"/select,\"{path}\"";
        Process.Start(new ProcessStartInfo("explorer.exe", arguments) { UseShellExecute = false })?.Dispose();
    }

    public void Exit()
    {
        // The main window first stops whatever is still writing files.
        if (window is MainWindow main) main.CloseWhenIdle();
        else window.Close();
    }
}
