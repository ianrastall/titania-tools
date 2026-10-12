using CommunityToolkit.Mvvm.ComponentModel;
using PgnSimpleEdit.Models;

namespace PgnSimpleEdit.ViewModels;

public enum Tool
{
    Info,
    Join,
    Merge,
    Split,
    Sort,
}

public enum FileState
{
    Waiting,
    Reading,
    Ready,
    Failed,
}

/// <summary>A PGN file in the list, with the report on it once it has been read.</summary>
public sealed partial class FileItem : ObservableObject
{
    public FileItem(string path)
    {
        Path = path;
        Name = System.IO.Path.GetFileName(path);
        Refresh();
    }

    public string Path { get; }
    public string Name { get; }
    public long Size { get; private set; }

    /// <summary>Goes up each time the file is found to have changed; a reading that started before is out of date.</summary>
    public int Generation { get; private set; }

    /// <summary>Forgets what was read from the file, which has changed on disk, so that it is read again.</summary>
    /// <exception cref="IOException">The file is no longer there.</exception>
    [System.Diagnostics.CodeAnalysis.MemberNotNull(nameof(Detail))]
    public void Refresh()
    {
        Generation++;
        Report = null;
        Error = null;
        PercentRead = 0;
        State = FileState.Waiting;
        Detail = "waiting";
        Size = new FileInfo(Path).Length;
        Detail = $"{Format.Size(Size)} · waiting";
    }

    /// <summary>The second line in the list: size and number of games, or what is happening.</summary>
    [ObservableProperty]
    public partial string Detail { get; set; }

    public FileState State { get; set; }
    public ReportResult? Report { get; set; }
    public string? Error { get; set; }
    public int PercentRead { get; set; }

    /// <summary>Cancels the reading of this file while it is in progress.</summary>
    public CancellationTokenSource? Reading { get; set; }

    /// <summary>The reading in progress, which holds the file open until it completes.</summary>
    public Task? ReadTask { get; set; }
}

/// <summary>A headline number at the top of the report.</summary>
public sealed record Tile(string Label, string Value);

/// <summary>One line of a report section; the bar shows <see cref="Percent"/> of the whole.</summary>
public sealed record StatRow(string Label, string Value, double Percent = 0, bool HasBar = false);

public sealed record ReportSection(string Title, IReadOnlyList<StatRow> Rows);

public sealed record LogEntry(string Time, string Text);
