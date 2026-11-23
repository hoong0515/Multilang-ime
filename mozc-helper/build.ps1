# Build the Mozc stdio helper inside a Mozc checkout (see docs/mozc-helper.md).
#   powershell -ExecutionPolicy Bypass -File mozc-helper\build.ps1 [-Mozc K:\mozc-build\mozc]
# Output: target\mozc-helper\mozc_stdio.exe
param([string]$Mozc = "K:\mozc-build\mozc", [string[]]$BazelArgs = @())
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
$src = Join-Path $Mozc "src"
if (-not (Test-Path "$src\MODULE.bazel")) { throw "not a Mozc checkout: $Mozc" }

# 1. Copy the helper package
$pkg = Join-Path $src "multilang"
New-Item -ItemType Directory -Force $pkg | Out-Null
Copy-Item "$PSScriptRoot\mozc_stdio.cc", "$PSScriptRoot\BUILD.bazel" $pkg -Force

# 2. Let the package see the engine and session handler (private in upstream Mozc)
function Add-Visibility($file, $target) {
    $text = [IO.File]::ReadAllText($file)
    $marker = "name = `"$target`","
    $i = $text.IndexOf($marker)
    if ($i -lt 0) { throw "$target not found in $file" }
    $j = $text.IndexOf("visibility = [", $i)
    $entry = "`"//multilang:__pkg__`","
    $end = $text.IndexOf("]", $j)
    if ($text.Substring($j, $end - $j).Contains($entry)) { return }
    $text = $text.Insert($j + "visibility = [".Length, "`n        $entry")
    [IO.File]::WriteAllText($file, $text)
}
Add-Visibility "$src\session\BUILD.bazel" "session_handler"
Add-Visibility "$src\engine\BUILD.bazel" "engine_factory"

# 3. Pick a Visual Studio whose MSVC has ATL (atlbase.h); Mozc's Windows code needs it
if (-not $env:BAZEL_VC) {
    $vc = Get-ChildItem "C:\Program Files\Microsoft Visual Studio\*\*\VC", "C:\Program Files (x86)\Microsoft Visual Studio\*\*\VC" -ErrorAction SilentlyContinue |
        Where-Object { Get-ChildItem "$($_.FullName)\Tools\MSVC\*\atlmfc\include\atlbase.h" -ErrorAction SilentlyContinue } |
        Sort-Object FullName -Descending | Select-Object -First 1
    if (-not $vc) { throw "no Visual Studio with ATL found (install the 'C++ ATL' component)" }
    $env:BAZEL_VC = $vc.FullName
}
"BAZEL_VC=$env:BAZEL_VC"

# 4. Build
$bazelisk = (Get-Command bazelisk -ErrorAction SilentlyContinue).Source
if (-not $bazelisk) { $bazelisk = Join-Path $env:LOCALAPPDATA "Microsoft\WinGet\Links\bazelisk.exe" }
Push-Location $src
try {
    & $bazelisk build //multilang:mozc_stdio --config oss_windows --config release_build @BazelArgs
    if ($LASTEXITCODE) { throw "bazel build failed" }
} finally { Pop-Location }

$out = Join-Path $Root "target\mozc-helper"
New-Item -ItemType Directory -Force $out | Out-Null
Copy-Item "$src\bazel-bin\multilang\mozc_stdio.exe" $out -Force
"built: $out\mozc_stdio.exe"
