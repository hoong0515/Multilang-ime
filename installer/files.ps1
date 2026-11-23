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

# Folders we never install into (or delete in)
function Test-ForbiddenDest([string]$dest) {
    $dest = Get-FullDir $dest
    $forbidden = @($env:windir, $env:ProgramFiles, ${env:ProgramFiles(x86)}, $env:ProgramData, $env:USERPROFILE,
        $env:SystemDrive, $env:LOCALAPPDATA, $env:APPDATA, [Environment]::GetFolderPath("Desktop"),
        [Environment]::GetFolderPath("MyDocuments")) | Where-Object { $_ } | ForEach-Object { Get-FullDir $_ }
    ($dest.Length -le 3) -or ($forbidden -contains $dest)
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

# <name>.old<digits> copies of one of our files, in the same folder
function Get-OldCopies([string]$dest, [string]$rel) {
    $path = Join-Path $dest $rel
    $dir = Split-Path $path
    $leaf = Split-Path $path -Leaf
    if (-not (Test-Path -LiteralPath $dir -PathType Container)) { return @() }
    @(Get-ChildItem -LiteralPath $dir -File -Force -ErrorAction SilentlyContinue |
        Where-Object { $_.Name -match ('^' + [regex]::Escape($leaf) + '\.old\d+$') } | ForEach-Object { $_.FullName })
}

# Copy the package into $dest, replacing our earlier files; writes the manifest. Returns paths booked for reboot.
function Install-PackageFiles([string]$src, [string]$dest, [string[]]$previous, [string[]]$exclude) {
    $src = Get-FullDir $src
    $dest = Get-FullDir $dest
    $booked = @()
    $files = @(Get-ChildItem -LiteralPath $src -Recurse -File | Where-Object { $exclude -notcontains $_.Name })
    $newList = @($files | ForEach-Object { $_.FullName.Substring($src.Length).TrimStart('\') })
    New-Item -ItemType Directory -Force $dest | Out-Null
    # old copies of our files, and files of the previous version that this one no longer ships
    foreach ($rel in (@($previous) + $newList | Select-Object -Unique)) {
        foreach ($o in (Get-OldCopies $dest $rel)) { if ((Remove-NowOrAtReboot $o) -eq "booked") { $booked += $o } }
    }
    foreach ($rel in $previous) {
        $p = Join-Path $dest $rel
        if (($newList -notcontains $rel) -and ($rel -ne $ManifestName) -and (Test-Path -LiteralPath $p -PathType Leaf)) {
            if ((Remove-NowOrAtReboot $p) -eq "booked") { $booked += $p }
        }
    }
    foreach ($f in $files) {
        $rel = $f.FullName.Substring($src.Length).TrimStart('\')
        $target = Join-Path $dest $rel
        New-Item -ItemType Directory -Force (Split-Path $target) | Out-Null
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
        $targets = @()
        if (Test-Path -LiteralPath $path -PathType Leaf) { $targets += $path }
        $targets += Get-OldCopies $dest $rel
        foreach ($t in $targets) { if ((Remove-NowOrAtReboot $t) -eq "booked") { $booked += $t } }
    }
    # folders our files were in, deepest first, then $dest itself
    $dirs = @($list | ForEach-Object { Split-Path (Join-Path $dest $_) } | Select-Object -Unique |
        Where-Object { $_ -ne $dest } | Sort-Object { $_.Length } -Descending) + $dest
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
