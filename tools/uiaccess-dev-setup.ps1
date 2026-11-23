# Development only: sign the candidate host with a self-signed certificate and put it in Program Files,
# so Windows runs it with UIAccess (its window can then appear above the taskbar search UI).
# Release builds are signed with a real certificate instead (docs/release.md); do not ship this certificate.
#
#   powershell -ExecutionPolicy Bypass -File tools\uiaccess-dev-setup.ps1            # set up (asks for admin)
#   powershell -ExecutionPolicy Bypass -File tools\uiaccess-dev-setup.ps1 -Remove    # undo everything
#
# What it does (admin):
#   1. creates "CN=Multilang IME Dev Signing" (code signing, private key not exportable) in your personal store
#   2. trusts its public part in LocalMachine\Root and TrustedPublisher (needed for UIAccess)
#   3. signs target\release\MultilangIME-CandWin.exe and copies it to C:\Program Files\MultilangIME\
# Anything signed with this key will be trusted on this PC, so remove it when you no longer need it (-Remove).
param([switch]$Remove, [string]$Exe)
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
if (-not $Exe) { $Exe = Join-Path $Root "target\release\MultilangIME-CandWin.exe" }
$Subject = "CN=Multilang IME Dev Signing"
$Dest = Join-Path $env:ProgramFiles "MultilangIME"
$Target = Join-Path $Dest "MultilangIME-CandWin.exe"

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin) {
    $argv = @("-ExecutionPolicy", "Bypass", "-NoExit", "-File", "`"$PSCommandPath`"", "-Exe", "`"$Exe`"")
    if ($Remove) { $argv += "-Remove" }
    Start-Process powershell -Verb RunAs -ArgumentList $argv
    return
}

function Find-Certs($store) { Get-ChildItem $store -ErrorAction SilentlyContinue | Where-Object { $_.Subject -eq $Subject } }

# stop a running host so the file can be replaced or removed
Get-Process "MultilangIME-CandWin" -ErrorAction SilentlyContinue | Stop-Process -Force

if ($Remove) {
    foreach ($s in "Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher", "Cert:\CurrentUser\My") {
        Find-Certs $s | ForEach-Object { Remove-Item $_.PSPath -DeleteKey -ErrorAction SilentlyContinue; Remove-Item $_.PSPath -ErrorAction SilentlyContinue; "removed certificate from $s" }
    }
    if (Test-Path $Target) { Remove-Item $Target -Force; "removed $Target" }
    "done"
    return
}

if (-not (Test-Path $Exe)) { throw "build it first: cargo build --release -p candhost ($Exe)" }

# 1. certificate
$cert = Find-Certs "Cert:\CurrentUser\My" | Where-Object { $_.HasPrivateKey } | Select-Object -First 1
if (-not $cert) {
    $cert = New-SelfSignedCertificate -Type CodeSigningCert -Subject $Subject -CertStoreLocation "Cert:\CurrentUser\My" `
        -KeyExportPolicy NonExportable -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 -NotAfter (Get-Date).AddYears(2)
    "created certificate $($cert.Thumbprint)"
}

# 2. trust (public part only)
$cer = Join-Path $env:TEMP "multilang-ime-dev-signing.cer"
Export-Certificate -Cert $cert -FilePath $cer | Out-Null
foreach ($s in "Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher") {
    if (-not (Find-Certs $s | Where-Object { $_.Thumbprint -eq $cert.Thumbprint })) {
        Import-Certificate -FilePath $cer -CertStoreLocation $s | Out-Null
        "trusted in $s"
    }
}
Remove-Item $cer -Force

# 3. sign and install
New-Item -ItemType Directory -Force $Dest | Out-Null
Copy-Item $Exe $Target -Force
$sig = Set-AuthenticodeSignature -FilePath $Target -Certificate $cert -HashAlgorithm SHA256
if ($sig.Status -ne "Valid") { throw "signing failed: $($sig.Status) $($sig.StatusMessage)" }
"signed and installed: $Target"

# restart the backend (it launches the host unelevated within a few seconds)
Get-Process backend -ErrorAction SilentlyContinue | Stop-Process -Force
Start-ScheduledTask -TaskName "MultilangIME Backend" -ErrorAction SilentlyContinue
""
"Done. Restart the search UI to load the new TIP: sign out and in, or end 'Search' (SearchHost.exe) in Task Manager."
