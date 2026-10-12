using System.Text.Json.Serialization;

namespace PgnSimpleEdit.Models;

// The JSON documents exchanged with titania_core.dll. Property names map to snake_case.

/// <summary>A request to the core. Only the properties an operation uses are set.</summary>
internal sealed class CoreRequest
{
    public required string Op { get; init; }
    public string? Input { get; init; }
    public IReadOnlyList<string>? Inputs { get; init; }
    public string? Output { get; init; }
    public string? OutputDir { get; init; }
    public bool? RemoveDuplicates { get; init; }
    public IReadOnlyList<SortKey>? Keys { get; init; }
    public string? Mode { get; init; }
    public long? Count { get; init; }
}

internal sealed record SortKey(string Field, bool Descending);

internal sealed record CoreError(string? Error);

public sealed record NameCount(string Name, long Games);

public sealed class ReportResult
{
    public string Path { get; init; } = "";
    public long FileSize { get; init; }
    public long Games { get; init; }
    public long ElapsedMs { get; init; }
    public string Encoding { get; init; } = "";
    public string LineEndings { get; init; } = "";

    public long WhiteWins { get; init; }
    public long BlackWins { get; init; }
    public long Draws { get; init; }
    public long OtherResults { get; init; }

    public string? FirstDate { get; init; }
    public string? LastDate { get; init; }
    public IReadOnlyList<NameCount> Years { get; init; } = [];
    public long Undated { get; init; }

    public long Players { get; init; }
    public IReadOnlyList<NameCount> TopPlayers { get; init; } = [];
    public long Events { get; init; }
    public IReadOnlyList<NameCount> TopEvents { get; init; } = [];
    public long EcoCodes { get; init; }
    public IReadOnlyList<NameCount> TopEco { get; init; } = [];

    public long RatedGames { get; init; }
    public int MinElo { get; init; }
    public int MaxElo { get; init; }
    public double AverageElo { get; init; }

    public long TotalPlies { get; init; }
    public int MinPlies { get; init; }
    public int MaxPlies { get; init; }
    public long WithoutMoves { get; init; }
    public long WithComments { get; init; }
    public long WithVariations { get; init; }
    public long IncompleteHeaders { get; init; }
    public long FromPosition { get; init; }

    public IReadOnlyList<NameCount> Tags { get; init; } = [];
}

internal sealed class JoinResult
{
    public string Output { get; init; } = "";
    public long Files { get; init; }
    public long Games { get; init; }
    public long BytesWritten { get; init; }
    public long ElapsedMs { get; init; }
}

/// <summary>The result of both merge and sort.</summary>
internal sealed class CombineResult
{
    public string Output { get; init; } = "";
    public long Files { get; init; }
    public long GamesRead { get; init; }
    public long GamesWritten { get; init; }
    public long DuplicatesRemoved { get; init; }
    public long BytesWritten { get; init; }
    public long ElapsedMs { get; init; }
}

internal sealed class SplitResult
{
    public string OutputDir { get; init; } = "";
    public long Games { get; init; }
    public long Files { get; init; }
    public IReadOnlyList<NameCount> List { get; init; } = [];
    public long ElapsedMs { get; init; }
}

[JsonSourceGenerationOptions(PropertyNamingPolicy = JsonKnownNamingPolicy.SnakeCaseLower, DefaultIgnoreCondition = JsonIgnoreCondition.WhenWritingNull)]
[JsonSerializable(typeof(CoreRequest))]
[JsonSerializable(typeof(CoreError))]
[JsonSerializable(typeof(ReportResult))]
[JsonSerializable(typeof(JoinResult))]
[JsonSerializable(typeof(CombineResult))]
[JsonSerializable(typeof(SplitResult))]
internal sealed partial class CoreJson : JsonSerializerContext;
