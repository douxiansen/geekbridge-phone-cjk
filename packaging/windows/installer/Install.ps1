# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
  Installs GeekBridge Phone on Windows 10 (2004 and later) or Windows 11.

.DESCRIPTION
  GeekBridge Phone is installed as a Windows package, because Windows only lets an installed app
  take phone calls and carry their voice through the computer.

  The package is signed with the project's own certificate, GeekBridge.cer, which is not one
  Windows already trusts. This script therefore:
    1. asks for administrator rights (one prompt),
    2. trusts GeekBridge.cer for packages (the "Trusted People" store of this computer),
    3. installs GeekBridge.Phone.msix.
  Nothing else is changed. Run Uninstall.ps1 to remove the app and, if you want, the certificate.

  Right-click this file and choose "Run with PowerShell", or run it from a terminal.
#>
$ErrorActionPreference = 'Stop'

$here = Split-Path -Parent $MyInvocation.MyCommand.Path
$msix = Join-Path $here 'GeekBridge.Phone.msix'
$cer  = Join-Path $here 'GeekBridge.cer'

foreach ($f in @($msix, $cer)) {
    if (-not (Test-Path $f)) { throw "Missing $(Split-Path -Leaf $f) next to this script." }
}
if ([Environment]::OSVersion.Version.Build -lt 19041) {
    throw 'GeekBridge Phone needs Windows 10 version 2004 (build 19041) or later.'
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()
         ).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    Write-Host 'Windows will ask for administrator rights to trust the GeekBridge certificate.'
    Start-Process powershell -Verb RunAs -Wait -ArgumentList @(
        '-NoProfile', '-ExecutionPolicy', 'Bypass', '-File', "`"$($MyInvocation.MyCommand.Path)`"")
    return
}

Write-Host 'Trusting the GeekBridge certificate...'
Import-Certificate -FilePath $cer -CertStoreLocation Cert:\LocalMachine\TrustedPeople | Out-Null

# A copy registered for development (build-dev-package.ps1) has the same identity and must go
# first; a normal earlier release is simply upgraded in place.
$old = Get-AppxPackage GeekBridge.Phone
if ($old -and $old.IsDevelopmentMode) {
    Get-Process geekbridge-phone -ErrorAction SilentlyContinue | Stop-Process -Force
    $old | Remove-AppxPackage
}

Write-Host 'Installing GeekBridge Phone...'
Add-AppxPackage -Path $msix

Write-Host ''
Write-Host 'Done. Start "GeekBridge Phone" from the Start menu.'
Write-Host 'First time: pair your phone from the app, then on the phone open Bluetooth settings,'
Write-Host 'tap the gear next to this computer and switch on Calls (call audio).'
Read-Host 'Press Enter to close'
