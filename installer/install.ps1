# Multilang IME installer (per machine). Run from the unpacked release folder:
#   powershell -ExecutionPolicy Bypass -File install.ps1 [-Language ko|ja|en] [-Dest <folder>] [-NoLanguageList] [-DryRun]
# Needs administrator rights (asks with UAC when started normally).
# -Language: the input language the IME is listed under (taskbar language label, and the language apps
#   use to pick a font). It types Korean, English and Japanese whichever is chosen. Default: ko.
param(
    [ValidateSet("ko", "ja", "en")][string]$Language = "ko",
    [string]$Dest = (Join-Path $env:ProgramFiles "MultilangIME"),
    [switch]$NoLanguageList,
    [switch]$DryRun
)
$ErrorActionPreference = "Stop"
$Src = $PSScriptRoot
$TaskName = "MultilangIME Backend"
$Clsid = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"
# language -> (LANGID, profile GUID, language tag) as registered by the TIP (tip/src/lib.rs PROFILES)
$Profiles = @{
    ko = @("0412", "{6E9DBD5F-4A03-40FA-8186-35DEDF98E150}", "ko-KR")
    ja = @("0411", "{B21CF121-71FB-4C56-8B91-3BAE60CB6E74}", "ja-JP")
    en = @("0409", "{681B9E1A-408F-4AF6-8EB3-AF2C9283A82F}", "en-US")
}

function Step($msg, [scriptblock]$do) {
    Write-Host "- $msg"
    if (-not $DryRun) { & $do }
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin -and -not $DryRun) {
    $argv = @("-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"", "-Dest", "`"$Dest`"", "-Language", $Language)
    if ($NoLanguageList) { $argv += "-NoLanguageList" }
    Start-Process powershell -Verb RunAs -ArgumentList $argv -Wait
    return
}

foreach ($f in @("multilang_tip.dll", "backend.exe", "MultilangIME-Settings.exe", "MultilangIME-CandWin.exe", "engine_model.json", "hanja.txt", "x86\multilang_tip.dll")) {
    if (-not (Test-Path (Join-Path $Src $f))) { throw "missing in package: $f" }
}

if (-not ((Test-Path (Join-Path $Src "mozc\mozc_stdio.exe")) -or (Test-Path (Join-Path $Src "mozc\mozc_server.exe")))) {
    throw "missing in package: mozc\mozc_stdio.exe (or mozc_server.exe)"
}

# --- Safety (installer\files.ps1): only a folder of our own, and inside it only the files we installed.
. (Join-Path $Src "files.ps1")
$Dest = Get-FullDir $Dest
$registered = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME" -ErrorAction SilentlyContinue).InstallLocation
$previous = Get-PreviousInstall $Dest $registered   # throws when $Dest is not ours
$script:booked = @()

# 1. Stop our processes running from $Dest (exact folder, not a prefix of another one)
Step "stop old backend" {
    Get-Process backend, mozc_server, mozc_stdio, MultilangIME-CandWin -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith("$Dest\", [StringComparison]::OrdinalIgnoreCase) } |
        Stop-Process -Force
}

# 2. Copy files (a DLL still loaded by an app is renamed and deleted at the next reboot); writes the manifest
Step "copy files to $Dest" {
    $script:booked = Install-PackageFiles $Src $Dest $previous @("install.ps1", "install.cmd")
}

# 3. Sandboxed apps (taskbar search, Store apps) load the DLL and read the data files:
#    give ALL APPLICATION PACKAGES and ALL RESTRICTED APPLICATION PACKAGES read access
#    (inherited by the files in this folder, which is ours alone: see the safety check above).
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

# 6. Start menu shortcut for the settings app (all users)
Step "start menu shortcut" {
    $sh = (New-Object -ComObject WScript.Shell).CreateShortcut((Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\Multilang IME Settings.lnk"))
    $sh.TargetPath = Join-Path $Dest "MultilangIME-Settings.exe"
    $sh.WorkingDirectory = $Dest
    $sh.Save()
}

# 7. Show the IME under the chosen language only (this user): enable that profile, disable the others,
#    and put it in that language's input methods (adding the language if it is missing).
# per-user settings (profile on/off, language list) go to the person signed in; when elevated as another
# administrator account they would land on that account, so they are skipped with a note
$perUser = Test-SameUserAsDesktop
if (-not $perUser) {
    Write-Host "Elevated as a different account than the signed-in user: skipping the per-user steps."
    Write-Host "The signed-in user can add Multilang IME in Settings > Time & language > Language."
}
if ($perUser) { Step "enable the '$Language' profile only" {
    foreach ($k in $Profiles.Keys) {
        $p = $Profiles[$k]
        $key = "HKCU:\Software\Microsoft\CTF\TIP\$Clsid\LanguageProfile\0x0000$($p[0])\$($p[1])"
        New-Item -Path $key -Force | Out-Null
        New-ItemProperty -Path $key -Name Enable -Value ([int]($k -eq $Language)) -PropertyType DWord -Force | Out-Null
    }
} }
if ($perUser -and -not $NoLanguageList) {
    Step "add input method to the language list ($($Profiles[$Language][2]))" {
        $p = $Profiles[$Language]
        $tip = "$($p[0]):$Clsid$($p[1])"
        $prefix = $p[2].Split("-")[0]
        $list = Get-WinUserLanguageList
        # ours only under the chosen language
        foreach ($l in $list) {
            foreach ($t in @($l.InputMethodTips | Where-Object { $_ -like "*$Clsid*" -and $_ -ne $tip })) { [void]$l.InputMethodTips.Remove($t) }
        }
        $lang = $list | Where-Object { $_.LanguageTag -eq $prefix -or $_.LanguageTag -like "$prefix-*" } | Select-Object -First 1
        if (-not $lang) { $list.Add($p[2]); $lang = $list | Where-Object { $_.LanguageTag -like "$prefix*" } | Select-Object -First 1 }
        if (-not ($lang.InputMethodTips -contains $tip)) { $lang.InputMethodTips.Add($tip) }
        Set-WinUserLanguageList $list -Force
    }
}

# 8. Programs and Features (Settings > Apps) entry, so it can be removed like any other app
Step "Programs and Features entry" {
    $key = "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME"
    $version = (Get-Item (Join-Path $Dest "multilang_tip.dll")).VersionInfo.ProductVersion
    $sizeKb = [int]((Get-ChildItem $Dest -Recurse -File | Measure-Object Length -Sum).Sum / 1KB)
    New-Item -Path $key -Force | Out-Null
    $values = @{
        DisplayName     = "Multilang IME"
        DisplayVersion  = $version
        Publisher       = "Multilang IME contributors"
        InstallLocation = $Dest
        DisplayIcon     = (Join-Path $Dest "MultilangIME-Settings.exe")
        UninstallString = "powershell.exe -NoProfile -ExecutionPolicy Bypass -File `"$(Join-Path $Dest 'uninstall.ps1')`""
    }
    foreach ($k in $values.Keys) { New-ItemProperty -Path $key -Name $k -Value $values[$k] -PropertyType String -Force | Out-Null }
    foreach ($k in "NoModify", "NoRepair") { New-ItemProperty -Path $key -Name $k -Value 1 -PropertyType DWord -Force | Out-Null }
    New-ItemProperty -Path $key -Name EstimatedSize -Value $sizeKb -PropertyType DWord -Force | Out-Null
}

Write-Host ""
Write-Host "Installed to $Dest. Apps that were already running need a restart to use the IME."
if ($script:booked.Count -gt 0) { Write-Host "$($script:booked.Count) old file(s) still in use will be deleted at the next reboot." }
Write-Host "Tip: for Japanese text in Notepad, use a font with both Korean and Japanese glyphs (e.g. Noto Sans KR)."
