@echo off
:: 气象站数据监控系统 — 启动入口 (CMD/Batch)
:: 自动调用 PowerShell 脚本，处理 UTF-8 字符集

chcp 65001 > nul

title 气象站数据监控系统

:: 如果 PowerShell 可用，优先使用功能完整的 ps1 脚本
where pwsh >nul 2>&1
if %errorlevel% == 0 (
    echo [INFO] 使用 PowerShell 启动...
    pwsh -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
    goto :end
)

where powershell >nul 2>&1
if %errorlevel% == 0 (
    echo [INFO] 使用 PowerShell 启动...
    powershell -NoProfile -ExecutionPolicy Bypass -File "%~dp0start.ps1" %*
    goto :end
)

:: 降级到纯 Batch 启动
setlocal enabledelayedexpansion

echo ==========================================
echo   气象站数据监控系统 (Batch 简易模式)
echo ==========================================
echo.

:: 检查 config.toml
if not exist "config.toml" (
    if exist "config.toml.example" (
        echo [INFO] config.toml 不存在，从模板创建...
        copy "config.toml.example" "config.toml" > nul
        echo [INFO] 已创建 config.toml，生产环境请编辑数据库配置
        echo.
    )
)

:: 查找可执行文件
set "EXE_PATH=target\release\weather-monitor.exe"
set "EXE_PATH_DEBUG=target\debug\weather-monitor.exe"

if not exist "!EXE_PATH!" (
    if not exist "!EXE_PATH_DEBUG!" (
        echo [INFO] 可执行文件不存在，尝试编译...
        if exist "%USERPROFILE%\.cargo\bin\cargo.exe" (
            "%USERPROFILE%\.cargo\bin\cargo.exe" build --release --bin weather-monitor
        ) else (
            echo [ERROR] 未找到 cargo。请安装 Rust 或手动编译。
            pause
            exit /b 1
        )
    ) else (
        set "EXE_PATH=!EXE_PATH_DEBUG!"
    )
)

echo [INFO] 启动: !EXE_PATH!
echo [INFO] 访问: http://localhost:8080
echo [INFO] 按 Ctrl+C 停止服务
echo.

"!EXE_PATH!"

if errorlevel 1 (
    echo.
    echo [ERROR] 程序异常退出
    pause > nul
)

:end
