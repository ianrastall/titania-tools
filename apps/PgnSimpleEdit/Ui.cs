using Microsoft.UI.Xaml;

namespace PgnSimpleEdit;

/// <summary>Small conversion functions for x:Bind.</summary>
public static class Ui
{
    public static Visibility Show(bool value) => value ? Visibility.Visible : Visibility.Collapsed;
    public static Visibility Hide(bool value) => value ? Visibility.Collapsed : Visibility.Visible;
    public static bool Not(bool value) => !value;
}

/// <summary>How numbers are written out for people.</summary>
public static class Format
{
    private static readonly string[] Units = ["bytes", "KB", "MB", "GB", "TB"];

    public static string Size(long bytes)
    {
        double value = bytes;
        int unit = 0;
        while (value >= 1024 && unit < Units.Length - 1)
        {
            value /= 1024;
            unit++;
        }
        return unit == 0 ? $"{bytes:N0} bytes" : $"{value:0.##} {Units[unit]}";
    }

    public static string Games(long count) => count == 1 ? "1 game" : $"{count:N0} games";

    public static string Files(long count) => count == 1 ? "1 file" : $"{count:N0} files";

    public static string Duration(long milliseconds) => milliseconds switch
    {
        < 1000 => $"{milliseconds} ms",
        < 60_000 => $"{milliseconds / 1000.0:0.0} s",
        _ => $"{milliseconds / 60_000} min {milliseconds / 1000 % 60} s",
    };

    /// <summary>Full moves for a number of half-moves.</summary>
    public static string Moves(long plies)
    {
        long moves = (plies + 1) / 2;
        return moves == 1 ? "1 move" : $"{moves:N0} moves";
    }

    public static string Percent(long part, long whole) => whole == 0 ? "" : $"{100.0 * part / whole:0.#}%";
}
