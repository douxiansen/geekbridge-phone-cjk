# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
  Builds the signed Windows release: dist\windows\GeekBridge-Phone-<version>-windows-x64.zip

.DESCRIPTION
  Needs the Windows SDK (makeappx.exe, signtool.exe), Rust with the MSVC toolchain, Python 3, and
  a code signing certificate whose subject is the manifest's Publisher ("CN=GeekBridge").
  The certificate's private key stays in your certificate store; only the public GeekBridge.cer
  goes into the zip.

  Create the certificate once (valid three years):
    New-SelfSignedCertificate -Type Custom -Subject 'CN=GeekBridge' -KeyUsage DigitalSignature `
      -FriendlyName 'GeekBridge Phone' -CertStoreLocation Cert:\CurrentUser\My `
      -NotAfter (Get-Date).AddYears(3) `
      -TextExtension @('2.5.29.37={text}1.3.6.1.5.5.7.3.3', '2.5.29.19={text}')
  Back it up (certmgr.msc > Personal > export with the private key): updates must be signed by the
  same certificate.

.EXAMPLE
  .\build-release.ps1 -Version 0.1.4 -CertThumbprint <thumbprint>
#>
param(
    [Parameter(Mandatory)] [string]$Version,
    [Parameter(Mandatory)] [string]$CertThumbprint,
    # Fourth number of the package version. Raise it for a respin of the same release: Windows
    # refuses to install a changed package under an unchanged version.
    [int]$Revision = 0
)
$ErrorActionPreference = 'Stop'

$repo = Resolve-Path "$PSScriptRoot\..\.."
$out  = Join-Path $repo 'dist\windows'
$stage = Join-Path $out 'stage'
$kit = Get-ChildItem 'C:\Program Files (x86)\Windows Kits\10\bin' -Directory |
       Where-Object { Test-Path "$($_.FullName)\x64\makeappx.exe" } |
       Sort-Object Name -Descending | Select-Object -First 1
if (-not $kit) { throw 'Windows SDK (makeappx.exe) not found.' }
$makeappx = "$($kit.FullName)\x64\makeappx.exe"
$signtool = "$($kit.FullName)\x64\signtool.exe"

$cert = Get-Item "Cert:\CurrentUser\My\$CertThumbprint" -ErrorAction Stop
if ($cert.Subject -ne 'CN=GeekBridge') { throw "Certificate subject is $($cert.Subject); the manifest needs CN=GeekBridge." }

Write-Host 'Building (release, static runtime)...'
Push-Location $repo
try {
    $env:RUSTFLAGS = '-C target-feature=+crt-static'
    cargo build --release -p phone-app
    if ($LASTEXITCODE -ne 0) { throw 'cargo build failed' }
} finally { Remove-Item Env:RUSTFLAGS -ErrorAction SilentlyContinue; Pop-Location }

Remove-Item $out -Recurse -Force -ErrorAction SilentlyContinue
New-Item -ItemType Directory -Force $stage | Out-Null
Copy-Item "$repo\target\release\geekbridge-phone.exe" $stage
python "$PSScriptRoot\make-icons.py" "$stage\Assets"
if ($LASTEXITCODE -ne 0) { throw 'icon generation failed' }

# Manifest with the release version (four numbers).
$four = ($Version -split '[.-]')[0..2] -join '.'
$manifest = (Get-Content "$PSScriptRoot\AppxManifest.xml" -Raw) -replace 'Version="[0-9.]+" ProcessorArchitecture', "Version=`"$four.$Revision`" ProcessorArchitecture"
Set-Content "$stage\AppxManifest.xml" $manifest -Encoding UTF8

$msix = Join-Path $out 'GeekBridge.Phone.msix'
& $makeappx pack /d $stage /p $msix /o | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'makeappx failed' }
& $signtool sign /fd SHA256 /sha1 $CertThumbprint /tr http://timestamp.digicert.com /td SHA256 $msix | Out-Host
if ($LASTEXITCODE -ne 0) { throw 'signtool failed' }

$bundle = Join-Path $out 'bundle'
New-Item -ItemType Directory -Force $bundle | Out-Null
Copy-Item $msix $bundle
Export-Certificate -Cert $cert -FilePath "$bundle\GeekBridge.cer" | Out-Null
Copy-Item "$PSScriptRoot\installer\Install.ps1", "$PSScriptRoot\installer\Uninstall.ps1" $bundle
Copy-Item "$PSScriptRoot\installer\READ-ME-FIRST.txt" $bundle

$zip = Join-Path $out "GeekBridge-Phone-$Version-windows-x64.zip"
Compress-Archive -Path "$bundle\*" -DestinationPath $zip -Force
Remove-Item $stage, $bundle -Recurse -Force
(Get-FileHash $zip -Algorithm SHA256).Hash.ToLower() + "  " + (Split-Path -Leaf $zip) | Set-Content "$zip.sha256"
Write-Host ''
Write-Host "Built $zip"
