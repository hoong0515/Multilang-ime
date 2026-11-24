# Admin test for installer\local-signing.ps1 on this PC (needs an installed Multilang IME in Program Files).
# Copies the script into the install folder, enables, checks the certificate and the signature, disables, checks
# that everything is gone, and removes the copied script. The installed CandWin.exe stays signed (now untrusted);
# the next install replaces it.
#   powershell -ExecutionPolicy Bypass -File tests\local-signing.ps1   (from an administrator PowerShell)
$ErrorActionPreference = "Stop"
$reg = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME").InstallLocation
$script = Join-Path $reg "local-signing.ps1"
$hadScript = Test-Path $script
Copy-Item (Join-Path $PSScriptRoot "..\installer\local-signing.ps1") $script -Force
$fail = 0
function Check($name, $ok) { if ($ok) { "ok   $name" } else { "FAIL $name"; $script:fail++ } }
function Ours($s) { @(Get-ChildItem $s | Where-Object { $_.Subject -like "CN=Multilang IME Local Signing*" }) }
try {
    & $script -Enable -AcceptRisk "wrong phrase" | Out-Null
    Check "wrong phrase changes nothing" ((Ours "Cert:\LocalMachine\Root").Count -eq 0)
    & $script -Enable -AcceptRisk "I accept the risk on $env:COMPUTERNAME"
    $root = Ours "Cert:\LocalMachine\Root"
    Check "one trusted root" ($root.Count -eq 1)
    $c = $root[0]
    $eku = @($c.EnhancedKeyUsageList | ForEach-Object { $_.ObjectId })
    Check "EKU code signing only" (($eku.Count -eq 1) -and ($eku[0] -eq "1.3.6.1.5.5.7.3.3"))
    $bc = $c.Extensions | Where-Object { $_.Oid.Value -eq "2.5.29.19" }
    Check "not a CA (critical)" ($bc -and $bc.Critical -and -not $bc.CertificateAuthority)
    Check "no private key in root" (-not $c.HasPrivateKey)
    Check "no key left in My stores" (((Ours "Cert:\LocalMachine\My").Count + (Ours "Cert:\CurrentUser\My").Count) -eq 0)
    $sig = Get-AuthenticodeSignature (Join-Path $reg "MultilangIME-CandWin.exe")
    Check "CandWin signature valid" ($sig.Status -eq "Valid")
    Check "registry thumbprint" ((Get-ItemProperty "HKLM:\SOFTWARE\MultilangIME\LocalSigning").Thumbprint -eq $c.Thumbprint)
    & $script -Status
} finally {
    & $script -Disable
}
Check "root removed" ((Ours "Cert:\LocalMachine\Root").Count -eq 0)
Check "registry removed" (-not (Test-Path "HKLM:\SOFTWARE\MultilangIME"))
$sig = Get-AuthenticodeSignature (Join-Path $reg "MultilangIME-CandWin.exe")
Check "CandWin no longer trusted" ($sig.Status -ne "Valid")
if (-not $hadScript) { Remove-Item $script -Force }
""
if ($fail) { "$fail check(s) failed"; exit 1 } else { "all checks passed" }
