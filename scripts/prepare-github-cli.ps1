# Download only the pinned public GitHub CLI executable and its license into the build workspace.
[CmdletBinding()]
param()
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
$version = '2.100.0'
$expectedSha256 = '227e35230b25db3fa1b997bab7cf4d67df0470a3b75b99e4ee66bce1a7cd4e72'
$tools = Join-Path $root 'src-tauri/tools'
$exe = Join-Path $tools 'gh.exe'
$license = Join-Path $tools 'LICENSE.github-cli.txt'
$stamp = Join-Path $tools 'github-cli-version.txt'
if ((Test-Path -LiteralPath $exe -PathType Leaf) -and (Test-Path -LiteralPath $license -PathType Leaf) -and (Test-Path -LiteralPath $stamp -PathType Leaf)) {
    if ([IO.File]::ReadAllText($stamp, [Text.Encoding]::UTF8).Trim() -eq $version) {
        Write-Output "GitHub CLI $version is already prepared in the workspace."
        return
    }
}
$cache = Join-Path $root '.cache/github-cli'
New-Item -ItemType Directory -Force -Path $cache, $tools | Out-Null
$archive = Join-Path $cache "gh_${version}_windows_amd64.zip"
if (-not (Test-Path -LiteralPath $archive -PathType Leaf)) {
    Invoke-WebRequest -Uri "https://github.com/cli/cli/releases/download/v$version/gh_${version}_windows_amd64.zip" -OutFile $archive -UseBasicParsing
}
$actual = (Get-FileHash -LiteralPath $archive -Algorithm SHA256).Hash.ToLowerInvariant()
if ($actual -ne $expectedSha256) { throw 'The GitHub CLI archive does not match the pinned official release digest.' }
Add-Type -AssemblyName System.IO.Compression.FileSystem
$zip = [IO.Compression.ZipFile]::OpenRead($archive)
try {
    foreach ($item in @(@{ Suffix = 'bin/gh.exe'; Destination = $exe }, @{ Suffix = 'LICENSE'; Destination = $license })) {
        $entries = @($zip.Entries | Where-Object { $_.FullName -eq $item.Suffix -or $_.FullName.EndsWith('/' + $item.Suffix) })
        if ($entries.Count -ne 1) { throw "Expected one public archive entry: $($item.Suffix)" }
        $inputStream = $entries[0].Open()
        $outputStream = [IO.File]::Create($item.Destination)
        try { $inputStream.CopyTo($outputStream) } finally { $outputStream.Dispose(); $inputStream.Dispose() }
    }
} finally { $zip.Dispose() }
[IO.File]::WriteAllText($stamp, $version, [Text.UTF8Encoding]::new($false))
Write-Output "Prepared official GitHub CLI $version and its MIT license. No login data was copied."
