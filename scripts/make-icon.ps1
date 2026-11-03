# Generates the Murmur app icon (1024x1024 PNG) with GDI+.
# Run from the project root: powershell -ExecutionPolicy Bypass -File scripts/make-icon.ps1

Add-Type -AssemblyName System.Drawing

$size = 1024
$bmp = New-Object System.Drawing.Bitmap($size, $size)
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = [System.Drawing.Drawing2D.SmoothingMode]::AntiAlias
$g.InterpolationMode = [System.Drawing.Drawing2D.InterpolationMode]::HighQualityBicubic
$g.Clear([System.Drawing.Color]::Transparent)

function New-RoundedPath([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $path = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $path.AddArc($x, $y, $d, $d, 180, 90)
    $path.AddArc(($x + $w - $d), $y, $d, $d, 270, 90)
    $path.AddArc(($x + $w - $d), ($y + $h - $d), $d, $d, 0, 90)
    $path.AddArc($x, ($y + $h - $d), $d, $d, 90, 90)
    $path.CloseFigure()
    return $path
}

# Rounded square backdrop with a soft vertical gradient.
$bgRect = New-Object System.Drawing.Rectangle(0, 0, $size, $size)
$bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    $bgRect,
    [System.Drawing.Color]::FromArgb(255, 26, 28, 34),
    [System.Drawing.Color]::FromArgb(255, 6, 7, 9),
    90.0
)
$bgPath = New-RoundedPath 0 0 $size $size 232
$g.FillPath($bgBrush, $bgPath)

# Inner border for a bit of depth.
$borderPen = New-Object System.Drawing.Pen([System.Drawing.Color]::FromArgb(34, 255, 255, 255), 3)
$g.DrawPath($borderPen, $bgPath)

# Five sound bars, mint gradient.
$barWidth = 68
$gap = 46
$heights = @(292, 452, 612, 452, 292)
$totalWidth = (5 * $barWidth) + (4 * $gap)
$startX = ($size - $totalWidth) / 2
$centerY = $size / 2

$barBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush(
    (New-Object System.Drawing.Rectangle(0, 160, $size, 704)),
    [System.Drawing.Color]::FromArgb(255, 233, 255, 242),
    [System.Drawing.Color]::FromArgb(255, 108, 235, 161),
    90.0
)

for ($i = 0; $i -lt 5; $i++) {
    $h = $heights[$i]
    $x = $startX + ($i * ($barWidth + $gap))
    $y = $centerY - ($h / 2)
    $path = New-RoundedPath $x $y $barWidth $h ($barWidth / 2)
    $g.FillPath($barBrush, $path)
    $path.Dispose()
}

$out = Join-Path (Get-Location) "app-icon.png"
$bmp.Save($out, [System.Drawing.Imaging.ImageFormat]::Png)

$barBrush.Dispose()
$borderPen.Dispose()
$bgBrush.Dispose()
$bgPath.Dispose()
$g.Dispose()
$bmp.Dispose()

Write-Host "Wrote $out"
