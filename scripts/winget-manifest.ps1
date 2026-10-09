# Writes the winget manifests for a release (Windows Package Manager,
# https://github.com/microsoft/winget-pkgs). Installing through winget does
# not show the "Windows protected your PC" SmartScreen warning.
#
#   powershell -ExecutionPolicy Bypass -File scripts\winget-manifest.ps1 -Tag v1.1.0 -Dist dist -Out winget
#
# Then submit the folder (see docs/code-signing.md, "winget"):
#   wingetcreate submit --token <GitHub token> winget\manifests\i\IbrarAhmad\Syscura\1.1.0
param(
  [Parameter(Mandatory)] [string] $Tag,
  [string] $Dist = "dist",
  [string] $Out = "winget"
)
$ErrorActionPreference = "Stop"
$id = "IbrarAhmad.Syscura"
$version = $Tag.TrimStart("v")
$base = "https://github.com/hiibrarahmad/syscura/releases/download/$Tag"
$schema = "1.10.0"

$installers = @()
foreach ($a in @(@{ arch = "x64"; file = "Syscura-$version-setup.exe" }, @{ arch = "arm64"; file = "Syscura-$version-setup-arm64.exe" })) {
  $path = Join-Path $Dist $a.file
  if (-not (Test-Path $path)) { Write-Warning "$($a.file) is missing; skipping $($a.arch)"; continue }
  $hash = (Get-FileHash $path -Algorithm SHA256).Hash.ToUpper()
  $installers += @"
- Architecture: $($a.arch)
  InstallerUrl: $base/$($a.file)
  InstallerSha256: $hash
"@
}
if (-not $installers) { throw "no installers found in $Dist" }

$dir = Join-Path $Out "manifests\i\IbrarAhmad\Syscura\$version"
New-Item -ItemType Directory -Force $dir | Out-Null
$header = "# yaml-language-server: `$schema=https://aka.ms/winget-manifest"

@"
$header.version.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
DefaultLocale: en-US
ManifestType: version
ManifestVersion: $schema
"@ | Set-Content (Join-Path $dir "$id.yaml") -Encoding utf8

@"
$header.installer.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
InstallerType: nullsoft
Scope: machine
InstallModes:
- interactive
- silent
- silentWithProgress
UpgradeBehavior: install
ElevationRequirement: elevatesSelf
ReleaseDate: $(Get-Date -Format yyyy-MM-dd)
Installers:
$($installers -join "`n")
ManifestType: installer
ManifestVersion: $schema
"@ | Set-Content (Join-Path $dir "$id.installer.yaml") -Encoding utf8

@"
$header.defaultLocale.$schema.schema.json
PackageIdentifier: $id
PackageVersion: $version
PackageLocale: en-US
Publisher: Ibrar Ahmad
PublisherUrl: https://github.com/hiibrarahmad
PublisherSupportUrl: https://github.com/hiibrarahmad/syscura/issues
Author: Ibrar Ahmad
PackageName: Syscura
PackageUrl: https://github.com/hiibrarahmad/syscura
License: MIT
LicenseUrl: https://github.com/hiibrarahmad/syscura/blob/main/LICENSE
Copyright: Copyright (c) 2026 Ibrar Ahmad
ShortDescription: Free, tiny PC health and security guard that explains Windows problems and fixes what it safely can.
Description: |-
  Syscura watches Windows for software, hardware and security problems, explains them in plain language,
  and fixes what it safely can, verifying every fix. It checks Windows' security settings, looks for
  malware tricks antivirus misses, tracks drive health, and backs up your files. Free and open source.
Moniker: syscura
Tags:
- security
- monitoring
- repair
- health
- malware
- windows
ReleaseNotesUrl: https://github.com/hiibrarahmad/syscura/releases/tag/$Tag
ManifestType: defaultLocale
ManifestVersion: $schema
"@ | Set-Content (Join-Path $dir "$id.locale.en-US.yaml") -Encoding utf8

Get-ChildItem $dir | ForEach-Object { "wrote $($_.FullName)" }
