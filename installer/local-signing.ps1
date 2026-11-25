# Optional, off by default: let the candidate window show above the taskbar search box on THIS PC by
# signing MultilangIME-CandWin.exe with a certificate made for this PC only.
#
# Releases are unsigned. Windows starts the candidate window helper with UIAccess (needed to draw above the
# search UI) only when it is signed by a trusted certificate. This script makes such a certificate here.
# It changes a security setting of this PC (a new trusted root certificate), so it is deliberately not in
# the setup wizard or the settings app: run it yourself from an ADMINISTRATOR PowerShell, read the notice,
# and type the confirmation line.
#
#   powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Status
#   powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Enable
#   powershell -ExecutionPolicy Bypass -File "C:\Program Files\MultilangIME\local-signing.ps1" -Disable
#
# Safeguards (docs/code-signing-policy.md, "Local signing"):
# - a new key for every run and every PC; nothing is shipped in the package
# - the private key is non-exportable and deleted right after signing: nothing is left that could sign again
# - the certificate allows code signing only (EKU) and is not a CA (basic constraints), so even a copy of the key
#   could not issue or act as a website (TLS) certificate
# - only the installed MultilangIME-CandWin.exe in the registered install folder (Program Files, admin-only) is signed
# - install (update) and uninstall remove the certificate; run -Enable again after an update
param([switch]$Enable, [switch]$Disable, [switch]$Status, [switch]$Quiet, [string]$AcceptRisk)
$ErrorActionPreference = "Stop"
$SubjectPrefix = "CN=Multilang IME Local Signing"
$Subject = "$SubjectPrefix ($env:COMPUTERNAME)"
$RegKey = "HKLM:\SOFTWARE\MultilangIME\LocalSigning"
$Exe = Join-Path $PSScriptRoot "MultilangIME-CandWin.exe"
$Phrase = "I accept the risk on $env:COMPUTERNAME"

function Say($m) { if (-not $Quiet) { Write-Host $m } }
function Find-Ours($store) {
    Get-ChildItem $store -ErrorAction SilentlyContinue | Where-Object { $_.Subject -like "$SubjectPrefix*" }
}
function Test-Admin {
    ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
        [Security.Principal.WindowsBuiltInRole]::Administrator)
}

function Remove-LocalSigning {
    $n = 0
    foreach ($s in "Cert:\LocalMachine\Root", "Cert:\LocalMachine\TrustedPublisher", "Cert:\LocalMachine\My", "Cert:\CurrentUser\My") {
        foreach ($c in @(Find-Ours $s)) {
            # -DeleteKey for a copy that still has its private key (an interrupted -Enable)
            if ($c.HasPrivateKey) { Remove-Item $c.PSPath -DeleteKey -ErrorAction SilentlyContinue }
            if (Test-Path $c.PSPath) { Remove-Item $c.PSPath -ErrorAction SilentlyContinue }
            $n++
        }
    }
    if (Test-Path $RegKey) {
        Remove-Item $RegKey -Force
    }
    # the empty HKLM\SOFTWARE\MultilangIME left behind: tidy-up only, never a reason to fail (an install or
    # update calls this; removing it was refused with "registry access not allowed" when run from setup.exe)
    $parent = Split-Path $RegKey
    try {
        $k = Get-Item $parent -ErrorAction Stop
        if ($k.SubKeyCount -eq 0 -and $k.ValueCount -eq 0) { Remove-Item $parent -Force -ErrorAction Stop }
    } catch {
        if (Test-Path $parent) { Say "  (left the empty key $parent : $($_.Exception.Message))" }
    }
    return $n
}

function Show-Status {
    $roots = @(Find-Ours "Cert:\LocalMachine\Root")
    if ($roots.Count -eq 0) { "Local signing: off (no Multilang IME certificate is trusted on this PC)." }
    foreach ($c in $roots) { "Local signing: ON. Trusted root '$($c.Subject)', thumbprint $($c.Thumbprint), valid until $($c.NotAfter.ToString('yyyy-MM-dd'))." }
    $keys = @(Find-Ours "Cert:\LocalMachine\My") + @(Find-Ours "Cert:\CurrentUser\My") | Where-Object { $_.HasPrivateKey }
    if ($keys.Count -gt 0) { "WARNING: a private key is still present (interrupted run). Run -Disable." }
    if (Test-Path $Exe) {
        $sig = Get-AuthenticodeSignature -FilePath $Exe
        "MultilangIME-CandWin.exe: $($sig.Status)$(if ($sig.SignerCertificate) { ' (' + $sig.SignerCertificate.Subject + ')' })"
    }
}

if ($Status) { Show-Status; return }

if (-not ($Enable -or $Disable)) {
    Write-Host "Use -Status, -Enable or -Disable (see the top of this script)."
    exit 2
}
if (-not (Test-Admin)) {
    # no automatic elevation, on purpose
    Write-Host "Run this from an administrator PowerShell (right-click PowerShell > Run as administrator)."
    exit 1
}

if ($Disable) {
    Get-Process "MultilangIME-CandWin" -ErrorAction SilentlyContinue | Stop-Process -Force
    $n = Remove-LocalSigning
    Say "Removed $n Multilang IME local signing certificate(s). The candidate window is hidden again in the taskbar search box."
    return
}

# --- Enable
# only the registered install, in a folder only administrators can write to
$reg = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME" -ErrorAction SilentlyContinue).InstallLocation
$here = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')
if (-not $reg -or ([IO.Path]::GetFullPath($reg).TrimEnd('\') -ne $here)) {
    Write-Host "Run the copy in the Multilang IME install folder$(if ($reg) { " ($reg)" }). Nothing was changed."
    exit 1
}
$pf = @($env:ProgramFiles, ${env:ProgramFiles(x86)}) | Where-Object { $_ } | ForEach-Object { [IO.Path]::GetFullPath($_).TrimEnd('\') + '\' }
if (-not ($pf | Where-Object { $here.StartsWith($_, [StringComparison]::OrdinalIgnoreCase) })) {
    Write-Host "The install folder is not under Program Files; Windows would not grant UIAccess there. Nothing was changed."
    exit 1
}
if (-not (Test-Path -LiteralPath $Exe)) { Write-Host "Missing: $Exe"; exit 1 }
$ver = (Get-Item -LiteralPath $Exe).VersionInfo
if ($ver.ProductName -ne "Multilang IME") { Write-Host "Unexpected file (product name '$($ver.ProductName)'): $Exe. Nothing was changed."; exit 1 }
$cur = Get-AuthenticodeSignature -FilePath $Exe
if ($cur.SignerCertificate -and $cur.SignerCertificate.Subject -notlike "$SubjectPrefix*") {
    Write-Host "MultilangIME-CandWin.exe is already signed by '$($cur.SignerCertificate.Subject)'. Nothing to do."
    return
}

Write-Host @"

==================================================================================================
 SECURITY NOTICE - read before you continue / 보안 안내 - 계속하기 전에 읽으세요
==================================================================================================
 This adds a NEW TRUSTED ROOT CERTIFICATE to this PC (Local Machine) and uses it to sign
 MultilangIME-CandWin.exe, so the candidate window can appear above the taskbar search box.

 Risks:
  - Any file signed with this certificate's key is trusted by this PC. The key is made here, cannot be
    exported, and is deleted right after signing, but for a few seconds it exists in this session.
  - The signed helper runs with UIAccess: it can draw above and send input to other windows, including
    administrator windows. A bug in it could be misused by other programs on this PC.
  - Security software or company policy may flag or block a new root certificate.
 Limits on the certificate: code signing only, not a certificate authority, this PC only.
 Undo at any time with -Disable. Updating or uninstalling Multilang IME removes it too.
 Without this, typing in the search box still works; only the candidate window stays hidden.

 이 PC에 새 "신뢰할 수 있는 루트 인증서"를 추가하고, 그 인증서로 MultilangIME-CandWin.exe에 서명합니다.
  - 이 키로 서명된 파일은 이 PC에서 신뢰됩니다. 키는 내보낼 수 없고 서명 직후 지우지만, 몇 초 동안은 존재합니다.
  - 서명된 후보창 프로그램은 UIAccess 권한으로 관리자 창을 포함한 다른 창 위에 그리고 입력을 보낼 수 있습니다.
  - 백신이나 회사 정책이 새 루트 인증서를 막거나 경고할 수 있습니다.
 -Disable로 언제든 되돌립니다. 업데이트·제거 때도 지워집니다. 이 설정 없이도 검색창 입력은 됩니다.
==================================================================================================

"@
$answer = $AcceptRisk
if (-not $answer) { $answer = Read-Host "To continue, type exactly: $Phrase" }
if ($answer -cne $Phrase) { Write-Host "Not confirmed. Nothing was changed."; exit 1 }

# start clean: an earlier certificate (another run) is removed first
[void](Remove-LocalSigning)
Get-Process "MultilangIME-CandWin" -ErrorAction SilentlyContinue | Stop-Process -Force

$cert = $null
try {
    $cert = New-SelfSignedCertificate -Subject $Subject -CertStoreLocation "Cert:\LocalMachine\My" `
        -KeyUsage DigitalSignature -KeyExportPolicy NonExportable -KeyAlgorithm RSA -KeyLength 3072 -HashAlgorithm SHA256 `
        -NotAfter (Get-Date).AddYears(3) -FriendlyName "Multilang IME candidate window (this PC only)" `
        -TextExtension @("2.5.29.37={critical}{text}1.3.6.1.5.5.7.3.3", "2.5.29.19={critical}{text}ca=0")
    # trust the public part only
    $cer = Join-Path $env:TEMP ("mlime-" + [guid]::NewGuid().ToString("N") + ".cer")
    Export-Certificate -Cert $cert -FilePath $cer | Out-Null
    try { $root = Import-Certificate -FilePath $cer -CertStoreLocation "Cert:\LocalMachine\Root" } finally { Remove-Item $cer -Force }
    $sig = Set-AuthenticodeSignature -FilePath $Exe -Certificate $cert -HashAlgorithm SHA256
    if ($sig.Status -ne "Valid") { throw "signing failed: $($sig.Status) $($sig.StatusMessage)" }
} catch {
    [void](Remove-LocalSigning)
    throw
} finally {
    # the private key goes away in every case
    if ($cert -and (Test-Path $cert.PSPath)) { Remove-Item $cert.PSPath -DeleteKey }
}
if (@(Find-Ours "Cert:\LocalMachine\My").Count -gt 0) { throw "private key was not removed; run -Disable" }
New-Item -Path $RegKey -Force | Out-Null
New-ItemProperty -Path $RegKey -Name Thumbprint -Value $root.Thumbprint -PropertyType String -Force | Out-Null

Say "Signed $Exe for this PC (thumbprint $($root.Thumbprint)); the private key has been deleted."
Say "The backend starts the candidate window within a minute. Sign out and in (or restart 'Search') if the search box was open."
