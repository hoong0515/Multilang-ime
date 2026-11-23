# Multilang IME installer (per machine). Run from the unpacked release folder:
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Dest <folder>] [-NoLanguageList] [-DryRun]
# Needs administrator rights (asks with UAC when started normally).
param(
    [string]$Dest = (Join-Path $env:ProgramFiles "MultilangIME"),
    [switch]$NoLanguageList,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$Src = $PSScriptRoot
$TaskName = "MultilangIME Backend"
$Clsid = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"
$ProfileKo = "{6E9DBD5F-4A03-40FA-8186-35DEDF98E150}"

function Step($msg, [scriptblock]$do) {
    Write-Host "- $msg"
    if (-not $DryRun) { & $do }
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin -and -not $DryRun) {
    $argv = @("-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Dest", "`"$Dest`"")
    if ($NoLanguageList) { $argv += "-NoLanguageList" }
    Start-Process powershell -Verb RunAs -ArgumentList $argv -Wait
    return
}

foreach ($f in @("multilang_tip.dll", "backend.exe", "engine_model.json", "hanja.txt", "x86\multilang_tip.dll", "mozc\mozc_server.exe")) {
    if (-not (Test-Path (Join-Path $Src $f))) { throw "missing in package: $f" }
}

# 1. Stop the running backend and Mozc started from an older install in $Dest
Step "stop old backend" {
    Get-Process backend, mozc_server -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($Dest, [StringComparison]::OrdinalIgnoreCase) } |
        Stop-Process -Force
}

# 2. Copy files. A DLL still loaded by a running app cannot be overwritten, but it can be
#    renamed; the old copy is deleted at the next install or reboot.
Step "copy files to $Dest" {
    New-Item -ItemType Directory -Force $Dest | Out-Null
    foreach ($dll in @("multilang_tip.dll", "x86\multilang_tip.dll")) {
        $p = Join-Path $Dest $dll
        Get-ChildItem "$p.old*" -ErrorAction SilentlyContinue | Remove-Item -Force -ErrorAction SilentlyContinue
        if (Test-Path $p) {
            try { Remove-Item $p -Force } catch { Rename-Item $p ("$p.old" + [DateTime]::Now.Ticks) }
        }
    }
    Get-ChildItem $Src -Exclude "install.ps1", "install.cmd" | Copy-Item -Destination $Dest -Recurse -Force
}

# 3. Sandboxed apps (taskbar search, Store apps) load the DLL and read the data files:
#    give ALL APPLICATION PACKAGES and ALL RESTRICTED APPLICATION PACKAGES read access.
Step "grant read access to app containers" {
    icacls $Dest /grant "*S-1-15-2-1:(OI)(CI)(RX)" /grant "*S-1-15-2-2:(OI)(CI)(RX)" /T /Q | Out-Null
}

# 4. Register the text service (64-bit and 32-bit)
Step "register 64-bit TIP" {
    $p = Start-Process "$env:windir\System32\regsvr32.exe" -ArgumentList "/s", "`"$Dest\multilang_tip.dll`"" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "regsvr32 (x64) failed: $($p.ExitCode)" }
}
Step "register 32-bit TIP" {
    $p = Start-Process "$env:windir\SysWOW64\regsvr32.exe" -ArgumentList "/s", "`"$Dest\x86\multilang_tip.dll`"" -Wait -PassThru
    if ($p.ExitCode -ne 0) { throw "regsvr32 (x86) failed: $($p.ExitCode)" }
}

# 5. Backend: start at logon for every user, as that user (not elevated)
Step "scheduled task '$TaskName'" {
    $exe = Join-Path $Dest "backend.exe"
    $action = New-ScheduledTaskAction -Execute $exe -WorkingDirectory $Dest
    $trigger = New-ScheduledTaskTrigger -AtLogOn
    $principal = New-ScheduledTaskPrincipal -GroupId "S-1-5-32-545" -RunLevel Limited
    $settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
        -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
    Register-ScheduledTask -TaskName $TaskName -Action $action -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
    Start-ScheduledTask -TaskName $TaskName -ErrorAction SilentlyContinue
}

# 6. Add the Korean profile to this user's input methods
if (-not $NoLanguageList) {
    Step "add input method to the language list" {
        $tip = "0412:$Clsid$ProfileKo"
        $list = Get-WinUserLanguageList
        $ko = $list | Where-Object { $_.LanguageTag -eq "ko" -or $_.LanguageTag -eq "ko-KR" } | Select-Object -First 1
        if (-not $ko) { $list.Add("ko-KR"); $ko = $list | Where-Object { $_.LanguageTag -like "ko*" } | Select-Object -First 1 }
        if (-not ($ko.InputMethodTips -contains $tip)) { $ko.InputMethodTips.Add($tip) }
        Set-WinUserLanguageList $list -Force
    }
}

Write-Host ""
Write-Host "Installed to $Dest. Apps that were already running need a restart to use the IME."
Write-Host "Tip: for Japanese text in Notepad, use a font with both Korean and Japanese glyphs (e.g. Noto Sans KR)."
