# Build the settings app with the .NET Framework 4.x csc. Output: targetsettingsMultilangIME-Settings.exe
$ErrorActionPreference = "Stop"
$Root = Split-Path $PSScriptRoot -Parent
$csc = Join-Path $env:windir "Microsoft.NET\Framework64\v4.0.30319\csc.exe"
$out = Join-Path $Root "target\settings"
New-Item -ItemType Directory -Force $out | Out-Null
& $csc /nologo /codepage:65001 /target:winexe /optimize+ /platform:anycpu `
    /r:System.Web.Extensions.dll /r:System.Windows.Forms.dll /r:System.Drawing.dll /r:System.Core.dll `
    /win32icon:"$Root\res\multilang.ico" /out:"$out\MultilangIME-Settings.exe" "$PSScriptRoot\Settings.cs"
if ($LASTEXITCODE) { throw "csc failed" }
"built: $out\MultilangIME-Settings.exe"
