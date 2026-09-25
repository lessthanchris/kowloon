# Builds a release and zips it for download: dist\Kowloon-<tag>-windows.zip
# Usage: powershell -File packaging\package.ps1 -Tag v0.1.0-alpha.1
param([Parameter(Mandatory = $true)][string]$Tag)
$ErrorActionPreference = 'Stop'
$root = Split-Path -Parent $PSScriptRoot
Set-Location $root

# A separate target dir, so a copy of the game that's running doesn't block the build.
cargo build --release -p kwc-app --target-dir target/dist
if ($LASTEXITCODE -ne 0) { throw 'build failed' }

$name = "Kowloon-$Tag-windows"
$stage = Join-Path $root "dist\$name"
Remove-Item -Recurse -Force $stage -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item target\dist\release\kowloon.exe $stage
Copy-Item packaging\README-ALPHA.txt (Join-Path $stage 'README.txt')

$zip = Join-Path $root "dist\$name.zip"
Remove-Item -Force $zip -ErrorAction SilentlyContinue
Compress-Archive -Path "$stage\*" -DestinationPath $zip
Write-Output $zip
