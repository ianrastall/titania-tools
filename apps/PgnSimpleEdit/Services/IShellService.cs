namespace PgnSimpleEdit.Services;

/// <summary>Things the view models need from the windowing shell: pickers, dialogs, Explorer.</summary>
public interface IShellService
{
    Task<IReadOnlyList<string>> PickPgnFilesAsync();
    Task<string?> PickSaveFileAsync(string suggestedName);
    Task<string?> PickFolderAsync();
    Task ShowMessageAsync(string title, string message);
    Task<bool> ConfirmAsync(string title, string message, string confirmText);
    /// <summary>Opens Explorer on a folder, or on the folder of a file with the file selected.</summary>
    void ShowInExplorer(string path);
    void Exit();
}
