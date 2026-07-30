#Requires -Version 5.1
# 气象站数据监控系统 — 灵活启动脚本
# 支持: 开发/发布模式、前后台运行、端口覆盖、服务管理

param(
    [Alias("m")]
    [ValidateSet("dev","release","debug")]
    [string]$Mode = "release",

    [Alias("p")]
    [int]$Port = 0,

    [Alias("f")]
    [switch]$Foreground,

    [Alias("s")]
    [switch]$Stop,

    [switch]$Status,

    [Alias("r")]
    [switch]$Rebuild,

    [switch]$Simulated,

    [switch]$Logs,

    [switch]$Help
)

# ========== 字符集与输出编码 ==========
[Console]::OutputEncoding = [System.Text.Encoding]::UTF8
$OutputEncoding          = [System.Text.Encoding]::UTF8
chcp 65001 | Out-Null

# ========== 颜色定义 ==========
$CLR_C  = "Cyan"
$CLR_G  = "Green"
$CLR_Y  = "Yellow"
$CLR_R  = "Red"
$CLR_W  = "White"

# ========== 帮助信息 ==========
function Show-Help {
    @"
气象站数据监控系统启动脚本 (PowerShell)
用法: .\start.ps1 [参数]

  -Mode       dev|release|debug   编译模式 (默认: release)
  -Port       N                  覆盖监听端口 (0=使用配置文件)
  -Foreground                    前台运行 (默认后台运行)
  -Stop                          停止正在运行的服务
  -Status                        查看服务运行状态
  -Rebuild                       强制重新编译
  -Simulated                     强制使用模拟模式 (不连真实数据库)
  -Logs                          查看最近 50 行日志
  -Help                          显示本帮助

示例:
  .\start.ps1                   # 后台启动 (release 模式)
  .\start.ps1 -Mode dev         # 后台启动 (debug 模式)
  .\start.ps1 -Foreground       # 前台运行，Ctrl+C 停止
  .\start.ps1 -Port 9090        # 使用 9090 端口启动
  .\start.ps1 -Stop             # 停止服务
  .\start.ps1 -Status           # 查看状态
  .\start.ps1 -Rebuild          # 强制重新编译并启动
"@ | Write-Host -ForegroundColor $CLR_C
}

if ($Help) {
    Show-Help
    exit 0
}

# ========== 路径与配置 ==========
$scriptDir = $PSScriptRoot
if (-not $scriptDir) { $scriptDir = (Get-Location).Path }
Set-Location $scriptDir

$projectName   = "weather-monitor"
$cargoDir      = Join-Path $env:USERPROFILE ".cargo\bin"
$cargoExe      = Join-Path $cargoDir "cargo.exe"
$targetMap     = @{ dev = "debug"; debug = "debug"; release = "release" }
$profile       = $targetMap[$Mode]
$exePath       = Join-Path $scriptDir "target\$profile\$projectName.exe"
$fallbackExe   = Join-Path $scriptDir "..\target\release\$projectName.exe"
$logFile       = Join-Path $scriptDir "server.log"
$lockFile      = Join-Path $env:TEMP "stchck.pid"

# ========== 查找可执行文件 ==========
function Find-Executable {
    if (Test-Path $exePath) { return $exePath }
    if (Test-Path $fallbackExe) { return $fallbackExe }
    return $null
}

# ========== 获取运行中进程 ==========
function Get-RunningProcess {
    $proc = Get-Process -Name $projectName -ErrorAction SilentlyContinue | Select-Object -First 1
    return $proc
}

# ========== 状态查看 ==========
if ($Status) {
    $proc = Get-RunningProcess
    if ($proc) {
        Write-Host "[状态] 服务正在运行" -ForegroundColor $CLR_G
        Write-Host "       PID:  $($proc.Id)" -ForegroundColor $CLR_W
        Write-Host "       启动时间: $($proc.StartTime)" -ForegroundColor $CLR_W
        try {
            $resp = Invoke-WebRequest -Uri "http://localhost:8080/api/summary" -UseBasicParsing -TimeoutSec 3 -ErrorAction Stop
            Write-Host "       API: 正常响应" -ForegroundColor $CLR_G
            Write-Host "       数据: $($resp.Content)" -ForegroundColor $CLR_W
        } catch {
            Write-Host "       API: 未响应 (可能正在初始化)" -ForegroundColor $CLR_Y
        }
    } else {
        Write-Host "[状态] 服务未运行" -ForegroundColor $CLR_Y
    }
    exit 0
}

# ========== 停止服务 ==========
if ($Stop) {
    $proc = Get-RunningProcess
    if ($proc) {
        Write-Host "[停止] 正在结束 PID $($proc.Id)..." -ForegroundColor $CLR_Y
        Stop-Process -Id $proc.Id -Force
        Start-Sleep -Milliseconds 800
        Write-Host "[停止] 服务已停止" -ForegroundColor $CLR_G
    } else {
        Write-Host "[停止] 未找到运行中的服务" -ForegroundColor $CLR_Y
    }
    exit 0
}

# ========== 查看日志 ==========
if ($Logs) {
    if (Test-Path $logFile) {
        Write-Host "[日志] 最近 50 行日志 ($logFile):" -ForegroundColor $CLR_C
        Write-Host "----------------------------------------" -ForegroundColor $CLR_C
        Get-Content $logFile -Tail 50 | Write-Host
    } else {
        Write-Host "[日志] 日志文件不存在: $logFile" -ForegroundColor $CLR_Y
    }
    exit 0
}

# ========== 横幅 ==========
Write-Host "==========================================" -ForegroundColor $CLR_C
Write-Host "  气象站数据监控系统启动脚本" -ForegroundColor $CLR_C
Write-Host "==========================================" -ForegroundColor $CLR_C
Write-Host ""

# ========== 检查 config.toml ==========
if (-not (Test-Path "config.toml")) {
    if (Test-Path "config.toml.example") {
        Write-Host "[INFO] config.toml 不存在，从模板创建..." -ForegroundColor $CLR_Y
        Copy-Item "config.toml.example" "config.toml"
        Write-Host "[INFO] 已创建 config.toml，生产环境请编辑数据库配置" -ForegroundColor $CLR_Y
        Write-Host ""
    }
}

# ========== 强制模拟模式 ==========
if ($Simulated) {
    $env:STCHCK_SIMULATED = "1"
    Write-Host "[INFO] 已强制启用模拟模式 (不连接真实数据库)" -ForegroundColor $CLR_Y
}

# ========== 端口覆盖 ==========
if ($Port -gt 0) {
    $env:STCHCK_PORT = "$Port"
    Write-Host "[INFO] 覆盖端口为: $Port" -ForegroundColor $CLR_Y
}

# ========== 编译 ==========
$exe = Find-Executable
if ((-not $exe) -or $Rebuild) {
    if (-not (Test-Path $cargoExe)) {
        # 尝试 PATH
        $cargoExe = (Get-Command cargo.exe -ErrorAction SilentlyContinue).Source
    }

    if (-not $cargoExe) {
        Write-Host "[ERROR] 未找到 cargo.exe。请先安装 Rust 工具链。" -ForegroundColor $CLR_R
        Write-Host "        下载地址: https://rustup.rs/" -ForegroundColor $CLR_W
        exit 1
    }

    Write-Host "[INFO] 正在编译 ($Mode 模式)..." -ForegroundColor $CLR_Y
    $buildArgs = if ($Mode -eq "release") { @("build", "--release", "--bin", $projectName) } else { @("build", "--bin", $projectName) }

    & $cargoExe @buildArgs
    if ($LASTEXITCODE -ne 0) {
        Write-Host "[ERROR] 编译失败 (退出码: $LASTEXITCODE)" -ForegroundColor $CLR_R
        exit 1
    }
    Write-Host "[INFO] 编译完成" -ForegroundColor $CLR_G
    Write-Host ""
    $exe = Find-Executable
}

if (-not $exe -or -not (Test-Path $exe)) {
    Write-Host "[ERROR] 找不到可执行文件: $exePath" -ForegroundColor $CLR_R
    exit 1
}

# ========== 检查已有实例 ==========
$existing = Get-RunningProcess
if ($existing) {
    Write-Host "[WARN] 检测到已有实例在运行 (PID: $($existing.Id))" -ForegroundColor $CLR_Y
    if (-not $Foreground) {
        Write-Host "[INFO] 如需重启请先执行: .\start.ps1 -Stop" -ForegroundColor $CLR_W
        exit 1
    }
}

# ========== 启动 ==========
Write-Host "[INFO] 可执行文件: $exe" -ForegroundColor $CLR_G

if ($Foreground) {
    Write-Host "[INFO] 前台运行模式 — 按 Ctrl+C 停止服务" -ForegroundColor $CLR_G
    Write-Host "[INFO] 访问地址: http://localhost:$(&{ if($Port -gt 0){$Port}else{8080} })" -ForegroundColor $CLR_G
    Write-Host ""
    try {
        & $exe
    } catch {
        # Ctrl+C 触发
    }
    if ($LASTEXITCODE -ne 0 -and $LASTEXITCODE -ne $null) {
        Write-Host ""
        Write-Host "[ERROR] 程序异常退出 (退出码: $LASTEXITCODE)" -ForegroundColor $CLR_R
    }
} else {
    # 后台运行
    $actualPort = if ($Port -gt 0) { $Port } else { 8080 }
    Write-Host "[INFO] 后台运行模式" -ForegroundColor $CLR_G
    Write-Host "[INFO] 访问地址: http://localhost:$actualPort" -ForegroundColor $CLR_G
    Write-Host "[INFO] 日志文件: $logFile" -ForegroundColor $CLR_G
    Write-Host "[INFO] 停止命令: .\start.ps1 -Stop" -ForegroundColor $CLR_G
    Write-Host ""

    # 启动进程并重定向输出到日志
    $pinfo = New-Object System.Diagnostics.ProcessStartInfo
    $pinfo.FileName = $exe
    $pinfo.WorkingDirectory = $scriptDir
    $pinfo.UseShellExecute = $false
    $pinfo.RedirectStandardOutput = $true
    $pinfo.RedirectStandardError = $true
    $pinfo.CreateNoWindow = $true

    $proc = New-Object System.Diagnostics.Process
    $proc.StartInfo = $pinfo

    # 异步写日志
    $proc.Start() | Out-Null

    # 输出 PID 便于管理
    $proc.Id | Out-File -FilePath $lockFile -Encoding utf8 -Force

    # 后台读取输出到日志
    Start-Job -ScriptBlock {
        param($p, $log)
        $stdout = $p.StandardOutput
        $stderr = $p.StandardError
        while (-not $stdout.EndOfStream) {
            $line = $stdout.ReadLine()
            if ($line) { Add-Content -Path $log -Value $line -Encoding UTF8 }
        }
        while (-not $stderr.EndOfStream) {
            $line = $stderr.ReadLine()
            if ($line) { Add-Content -Path $log -Value "[ERR] $line" -Encoding UTF8 }
        }
    } -ArgumentList $proc, $logFile | Out-Null

    Start-Sleep -Seconds 3

    # 快速健康检查
    try {
        $testUri = "http://localhost:$actualPort/api/summary"
        $resp = Invoke-WebRequest -Uri $testUri -UseBasicParsing -TimeoutSec 5 -ErrorAction Stop
        Write-Host "[INFO] 服务启动成功! API 响应正常" -ForegroundColor $CLR_G
        Write-Host "       $($resp.Content)" -ForegroundColor $CLR_W
    } catch {
        Write-Host "[WARN] 服务可能仍在初始化，API 尚未响应" -ForegroundColor $CLR_Y
        Write-Host "       3 秒后再次检查，或查看日志: .\start.ps1 -Logs" -ForegroundColor $CLR_W
    }
}
