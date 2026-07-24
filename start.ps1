#!/usr/bin/env pwsh
#Requires -Version 5.1

[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$Host.UI.RawUI.WindowTitle = "气象站数据监控系统"

Write-Host "==========================================" -ForegroundColor Cyan
Write-Host "  气象站数据监控系统启动脚本" -ForegroundColor Cyan
Write-Host "==========================================" -ForegroundColor Cyan
Write-Host ""

# 检查 config.toml
if (-not (Test-Path "config.toml")) {
    if (Test-Path "config.toml.example") {
        Write-Host "[INFO] config.toml 不存在，从模板创建..." -ForegroundColor Yellow
        Copy-Item "config.toml.example" "config.toml"
        Write-Host "[INFO] 已创建 config.toml，请根据环境编辑数据库配置" -ForegroundColor Yellow
        Write-Host ""
    } else {
        Write-Host "[WARNING] config.toml 和 config.toml.example 均不存在，使用默认配置" -ForegroundColor Yellow
        Write-Host ""
    }
}

# 检查二进制并自动编译
$exePaths = @(
    "target/release/weather-monitor.exe",
    "target/debug/weather-monitor.exe"
)

$exePath = $null
foreach ($path in $exePaths) {
    if (Test-Path $path) {
        $exePath = $path
        break
    }
}

if (-not $exePath) {
    Write-Host "[INFO] 可执行文件不存在，正在编译（Debug 模式）..." -ForegroundColor Yellow
    Write-Host ""
    $cargo = "$env:USERPROFILE\.cargo\bin\cargo.exe"
    if (Test-Path $cargo) {
        & $cargo build
    } else {
        Write-Host "[ERROR] 未找到 cargo，请手动编译：cargo build --release" -ForegroundColor Red
        Write-Host ""
        Read-Host "按 Enter 键退出"
        exit 1
    }
    $exePath = "target/debug/weather-monitor.exe"
}

Write-Host "[INFO] 启动程序: $exePath" -ForegroundColor Green
Write-Host "[INFO] 访问地址: http://localhost:8080" -ForegroundColor Green
Write-Host "[INFO] 按 Ctrl+C 停止服务" -ForegroundColor Green
Write-Host ""

if ($IsLinux -or $IsMacOS) {
    # Linux/macOS
    & $exePath
} else {
    # Windows
    $proc = Start-Process -FilePath $exePath -NoNewWindow -PassThru
    try {
        $proc.WaitForExit()
    } catch {
        # Ctrl+C pressed
    }
}

if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) {
    Write-Host ""
    Write-Host "[ERROR] 程序异常退出 (退出码: $LASTEXITCODE)" -ForegroundColor Red
    Read-Host "按 Enter 键关闭"
}
