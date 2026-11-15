# Run with PowerShell 7 from the repository root. Packaging never copies user data.
[CmdletBinding()]
param(
    [string]$TargetDirectory = '',
    [string]$OutputDirectory = 'artifacts'
)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
if (-not $TargetDirectory) {
    if ($env:CARGO_TARGET_DIR) { $TargetDirectory = $env:CARGO_TARGET_DIR }
    else { $TargetDirectory = Join-Path $root 'src-tauri/target' }
}
$target = (Resolve-Path -LiteralPath $TargetDirectory).Path
if (-not [IO.Path]::IsPathRooted($OutputDirectory)) { $OutputDirectory = Join-Path $root $OutputDirectory }
$output = [IO.Path]::GetFullPath($OutputDirectory)
$version = (Get-Content -Raw -Encoding utf8 (Join-Path $root 'package.json') | ConvertFrom-Json).version
if ($version -notmatch '^\d+\.\d+\.\d+(-[a-zA-Z0-9.-]+)?$') { throw 'Invalid package version.' }
$exe = Join-Path $target 'release/releasehub.exe'
if (-not (Test-Path -LiteralPath $exe -PathType Leaf)) { throw 'Build the Windows executable first.' }
$installers = @(Get-ChildItem -LiteralPath (Join-Path $target 'release/bundle/nsis') -File -Filter '*-setup.exe')
if ($installers.Count -ne 1) { throw 'Expected exactly one NSIS installer; use a clean build directory.' }
New-Item -ItemType Directory -Force -Path $output | Out-Null
$zipPath = Join-Path $output "ReleaseHub-$version-windows-x64-portable.zip"
$setupPath = Join-Path $output "ReleaseHub-$version-windows-x64-setup.exe"
$checksumsPath = Join-Path $output 'SHA256SUMS.txt'
foreach ($destination in @($zipPath, $setupPath, $checksumsPath)) {
    if (Test-Path -LiteralPath $destination) { throw "Output already exists; select a fresh OutputDirectory: $destination" }
}
$stage = Join-Path $output ('portable-stage-' + [Guid]::NewGuid().ToString('N'))
$portable = Join-Path $stage 'ReleaseHub'
New-Item -ItemType Directory -Path $portable -Force | Out-Null
# Explicit allowlist: no recursive project-folder copy, credentials, databases or configuration.
Copy-Item -LiteralPath $exe -Destination (Join-Path $portable 'ReleaseHub.exe')
Copy-Item -LiteralPath (Join-Path $root 'LICENSE') -Destination (Join-Path $portable 'LICENSE')
Copy-Item -LiteralPath (Join-Path $root 'docs/PORTABLE.txt') -Destination (Join-Path $portable 'README.txt')
[IO.File]::WriteAllText((Join-Path $portable 'portable.flag'), 'ReleaseHub portable mode', [Text.UTF8Encoding]::new($false))
$integrations = Join-Path $portable 'integrations'
New-Item -ItemType Directory -Path $integrations | Out-Null
Copy-Item -LiteralPath (Join-Path $root 'integrations/releasehub-agent.ps1') -Destination $integrations
Copy-Item -LiteralPath (Join-Path $root 'docs/AGENT_GUIDE.md') -Destination (Join-Path $integrations 'AGENT_GUIDE.md')
$tools = Join-Path $portable 'tools'
New-Item -ItemType Directory -Path $tools | Out-Null
foreach ($name in @('gh.exe', 'releasehub-mcp.exe', 'LICENSE.github-cli.txt', 'github-cli-version.txt')) {
    $source = Join-Path $root ('src-tauri/tools/' + $name)
    if (-not (Test-Path -LiteralPath $source -PathType Leaf)) { throw "Missing prepared public component: $name" }
    Copy-Item -LiteralPath $source -Destination (Join-Path $tools $name)
}
Compress-Archive -LiteralPath $portable -DestinationPath $zipPath -CompressionLevel Optimal
Copy-Item -LiteralPath $installers[0].FullName -Destination $setupPath
$lines = foreach ($path in @($zipPath, $setupPath)) {
    $hash = (Get-FileHash -LiteralPath $path -Algorithm SHA256).Hash.ToLowerInvariant()
    '{0}  {1}' -f $hash, (Split-Path -Leaf $path)
}
[IO.File]::WriteAllLines($checksumsPath, $lines, [Text.UTF8Encoding]::new($false))
Get-Item -LiteralPath $zipPath, $setupPath, $checksumsPath | Select-Object Name, Length
