# Tests for installer\files.ps1 (what the installer and uninstaller may delete). Runs in a temp folder only;
# touches nothing else (no registration, no Program Files). Run without administrator rights.
#   powershell -ExecutionPolicy Bypass -File tests\installer-files.ps1
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
. (Join-Path $Root "installer\files.ps1")

$script:fails = 0
function Check([bool]$cond, [string]$what) {
    if ($cond) { Write-Host "  ok   $what" } else { Write-Host "  FAIL $what"; $script:fails++ }
}
function Throws([scriptblock]$b) { try { & $b; $false } catch { $true } }

$base = Join-Path ([IO.Path]::GetTempPath()) ("ml-files-test-" + [Guid]::NewGuid().ToString("N"))
New-Item -ItemType Directory $base | Out-Null
try {
    # a fake package
    $pkg = Join-Path $base "pkg"
    New-Item -ItemType Directory "$pkg\x86", "$pkg\mozc" | Out-Null
    foreach ($f in "backend.exe", "engine_model.json", "multilang_tip.dll", "x86\multilang_tip.dll", "mozc\mozc_stdio.exe", "uninstall.ps1", "install.ps1") {
        "v2 $f" | Set-Content (Join-Path $pkg $f)
    }

    Write-Host "1. manifest paths"
    Check (Test-RelPath "x86\multilang_tip.dll") "relative path accepted"
    foreach ($bad in "..\evil.txt", "x86\..\..\evil.txt", "C:\Windows\x.dll", "\Windows\x.dll", "a:b", "*.dll", "") {
        Check (-not (Test-RelPath $bad)) "rejected: '$bad'"
    }

    Write-Host "2. folders we refuse"
    foreach ($d in $env:windir, $env:ProgramFiles, $env:USERPROFILE, "C:\", $env:LOCALAPPDATA) {
        Check (Test-ForbiddenDest $d) "forbidden: $d"
    }
    Check (-not (Test-ForbiddenDest (Join-Path $env:ProgramFiles "MultilangIME"))) "allowed: Program Files\MultilangIME"

    Write-Host "3. a non-empty folder that is not ours is refused, and left untouched"
    $shared = Join-Path $base "shared"
    New-Item -ItemType Directory $shared | Out-Null
    "user data" | Set-Content "$shared\notes.txt"
    "user data" | Set-Content "$shared\backend.exe"
    Check (Throws { Get-PreviousInstall $shared $null }) "Get-PreviousInstall throws"
    Check ((Get-Content "$shared\notes.txt") -eq "user data" -and (Get-Content "$shared\backend.exe") -eq "user data") "files unchanged"
    Check (Throws { Get-PreviousInstall $shared (Join-Path $base "elsewhere") }) "registered elsewhere: still refused"

    Write-Host "4. fresh install into a new folder"
    $dest = Join-Path $base "MultilangIME"
    $prev = Get-PreviousInstall $dest $null
    Check ($prev.Count -eq 0) "no previous files"
    $booked = Install-PackageFiles $pkg $dest $prev @("install.ps1", "install.cmd")
    Check ($booked.Count -eq 0) "nothing booked"
    Check (Test-Path "$dest\x86\multilang_tip.dll") "files copied"
    Check (-not (Test-Path "$dest\install.ps1")) "install.ps1 not copied"
    $manifest = @(Get-Content "$dest\install-manifest.txt")
    Check ($manifest -contains "x86\multilang_tip.dll" -and $manifest -contains "install-manifest.txt" -and $manifest.Count -eq 7) "manifest lists 6 files + itself"

    Write-Host "5. update: user files and look-alikes survive, our old copies go, a file in use is renamed"
    "mine" | Set-Content "$dest\notes.txt"
    "mine" | Set-Content "$dest\notes.txt.old123"             # .old copy of a file that is not ours
    "mine" | Set-Content "$dest\backend.exe.old123.bak"       # not exactly <name>.old<digits>
    "old copy" | Set-Content "$dest\backend.exe.old637000000000000000"
    New-Item -ItemType Directory "$dest\logs" | Out-Null
    "log" | Set-Content "$dest\logs\tip-debug.log"
    foreach ($f in "backend.exe", "multilang_tip.dll") { "v3 $f" | Set-Content (Join-Path $pkg $f) }
    # a loaded DLL: open with read + delete sharing (rename allowed, overwrite not)
    $lock = [IO.File]::Open("$dest\multilang_tip.dll", 'Open', 'Read', 'Read, Delete')
    $prev = Get-PreviousInstall $dest $null
    Check ($prev.Count -eq 7) "previous = manifest"
    $null = Install-PackageFiles $pkg $dest $prev @("install.ps1", "install.cmd")
    $lock.Close()
    Check ((Get-Content "$dest\backend.exe") -eq "v3 backend.exe") "backend.exe updated"
    Check ((Get-Content "$dest\multilang_tip.dll") -eq "v3 multilang_tip.dll") "DLL in use replaced"
    Check (-not (Test-Path "$dest\backend.exe.old637000000000000000")) "our old copy removed"
    Check ((Get-Content "$dest\notes.txt") -eq "mine") "notes.txt kept"
    Check ((Get-Content "$dest\notes.txt.old123") -eq "mine") "notes.txt.old123 kept (not ours)"
    Check ((Get-Content "$dest\backend.exe.old123.bak") -eq "mine") "backend.exe.old123.bak kept (not our pattern)"
    Check (Test-Path "$dest\logs\tip-debug.log") "logs folder kept"

    Write-Host "5b. a DLL mapped into a process (like a loaded TIP) is renamed, the new one copied"
    Add-Type -Namespace MLTest -Name K32 -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern IntPtr LoadLibraryEx(string path, IntPtr file, int flags);
[DllImport("kernel32.dll")] public static extern bool FreeLibrary(IntPtr h);
'@
    Copy-Item "$env:windir\System32\version.dll" "$dest\x86\multilang_tip.dll" -Force   # stand-in DLL, mapped as an image only
    $h = [MLTest.K32]::LoadLibraryEx("$dest\x86\multilang_tip.dll", [IntPtr]::Zero, 0x20)  # LOAD_LIBRARY_AS_IMAGE_RESOURCE
    Check ($h -ne [IntPtr]::Zero) "stand-in DLL mapped"
    Check (Throws { Remove-Item -LiteralPath "$dest\x86\multilang_tip.dll" -Force -ErrorAction Stop }) "mapped DLL cannot be deleted"
    $prev = Get-PreviousInstall $dest $null
    $null = Install-PackageFiles $pkg $dest $prev @("install.ps1", "install.cmd")
    $olds = @(Get-ChildItem "$dest\x86" -Filter "multilang_tip.dll.old*")
    Check ((Get-Content "$dest\x86\multilang_tip.dll") -eq "v2 x86\multilang_tip.dll") "new DLL in place"
    Check ($olds.Count -eq 1) "old DLL renamed to multilang_tip.dll.old<ticks> (deleted at reboot as admin)"
    [void][MLTest.K32]::FreeLibrary($h)
    # next update cleans the renamed copy up once it is no longer in use
    $prev = Get-PreviousInstall $dest $null
    $null = Install-PackageFiles $pkg $dest $prev @("install.ps1", "install.cmd")
    Check (@(Get-ChildItem "$dest\x86" -Filter "multilang_tip.dll.old*").Count -eq 0) "renamed copy removed by the next update"

    Write-Host "5c. booking a delete at reboot passes a real NULL (no 'path not found')"
    $probe = Join-Path $base "probe.txt"; "x" | Set-Content $probe
    $ok = Register-DeleteAtReboot $probe
    $err = [Runtime.InteropServices.Marshal]::GetLastWin32Error()
    $admin = ([Security.Principal.WindowsPrincipal][Security.Principal.WindowsIdentity]::GetCurrent()).IsInRole([Security.Principal.WindowsBuiltInRole]::Administrator)
    if ($admin) { Check $ok "booked (admin)" } else { Check ((-not $ok) -and $err -eq 5) "not admin: refused with access denied (5), not path-not-found (3): got $err" }

    Write-Host "6. update drops a file the new version no longer ships (only if it was in the manifest)"
    Remove-Item "$pkg\mozc\mozc_stdio.exe"
    "mine" | Set-Content "$dest\mozc\user.txt"
    $prev = Get-PreviousInstall $dest $null
    $null = Install-PackageFiles $pkg $dest $prev @("install.ps1", "install.cmd")
    Check (-not (Test-Path "$dest\mozc\mozc_stdio.exe")) "dropped file removed"
    Check ((Get-Content "$dest\mozc\user.txt") -eq "mine") "user file in mozc\ kept"
    "v2" | Set-Content "$pkg\mozc\mozc_stdio.exe"

    Write-Host "7. a tampered manifest cannot reach outside the folder"
    $outside = Join-Path $base "outside.txt"
    "precious" | Set-Content $outside
    Add-Content "$dest\install-manifest.txt" "..\outside.txt"
    Add-Content "$dest\install-manifest.txt" $outside
    $list = @(Get-Content "$dest\install-manifest.txt")
    $null = Remove-InstalledFiles $dest $list
    Check ((Get-Content $outside) -eq "precious") "outside.txt kept"

    Write-Host "8. uninstall removed only our files; the folder stays because user files remain"
    Check (-not (Test-Path "$dest\backend.exe") -and -not (Test-Path "$dest\x86")) "our files and the empty x86 folder removed"
    Check (-not (Test-Path "$dest\install-manifest.txt")) "manifest removed"
    Check ((Test-Path "$dest\notes.txt") -and (Test-Path "$dest\notes.txt.old123") -and (Test-Path "$dest\backend.exe.old123.bak")) "user files kept"
    Check ((Test-Path "$dest\logs\tip-debug.log") -and (Test-Path "$dest\mozc\user.txt")) "user folders kept"
    Check (Test-Path $dest) "folder kept (not empty)"

    Write-Host "9. uninstall of a clean install removes the folder"
    $clean = Join-Path $base "Clean"
    $null = Install-PackageFiles $pkg $clean @() @("install.ps1", "install.cmd")
    $null = Remove-InstalledFiles $clean @(Get-Content "$clean\install-manifest.txt")
    Check (-not (Test-Path $clean)) "folder removed"

    Write-Host "9b. uninstall with a DLL in use renames it aside, so a reinstall before the reboot is not deleted then"
    $busy = Join-Path $base "Busy"
    $null = Install-PackageFiles $pkg $busy @() @("install.ps1", "install.cmd")
    Copy-Item "$env:windir\System32\version.dll" "$busy\x86\multilang_tip.dll" -Force
    $h2 = [MLTest.K32]::LoadLibraryEx("$busy\x86\multilang_tip.dll", [IntPtr]::Zero, 0x20)
    Check ($h2 -ne [IntPtr]::Zero) "stand-in DLL mapped"
    $booked = Remove-InstalledFiles $busy @(Get-Content "$busy\install-manifest.txt")
    Check (-not (Test-Path "$busy\x86\multilang_tip.dll")) "the DLL name is free after uninstall"
    Check (@(Get-ChildItem "$busy\x86" -Filter "multilang_tip.dll.old*").Count -eq 1) "the DLL in use was renamed to .old<ticks>"
    Check (@($booked | Where-Object { $_ -like "*\multilang_tip.dll" }).Count -eq 0) "the original name is not booked for the reboot"
    $null = Install-PackageFiles $pkg $busy @() @("install.ps1", "install.cmd")
    Check ((Get-Content "$busy\x86\multilang_tip.dll") -eq "v2 x86\multilang_tip.dll") "reinstalled DLL in place"
    if ($admin) {
        $pend = @((Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name PendingFileRenameOperations -ErrorAction SilentlyContinue).PendingFileRenameOperations)
        Check (@($pend | Where-Object { $_ -like "*\Busy\x86\multilang_tip.dll" }).Count -eq 0) "no reboot delete of the reinstalled DLL (admin)"
    }
    [void][MLTest.K32]::FreeLibrary($h2)
    $null = Remove-InstalledFiles $busy @(Get-Content "$busy\install-manifest.txt")

    Write-Host "10. legacy install (no manifest): only when it is the registered folder with our files"
    $legacy = Join-Path $base "Legacy"
    New-Item -ItemType Directory $legacy | Out-Null
    foreach ($f in "backend.exe", "engine_model.json") { "v1" | Set-Content "$legacy\$f" }
    "mine" | Set-Content "$legacy\keep.txt"
    $l = Get-PreviousInstall $legacy $legacy
    Check ($l -contains "multilang_tip.dll" -and $l -notcontains "keep.txt") "fixed 0.1.0 list used"
    Check (Throws { Get-PreviousInstall $legacy $null }) "not registered: refused"
    $null = Remove-InstalledFiles $legacy $l
    Check (-not (Test-Path "$legacy\backend.exe") -and (Test-Path "$legacy\keep.txt")) "legacy uninstall kept keep.txt"

    Write-Host "10b. junctions: a folder link inside the install folder cannot lead outside it"
    $victim = Join-Path $base "victim"
    New-Item -ItemType Directory $victim | Out-Null
    "precious" | Set-Content "$victim\multilang_tip.dll"
    "precious" | Set-Content "$victim\multilang_tip.dll.old123"
    $jdest = Join-Path $base "JDest"
    $null = Install-PackageFiles $pkg $jdest @() @("install.ps1", "install.cmd")
    Remove-Item "$jdest\x86" -Recurse -Force
    New-Item -ItemType Junction -Path "$jdest\x86" -Target $victim | Out-Null
    Check (-not (Test-SafeTarget $jdest "$jdest\x86\multilang_tip.dll")) "target through junction rejected"
    Check (Test-SafeTarget $jdest "$jdest\backend.exe") "plain target accepted"
    Check (-not (Test-SafeTarget $jdest "$base\outside.txt")) "target outside rejected"
    $null = Remove-InstalledFiles $jdest @(Get-Content "$jdest\install-manifest.txt")
    Check ((Get-Content "$victim\multilang_tip.dll") -eq "precious" -and (Test-Path "$victim\multilang_tip.dll.old123")) "uninstall did not delete through the junction"
    Check (Throws { Install-PackageFiles $pkg $jdest @() @("install.ps1", "install.cmd") }) "install refuses to write through the junction"
    Check ((Get-Content "$victim\multilang_tip.dll") -eq "precious") "victim file unchanged after refused install"
    # PowerShell 5.1 Remove-Item prompts (and hangs) on a junction; rmdir removes the link only, not its target
    cmd /c rmdir "$jdest\x86"
    $jlink = Join-Path $base "JLink"
    New-Item -ItemType Junction -Path $jlink -Target $victim | Out-Null
    Check (Test-ForbiddenDest $jlink) "install folder that is a junction: forbidden"
    cmd /c rmdir "$jlink"
    Check (Test-ForbiddenDest (Join-Path $env:windir "System32\MultilangIME")) "folder under Windows: forbidden"

    Write-Host "11. deleting in a forbidden folder is refused"
    Check (Throws { Remove-InstalledFiles $env:windir @("notepad.exe") }) "Remove-InstalledFiles refuses Windows folder"
}
finally {
    # unlink any junction left by a failed check first, so cleanup never follows a link
    Get-ChildItem $base -Recurse -Force -Directory -ErrorAction SilentlyContinue |
        Where-Object { $_.Attributes -band [IO.FileAttributes]::ReparsePoint } | ForEach-Object { cmd /c rmdir "$($_.FullName)" }
    cmd /c rmdir /s /q "$base"
}
Write-Host ""
if ($script:fails -gt 0) { Write-Host "$($script:fails) check(s) failed"; exit 1 } else { Write-Host "all checks passed" }
