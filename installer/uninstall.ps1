# Multilang IME uninstaller.
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-RemoveUserData] [-DryRun]
# -RemoveUserData also deletes %LOCALAPPDATA%\multilang-ime (settings and learning) of the current user.
param([switch]$RemoveUserData, [switch]$DryRun)
$ErrorActionPreference = "Stop"
$Dest = $PSScriptRoot
$TaskName = "MultilangIME Backend"
$Clsid = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"

function Step($msg, [scriptblock]$do) {
    Write-Host "- $msg"
    if (-not $DryRun) { & $do }
}

$admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole(
    [Security.Principal.WindowsBuiltInRole]::Administrator)
if (-not $admin -and -not $DryRun) {
    $argv = @("-ExecutionPolicy", "Bypass", "-File", "`"$PSCommandPath`"")
    if ($RemoveUserData) { $argv += "-RemoveUserData" }
    Start-Process powershell -Verb RunAs -ArgumentList $argv -Wait
    return
}

Step "remove input method from the language list" {
    $list = Get-WinUserLanguageList
    foreach ($l in $list) {
        foreach ($t in @($l.InputMethodTips | Where-Object { $_ -like "*$Clsid*" })) { [void]$l.InputMethodTips.Remove($t) }
    }
    Set-WinUserLanguageList $list -Force
}
Step "remove scheduled task" {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
}
Step "stop backend and Mozc" {
    Get-Process backend, mozc_server -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith($Dest, [StringComparison]::OrdinalIgnoreCase) } |
        Stop-Process -Force
}
Step "unregister TIP" {
    if (Test-Path "$Dest\x86\multilang_tip.dll") { Start-Process "$env:windir\SysWOW64\regsvr32.exe" -ArgumentList "/s", "/u", "`"$Dest\x86\multilang_tip.dll`"" -Wait }
    if (Test-Path "$Dest\multilang_tip.dll") { Start-Process "$env:windir\System32\regsvr32.exe" -ArgumentList "/s", "/u", "`"$Dest\multilang_tip.dll`"" -Wait }
}
if ($RemoveUserData) {
    Step "delete user data $env:LOCALAPPDATA\multilang-ime" {
        Remove-Item "$env:LOCALAPPDATA\multilang-ime" -Recurse -Force -ErrorAction SilentlyContinue
    }
}
Write-Host ""
Write-Host "Unregistered. Delete the folder $Dest after restarting apps (or after a reboot) if files are still in use."
