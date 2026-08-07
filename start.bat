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

REM If data_source=doris but password env is missing, warn early (falls back to simulation)
REM NOTE: keep echo lines ASCII-only -- cmd parses .bat as GBK and mojibake breaks lines
findstr /C:"data_source = \"doris\"" config.toml > nul 2>&1
if not errorlevel 1 (
    if not defined DORIS_DB_PASSWORD (
        echo [WARNING] data_source=doris but DORIS_DB_PASSWORD is NOT set!
        echo [WARNING] Doris connection will fail and fall back to simulation mode.
        echo [WARNING] Run start-doris.bat instead, or: set DORIS_DB_PASSWORD=your_password
        echo.
    )
)

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
echo [INFO] Logs: logs\weather-monitor.log (debug: set RUST_LOG=weather_monitor=debug)
echo [INFO] Press Ctrl+C to stop
echo.

"%EXE_PATH%"

if errorlevel 1 (
    echo.
    echo [ERROR] Program exited with error, press any key to close...
    pause > nul
)
