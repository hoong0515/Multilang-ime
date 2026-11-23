@echo off
rem one line: cmd reads it whole before uninstall.ps1 deletes this folder
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0uninstall.ps1" %* & pause & exit /b
