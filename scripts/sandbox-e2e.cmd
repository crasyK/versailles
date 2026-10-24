@echo off
REM npm run e2e inside Windows Sandbox (#38): a clean Windows, so the taskbar
REM phase always runs and your own Versailles is never touched. Windows Pro /
REM Enterprise / Education only (Home has no Sandbox). UNTESTED so far: it was
REM written on a Home machine; expect to adjust it on the first run.
REM
REM   npm run build:test
REM   scripts\sandbox-e2e.cmd
REM
REM Maps read-only: this repo (C:\repo), the build folder (C:\build:
REM src-tauri\target\release-test, else release) and a Node folder (C:\node:
REM VERSAILLES_SANDBOX_NODE, else the folder of node.exe on PATH; it must hold
REM a standalone node.exe, such as the nodejs.org .zip or C:\Program Files\nodejs,
REM not a version-manager shim). Read-write: %TEMP%\versailles-sandbox
REM (C:\results): e2e.log, logs\, done.json. Inside, the Sandbox's LogonCommand
REM runs this file again with --inside.
setlocal
if /i "%~1"=="--inside" goto inside

for %%I in ("%~dp0..") do set "REPO=%%~fI"
set "BUILD=%REPO%\src-tauri\target\release-test"
if not exist "%BUILD%\versailles.exe" set "BUILD=%REPO%\src-tauri\target\release"
if not exist "%BUILD%\versailles.exe" (
  echo No build found under src-tauri\target. Run npm run build:test first.
  exit /b 2
)
set "NODEDIR=%VERSAILLES_SANDBOX_NODE%"
if not defined NODEDIR for /f "delims=" %%N in ('where node 2^>nul') do if not defined NODEDIR set "NODEDIR=%%~dpN"
if "%NODEDIR:~-1%"=="\" set "NODEDIR=%NODEDIR:~0,-1%"
if not exist "%NODEDIR%\node.exe" (
  echo No node.exe found. Set VERSAILLES_SANDBOX_NODE to a folder with a standalone node.exe.
  exit /b 2
)
if not exist "%WINDIR%\System32\WindowsSandbox.exe" (
  echo Windows Sandbox is not available. Turn on the "Windows Sandbox" feature; Windows Home has none.
  exit /b 2
)
set "RESULTS=%TEMP%\versailles-sandbox"
if exist "%RESULTS%" rmdir /s /q "%RESULTS%"
mkdir "%RESULTS%"
set "WSB=%TEMP%\versailles-sandbox.wsb"
powershell -NoProfile -Command "$e = { param($s) [Security.SecurityElement]::Escape($s) }; $t = Get-Content -Raw (Join-Path $env:REPO 'tests\e2e\sandbox.wsb'); $t = $t.Replace('__REPO__', (& $e $env:REPO)).Replace('__BUILD__', (& $e $env:BUILD)).Replace('__NODE__', (& $e $env:NODEDIR)).Replace('__RESULTS__', (& $e $env:RESULTS)); [IO.File]::WriteAllText($env:WSB, $t)"
echo Starting Windows Sandbox: build %BUILD%, Node %NODEDIR%, results %RESULTS%
start "" "%WSB%"

REM Wait up to 30 minutes for done.json.
set /a WAITED=0
:wait
if exist "%RESULTS%\done.json" goto done
if %WAITED% geq 360 (
  echo Timed out waiting for %RESULTS%\done.json.
  exit /b 1
)
timeout /t 5 /nobreak >nul
set /a WAITED+=1
goto wait

:done
type "%RESULTS%\e2e.log"
set "CODE=1"
for /f %%C in ('powershell -NoProfile -Command "(Get-Content -Raw (Join-Path $env:RESULTS 'done.json') | ConvertFrom-Json).exitCode"') do set "CODE=%%C"
echo.
echo e2e exit code %CODE%. Results in %RESULTS%. Close the Sandbox window to discard it.
exit /b %CODE%

:inside
set "OUT=C:\results"
REM WebView2 does not find its runtime in the Sandbox (WebView2Feedback #5697):
REM point it at the newest runtime folder.
for /f "delims=" %%D in ('powershell -NoProfile -Command "Get-ChildItem 'C:\Program Files (x86)\Microsoft\EdgeWebView\Application' -Directory -ErrorAction SilentlyContinue | Where-Object Name -match '^\d+(\.\d+)+$' | Sort-Object { [version]$_.Name } -Descending | Select-Object -First 1 -ExpandProperty FullName"') do set "WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=%%D"
> "%OUT%\e2e.log" echo WEBVIEW2_BROWSER_EXECUTABLE_FOLDER=%WEBVIEW2_BROWSER_EXECUTABLE_FOLDER%
set "PATH=C:\node;%PATH%"
REM The logon command can run before Explorer's taskbar is up.
powershell -NoProfile -Command "$end = (Get-Date).AddMinutes(2); while ((Get-Date) -lt $end -and -not (Get-Process explorer -ErrorAction SilentlyContinue)) { Start-Sleep 1 }; Start-Sleep 10"
node C:\repo\scripts\e2e.mjs --exe C:\build\versailles.exe >> "%OUT%\e2e.log" 2>&1
set "CODE=%ERRORLEVEL%"
xcopy /e /i /q /y "%LOCALAPPDATA%\com.versailles.widgets\logs" "%OUT%\logs" >nul 2>&1
> "%OUT%\done.json" echo {"exitCode": %CODE%}
exit /b 0
