using PgnSimpleEdit.Models;

namespace PgnSimpleEdit.ViewModels;

/// <summary>Lays a report from the core out as tiles and sections for the Info view.</summary>
internal static class ReportBuilder
{
    private const int MaxYears = 15;

    public static IReadOnlyList<Tile> Tiles(ReportResult r) =>
    [
        new("Games", r.Games.ToString("N0")),
        new("Players", r.Players.ToString("N0")),
        new("Events", r.Events.ToString("N0")),
        new("Dates", DateRange(r)),
        new("Average rating", r.RatedGames > 0 ? r.AverageElo.ToString("N0") : "—"),
        new("Average length", r.Games > 0 ? Format.Moves(r.TotalPlies / r.Games) : "—"),
        new("File size", Format.Size(r.FileSize)),
    ];

    public static IReadOnlyList<ReportSection> LeftSections(ReportResult r)
    {
        var sections = new List<ReportSection>
        {
            new("Results",
            [
                Share("White wins", r.WhiteWins, r.Games),
                Share("Draws", r.Draws, r.Games),
                Share("Black wins", r.BlackWins, r.Games),
                Share("Unfinished or unknown", r.OtherResults, r.Games),
            ]),
        };
        AddTop(sections, Title("Players", r.TopPlayers.Count, r.Players), r.TopPlayers);
        AddTop(sections, Title("Openings (ECO)", r.TopEco.Count, r.EcoCodes), r.TopEco);
        sections.Add(new("Tags", [.. r.Tags.Select(tag => new StatRow(tag.Name, Coverage(tag.Games, r.Games)))]));
        return sections;
    }

    public static IReadOnlyList<ReportSection> RightSections(ReportResult r)
    {
        var sections = new List<ReportSection>();

        if (r.Years.Count > 0)
        {
            // Newest first; a long history is cut off at the most recent years.
            long largest = r.Years.Max(year => year.Games);
            var rows = r.Years.Reverse().Take(MaxYears).Select(year => Bar(year.Name, year.Games, largest)).ToList();
            if (r.Undated > 0) rows.Add(new StatRow("No date", r.Undated.ToString("N0")));
            sections.Add(new(Title("Years", Math.Min(r.Years.Count, MaxYears), r.Years.Count, "Most recent"), rows));
        }

        AddTop(sections, Title("Events", r.TopEvents.Count, r.Events), r.TopEvents);

        var details = new List<StatRow>();
        if (r.Games > 0)
        {
            details.Add(new("Shortest game", Format.Moves(r.MinPlies)));
            details.Add(new("Longest game", Format.Moves(r.MaxPlies)));
        }
        details.Add(new("With comments", Coverage(r.WithComments, r.Games)));
        details.Add(new("With variations", Coverage(r.WithVariations, r.Games)));
        details.Add(new("With ratings", Coverage(r.RatedGames, r.Games)));
        if (r.RatedGames > 0) details.Add(new("Rating range", $"{r.MinElo:N0} – {r.MaxElo:N0}"));
        details.Add(new("Without moves", r.WithoutMoves.ToString("N0")));
        details.Add(new("Missing a standard tag", r.IncompleteHeaders.ToString("N0")));
        details.Add(new("Starting from a set-up position", r.FromPosition.ToString("N0")));
        sections.Add(new("Games", details));

        sections.Add(new("File",
        [
            new("Folder", Path.GetDirectoryName(r.Path) ?? ""),
            new("Size", $"{Format.Size(r.FileSize)} ({r.FileSize:N0} bytes)"),
            new("Text encoding", r.Encoding),
            new("Line endings", r.LineEndings),
            new("Read in", Format.Duration(r.ElapsedMs)),
        ]));
        return sections;
    }

    private static string DateRange(ReportResult r)
    {
        if (r.FirstDate is not { } first || r.LastDate is not { } last) return "Unknown";
        if (first == last) return first;
        // Years alone when the file covers several of them; otherwise the full dates.
        return first[..4] == last[..4] ? $"{first} – {last}" : $"{first[..4]} – {last[..4]}";
    }

    private static void AddTop(List<ReportSection> sections, string title, IReadOnlyList<NameCount> top)
    {
        if (top.Count == 0) return;
        long largest = top[0].Games;
        sections.Add(new(title, [.. top.Select(entry => Bar(entry.Name, entry.Games, largest))]));
    }

    /// <summary>"Players" when everything is shown, "Players: top 15 of 482" when it is not.</summary>
    private static string Title(string what, int shown, long total, string top = "Top") =>
        shown >= total ? what : $"{what}: {top.ToLowerInvariant()} {shown} of {total:N0}";

    private static StatRow Share(string label, long part, long whole) =>
        new(label, $"{part:N0}   {Format.Percent(part, whole)}".TrimEnd(), whole == 0 ? 0 : 100.0 * part / whole, true);

    private static StatRow Bar(string label, long value, long largest) =>
        new(label, value.ToString("N0"), largest == 0 ? 0 : 100.0 * value / largest, true);

    private static string Coverage(long part, long whole) =>
        part == whole && whole > 0 ? "all games" : $"{part:N0}   {Format.Percent(part, whole)}".TrimEnd();
}
