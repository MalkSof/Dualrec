# Genera el ícono fuente de Dualrec (1024x1024). Después:
#   cd apps/desktop; npx tauri icon src-tauri/app-icon.png
param([string]$Out = "$PSScriptRoot\..\apps\desktop\src-tauri\app-icon.png")

Add-Type -AssemblyName System.Drawing

function New-RoundedRect([float]$x, [float]$y, [float]$w, [float]$h, [float]$r) {
    $p = New-Object System.Drawing.Drawing2D.GraphicsPath
    $d = $r * 2
    $p.AddArc($x, $y, $d, $d, 180, 90)
    $p.AddArc($x + $w - $d, $y, $d, $d, 270, 90)
    $p.AddArc($x + $w - $d, $y + $h - $d, $d, $d, 0, 90)
    $p.AddArc($x, $y + $h - $d, $d, $d, 90, 90)
    $p.CloseFigure()
    return $p
}

$size = 1024
$bmp = New-Object System.Drawing.Bitmap $size, $size
$g = [System.Drawing.Graphics]::FromImage($bmp)
$g.SmoothingMode = 'AntiAlias'
$g.Clear([System.Drawing.Color]::Transparent)

# Fondo: cuadrado redondeado con degradé oscuro.
$bg = New-RoundedRect 64 64 896 896 208
$bgBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush `
    (New-Object System.Drawing.PointF 64, 64), (New-Object System.Drawing.PointF 960, 960), `
    ([System.Drawing.Color]::FromArgb(255, 30, 27, 58)), ([System.Drawing.Color]::FromArgb(255, 11, 13, 18))
$g.FillPath($bgBrush, $bg)

# Pantalla: rectángulo con degradé violeta-azul.
$screen = New-RoundedRect 200 270 560 380 56
$scBrush = New-Object System.Drawing.Drawing2D.LinearGradientBrush `
    (New-Object System.Drawing.PointF 200, 270), (New-Object System.Drawing.PointF 760, 650), `
    ([System.Drawing.Color]::FromArgb(255, 139, 124, 255)), ([System.Drawing.Color]::FromArgb(255, 77, 163, 255))
$g.FillPath($scBrush, $screen)

# Cámara: burbuja circular superpuesta, con borde del color del fondo.
$ring = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 17, 17, 32))
$g.FillEllipse($ring, 520, 470, 340, 340)
$cam = New-Object System.Drawing.Drawing2D.LinearGradientBrush `
    (New-Object System.Drawing.PointF 548, 498), (New-Object System.Drawing.PointF 832, 782), `
    ([System.Drawing.Color]::FromArgb(255, 255, 107, 129)), ([System.Drawing.Color]::FromArgb(255, 255, 61, 87))
$g.FillEllipse($cam, 548, 498, 284, 284)
$dot = New-Object System.Drawing.SolidBrush ([System.Drawing.Color]::FromArgb(255, 255, 255, 255))
$g.FillEllipse($dot, 640, 590, 100, 100)

$g.Dispose()
$bmp.Save((Resolve-Path -LiteralPath (Split-Path $Out)).Path + '\' + (Split-Path $Out -Leaf), [System.Drawing.Imaging.ImageFormat]::Png)
$bmp.Dispose()
Write-Output "Icono generado en $Out"
