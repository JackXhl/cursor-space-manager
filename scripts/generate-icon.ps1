<#
.SYNOPSIS
    Draws the source app icon.

.DESCRIPTION
    Produces a 1024x1024 PNG that `npx tauri icon` expands into every platform
    format. The mark is a drive platter with an arrow leaving it, which reads at
    16px without relying on fine detail.

    Regenerate with:  pwsh -File scripts/generate-icon.ps1
#>

Add-Type -AssemblyName System.Drawing

$size = 1024
$bitmap = New-Object System.Drawing.Bitmap($size, $size)
$graphics = [System.Drawing.Graphics]::FromImage($bitmap)
$graphics.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$graphics.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$graphics.Clear([System.Drawing.Color]::Transparent)

function New-RoundedPath([int]$x, [int]$y, [int]$w, [int]$h, [int]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $path.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $path.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

# Background: rounded square with a vertical blue gradient.
$backgroundPath = New-RoundedPath 0 0 $size $size 224
$gradient = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Point(0, 0)),
    (New-Object System.Drawing.Point(0, $size)),
    [System.Drawing.Color]::FromArgb(255, 47, 116, 214),
    [System.Drawing.Color]::FromArgb(255, 25, 66, 138)
)
$graphics.FillPath($gradient, $backgroundPath)

# Drive platter: three stacked discs.
$white = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
$shade = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 198, 220, 252))

$platterX = 168
$platterWidth = 400
$platterHeight = 118
$gap = 150
for ($i = 2; $i -ge 0; $i--) {
    $top = 300 + ($i * $gap)
    $graphics.FillEllipse($shade, $platterX, $top, $platterWidth, $platterHeight)
    $graphics.FillEllipse($white, $platterX, ($top - 22), $platterWidth, $platterHeight)
}

# Arrow: data leaving the drive toward another volume.
$arrow = New-Object System.Drawing.Drawing2D.GraphicsPath
$points = @(
    (New-Object System.Drawing.PointF(596, 470)),
    (New-Object System.Drawing.PointF(770, 470)),
    (New-Object System.Drawing.PointF(770, 372)),
    (New-Object System.Drawing.PointF(936, 528)),
    (New-Object System.Drawing.PointF(770, 684)),
    (New-Object System.Drawing.PointF(770, 586)),
    (New-Object System.Drawing.PointF(596, 586))
)
$arrow.AddPolygon($points)
$graphics.FillPath($white, $arrow)

$directory = Join-Path $PSScriptRoot '..\src-tauri\icons'
New-Item -ItemType Directory -Path $directory -Force | Out-Null
$output = Join-Path $directory 'source.png'
$bitmap.Save($output, [System.Drawing.Imaging.ImageFormat]::Png)

$graphics.Dispose()
$bitmap.Dispose()
Write-Output "Wrote $output"
