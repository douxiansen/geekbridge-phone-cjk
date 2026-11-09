# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
  Builds GeekBridge Phone and registers it as a package for development, so Windows lets it
  take phone calls (create the phone line and carry the voice).

.DESCRIPTION
  Needs Developer Mode (Settings > System > For developers). The package is registered from the
  folder this script fills, so no signing is involved. For other people's computers a signed
  package is needed instead; see docs/WINDOWS.md.

.EXAMPLE
  .\build-dev-package.ps1            # debug build
  .\build-dev-package.ps1 -Release   # optimised build
#>
param([switch]$Release)
$ErrorActionPreference = 'Stop'

$repo   = Resolve-Path "$PSScriptRoot\..\.."
$layout = Join-Path $PSScriptRoot 'layout'
$profileDir = if ($Release) { 'release' } else { 'debug' }

Write-Host "Building geekbridge-phone ($profileDir)..."
Push-Location $repo
try {
    if ($Release) { cargo build -p phone-app --release } else { cargo build -p phone-app }
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally { Pop-Location }

# Stop a running copy: it locks the package folder.
Get-Process geekbridge-phone -ErrorAction SilentlyContinue | Stop-Process -Force

New-Item -ItemType Directory -Force $layout | Out-Null
Copy-Item "$repo\target\$profileDir\geekbridge-phone.exe" $layout -Force
Copy-Item "$PSScriptRoot\AppxManifest.xml" $layout -Force
python "$PSScriptRoot\make-icons.py" "$layout\Assets"
if ($LASTEXITCODE -ne 0) { throw 'icon generation failed' }

if (Get-AppxPackage GeekBridge.Phone) {
    Get-AppxPackage GeekBridge.Phone | Remove-AppxPackage
}
Add-AppxPackage -Register "$layout\AppxManifest.xml"

$pkg = Get-AppxPackage GeekBridge.Phone
Write-Host ''
Write-Host "Installed: $($pkg.PackageFullName)"
Write-Host "Start it from the Start menu (GeekBridge Phone) or run:"
Write-Host "  explorer.exe shell:AppsFolder\$($pkg.PackageFamilyName)!App"
