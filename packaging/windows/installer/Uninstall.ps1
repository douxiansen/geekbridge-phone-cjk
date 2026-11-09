# SPDX-License-Identifier: GPL-3.0-or-later
<#
.SYNOPSIS
  Removes GeekBridge Phone, and optionally the certificate Install.ps1 trusted.
#>
$ErrorActionPreference = 'Stop'

Get-Process geekbridge-phone -ErrorAction SilentlyContinue | Stop-Process -Force
$pkg = Get-AppxPackage GeekBridge.Phone
if ($pkg) {
    $pkg | Remove-AppxPackage
    Write-Host 'GeekBridge Phone removed.'
} else {
    Write-Host 'GeekBridge Phone is not installed.'
}

$answer = Read-Host 'Also stop trusting the GeekBridge certificate? Needs administrator rights (y/N)'
if ($answer -match '^[yY]') {
    $cer = Join-Path (Split-Path -Parent $MyInvocation.MyCommand.Path) 'GeekBridge.cer'
    $thumb = if (Test-Path $cer) { (New-Object Security.Cryptography.X509Certificates.X509Certificate2 $cer).Thumbprint }
    if (-not $thumb) { Write-Host 'GeekBridge.cer not found next to this script; nothing removed.'; return }
    Start-Process powershell -Verb RunAs -Wait -ArgumentList @(
        '-NoProfile', '-Command',
        "Remove-Item Cert:\LocalMachine\TrustedPeople\$thumb -ErrorAction SilentlyContinue")
    Write-Host 'Certificate no longer trusted.'
}
Read-Host 'Press Enter to close'
