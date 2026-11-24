# File handling shared by install.ps1 and uninstall.ps1 (dot-sourced). Tested by tests\installer-files.ps1.
#
# Safety rules:
# - The install folder must be a folder of our own: new, empty, or an earlier Multilang IME install
#   (it has install-manifest.txt, or it is the folder registered in Programs and Features with our files).
# - We delete only what the manifest lists (plus <name>.old<digits> copies of those files), never by
#   listing a folder. Manifest lines must be plain relative paths inside the folder.
# - Folders are removed only when they end up empty.
# - Files still loaded by running apps (the TIP DLLs) are renamed and booked for deletion at the next
#   reboot (MoveFileEx MOVEFILE_DELAY_UNTIL_REBOOT; needs administrator rights).
# - A file that is already there with the same content is left alone (the TIP stub stays put while apps use
#   it), and the TIP pointer files (*.impl) are written last, after the implementation they name.

# The new name is an IntPtr so that a real NULL reaches the API: PowerShell turns $null into "" for a
# [string] parameter, and MoveFileEx(path, "", DELAY_UNTIL_REBOOT) fails with "path not found".
if (-not ("MultilangIME.NativeV2" -as [type])) {
    Add-Type -Namespace MultilangIME -Name NativeV2 -MemberDefinition @'
[DllImport("kernel32.dll", SetLastError = true, CharSet = CharSet.Unicode)]
public static extern bool MoveFileEx(string existing, IntPtr replacement, int flags);
'@
}

# Book a file or (empty) folder for deletion at the next reboot. Needs administrator rights.
function Register-DeleteAtReboot([string]$path) {
    # already booked (an earlier install or uninstall): do not add the same entry again
    $pending = @((Get-ItemProperty "HKLM:\SYSTEM\CurrentControlSet\Control\Session Manager" -Name PendingFileRenameOperations -ErrorAction SilentlyContinue).PendingFileRenameOperations)
    # (Windows 11 writes delete entries as "*1\??\<path>")
    if (($pending -contains "\??\$path") -or ($pending -contains "*1\??\$path")) { return $true }
    [MultilangIME.NativeV2]::MoveFileEx($path, [IntPtr]::Zero, 4)  # MOVEFILE_DELAY_UNTIL_REBOOT
}

$ManifestName = "install-manifest.txt"

# Files of the 0.1.0 package, for an install made before the manifest existed
$Legacy010 = @("LICENSE.txt", "MultilangIME-CandWin.exe", "MultilangIME-Settings.exe", "README.md", "THIRD-PARTY-NOTICES.md",
    "backend.exe", "engine_model.json", "hanja.txt", "licenses\mozc-LICENSE.txt", "mozc\documents\credits_en.html",
    "mozc\mozc_stdio.exe", "mozc\mozc_server.exe", "mozc\msvcp140.dll", "mozc\msvcp140_1.dll", "mozc\msvcp140_2.dll",
    "mozc\vcruntime140.dll", "mozc\vcruntime140_1.dll", "multilang_tip.dll", "uninstall.cmd", "uninstall.ps1", "x86\multilang_tip.dll")

function Get-FullDir([string]$p) { [IO.Path]::GetFullPath($p).TrimEnd('\') }

# A manifest line: a plain relative path inside the folder (no drive, no root, no "..", no wildcards)
function Test-RelPath([string]$rel) {
    [bool]($rel -and -not [IO.Path]::IsPathRooted($rel) -and $rel -notmatch '(^|[\\/])\.\.([\\/]|$)' -and
        $rel -notmatch '[:*?"<>|]' -and $rel -notmatch '^[\\/]')
}

# Folders we never install into (or delete in): a drive root, these folders themselves, anything under
# Windows, and a folder that is a junction or symbolic link (it may point anywhere).
function Test-ForbiddenDest([string]$dest) {
    $dest = Get-FullDir $dest
    $forbidden = @($env:windir, $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:ProgramData, $env:USERPROFILE,
        $env:SystemDrive, $env:LOCALAPPDATA, $env:APPDATA, [Environment]::GetFolderPath("Desktop"),
        [Environment]::GetFolderPath("MyDocuments")) | Where-Object { $_ } | ForEach-Object { Get-FullDir $_ }
    if ($dest.Length -le 3 -or ($forbidden -contains $dest)) { return $true }
    $win = Get-FullDir $env:windir
    if ($dest.StartsWith("$win\", [StringComparison]::OrdinalIgnoreCase)) { return $true }
    $item = Get-Item -LiteralPath $dest -Force -ErrorAction SilentlyContinue
    if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $true }
    return $false
}

# A path we may write or delete: inside $dest, and no folder between $dest and it (nor the file itself)
# is a junction or symbolic link, so a manifest entry cannot lead outside the install folder.
function Test-SafeTarget([string]$dest, [string]$path) {
    $dest = Get-FullDir $dest
    $full = [IO.Path]::GetFullPath($path)
    if (-not $full.StartsWith("$dest\", [StringComparison]::OrdinalIgnoreCase)) { return $false }
    $p = $full
    while ($p.Length -gt $dest.Length) {
        $item = Get-Item -LiteralPath $p -Force -ErrorAction SilentlyContinue
        if ($item -and ($item.Attributes -band [IO.FileAttributes]::ReparsePoint)) { return $false }
        $p = Split-Path $p
    }
    return $true
}

# The user signed in at the console (owner of explorer.exe in this session), "DOMAIN\name".
# When an installer is elevated with a different administrator account, per-user settings
# (language list, HKCU) would land on that account, so the scripts skip them and say so.
function Get-InteractiveUser {
    $sid = (Get-Process -Id $PID).SessionId
    $ex = Get-CimInstance Win32_Process -Filter "Name='explorer.exe'" -ErrorAction SilentlyContinue |
        Where-Object { $_.SessionId -eq $sid } | Select-Object -First 1
    if (-not $ex) { return $null }
    $o = Invoke-CimMethod -InputObject $ex -MethodName GetOwner -ErrorAction SilentlyContinue
    if ($o -and $o.User) { return "$($o.Domain)\$($o.User)" }
    return $null
}

function Test-SameUserAsDesktop {
    $me = [Security.Principal.WindowsIdentity]::GetCurrent().Name
    $desk = Get-InteractiveUser
    (-not $desk) -or ($desk -ieq $me)
}

# Does $dest hold only what an uninstall leaves before the reboot: <one of our files>.old<digits>, and the
# diagnostic files (MULTILANG_TIP_DEBUG, tip-debug.log, logs\)? Folders count when they hold only such files.
function Test-OnlyLeftovers([string]$dest) {
    $dest = Get-FullDir $dest
    # (plus the TIP implementation, which lives in impl\<hash>\ since the stub/impl split)
    $ours = @(@($Legacy010 | ForEach-Object { Split-Path $_ -Leaf }) + "multilang_tip_impl.dll") | Select-Object -Unique
    $files = @(Get-ChildItem -LiteralPath $dest -Force -Recurse -File -ErrorAction SilentlyContinue)
    if ($files.Count -eq 0) { return $false }
    foreach ($f in $files) {
        $rel = $f.FullName.Substring($dest.Length).TrimStart('\')
        $isOld = ($f.Name -match '^(.+)\.old\d+$') -and ($ours -contains $Matches[1])
        $isDiag = ($rel -eq "MULTILANG_TIP_DEBUG") -or ($rel -eq "tip-debug.log") -or ($rel -like "logs\*")
        if (-not ($isOld -or $isDiag)) { return $false }
    }
    return $true
}

# The relative paths we installed before into $dest (empty for a new or empty folder).
# Throws when $dest is not ours. $registered = InstallLocation from Programs and Features (or $null).
function Get-PreviousInstall([string]$dest, [string]$registered) {
    $dest = Get-FullDir $dest
    if (Test-ForbiddenDest $dest) { throw "refusing to use $dest (choose a folder of its own)" }
    $manifest = Join-Path $dest $ManifestName
    if (Test-Path -LiteralPath $manifest) {
        return @(Get-Content -LiteralPath $manifest | Where-Object { Test-RelPath $_ })
    }
    $isLegacy = $registered -and ((Get-FullDir $registered) -eq $dest) -and
        (Test-Path -LiteralPath (Join-Path $dest "backend.exe")) -and (Test-Path -LiteralPath (Join-Path $dest "engine_model.json"))
    if ($isLegacy) { return $Legacy010 }
    # left over from an uninstall before the reboot: only renamed copies of our files waiting for the reboot
    # and diagnostic logs. Install over them (they are not deleted here; the reboot or the next install does).
    if ((Test-Path -LiteralPath $dest) -and (Test-OnlyLeftovers $dest)) { return @() }
    if ((Test-Path -LiteralPath $dest) -and @(Get-ChildItem -LiteralPath $dest -Force).Count -gt 0) {
        throw "$dest is not empty and is not a Multilang IME install folder; choose an empty folder"
    }
    return @()
}

# Delete now, or book for deletion at the next reboot. Returns "deleted", "booked" or "failed".
function Remove-NowOrAtReboot([string]$path) {
    try { Remove-Item -LiteralPath $path -Force -ErrorAction Stop; return "deleted" }
    catch {
        if (Register-DeleteAtReboot $path) { return "booked" }
        return "failed"
    }
}

# Delete one of our files; if it is in use, rename it to <name>.old<ticks> first and book that copy for the
# reboot. Booking the original name instead would delete a file a reinstall puts there before the reboot.
# Returns the booked path, or "" when the file was deleted (or nothing could be booked).
function Remove-OrMoveAside([string]$path) {
    try { Remove-Item -LiteralPath $path -Force -ErrorAction Stop; return "" }
    catch {
        $old = "$path.old" + [DateTime]::Now.Ticks
        try { Rename-Item -LiteralPath $path (Split-Path $old -Leaf) -ErrorAction Stop }
        catch { $old = $path }   # cannot even rename: book the original name (nothing reinstalled can be in its way)
        if (Register-DeleteAtReboot $old) { return $old }
        return ""
    }
}

# <name>.old<digits> copies of one of our files, in the same folder
function Get-OldCopies([string]$dest, [string]$rel) {
    $path = Join-Path $dest $rel
    $dir = Split-Path $path
    $leaf = Split-Path $path -Leaf
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) { return @() }
    @(Get-ChildItem -LiteralPath $dir -File -Force -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match ('^' + [regex]::Escape($leaf) + '\.old\d+$') } | ForEach-Object { $_.FullName })
}

# Old TIP implementation folders (impl\<16 hex>\ under $dest and $dest) that an earlier update left
# behind: renamed copies (multilang_tip_impl.dll.old<digits>) that are no longer in use, then empty folders.
# Matches only that exact pattern, so it cannot reach other files. Returns paths booked for reboot.
function Remove-StaleImpl([string]$dest) {
    $booked = @()
    foreach ($root in (Join-Path $dest "impl"), (Join-Path $dest "x86\impl")) {
        if (-not (Test-Path -LiteralPath $root -PathType Container) -or -not (Test-SafeTarget $dest $root)) { continue }
        foreach ($d in @(Get-ChildItem -LiteralPath $root -Directory -Force | Where-Object { $_.Name -match '^[0-9a-f]{16}$' })) {
            if (-not (Test-SafeTarget $dest $d.FullName)) { continue }
            foreach ($f in @(Get-ChildItem -LiteralPath $d.FullName -File -Force | Where-Object { $_.Name -match '^multilang_tip_impl\.dll\.old\d+$' })) {
                if ((Remove-NowOrAtReboot $f.FullName) -eq "booked") { $booked += $f.FullName }
            }
            if (@(Get-ChildItem -LiteralPath $d.FullName -Force).Count -eq 0) {
                Remove-Item -LiteralPath $d.FullName -Force -ErrorAction SilentlyContinue
            }
        }
    }
    return , $booked
}

# Copy the package into $dest, replacing our earlier files; writes the manifest. Returns paths booked for reboot.
function Install-PackageFiles([string]$src, [string]$dest, [string[]]$previous, [string[]]$exclude) {
    $src = Get-FullDir $src
    $dest = Get-FullDir $dest
    if (Test-ForbiddenDest $dest) { throw "refusing to install into $dest" }
    $booked = @()
    # pointer files (*.impl) last: a TIP loading while we copy must not find a pointer to a missing file
    $files = @(Get-ChildItem -LiteralPath $src -Recurse -File | Where-Object { $exclude -notcontains $_.Name } |
        Sort-Object { $_.Extension -eq ".impl" })
    $newList = @($files | ForEach-Object { $_.FullName.Substring($src.Length).TrimStart('\') })
    New-Item -ItemType Directory -Force $dest | Out-Null
    # old copies of our files, and files of the previous version that this one no longer ships
    foreach ($rel in (@($previous) + $newList | Select-Object -Unique)) {
        foreach ($o in (Get-OldCopies $dest $rel)) {
            if ((Test-SafeTarget $dest $o) -and (Remove-NowOrAtReboot $o) -eq "booked") { $booked += $o }
        }
    }
    foreach ($f in $files) {
        $rel = $f.FullName.Substring($src.Length).TrimStart('\')
        $target = Join-Path $dest $rel
        if (-not (Test-SafeTarget $dest $target)) { throw "refusing to write through a junction or link: $target" }
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
        if ((Test-Path -LiteralPath $target -PathType Leaf) -and
            (Get-FileHash -LiteralPath $target -Algorithm SHA256).Hash -eq (Get-FileHash -LiteralPath $f.FullName -Algorithm SHA256).Hash) {
            continue   # same file already there (it may be loaded by apps): nothing to replace
        }
        if (Test-Path -LiteralPath $target) {
            try { Remove-Item -LiteralPath $target -Force -ErrorAction Stop }
            catch {
                # in use: rename it out of the way, delete it at the next reboot
                $old = "$target.old" + [DateTime]::Now.Ticks
                Rename-Item -LiteralPath $target (Split-Path $old -Leaf)
                if ((Remove-NowOrAtReboot $old) -eq "booked") { $booked += $old }
            }
        }
        Copy-Item -LiteralPath $f.FullName $target -Force
    }
    # files of the previous version that this one no longer ships (an old TIP implementation folder):
    # after the copy, so the old pointer never names a file that is already gone
    foreach ($rel in $previous) {
        $p = Join-Path $dest $rel
        if (($newList -notcontains $rel) -and ($rel -ne $ManifestName) -and (Test-SafeTarget $dest $p) -and (Test-Path -LiteralPath $p -PathType Leaf)) {
            $b = Remove-OrMoveAside $p
            if ($b) { $booked += $b }
        }
    }
    foreach ($b in (Remove-StaleImpl $dest)) { $booked += $b }
    ($newList + $ManifestName) | Set-Content -LiteralPath (Join-Path $dest $ManifestName) -Encoding UTF8
    return , $booked
}

# Delete the files in $list (and their .old copies) from $dest; folders only when empty.
# Returns the paths booked for deletion at the next reboot.
function Remove-InstalledFiles([string]$dest, [string[]]$list) {
    $dest = Get-FullDir $dest
    if (Test-ForbiddenDest $dest) { throw "refusing to delete in $dest" }
    $list = @($list | Where-Object { Test-RelPath $_ })
    $booked = @()
    foreach ($rel in $list) {
        $path = Join-Path $dest $rel
        # old copies first (already renamed), then the file itself (renamed aside if in use)
        foreach ($t in (Get-OldCopies $dest $rel)) {
            if ((Test-SafeTarget $dest $t) -and (Remove-NowOrAtReboot $t) -eq "booked") { $booked += $t }
        }
        if ((Test-Path -LiteralPath $path -PathType Leaf) -and (Test-SafeTarget $dest $path)) {
            $b = Remove-OrMoveAside $path
            if ($b) { $booked += $b }
        }
    }
    # folders our files were in, deepest first, then $dest itself
    $dirs = @($list | ForEach-Object { Split-Path (Join-Path $dest $_) } | Select-Object -Unique |
        Where-Object { $_ -ne $dest -and (Test-SafeTarget $dest $_) } | Sort-Object { $_.Length } -Descending) + $dest
    foreach ($d in $dirs) {
        if (-not (Test-Path -LiteralPath $d -PathType Container)) { continue }
        $left = @(Get-ChildItem -LiteralPath $d -Force -Recurse -File | ForEach-Object { $_.FullName })
        $leftDirs = @(Get-ChildItem -LiteralPath $d -Force -Recurse -Directory | ForEach-Object { $_.FullName })
        if ($left.Count -eq 0 -and $leftDirs.Count -eq 0) {
            Remove-Item -LiteralPath $d -Force -ErrorAction SilentlyContinue
        } elseif ($left.Count -gt 0 -and @($left | Where-Object { $booked -notcontains $_ }).Count -eq 0 -and
                  @($leftDirs | Where-Object { $dirs -notcontains $_ }).Count -eq 0) {
            # only our booked files remain (in our folders): book the folder after them
            [void](Register-DeleteAtReboot $d)
        }
    }
    return , $booked
}
