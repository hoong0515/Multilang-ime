# Multilang IME uninstaller.
#   powershell -ExecutionPolicy Bypass -File uninstall.ps1 [-RemoveUserData] [-DryRun]
# -RemoveUserData also deletes %LOCALAPPDATA%\multilang-ime (settings and learning) of the current user.
param([switch]$RemoveUserData, [switch]$DryRun)
$ErrorActionPreference = "Stop"
$Dest = [IO.Path]::GetFullPath($PSScriptRoot).TrimEnd('\')
$TaskName = "MultilangIME Backend"
$Clsid = "{05C49E35-3B3F-4BC3-8D48-EF1336F9CD4D}"
# the registered install folder (read before the Programs and Features entry is removed below);
# used only for an install made before install-manifest.txt existed, and only with our files present
$reg = (Get-ItemProperty "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME" -ErrorAction SilentlyContinue).InstallLocation
$script:registeredDest = $null
if ($reg -and (Test-Path (Join-Path $PSScriptRoot "backend.exe")) -and (Test-Path (Join-Path $PSScriptRoot "engine_model.json"))) {
    $script:registeredDest = [IO.Path]::GetFullPath($reg).TrimEnd('\')
}

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
Step "remove start menu shortcut" {
    Remove-Item (Join-Path $env:ProgramData "Microsoft\Windows\Start Menu\Programs\Multilang IME Settings.lnk") -Force -ErrorAction SilentlyContinue
}
Step "remove Programs and Features entry" {
    Remove-Item "HKLM:\SOFTWARE\Microsoft\Windows\CurrentVersion\Uninstall\MultilangIME" -Recurse -Force -ErrorAction SilentlyContinue
}
Step "remove scheduled task" {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
}
Step "stop backend and Mozc" {
    Get-Process backend, mozc_server, mozc_stdio, MultilangIME-CandWin -ErrorAction SilentlyContinue |
        Where-Object { $_.Path -and $_.Path.StartsWith("$Dest\", [StringComparison]::OrdinalIgnoreCase) } |
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

Step "remove per-user profile settings" {
    Remove-Item "HKCU:\Software\Microsoft\CTF\TIP\$Clsid" -Recurse -Force -ErrorAction SilentlyContinue
}

# --- Delete only what the installer put here (installer\files.ps1): the files in install-manifest.txt and
# their <name>.old<digits> copies. Nothing else in this folder (or anywhere else) is touched; folders go only
# when empty; files still loaded by apps are deleted at the next reboot.
. (Join-Path $PSScriptRoot "files.ps1")
$Manifest = Join-Path $Dest $ManifestName
$list = @()
if (Test-Path -LiteralPath $Manifest) {
    $list = @(Get-Content -LiteralPath $Manifest | Where-Object { Test-RelPath $_ })
} elseif ($script:registeredDest -and ($script:registeredDest -eq $Dest)) {
    $list = $Legacy010
}
$script:later = @()
if ($list.Count -gt 0 -and -not (Test-ForbiddenDest $Dest)) {
    Step "delete the installed files in $Dest" {
        $script:later = Remove-InstalledFiles $Dest $list
    }
}
Write-Host ""
if ($list.Count -eq 0) {
    Write-Host "Unregistered. No install manifest in $Dest, so no files were deleted."
} elseif ($script:later.Count -gt 0) {
    Write-Host "Removed. $($script:later.Count) file(s) still in use by running apps will be deleted at the next reboot."
} else {
    Write-Host "Removed."
}
if ((Test-Path -LiteralPath $Dest) -and $script:later.Count -eq 0 -and -not $DryRun) {
    $rest = @(Get-ChildItem -LiteralPath $Dest -Force -Recurse -File)
    if ($rest.Count -gt 0) { Write-Host "Left in place (not installed by Multilang IME): $($rest.Count) file(s) in $Dest" }
}
