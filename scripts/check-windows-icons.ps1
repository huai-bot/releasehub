# Verify the icons Windows actually extracts from compiled distribution files.
[CmdletBinding()]
param([string[]]$Paths = @('src-tauri/target/release/releasehub.exe'))
$ErrorActionPreference = 'Stop'
if (-not $IsWindows) { throw 'Icon verification requires Windows.' }
Add-Type -AssemblyName System.Drawing
foreach ($path in $Paths) {
    $absolute = (Resolve-Path -LiteralPath $path).Path
    $icon = [System.Drawing.Icon]::ExtractAssociatedIcon($absolute)
    if (-not $icon) { throw "No Windows icon found: $path" }
    $bitmap = $icon.ToBitmap()
    try {
        $blue = 0; $white = 0; $opaque = 0
        for ($y = 0; $y -lt $bitmap.Height; $y++) {
            for ($x = 0; $x -lt $bitmap.Width; $x++) {
                $pixel = $bitmap.GetPixel($x, $y)
                if ($pixel.A -lt 128) { continue }
                $opaque++
                if ([Math]::Abs($pixel.R - 29) -le 18 -and [Math]::Abs($pixel.G - 78) -le 18 -and [Math]::Abs($pixel.B - 216) -le 18) { $blue++ }
                if ($pixel.R -ge 220 -and $pixel.G -ge 220 -and $pixel.B -ge 220) { $white++ }
            }
        }
        if ($opaque -eq 0 -or $blue / $opaque -lt 0.35 -or $white / $opaque -lt 0.01) {
            throw "Compiled Windows icon is not the blue ReleaseHub mark: $path"
        }
        Write-Output "Verified blue Windows icon: $path ($($bitmap.Width)x$($bitmap.Height))"
    } finally { $bitmap.Dispose(); $icon.Dispose() }
}
