# Full install / uninstall / reinstall check on this PC (release plan step 2: a, b, d).
#   Run in an elevated PowerShell as the signed-in user:
#   powershell -ExecutionPolicy Bypass -File tests\install-cycle.ps1 [-Package dist\MultilangIME-0.1.0] [-Language ko]
# Steps: (1) reinstall the package over the current install, (2) uninstall with the installed uninstall.ps1
# (user data kept), (3) install again. After each step the registry, task, shortcut, language list and files are
# checked. The report goes to dist\install-cycle-report.txt. Reboot cleanup (step c) is checked afterwards with
# -AfterReboot, which only reads state and changes nothing.
param(
    [string]$Package = (Join-Path (Split-Path $PSScriptRoot -Parent) "dist\MultilangIME-0.1.0"),
    [ValidateSet("ko", "ja", "en")][string]$Language = "ko",
    [switch]$AfterReboot
)
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
$Report = Join-Path $Root "dist\install-cycle-report.txt"
$Dest = Join-Path $env:ProgramFiles "MultilangIME"
$Clsid = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"
$TaskName = "MultilangIME Backend"
$UserData = Join-Path $env:LOCALAPPDATA "multilang-ime"
$script:fail = 0
$script:lines = @()

function Say($s) { Write-Host $s; $script:lines += $s }
function Check($name, [bool]$ok, $detail = "") {
    if (-not $ok) { $script:fail++ }
    Say ("  {0}  {1}{2}" -f $(if ($ok) { "ok  " } else { "FAIL" }), $name, $(if ($detail) { " ($detail)" } else { "" }))
}

function Get-State {
    $tips = @()
    foreach ($l in (Get-WinUserLanguageList)) { $tips += @($l.InputMethodTips | Where-Object { $_ -like "*$Clsid*" }) }
    $pending = @((Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name PendingFileRenameOperations -ErrorAction SilentlyContinue).PendingFileRenameOperations |
        Where-Object { $_ -and $_ -like "*MultilangIME*" })
    [pscustomobject]@{
        Folder    = Test-Path $Dest
        Files     = if (Test-Path $Dest) { @(Get-ChildItem $Dest -Recurse -File -Force).Count } else { 0 }
        Manifest  = Test-Path (Join-Path $Dest "install-manifest.txt")
        Clsid64   = Test-Path "HKLM:\SOFTWARE\Classes\CLSID\$Clsid"
        Clsid32   = Test-Path "HKLM:\SOFTWARE\WOW6432Node\Classes\CLSID\$Clsid"
        CtfTip    = Test-Path "HKLM:\SOFTWARE\Microsoft\CTF\TIP\$Clsid"
        CtfTip32  = Test-Path "HKLM:\SOFTWARE\WOW6432Node\Microsoft\CTF\TIP\$Clsid"
        UserTip   = Test-Path "HKCU:\Software\Microsoft\CTF\TIP\$Clsid"
        Task      = [bool](Get-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue)
        Shortcut  = Test-Path (Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\Multilang IME Settings.lnk")
        Arp       = Test-Path "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME"
        LangList  = $tips.Count
        UserData  = Test-Path $UserData
        Pending   = $pending.Count
        PendingAt = ($pending -join "; ")
        Backend   = @(Get-Process backend -ErrorAction SilentlyContinue | Where-Object { $_.Path -like "$Dest\*" }).Count
    }
}

function Check-Installed($s) {
    Check "install folder and manifest" ($s.Folder -and $s.Manifest) "$($s.Files) files"
    Check "COM registration 64-bit / 32-bit" ($s.Clsid64 -and $s.Clsid32)
    Check "TSF registration (HKLM CTF\TIP, 64-bit and 32-bit views)" ($s.CtfTip -and $s.CtfTip32)
    Check "scheduled task" $s.Task
    Check "start menu shortcut" $s.Shortcut
    Check "Programs and Features entry" $s.Arp
    Check "language list has the input method" ($s.LangList -ge 1) "$($s.LangList)"
    Check "user data kept" $s.UserData
}

function Check-Removed($s, [bool]$allowPending) {
    Check "COM registration 64-bit / 32-bit removed" (-not $s.Clsid64 -and -not $s.Clsid32)
    Check "TSF registration (HKLM CTF\TIP, 64-bit and 32-bit views) removed" (-not $s.CtfTip -and -not $s.CtfTip32)
    Check "per-user profile settings (HKCU CTF\TIP) removed" (-not $s.UserTip)
    Check "scheduled task removed" (-not $s.Task)
    Check "start menu shortcut removed" (-not $s.Shortcut)
    Check "Programs and Features entry removed" (-not $s.Arp)
    Check "language list entry removed" ($s.LangList -eq 0) "$($s.LangList)"
    Check "backend stopped" ($s.Backend -eq 0)
    Check "user data kept (no -RemoveUserData)" $s.UserData
    if ($allowPending) {
        Check "install folder gone, or only files waiting for reboot" ((-not $s.Folder) -or ($s.Pending -gt 0)) "folder=$($s.Folder) files=$($s.Files) pending=$($s.Pending)"
    } else {
        Check "install folder gone" (-not $s.Folder) "files=$($s.Files)"
        Check "no pending reboot deletes for our folder" ($s.Pending -eq 0) $s.PendingAt
    }
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
Say "Multilang IME install cycle check  $(Get-Date -Format 'yyyy-MM-dd HH:mm')  admin=$admin"

if ($AfterReboot) {
    Say "[after reboot] state only, nothing changed"
    $s = Get-State
    Say ($s | Format-List | Out-String).Trim()
    if ($s.Arp) { Check-Installed $s } else { Check-Removed $s $false }
} else {
    if (-not $admin) { throw "run this in an elevated PowerShell" }
    if (-not (Test-Path (Join-Path $Package "install.ps1"))) { throw "package not found: $Package" }

    Say "[0] before"
    Say ((Get-State) | Format-List | Out-String).Trim()

    Say "[1] install over the current install (or over what an uninstall left before the reboot)"
    & (Join-Path $Package "install.ps1") -Language $Language | Out-Null
    Check-Installed (Get-State)

    Say "[2] uninstall (installed uninstall.ps1, user data kept)"
    & (Join-Path $Dest "uninstall.ps1") | Out-Null
    $s = Get-State
    Say ($s | Format-List | Out-String).Trim()
    Check-Removed $s $true

    Say "[3] install again"
    & (Join-Path $Package "install.ps1") -Language $Language | Out-Null
    Start-ScheduledTask -TaskName $TaskName
    Start-Sleep -Seconds 3
    $s = Get-State
    Check-Installed $s
    Check "backend running" ($s.Backend -ge 1) "$($s.Backend)"
}

Say ""
Say $(if ($script:fail -eq 0) { "all checks passed" } else { "$($script:fail) check(s) FAILED" })
New-Item -ItemType Directory -Force (Split-Path $Report) | Out-Null
$script:lines | Set-Content -Path $Report -Encoding UTF8
Write-Host "report: $Report"
if ($script:fail) { exit 1 }
