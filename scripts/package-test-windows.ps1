# Local, explicitly labelled test delivery. Official release packaging remains separate.
[CmdletBinding()]
param([string]$OutputDirectory = 'artifacts/test-0.4.0-round-2')
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not [IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $root $OutputDirectory }
$output = [IO.Path]::GetFullPath($OutputDirectory)
$version = (Get-Content -LiteralPath (Join-Path $root 'package.json') -Raw -Encoding utf8 | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?$') { throw 'Invalid package version.' }
$zip = Join-Path $output "ReleaseHub-$version-windows-x64-TEST.zip"
$sourceZip = Join-Path $output "ReleaseHub-$version-source-TEST.zip"
$portable = Join-Path $output 'ReleaseHub-TEST'
$sourceStage = Join-Path $output 'source-stage/ReleaseHub-source'
foreach ($path in @($zip, $sourceZip, $portable, $sourceStage)) {
    if (Test-Path -LiteralPath $path) { throw "Output already exists; choose a fresh output directory: $path" }
}
$exe = Join-Path $root 'src-tauri/target/release/releasehub.exe'
$mcp = Join-Path $root 'src-tauri/target/release/releasehub-mcp.exe'
foreach ($path in @($exe, $mcp)) {
    if (-not (Test-Path -LiteralPath $path -PathType Leaf)) { throw 'Build release binaries with tauri/custom-protocol before packaging.' }
}
New-Item -ItemType Directory -Path $portable, (Join-Path $portable 'tools'), (Join-Path $portable 'integrations'), $sourceStage -Force | Out-Null
# Explicit public-file allowlist. A data directory is never copied into the package.
Copy-Item -LiteralPath $exe -Destination (Join-Path $portable 'ReleaseHub.exe')
Copy-Item -LiteralPath $mcp -Destination (Join-Path $portable 'tools/releasehub-mcp.exe')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination $portable
Copy-Item -LiteralPath (Join-Path $root 'docs/PORTABLE.txt') -Destination (Join-Path $portable 'README.txt')
Copy-Item -LiteralPath (Join-Path $root 'docs/TESTING-0.4.md') -Destination (Join-Path $portable 'TESTING.md')
Copy-Item -LiteralPath (Join-Path $root 'integrations/releasehub-agent.ps1') -Destination (Join-Path $portable 'integrations')
Copy-Item -LiteralPath (Join-Path $root 'docs/AGENT_GUIDE.md') -Destination (Join-Path $portable 'integrations/AGENT_GUIDE.md')
[IO.File]::WriteAllText((Join-Path $portable 'portable.flag'), 'ReleaseHub portable test mode', [Text.UTF8Encoding]::new($false))
Compress-Archive -LiteralPath $portable -DestinationPath $zip -CompressionLevel Optimal
# Git's non-ignored source inventory excludes build output, user data and local dependencies.
$git = (Get-Command git -ErrorAction Stop).Source
Push-Location $root
try {
    $files = @(& $git -c core.quotepath=false ls-files --cached --others --exclude-standard)
    if ($LASTEXITCODE -ne 0) { throw 'Cannot enumerate source files.' }
    foreach ($relative in $files) {
        if ($relative -match '(^|/)(\.git|node_modules|data|private|\.local|artifacts|\.artifacts|target|dist|\.tools)(/|$)' -or $relative -match '(^|/)\.env($|\.)|\.(sqlite3?|db|pem|key|pfx|p12)$') { throw "Disallowed source entry: $relative" }
        $file = Join-Path $root $relative
        if (-not (Test-Path -LiteralPath $file -PathType Leaf)) { continue }
        $destination = Join-Path $sourceStage $relative
        New-Item -ItemType Directory -Path (Split-Path -Parent $destination) -Force | Out-Null
        Copy-Item -LiteralPath $file -Destination $destination
    }
} finally { Pop-Location }
Compress-Archive -LiteralPath $sourceStage -DestinationPath $sourceZip -CompressionLevel Optimal
Get-Item -LiteralPath $zip, $sourceZip | Select-Object Name, Length
