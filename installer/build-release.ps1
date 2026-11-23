# Build x64 + x86 and assemble the release folder and zip under dist\.
#   powershell -ExecutionPolicy Bypass -File installer\build-release.ps1 [-Version 0.1.0] [-NoBuild]
param([string]$Version = "0.1.0", [switch]$NoBuild)
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root

if (-not $NoBuild) {
    cargo build --release -p tip -p backend
    if ($LASTEXITCODE) { throw "x64 build failed" }
    cargo build --release -p tip --target i686-pc-windows-msvc
    if ($LASTEXITCODE) { throw "x86 build failed" }
    & "$Root\settings\build.ps1"
}

$Name = "MultilangIME-$Version"
$Out = Join-Path $Root "dist\$Name"
if (Test-Path $Out) { Remove-Item $Out -Recurse -Force }
New-Item -ItemType Directory -Force $Out, "$Out\x86", "$Out\licenses" | Out-Null

function Put($from, $to) {
    if (-not (Test-Path $from)) { throw "missing: $from" }
    Copy-Item $from (Join-Path $Out $to) -Recurse -Force
}
Put "target\release\multilang_tip.dll" "multilang_tip.dll"
Put "target\release\backend.exe" "backend.exe"
Put "target\settings\MultilangIME-Settings.exe" "MultilangIME-Settings.exe"
Put "target\i686-pc-windows-msvc\release\multilang_tip.dll" "x86\multilang_tip.dll"
Put "data\engine_model.json" "engine_model.json"
Put "third_party\libhangul\hanja.txt" "hanja.txt"
Put "deploy\x64\mozc" "mozc"
Put "LICENSE" "LICENSE.txt"
Put "THIRD-PARTY-NOTICES.md" "THIRD-PARTY-NOTICES.md"
Put "third_party\mozc\LICENSE" "licenses\mozc-LICENSE.txt"
Put "installer\README-install.md" "README.md"
foreach ($f in "install.ps1", "install.cmd", "uninstall.ps1", "uninstall.cmd") { Put "installer\$f" $f }
# Mozc user data is never shipped
Get-ChildItem "$Out\mozc" -Recurse -Include "*.db", "*.log" -ErrorAction SilentlyContinue | Remove-Item -Force

$Zip = Join-Path $Root "dist\$Name-win.zip"
if (Test-Path $Zip) { Remove-Item $Zip -Force }
Compress-Archive -Path "$Out\*" -DestinationPath $Zip
$hash = (Get-FileHash $Zip -Algorithm SHA256).Hash
"$hash  $Name-win.zip" | Set-Content "$Zip.sha256" -Encoding ascii
Write-Host "release: $Zip"
Write-Host "sha256:  $hash"
