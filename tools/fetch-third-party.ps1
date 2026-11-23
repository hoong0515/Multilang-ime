# Download third-party data files that are not kept in git, pinned and checksum-verified.
#   powershell -ExecutionPolicy Bypass -File tools\fetch-third-party.ps1
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
$files = @(
    @{  # libhangul Hanja dictionary (BSD-3-Clause)
        Url    = "https://raw.githubusercontent.com/libhangul/libhangul/717409ce61524bb3d8426060a384822f21354c62/data/hanja/hanja.txt"
        Path   = "third_party\libhangul\hanja.txt"
        Sha256 = "B1004034589F1357DAAEA3534A6136F6B5EF825AFA8779886B20F0B7908BBE3B"
    }
)
foreach ($f in $files) {
    $dest = Join-Path $Root $f.Path
    if ((Test-Path $dest) -and (Get-FileHash $dest -Algorithm SHA256).Hash -eq $f.Sha256) { "ok: $($f.Path)"; continue }
    New-Item -ItemType Directory -Force (Split-Path $dest) | Out-Null
    Invoke-WebRequest -Uri $f.Url -OutFile "$dest.tmp" -UseBasicParsing
    $hash = (Get-FileHash "$dest.tmp" -Algorithm SHA256).Hash
    if ($hash -ne $f.Sha256) { Remove-Item "$dest.tmp"; throw "checksum mismatch for $($f.Path): $hash" }
    Move-Item "$dest.tmp" $dest -Force
    "fetched: $($f.Path)"
}
