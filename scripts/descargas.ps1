# Muestra cuantas veces se bajo cada instalador de Dualrec (contador publico de GitHub).
# Uso: powershell -ExecutionPolicy Bypass -File scripts\descargas.ps1
$releases = Invoke-RestMethod "https://api.github.com/repos/MalkSof/Dualrec/releases" -Headers @{ "User-Agent" = "dualrec-descargas" }
if (-not $releases) { Write-Host "Aun no hay ninguna version publicada."; exit }
$total = 0
foreach ($r in $releases) {
    foreach ($a in $r.assets) {
        $total += $a.download_count
        [pscustomobject]@{ Version = $r.tag_name; Archivo = $a.name; Descargas = $a.download_count; Publicado = $r.published_at }
    }
}
Write-Host "`nTotal de descargas: $total"
