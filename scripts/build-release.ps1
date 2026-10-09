# Builds a Syscura release: the Setup installer plus a portable zip.
#
#   powershell -ExecutionPolicy Bypass -File scripts\build-release.ps1 [-Arch x64|arm64] [-Stage all|binaries|bundle|package]
#
# Output in dist\:
#   x64:   Syscura-<version>-setup.exe        Syscura-<version>-windows-x64.zip
#   arm64: Syscura-<version>-setup-arm64.exe  Syscura-<version>-windows-arm64.zip
#   SHA256SUMS.txt (Stage all only; the release workflow writes one for every file)
#
# The release workflow runs the stages one by one so the programs can be
# code-signed (SignPath) before they are packed into the installer, and the
# installer after that. Locally, -Stage all does everything in one go.
param(
  [ValidateSet("x64", "arm64")] [string] $Arch = "x64",
  [ValidateSet("all", "binaries", "bundle", "package")] [string] $Stage = "all"
)
$ErrorActionPreference = "Stop"
$root = Split-Path $PSScriptRoot -Parent
Set-Location $root

$version = (Get-Content ui\src-tauri\tauri.conf.json -Raw | ConvertFrom-Json).version
$triple = if ($Arch -eq "arm64") { "aarch64-pc-windows-msvc" } else { "x86_64-pc-windows-msvc" }
$release = "target\$triple\release"
Write-Host "Syscura $version for $Arch ($triple), stage $Stage"

if ($Stage -in "all", "binaries") {
  # 1. The agent and the command line tool.
  cargo build --release --target $triple -p syscura-agent -p syscura-cli
  if ($LASTEXITCODE) { throw "cargo build failed" }

  # 2. Tauri ships them next to Syscura.exe ("sidecars"); it expects the
  #    target triple in the file name.
  New-Item -ItemType Directory -Force ui\src-tauri\binaries | Out-Null
  Copy-Item "$release\syscura-agent.exe" "ui\src-tauri\binaries\syscura-agent-$triple.exe" -Force
  Copy-Item "$release\syscura-cli.exe" "ui\src-tauri\binaries\syscura-cli-$triple.exe" -Force

  # 3. The app itself (no installer yet).
  Push-Location ui
  npm ci
  if ($LASTEXITCODE) { Pop-Location; throw "npm ci failed" }
  npx tauri build --target $triple --no-bundle
  if ($LASTEXITCODE) { Pop-Location; throw "tauri build failed" }
  Pop-Location
}

if ($Stage -in "all", "bundle") {
  # 4. The installer, from the programs as they are now (signed, in CI).
  #    Signed sidecars come back as plain names; Tauri wants the triple.
  foreach ($n in "syscura-agent", "syscura-cli") {
    Copy-Item "$release\$n.exe" "ui\src-tauri\binaries\$n-$triple.exe" -Force
  }
  Push-Location ui
  npx tauri bundle --target $triple --bundles nsis
  if ($LASTEXITCODE) { Pop-Location; throw "tauri bundle failed" }
  Pop-Location
}

if ($Stage -in "all", "package") {
  # 5. Collect the release files.
  $out = Join-Path $root "dist"
  New-Item -ItemType Directory -Force $out | Out-Null
  # Files from other versions do not belong in this release.
  Get-ChildItem $out -File | Where-Object { $_.Name -notlike "*-$version-*" } | Remove-Item -Force
  $setupName = if ($Arch -eq "arm64") { "Syscura-$version-setup-arm64.exe" } else { "Syscura-$version-setup.exe" }
  $setup = Get-ChildItem "$release\bundle\nsis\*.exe" | Sort-Object LastWriteTime -Descending | Select-Object -First 1
  if (-not $setup) { throw "no installer in $release\bundle\nsis" }
  Copy-Item $setup.FullName (Join-Path $out $setupName) -Force

  $portable = Join-Path $env:TEMP "Syscura-$version-$Arch"
  Remove-Item $portable -Recurse -Force -ErrorAction SilentlyContinue
  New-Item -ItemType Directory $portable | Out-Null
  Copy-Item "$release\Syscura.exe", "$release\syscura-agent.exe", "$release\syscura-cli.exe", LICENSE, README.md $portable
  Compress-Archive -Path "$portable\*" -DestinationPath (Join-Path $out "Syscura-$version-windows-$Arch.zip") -Force

  if ($Stage -eq "all") {
    Get-ChildItem $out -File | Where-Object { $_.Name -notlike "SHA256SUMS*" } | ForEach-Object {
      "{0}  {1}" -f (Get-FileHash $_.FullName -Algorithm SHA256).Hash.ToLower(), $_.Name
    } | Set-Content (Join-Path $out "SHA256SUMS.txt") -Encoding ascii
  }
  Get-ChildItem $out | Format-Table Name, Length
}
