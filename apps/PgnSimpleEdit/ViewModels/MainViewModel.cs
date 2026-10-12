using System.Collections.ObjectModel;
using System.Diagnostics.CodeAnalysis;
using System.Text.Json.Serialization.Metadata;
using CommunityToolkit.Mvvm.ComponentModel;
using CommunityToolkit.Mvvm.Input;
using Microsoft.UI.Dispatching;
using Microsoft.UI.Xaml.Controls;
using PgnSimpleEdit.Interop;
using PgnSimpleEdit.Models;
using PgnSimpleEdit.Services;

namespace PgnSimpleEdit.ViewModels;

public sealed partial class MainViewModel : ObservableObject, IDisposable
{
    private const int MaxLogEntries = 2000;

    private static readonly string[] FieldNames =
        ["Date", "Event", "Site", "Round", "White player", "Black player", "Result", "ECO code", "White rating", "Black rating", "Average rating", "Number of moves"];
    private static readonly string[] FieldIds =
        ["date", "event", "site", "round", "white", "black", "result", "eco", "white_elo", "black_elo", "average_elo", "moves"];
    private static readonly string[] SplitModeIds = ["games", "parts", "year", "event", "eco", "result", "white", "black"];

    private readonly IShellService _shell;
    private readonly DispatcherQueue _dispatcher = DispatcherQueue.GetForCurrentThread();
    private CancellationTokenSource? _runCancel;
    private string _phase = "";
    private string? _lastOutput;
    private bool _readingFiles;
    private bool _disposed;
    private readonly bool _coreReady;
    // Requests the native core is working on; closing the app waits for them.
    private readonly HashSet<Task> _coreTasks = [];
    // Files in the list that the running tool is about to replace; they are not read meanwhile.
    private readonly HashSet<FileItem> _held = [];

    public MainViewModel(IShellService shell)
    {
        _shell = shell;
        StatusText = "Ready";
        ListSummary = "No files";
        ProgressText = "";
        InfoMessage = "";
        ResultTitle = "";
        ResultMessage = "";
        ShowLog = true;
        MergeRemoveDuplicates = true;
        GamesPerFile = 1000;
        NumberOfFiles = 10;

        try
        {
            CoreVersion = CoreClient.GetVersion();
            _coreReady = true;
        }
        catch (Exception ex) when (ex is DllNotFoundException or EntryPointNotFoundException or CoreException)
        {
            CoreVersion = "missing";
            AddLog($"The native core could not be loaded: {ex.Message}");
        }

        Files.CollectionChanged += (_, _) => OnFilesChanged();
        UpdateToolText();
        RefreshInfo();
    }

    public ObservableCollection<FileItem> Files { get; } = [];
    public ObservableCollection<LogEntry> Log { get; } = [];
    public ObservableCollection<Tile> Tiles { get; } = [];
    public ObservableCollection<ReportSection> LeftSections { get; } = [];
    public ObservableCollection<ReportSection> RightSections { get; } = [];

    public IReadOnlyList<string> SortFields { get; } = FieldNames;
    public IReadOnlyList<string> OptionalSortFields { get; } = ["(nothing)", .. FieldNames];
    public IReadOnlyList<string> MergeOrders { get; } = ["The order of the files", .. FieldNames];
    public IReadOnlyList<string> SplitModes { get; } =
    [
        "A set number of games in each file",
        "A set number of files",
        "One file for each year",
        "One file for each event",
        "One file for each ECO code",
        "One file for each result",
        "One file for each White player",
        "One file for each Black player",
    ];

    public string CoreVersion { get; }

    // ---- State -------------------------------------------------------------------------------

    [ObservableProperty]
    public partial Tool Tool { get; set; }

    [ObservableProperty]
    public partial FileItem? SelectedFile { get; set; }

    [ObservableProperty]
    public partial bool IsBusy { get; set; }

    [ObservableProperty]
    public partial string StatusText { get; set; }

    [ObservableProperty]
    public partial string ListSummary { get; set; }

    [ObservableProperty]
    public partial bool ShowLog { get; set; }

    [ObservableProperty]
    public partial bool IsProgressVisible { get; set; }

    [ObservableProperty]
    public partial bool IsProgressIndeterminate { get; set; }

    [ObservableProperty]
    public partial double ProgressValue { get; set; }

    [ObservableProperty]
    public partial string ProgressText { get; set; }

    public bool IsInfo => Tool == Tool.Info;
    public bool IsJoin => Tool == Tool.Join;
    public bool IsMerge => Tool == Tool.Merge;
    public bool IsSplit => Tool == Tool.Split;
    public bool IsSort => Tool == Tool.Sort;
    public bool IsAction => Tool != Tool.Info;
    public bool HasNoFiles => Files.Count == 0;

    [ObservableProperty]
    public partial string ToolTitle { get; set; }

    [ObservableProperty]
    public partial string ToolDescription { get; set; }

    [ObservableProperty]
    public partial string InputHint { get; set; }

    [ObservableProperty]
    public partial string RunLabel { get; set; }

    // ---- Info view ---------------------------------------------------------------------------

    [ObservableProperty]
    public partial bool IsInfoReady { get; set; }

    [ObservableProperty]
    public partial bool IsInfoReading { get; set; }

    /// <summary>Shown in place of the report: nothing selected, still reading, or the error.</summary>
    [ObservableProperty]
    public partial string InfoMessage { get; set; }

    // ---- Result of the last run --------------------------------------------------------------

    [ObservableProperty]
    public partial bool IsResultOpen { get; set; }

    [ObservableProperty]
    public partial string ResultTitle { get; set; }

    [ObservableProperty]
    public partial string ResultMessage { get; set; }

    [ObservableProperty]
    public partial InfoBarSeverity ResultSeverity { get; set; }

    [ObservableProperty]
    public partial bool CanShowOutput { get; set; }

    [ObservableProperty]
    public partial bool CanAddOutput { get; set; }

    // ---- Options -----------------------------------------------------------------------------

    [ObservableProperty]
    public partial bool MergeRemoveDuplicates { get; set; }

    [ObservableProperty]
    public partial int MergeOrderIndex { get; set; }

    [ObservableProperty]
    public partial bool MergeDescending { get; set; }

    [ObservableProperty]
    public partial int SplitModeIndex { get; set; }

    [ObservableProperty]
    public partial double GamesPerFile { get; set; }

    [ObservableProperty]
    public partial double NumberOfFiles { get; set; }

    public bool SplitNeedsGames => SplitModeIndex == 0;
    public bool SplitNeedsFiles => SplitModeIndex == 1;
    public bool MergeIsSorted => MergeOrderIndex > 0;

    [ObservableProperty]
    public partial int SortField1 { get; set; }

    [ObservableProperty]
    public partial bool SortDescending1 { get; set; }

    [ObservableProperty]
    public partial int SortField2 { get; set; }

    [ObservableProperty]
    public partial bool SortDescending2 { get; set; }

    [ObservableProperty]
    public partial int SortField3 { get; set; }

    [ObservableProperty]
    public partial bool SortDescending3 { get; set; }

    partial void OnToolChanged(Tool value)
    {
        OnPropertyChanged(nameof(IsInfo));
        OnPropertyChanged(nameof(IsJoin));
        OnPropertyChanged(nameof(IsMerge));
        OnPropertyChanged(nameof(IsSplit));
        OnPropertyChanged(nameof(IsSort));
        OnPropertyChanged(nameof(IsAction));
        UpdateToolText();
        RunCommand.NotifyCanExecuteChanged();
    }

    partial void OnSelectedFileChanged(FileItem? value)
    {
        UpdateToolText();
        RefreshInfo();
        RunCommand.NotifyCanExecuteChanged();
        RemoveSelectedCommand.NotifyCanExecuteChanged();
        MoveUpCommand.NotifyCanExecuteChanged();
        MoveDownCommand.NotifyCanExecuteChanged();
    }

    partial void OnIsBusyChanged(bool value)
    {
        RunCommand.NotifyCanExecuteChanged();
        CancelCommand.NotifyCanExecuteChanged();
    }

    partial void OnSplitModeIndexChanged(int value)
    {
        OnPropertyChanged(nameof(SplitNeedsGames));
        OnPropertyChanged(nameof(SplitNeedsFiles));
    }

    partial void OnMergeOrderIndexChanged(int value) => OnPropertyChanged(nameof(MergeIsSorted));

    private void OnFilesChanged()
    {
        OnPropertyChanged(nameof(HasNoFiles));
        UpdateToolText();
        UpdateListSummary();
        RunCommand.NotifyCanExecuteChanged();
        ClearFilesCommand.NotifyCanExecuteChanged();
        MoveUpCommand.NotifyCanExecuteChanged();
        MoveDownCommand.NotifyCanExecuteChanged();
    }

    [MemberNotNull(nameof(ToolTitle), nameof(ToolDescription), nameof(InputHint), nameof(RunLabel))]
    private void UpdateToolText()
    {
        string all = Files.Count switch
        {
            0 => "Add files to the list first.",
            1 => "Uses the one file in the list.",
            _ => $"Uses all {Files.Count} files in the list, in the order shown.",
        };
        string selected = SelectedFile is { } file ? $"Uses the selected file: {file.Name}" : "Select a file in the list first.";

        (ToolTitle, ToolDescription, InputHint, RunLabel) = Tool switch
        {
            Tool.Join => ("Join", "Puts the files end to end as one new file. Nothing inside them is changed.",
                Files.Count == 1 ? "Add at least one more file to the list." : all, "Join and save as…"),
            Tool.Merge => ("Merge", "Combines the files into one new file, leaving out games that appear more than once.", all, "Merge and save as…"),
            Tool.Split => ("Split", "Breaks one file up into several smaller files in a folder you choose.", selected, "Split into a folder…"),
            Tool.Sort => ("Sort", "Puts the games of one file in order and saves them as a new file.", selected, "Sort and save as…"),
            _ => ("Info", "What is inside the selected file.", "", ""),
        };
    }

    private void UpdateListSummary()
    {
        if (Files.Count == 0)
        {
            ListSummary = "No files";
            return;
        }
        long games = Files.Sum(file => file.Report?.Games ?? 0);
        bool complete = Files.All(file => file.Report is not null);
        ListSummary = $"{Format.Files(Files.Count)} · {Format.Size(Files.Sum(file => file.Size))} · {(complete ? "" : "at least ")}{Format.Games(games)}";
    }

    // ---- File list ---------------------------------------------------------------------------

    [RelayCommand]
    private async Task AddFilesAsync()
    {
        try
        {
            AddFiles(await _shell.PickPgnFilesAsync());
        }
        catch (Exception ex)
        {
            ReportProblem("The files could not be added", ex.Message);
        }
    }

    /// <summary>Shows a problem that is not the result of running a tool.</summary>
    public void ReportProblem(string title, string message) => ShowResult(InfoBarSeverity.Error, title, message);

    /// <summary>Adds files to the list; a folder adds the PGN files directly inside it.</summary>
    public void AddFiles(IEnumerable<string> paths)
    {
        FileItem? first = null;
        foreach (string path in paths.SelectMany(Expand))
        {
            if (Files.Any(file => string.Equals(file.Path, path, StringComparison.OrdinalIgnoreCase))) continue;
            try
            {
                var item = new FileItem(path);
                Files.Add(item);
                first ??= item;
            }
            catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
            {
                AddLog($"Could not add {Path.GetFileName(path)}: {ex.Message}");
            }
        }
        // Selecting a new file shows its report as soon as it has been read.
        if (first is not null) SelectedFile = first;
        ReadWaitingFiles();
    }

    private static IEnumerable<string> Expand(string path)
    {
        if (File.Exists(path)) return [path];
        if (!Directory.Exists(path)) return [];
        try
        {
            return Directory.EnumerateFiles(path, "*.pgn").Order(StringComparer.OrdinalIgnoreCase).ToList();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return [];
        }
    }

    [RelayCommand(CanExecute = nameof(HasSelection))]
    private void RemoveSelected()
    {
        if (SelectedFile is not { } item) return;
        int index = Files.IndexOf(item);
        item.Reading?.Cancel();
        Files.Remove(item);
        SelectedFile = Files.Count == 0 ? null : Files[Math.Min(index, Files.Count - 1)];
    }

    [RelayCommand(CanExecute = nameof(HasFiles))]
    private void ClearFiles()
    {
        foreach (FileItem item in Files) item.Reading?.Cancel();
        Files.Clear();
        SelectedFile = null;
    }

    [RelayCommand(CanExecute = nameof(CanMoveUp))]
    private void MoveUp() => MoveSelected(-1);

    [RelayCommand(CanExecute = nameof(CanMoveDown))]
    private void MoveDown() => MoveSelected(1);

    private void MoveSelected(int by)
    {
        if (SelectedFile is not { } item) return;
        int index = Files.IndexOf(item);
        Files.Move(index, index + by);
        // Moving an item can drop the list's selection; put it back.
        SelectedFile = item;
        MoveUpCommand.NotifyCanExecuteChanged();
        MoveDownCommand.NotifyCanExecuteChanged();
    }

    private bool HasSelection => SelectedFile is not null;
    private bool HasFiles => Files.Count > 0;
    private bool CanMoveUp => SelectedFile is { } item && Files.IndexOf(item) > 0;
    private bool CanMoveDown => SelectedFile is { } item && Files.IndexOf(item) < Files.Count - 1;

    // ---- Reading files for the Info view -----------------------------------------------------

    /// <summary>Reads the files that have not been read yet, one at a time, the selected one first.</summary>
    private async void ReadWaitingFiles()
    {
        if (_readingFiles) return;
        _readingFiles = true;
        try
        {
            while (!_disposed && NextWaiting() is { } item)
            {
                item.ReadTask = ReadAsync(item);
                await item.ReadTask;
                item.ReadTask = null;
                UpdateListSummary();
                if (item == SelectedFile) RefreshInfo();
            }
        }
        finally
        {
            _readingFiles = false;
            if (!IsBusy) HideProgress();
        }
    }

    private FileItem? NextWaiting() =>
        SelectedFile is { State: FileState.Waiting } selected && !_held.Contains(selected)
            ? selected
            : Files.FirstOrDefault(file => file.State == FileState.Waiting && !_held.Contains(file));

    /// <summary>Has a file read again because it has changed on disk.</summary>
    private void Invalidate(FileItem item)
    {
        item.Reading?.Cancel();
        try
        {
            item.Refresh();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            item.State = FileState.Failed;
            item.Error = ex.Message;
            item.Detail = "could not be read";
        }
        if (item == SelectedFile) RefreshInfo();
        UpdateListSummary();
    }

    /// <summary>Runs a request, keeping track of it so that closing the app can wait for it to end.</summary>
    private async Task<TResult> RunCoreAsync<TResult>(CoreRequest request, JsonTypeInfo<TResult> resultType, ICoreListener listener, CancellationToken cancellation)
    {
        Task<TResult> task = CoreClient.RunAsync(request, resultType, listener, cancellation);
        _coreTasks.Add(task);
        try
        {
            return await task;
        }
        finally
        {
            _coreTasks.Remove(task);
        }
    }

    private async Task ReadAsync(FileItem item)
    {
        using var cancel = new CancellationTokenSource();
        int generation = item.Generation;
        item.Reading = cancel;
        item.State = FileState.Reading;
        item.Detail = $"{Format.Size(item.Size)} · reading";
        if (item == SelectedFile) RefreshInfo();

        var listener = new UiListener(_dispatcher,
            (fraction, _) =>
            {
                if (item.State != FileState.Reading) return;
                item.PercentRead = (int)(Math.Max(fraction, 0) * 100);
                item.Detail = $"{Format.Size(item.Size)} · reading {item.PercentRead}%";
                if (item == SelectedFile) InfoMessage = $"Reading {item.Name}… {item.PercentRead}%";
                if (!IsBusy) ShowProgress($"Reading {item.Name}", fraction);
            },
            _ => { },
            AddLog);
        try
        {
            ReportResult report = await RunCoreAsync(new CoreRequest { Op = "report", Input = item.Path }, CoreJson.Default.ReportResult, listener, cancel.Token);
            // The file changed while it was being read: what was read is out of date.
            if (item.Generation != generation) return;
            item.Report = report;
            item.State = FileState.Ready;
            item.Detail = $"{Format.Size(item.Size)} · {Format.Games(report.Games)}";
        }
        catch (Exception) when (item.Generation != generation)
        {
            // The file changed while it was being read, and is waiting to be read again.
        }
        catch (OperationCanceledException) when (_held.Contains(item))
        {
            // A tool is about to replace the file; it is read again afterwards.
            item.State = FileState.Waiting;
            item.Detail = $"{Format.Size(item.Size)} · waiting";
        }
        catch (OperationCanceledException)
        {
            // The file was removed from the list while it was being read.
            item.State = FileState.Failed;
        }
        catch (Exception ex)
        {
            item.State = FileState.Failed;
            item.Error = ex.Message;
            item.Detail = $"{Format.Size(item.Size)} · could not be read";
            AddLog(ex.Message);
        }
        finally
        {
            item.Reading = null;
        }
    }

    private void RefreshInfo()
    {
        Tiles.Clear();
        LeftSections.Clear();
        RightSections.Clear();
        IsInfoReady = false;
        IsInfoReading = false;

        switch (SelectedFile)
        {
            case null:
                InfoMessage = Files.Count == 0
                    ? "Add a PGN file to see what is inside it. You can also drop files onto this window."
                    : "Select a file in the list to see what is inside it.";
                break;
            case { Report: { } report }:
                foreach (Tile tile in ReportBuilder.Tiles(report)) Tiles.Add(tile);
                foreach (ReportSection section in ReportBuilder.LeftSections(report)) LeftSections.Add(section);
                foreach (ReportSection section in ReportBuilder.RightSections(report)) RightSections.Add(section);
                InfoMessage = "";
                IsInfoReady = true;
                break;
            case { State: FileState.Failed } failed:
                InfoMessage = failed.Error ?? "This file could not be read.";
                break;
            case var item:
                InfoMessage = item.State == FileState.Reading ? $"Reading {item.Name}… {item.PercentRead}%" : $"Waiting to read {item.Name}…";
                IsInfoReading = true;
                break;
        }
    }

    // ---- Running a tool ----------------------------------------------------------------------

    [RelayCommand]
    private void SelectTool(string name) => Tool = Enum.Parse<Tool>(name);

    private bool CanRun => !IsBusy && _coreReady && !_disposed && Tool switch
    {
        Tool.Join => Files.Count >= 2,
        Tool.Merge => Files.Count >= 1,
        Tool.Split or Tool.Sort => SelectedFile is not null,
        _ => false,
    };

    [RelayCommand(CanExecute = nameof(CanRun))]
    private async Task RunAsync()
    {
        Tool tool = Tool;
        CoreRequest? request;
        try
        {
            request = await BuildRequestAsync(tool);
        }
        catch (Exception ex)
        {
            // A picker or dialog that fails must not take the app down with it.
            ShowResult(InfoBarSeverity.Error, $"{ToolName(tool)} could not start", ex.Message);
            return;
        }
        if (request is null || _disposed) return;

        using var cancel = new CancellationTokenSource();
        _runCancel = cancel;
        _lastOutput = null;
        IsResultOpen = false;
        IsBusy = true;
        _phase = ToolTitle;
        StatusText = $"{ToolTitle} is running…";
        ShowProgress(_phase, -1);
        AddLog($"{ToolTitle} started.");

        var listener = new UiListener(_dispatcher, OnRunProgress, OnRunPhase, AddLog);
        try
        {
            // Files in the list that are about to be replaced must not be open for reading by then.
            foreach (FileItem item in Files.Where(file => IsOutputOf(request, file)))
            {
                _held.Add(item);
                item.Reading?.Cancel();
            }
            await Task.WhenAll(_held.Select(item => item.ReadTask).OfType<Task>());

            string summary;
            switch (tool)
            {
                case Tool.Join:
                {
                    JoinResult r = await RunCoreAsync(request, CoreJson.Default.JoinResult, listener, cancel.Token);
                    _lastOutput = r.Output;
                    summary = $"Joined {Format.Files(r.Files)} into {Path.GetFileName(r.Output)}: {Format.Games(r.Games)}, {Format.Size(r.BytesWritten)}, in {Format.Duration(r.ElapsedMs)}.";
                    break;
                }
                case Tool.Merge:
                {
                    CombineResult r = await RunCoreAsync(request, CoreJson.Default.CombineResult, listener, cancel.Token);
                    _lastOutput = r.Output;
                    string duplicates = request.RemoveDuplicates == true
                        ? r.DuplicatesRemoved == 0 ? " No duplicates were found." : $" {r.DuplicatesRemoved:N0} of {Format.Games(r.GamesRead)} were duplicates and left out."
                        : "";
                    summary = $"Merged {Format.Files(r.Files)} into {Path.GetFileName(r.Output)}: {Format.Games(r.GamesWritten)}, {Format.Size(r.BytesWritten)}, in {Format.Duration(r.ElapsedMs)}.{duplicates}";
                    break;
                }
                case Tool.Sort:
                {
                    CombineResult r = await RunCoreAsync(request, CoreJson.Default.CombineResult, listener, cancel.Token);
                    _lastOutput = r.Output;
                    summary = $"Sorted {Format.Games(r.GamesWritten)} into {Path.GetFileName(r.Output)}, {Format.Size(r.BytesWritten)}, in {Format.Duration(r.ElapsedMs)}.";
                    break;
                }
                default:
                {
                    SplitResult r = await RunCoreAsync(request, CoreJson.Default.SplitResult, listener, cancel.Token);
                    _lastOutput = r.OutputDir;
                    foreach (NameCount file in r.List) AddLog($"{file.Name}: {Format.Games(file.Games)}");
                    if (r.Files > r.List.Count) AddLog($"…and {r.Files - r.List.Count:N0} more files.");
                    summary = $"Split {Format.Games(r.Games)} into {Format.Files(r.Files)} in {r.OutputDir}, in {Format.Duration(r.ElapsedMs)}.";
                    break;
                }
            }
            ShowResult(InfoBarSeverity.Success, $"{ToolName(tool)} finished", summary);
            CanAddOutput = tool != Tool.Split;
            CanShowOutput = true;
        }
        catch (OperationCanceledException)
        {
            ShowResult(InfoBarSeverity.Warning, $"{ToolName(tool)} cancelled", "Nothing was saved.");
        }
        catch (Exception ex)
        {
            ShowResult(InfoBarSeverity.Error, $"{ToolName(tool)} could not finish", ex.Message);
        }
        finally
        {
            _runCancel = null;
            IsBusy = false;
            HideProgress();
            // What the list shows of the files that were to be replaced may no longer be true.
            if (_held.Count > 0 && !_disposed)
            {
                FileItem[] replaced = [.. _held.Where(Files.Contains)];
                _held.Clear();
                foreach (FileItem item in replaced) Invalidate(item);
                ReadWaitingFiles();
            }
        }
    }

    private static string ToolName(Tool tool) => tool.ToString();

    /// <summary>Whether running the request can replace this file.</summary>
    private static bool IsOutputOf(CoreRequest request, FileItem file)
    {
        if (request.Output is { } output) return SamePath(file.Path, output);
        // A split writes files named after its input into the chosen folder.
        return request is { OutputDir: { } folder, Input: { } input }
            && SamePath(Path.GetDirectoryName(file.Path) ?? "", folder)
            && file.Name.StartsWith($"{Path.GetFileNameWithoutExtension(input)}_", StringComparison.OrdinalIgnoreCase);
    }

    private static bool SamePath(string a, string b)
    {
        try
        {
            return string.Equals(Path.TrimEndingDirectorySeparator(Path.GetFullPath(a)), Path.TrimEndingDirectorySeparator(Path.GetFullPath(b)), StringComparison.OrdinalIgnoreCase);
        }
        catch (Exception ex) when (ex is ArgumentException or IOException or NotSupportedException)
        {
            return false;
        }
    }

    /// <summary>Asks where to save and gathers the options. Null when the user backs out.</summary>
    private async Task<CoreRequest?> BuildRequestAsync(Tool tool)
    {
        List<string> all = [.. Files.Select(file => file.Path)];
        switch (tool)
        {
            case Tool.Join:
            {
                if (await _shell.PickSaveFileAsync("joined.pgn") is not { } output) return null;
                return new CoreRequest { Op = "join", Inputs = all, Output = output };
            }
            case Tool.Merge:
            {
                if (await _shell.PickSaveFileAsync("merged.pgn") is not { } output) return null;
                List<SortKey> keys = MergeOrderIndex > 0 ? [new(FieldIds[MergeOrderIndex - 1], MergeDescending)] : [];
                return new CoreRequest { Op = "merge", Inputs = all, Output = output, RemoveDuplicates = MergeRemoveDuplicates, Keys = keys };
            }
            case Tool.Sort when SelectedFile is { } file:
            {
                var keys = new List<SortKey> { new(FieldIds[Math.Max(SortField1, 0)], SortDescending1) };
                foreach ((int index, bool descending) in new[] { (SortField2, SortDescending2), (SortField3, SortDescending3) })
                {
                    // Index 0 is "(nothing)"; sorting twice by the same thing adds nothing.
                    if (index > 0 && keys.All(key => key.Field != FieldIds[index - 1])) keys.Add(new(FieldIds[index - 1], descending));
                }
                if (await _shell.PickSaveFileAsync($"{Path.GetFileNameWithoutExtension(file.Name)}-sorted.pgn") is not { } output) return null;
                return new CoreRequest { Op = "sort", Input = file.Path, Output = output, Keys = keys };
            }
            case Tool.Split when SelectedFile is { } file:
            {
                if (await _shell.PickFolderAsync() is not { } folder) return null;
                string stem = Path.GetFileNameWithoutExtension(file.Name);
                if (HasEarlierSplit(folder, stem) && !await _shell.ConfirmAsync("Replace files?",
                        $"This folder already has files named {stem}_….pgn. Files with the same names will be replaced.", "Replace"))
                    return null;
                int mode = Math.Max(SplitModeIndex, 0);
                double count = mode == 0 ? GamesPerFile : NumberOfFiles;
                // An emptied number box reads as NaN.
                return new CoreRequest { Op = "split", Input = file.Path, OutputDir = folder, Mode = SplitModeIds[mode], Count = double.IsNaN(count) ? 0 : (long)count };
            }
            default:
                return null;
        }
    }

    private static bool HasEarlierSplit(string folder, string stem)
    {
        try
        {
            return Directory.EnumerateFiles(folder, $"{stem}_*.pgn").Any();
        }
        catch (Exception ex) when (ex is IOException or UnauthorizedAccessException)
        {
            return false;
        }
    }

    [RelayCommand(CanExecute = nameof(IsBusy))]
    private void Cancel()
    {
        _runCancel?.Cancel();
        StatusText = "Cancelling…";
    }

    private void OnRunPhase(string phase)
    {
        if (!IsBusy) return;
        _phase = phase;
        StatusText = $"{phase}…";
        ShowProgress(phase, -1);
    }

    private void OnRunProgress(double fraction, long games)
    {
        if (!IsBusy) return;
        ShowProgress(games > 0 ? $"{_phase} · {Format.Games(games)}" : _phase, fraction);
    }

    /// <param name="fraction">Between 0 and 1, or negative when the amount of work is unknown.</param>
    private void ShowProgress(string text, double fraction)
    {
        ProgressText = text;
        IsProgressIndeterminate = fraction < 0;
        ProgressValue = Math.Clamp(fraction, 0, 1) * 100;
        IsProgressVisible = true;
    }

    private void HideProgress()
    {
        IsProgressVisible = false;
        ProgressText = "";
    }

    private void ShowResult(InfoBarSeverity severity, string title, string message)
    {
        CanShowOutput = false;
        CanAddOutput = false;
        ResultSeverity = severity;
        ResultTitle = title;
        ResultMessage = message;
        IsResultOpen = true;
        StatusText = title;
        AddLog($"{title}. {message}");
    }

    [RelayCommand]
    private void ShowOutput()
    {
        if (_lastOutput is { } path) _shell.ShowInExplorer(path);
    }

    [RelayCommand]
    private void AddOutput()
    {
        if (_lastOutput is not { } path) return;
        if (Files.FirstOrDefault(file => SamePath(file.Path, path)) is { } listed)
        {
            // Already in the list: show it, and read it again in case it is not up to date.
            if (listed.State != FileState.Reading) Invalidate(listed);
            SelectedFile = listed;
            ReadWaitingFiles();
        }
        else
        {
            AddFiles([path]);
        }
        CanAddOutput = false;
    }

    // ---- Log and the rest --------------------------------------------------------------------

    private void AddLog(string text)
    {
        if (Log.Count >= MaxLogEntries) Log.RemoveAt(0);
        Log.Add(new LogEntry(DateTime.Now.ToString("HH:mm:ss"), text));
    }

    [RelayCommand]
    private void ClearLog() => Log.Clear();

    [RelayCommand]
    private Task AboutAsync()
    {
        Version? version = typeof(MainViewModel).Assembly.GetName().Version;
        return _shell.ShowMessageAsync("PGN Simple Edit",
            $"Version {version?.ToString(3)}\nNative core {CoreVersion}\n\nJoin, merge, split and sort PGN chess game files, and see what is inside them.");
    }

    [RelayCommand]
    private void Exit() => _shell.Exit();

    public void Dispose()
    {
        _disposed = true;
        _runCancel?.Cancel();
        foreach (FileItem item in Files) item.Reading?.Cancel();
    }

    /// <summary>Cancels everything that is running and waits until the core has stopped and removed
    /// its unfinished files. The app must not exit before then.</summary>
    public async Task ShutdownAsync()
    {
        Dispose();
        foreach (Task task in _coreTasks.ToArray())
        {
            try
            {
                await task;
            }
            catch (Exception)
            {
                // How it ended does not matter any more, only that it has.
            }
        }
    }

    /// <summary>Carries events from the core's worker thread to the UI thread, dropping progress
    /// reports that arrive faster than the screen needs them.</summary>
    private sealed class UiListener(DispatcherQueue dispatcher, Action<double, long> progress, Action<string> phase, Action<string> log) : ICoreListener
    {
        private const long MinimumGapMs = 50;
        private long _lastProgress;

        public void OnProgress(ulong done, ulong total, ulong games)
        {
            long now = Environment.TickCount64;
            if (now - _lastProgress < MinimumGapMs && done < total) return;
            _lastProgress = now;
            double fraction = total == 0 ? -1 : (double)done / total;
            dispatcher.TryEnqueue(() => progress(fraction, (long)games));
        }

        public void OnPhase(string text) => dispatcher.TryEnqueue(() => phase(text));

        public void OnLog(string text) => dispatcher.TryEnqueue(() => log(text));
    }
}
