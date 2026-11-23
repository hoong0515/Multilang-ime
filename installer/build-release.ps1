# Assemble the release folder and zip under dist\ (used locally and by .github/workflows/build.yml).
#   powershell -ExecutionPolicy Bypass -File installer\build-release.ps1 [-Version 0.1.0] [-NoBuild]
#       [-MozcHelper target\mozc-helper\mozc_stdio.exe] [-MozcCredits <Mozc>\src\data\installer\credits_en.html]
# Inputs that are not in git:
#   data\engine_model.json     python -m ime_v0.export_model data\model_params.json   (requirements-model.txt)
#   third_party\libhangul\hanja.txt   tools\fetch-third-party.ps1 (pinned upstream, checksum verified)
#   mozc_stdio.exe + credits   mozc-helper\build.ps1 (Mozc checkout)
#   VC runtime DLLs            the Visual Studio redist folder (found with vswhere)
param(
    [string]$Version = "0.1.0",
    [switch]$NoBuild,
    [string]$MozcHelper = "target\mozc-helper\mozc_stdio.exe",
    [string]$MozcCredits = "K:\mozc-build\mozc\src\data\installer\credits_en.html"
)
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
Set-Location $Root

if (-not $NoBuild) {
    cargo build --release -p tip -p backend -p candhost
    if ($LASTEXITCODE) { throw "x64 build failed" }
    cargo build --release -p tip --target i686-pc-windows-msvc
    if ($LASTEXITCODE) { throw "x86 build failed" }
    & "$Root\settings\build.ps1"
}
if (-not (Test-Path "third_party\libhangul\hanja.txt")) { & "$Root\tools\fetch-third-party.ps1" }

# the version names a folder under dist\ that is deleted and rebuilt: allow only a plain version string
if ($Version -notmatch '^[0-9]+\.[0-9]+\.[0-9]+(-[0-9A-Za-z.]+)?$') { throw "invalid -Version: $Version" }
$Name = "MultilangIME-$Version"
$Dist = Join-Path $Root "dist"
$Out = Join-Path $Dist $Name
if (-not ([IO.Path]::GetFullPath($Out)).StartsWith([IO.Path]::GetFullPath($Dist) + "\")) { throw "output outside dist: $Out" }
if (Test-Path $Out) { Remove-Item -LiteralPath $Out -Recurse -Force }
New-Item -ItemType Directory -Force $Out, "$Out\x86", "$Out\licenses", "$Out\mozc\documents" | Out-Null

function Put($from, $to) {
    if (-not (Test-Path $from)) { throw "missing: $from" }
    Copy-Item $from (Join-Path $Out $to) -Recurse -Force
}
Put "target\release\multilang_tip.dll" "multilang_tip.dll"
Put "target\release\backend.exe" "backend.exe"
Put "target\release\MultilangIME-CandWin.exe" "MultilangIME-CandWin.exe"
Put "target\settings\MultilangIME-Settings.exe" "MultilangIME-Settings.exe"
Put "target\i686-pc-windows-msvc\release\multilang_tip.dll" "x86\multilang_tip.dll"
Put "data\engine_model.json" "engine_model.json"
Put "third_party\libhangul\hanja.txt" "hanja.txt"

# Mozc: self-built stdio helper, its credits, and the VC runtime it links against
Put $MozcHelper "mozc\mozc_stdio.exe"
Put $MozcCredits "mozc\documents\credits_en.html"
$vswhere = Join-Path ${env:ProgramFiles(x86)} "Microsoft Visual Studio\Installer\vswhere.exe"
$vs = & $vswhere -latest -products * -property installationPath
$crt = Get-ChildItem "$vs\VC\Redist\MSVC\*\x64\Microsoft.VC14*.CRT" -Directory | Sort-Object FullName -Descending | Select-Object -First 1
if (-not $crt) { throw "VC runtime redist not found under $vs" }
foreach ($dll in "vcruntime140.dll", "vcruntime140_1.dll", "msvcp140.dll", "msvcp140_1.dll", "msvcp140_2.dll") {
    Put (Join-Path $crt.FullName $dll) "mozc\$dll"
}

Put "LICENSE" "LICENSE.txt"
Put "THIRD-PARTY-NOTICES.md" "THIRD-PARTY-NOTICES.md"
Put "mozc\proto\LICENSE-mozc" "licenses\mozc-LICENSE.txt"
Put "installer\README-install.md" "README.md"
foreach ($f in "install.ps1", "install.cmd", "uninstall.ps1", "uninstall.cmd", "files.ps1") { Put "installer\$f" $f }

$Zip = Join-Path $Root "dist\$Name-win.zip"
if (Test-Path $Zip) { Remove-Item $Zip -Force }
Compress-Archive -Path "$Out\*" -DestinationPath $Zip
$hash = (Get-FileHash $Zip -Algorithm SHA256).Hash
"$hash  $Name-win.zip" | Set-Content "$Zip.sha256" -Encoding ascii
Write-Host "release: $Zip"
Write-Host "sha256:  $hash"
