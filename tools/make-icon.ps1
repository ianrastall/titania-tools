# Draws the PGN Simple Edit icon and writes it as a multi-size .ico.
# Only needed when the icon design changes; the result is checked in.
param(
    [string] $Output = (Join-Path $PSScriptRoot '..\apps\PgnSimpleEdit\Assets\AppIcon.ico')
)

$ErrorActionPreference = 'Stop'
Add-Type -AssemblyName System.Drawing

function New-IconPng([int] $size) {
    $bitmap = [System.Drawing.Bitmap]::new($size, $size, [System.Drawing.Imaging.PixelFormat]::Format32bppArgb)
    $g = [System.Drawing.Graphics]::FromImage($bitmap)
    $g.SmoothingMode = 'AntiAlias'
    $g.TextRenderingHint = 'AntiAliasGridFit'
    $g.Clear([System.Drawing.Color]::Transparent)

    # A rounded square in dark teal.
    $radius = [single]($size * 0.22)
    $d = $radius * 2
    $edge = [single]($size - 1)
    $path = [System.Drawing.Drawing2D.GraphicsPath]::new()
    $path.AddArc(0, 0, $d, $d, 180, 90)
    $path.AddArc($edge - $d, 0, $d, $d, 270, 90)
    $path.AddArc($edge - $d, $edge - $d, $d, $d, 0, 90)
    $path.AddArc(0, $edge - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    $rect = [System.Drawing.RectangleF]::new(0, 0, $size, $size)
    $fill = [System.Drawing.Drawing2D.LinearGradientBrush]::new($rect,
        [System.Drawing.Color]::FromArgb(255, 20, 140, 128), [System.Drawing.Color]::FromArgb(255, 15, 76, 92), 60.0)
    $g.FillPath($fill, $path)

    # A white knight, centred.
    $font = [System.Drawing.Font]::new('Segoe UI Symbol', [single]($size * 0.62), [System.Drawing.FontStyle]::Regular, [System.Drawing.GraphicsUnit]::Pixel)
    $format = [System.Drawing.StringFormat]::new()
    $format.Alignment = 'Center'
    $format.LineAlignment = 'Center'
    $textRect = [System.Drawing.RectangleF]::new(0, [single]($size * 0.02), $size, $size)
    $g.DrawString([string][char]0x265E, $font, [System.Drawing.Brushes]::White, $textRect, $format)

    $stream = [System.IO.MemoryStream]::new()
    $bitmap.Save($stream, [System.Drawing.Imaging.ImageFormat]::Png)
    $g.Dispose(); $bitmap.Dispose(); $font.Dispose(); $fill.Dispose(); $path.Dispose()
    , $stream.ToArray()
}

$sizes = 16, 20, 24, 32, 40, 48, 64, 256
$images = foreach ($size in $sizes) { , (New-IconPng $size) }

# ICO: a 6-byte header, one 16-byte directory entry per image, then the PNG data.
$out = [System.IO.MemoryStream]::new()
$writer = [System.IO.BinaryWriter]::new($out)
$writer.Write([uint16]0); $writer.Write([uint16]1); $writer.Write([uint16]$sizes.Count)
$offset = 6 + 16 * $sizes.Count
for ($i = 0; $i -lt $sizes.Count; $i++) {
    $dimension = if ($sizes[$i] -ge 256) { 0 } else { $sizes[$i] }
    $writer.Write([byte]$dimension); $writer.Write([byte]$dimension)
    $writer.Write([byte]0); $writer.Write([byte]0)
    $writer.Write([uint16]1); $writer.Write([uint16]32)
    $writer.Write([uint32]$images[$i].Length); $writer.Write([uint32]$offset)
    $offset += $images[$i].Length
}
foreach ($image in $images) { $writer.Write([byte[]]$image) }
$writer.Flush()

$Output = [System.IO.Path]::GetFullPath($Output)
New-Item -ItemType Directory -Force (Split-Path $Output) | Out-Null
[System.IO.File]::WriteAllBytes($Output, $out.ToArray())
Get-Item $Output
