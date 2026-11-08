@echo off
REM Build RustText for Windows (wrapper around build-windows.ps1).
REM Usage: scripts\build-windows.bat [-SkipTests] [-Profile debug]
powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0build-windows.ps1" %*
exit /b %ERRORLEVEL%
