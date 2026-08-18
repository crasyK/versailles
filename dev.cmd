@echo off
REM Ensures cargo is on PATH even in terminals opened before Rust was installed.
set "PATH=%USERPROFILE%\.cargo\bin;%PATH%"
cd /d "%~dp0"
npm run tauri:dev %*
