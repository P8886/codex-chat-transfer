@echo off
setlocal
cd /d "%~dp0"
set "MODE=%~1"
if defined MODE goto run
echo.
echo Codex Chat Transfer - Windows packaging
echo.
echo [1] Single EXE
echo [2] Portable ZIP
echo [3] Both
echo [0] Exit
echo.
choice /c 1230 /n /m "Select [1/2/3/0]: "
if errorlevel 4 exit /b 0
if errorlevel 3 (set "MODE=3") else if errorlevel 2 (set "MODE=2") else (set "MODE=1")
:run
powershell.exe -NoProfile -ExecutionPolicy Bypass -File "%~dp0scripts\package.ps1" -Mode "%MODE%"
set "RESULT=%ERRORLEVEL%"
if "%RESULT%"=="0" (echo Packaging complete. See dist.) else (echo Packaging failed. See error above.)
if "%~1"=="" pause
exit /b %RESULT%
