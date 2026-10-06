# Builds a Syscura release: the Setup installer plus a portable zip.
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1
# Output: dist\Syscura-<version>-setup.exe, dist\Syscura-<version>-windows-x64.zip, dist\SHA256SUMS.txt
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Get-Content ui\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
$triple = "x86_64-pc-windows-msvc"
Write-Host "Building Syscura $version"

# 1. The agent and the command line tool.
cargo build --release -p syscura-agent -p syscura-cli
if ($LASTEXITCODE) { throw "cargo build failed" }

# 2. Tauri ships them next to Syscura.exe ("sidecars"); it expects the
#    target triple in the file name.
New-Item -ItemType Directory -Force ui\src-tauri\binaries | Out-Null
Copy-Item target\release\syscura-agent.exe "ui\src-tauri\binaries\syscura-agent-$triple.exe" -Force
Copy-Item target\release\syscura-cli.exe "ui\src-tauri\binaries\syscura-cli-$triple.exe" -Force

# 3. The app and its installer.
Push-Location ui
npm ci
if ($LASTEXITCODE) { Pop-Location; throw "npm ci failed" }
npx tauri build
if ($LASTEXITCODE) { Pop-Location; throw "tauri build failed" }
Pop-Location

# 4. Collect the release files.
$out = Join-Path $root "dist"
Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $out | Out-Null
$setup = Get-ChildItem target\release\bundle\nsis\*.exe | Sort-Object LastWriteTime -Descending | Select-Object -First 1
Copy-Item $setup.FullName (Join-Path $out "Syscura-$version-setup.exe")

$portable = Join-Path $env:TEMP "Syscura-$version"
Remove-Item $portable -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory $portable | Out-Null
Copy-Item target\release\Syscura.exe, target\release\syscura-agent.exe, target\release\syscura-cli.exe, LICENSE, README.md $portable
Compress-Archive -Path "$portable\*" -DestinationPath (Join-Path $out "Syscura-$version-windows-x64.zip") -Force

Get-ChildItem $out -File | ForEach-Object {
  "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
} | Set-Content (Join-Path $out "SHA256SUMS.txt") -Encoding ascii
Get-ChildItem $out | Format-Table Name, Length
