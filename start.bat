@echo off
chcp 65001 > nul
title Weather Station Monitor
setlocal enabledelayedexpansion

echo ==========================================
echo   Weather Station Monitor
echo ==========================================
echo.

REM Check config.toml
if not exist "config.toml" goto :check_config
goto :skip_config

:check_config
if exist "config.toml.example" (
    echo [INFO] config.toml not found, copying from template...
    copy "config.toml.example" "config.toml" > nul
    echo [INFO] Created config.toml. Please edit database settings before production use.
    goto :skip_config
)
echo [WARNING] Neither config.toml nor config.toml.example found, using defaults.

:skip_config

REM Check executable
set "EXE_PATH=target\release\weather-monitor.exe"
set "EXE_PATH_DEBUG=target\debug\weather-monitor.exe"

if not exist "%EXE_PATH%" goto :check_exe
goto :run

:check_exe
if not exist "%EXE_PATH_DEBUG%" (
    echo [INFO] Executable not found, building (debug mode)...
    if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
        "%USERPROFILE%\.cargo\bin\cargo.exe" build
    ) else (
        echo [ERROR] cargo not found. Please install Rust or build manually: cargo build --release
        pause
        exit /b 1
    )
)
set "EXE_PATH=%EXE_PATH_DEBUG%"
goto :run

:run
echo [INFO] Starting: %EXE_PATH%
echo [INFO] Access: http://localhost:8080
echo [INFO] Press Ctrl+C to stop
echo.

"%EXE_PATH%"

if errorlevel 1 (
    echo.
    echo [ERROR] Program exited with error, press any key to close...
    pause > nul
)
