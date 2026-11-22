# 백엔드를 사용자 로그온 때 시작하는 작업 스케줄러 작업을 등록·해제한다 (관리자 권한 불필요).
#
#   powershell -ExecutionPolicy Bypass -File tools\backend-task.ps1 install   # 등록 후 바로 실행
#   powershell -ExecutionPolicy Bypass -File tools\backend-task.ps1 uninstall # 해제 (실행 중인 백엔드도 종료)
#
# 작업 스케줄러가 띄운 프로세스는 어떤 앱의 패키지 문맥도 물려받지 않는다.
# (패키지 앱이 띄운 백엔드는 그 앱의 가상 폴더·수명에 묶이는 문제가 있었다)
param([ValidateSet("install", "uninstall")][string]$Action = "install")

$TaskName = "MultilangIME Backend"
$Exe = Join-Path (Split-Path $PSScriptRoot -Parent) "deploy\x64\backend.exe"

if ($Action -eq "uninstall") {
    Unregister-ScheduledTask -TaskName $TaskName -Confirm:$false -ErrorAction SilentlyContinue
    Get-Process backend -ErrorAction SilentlyContinue | Where-Object { $_.Path -eq $Exe } | Stop-Process -Force
    "uninstalled"
    return
}

if (-not (Test-Path $Exe)) { throw "backend.exe not found: $Exe" }
$user = "$env:USERDOMAIN\$env:USERNAME"
$actionObj = New-ScheduledTaskAction -Execute $Exe -WorkingDirectory (Split-Path $Exe)
$trigger = New-ScheduledTaskTrigger -AtLogOn -User $user
$principal = New-ScheduledTaskPrincipal -UserId $user -LogonType Interactive -RunLevel Limited
$settings = New-ScheduledTaskSettingsSet -AllowStartIfOnBatteries -DontStopIfGoingOnBatteries `
    -ExecutionTimeLimit ([TimeSpan]::Zero) -RestartCount 3 -RestartInterval (New-TimeSpan -Minutes 1) -MultipleInstances IgnoreNew
Register-ScheduledTask -TaskName $TaskName -Action $actionObj -Trigger $trigger -Principal $principal -Settings $settings -Force | Out-Null
Start-ScheduledTask -TaskName $TaskName
"installed and started: $Exe"
