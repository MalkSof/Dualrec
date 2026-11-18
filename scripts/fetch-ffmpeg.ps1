# Descarga FFmpeg (build "essentials" de gyan.dev, ~100 MB) y lo deja como
# sidecar de Tauri en apps/desktop/src-tauri/binaries/ffmpeg-<triple>.exe.
#
#   powershell -ExecutionPolicy Bypass -File scripts/fetch-ffmpeg.ps1
#
# El build essentials trae lo que usa Dualrec: libx264, NVENC, Quick Sync,
# AMF y AAC. Es GPL: ver "Licencias de terceros" en el README.
param(
    [string]$Url = "https://www.gyan.dev/ffmpeg/builds/ffmpeg-release-essentials.zip"
)

$ErrorActionPreference = "Stop"
$ProgressPreference = "SilentlyContinue"

$triple = (& rustc -vV | Select-String "^host: ").ToString().Substring(6).Trim()
$binDir = Join-Path $PSScriptRoot "..\apps\desktop\src-tauri\binaries"
New-Item -ItemType Directory -Force $binDir | Out-Null
$target = Join-Path $binDir "ffmpeg-$triple.exe"

# El zip se extrae en una carpeta temporal nueva y vacía; de ahí solo se copia ffmpeg.exe.
$work = Join-Path ([System.IO.Path]::GetTempPath()) ("dualrec-ffmpeg-" + [guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory -Force $work | Out-Null
try {
    $zip = Join-Path $work "ffmpeg.zip"
    Write-Output "Descargando $Url ..."
    Invoke-WebRequest -Uri $Url -OutFile $zip
    $extract = Join-Path $work "x"
    Expand-Archive -LiteralPath $zip -DestinationPath $extract
    $exe = Get-ChildItem $extract -Recurse -Filter ffmpeg.exe | Select-Object -First 1
    if (-not $exe) { throw "El zip no contiene ffmpeg.exe" }
    Copy-Item -LiteralPath $exe.FullName -Destination $target -Force
    $mb = [math]::Round((Get-Item $target).Length / 1MB, 1)
    Write-Output "Listo: $target ($mb MB)"
}
finally {
    Remove-Item -Recurse -Force $work -ErrorAction SilentlyContinue
}
