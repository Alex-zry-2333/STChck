#!/bin/bash
# 气象站数据监控系统 — 灵活启动脚本 (Git Bash / Linux / macOS)
# 支持参数: dev/release 模式、端口覆盖、前后台、服务管理

set -e

# ========== 颜色 ==========
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m'

# ========== 默认参数 ==========
MODE="release"
PORT=0
FOREGROUND=0
STOP=0
STATUS=0
REBUILD=0
SIMULATED=0
LOGS=0

# ========== 帮助 ==========
show_help() {
    cat << 'EOF'
气象站数据监控系统启动脚本 (Bash)
用法: ./start.sh [选项]

  -m, --mode dev|release|debug   编译模式 (默认: release)
  -p, --port N                   覆盖监听端口 (0=使用配置文件)
  -f, --foreground               前台运行 (默认后台运行)
  -s, --stop                     停止正在运行的服务
      --status                   查看服务运行状态
  -r, --rebuild                  强制重新编译
      --simulated                强制使用模拟模式
      --logs                     查看最近 50 行日志
  -h, --help                     显示本帮助

示例:
  ./start.sh                     # 后台启动 (release 模式)
  ./start.sh -m dev              # 后台启动 (debug 模式)
  ./start.sh -f                  # 前台运行，Ctrl+C 停止
  ./start.sh -p 9090             # 使用 9090 端口启动
  ./start.sh -s                  # 停止服务
  ./start.sh --status            # 查看状态
  ./start.sh -r                  # 强制重新编译并启动
EOF
}

# ========== 参数解析 ==========
while [[ $# -gt 0 ]]; do
    case $1 in
        -m|--mode)
            MODE="$2"
            shift 2
            ;;
        -p|--port)
            PORT="$2"
            shift 2
            ;;
        -f|--foreground)
            FOREGROUND=1
            shift
            ;;
        -s|--stop)
            STOP=1
            shift
            ;;
        --status)
            STATUS=1
            shift
            ;;
        -r|--rebuild)
            REBUILD=1
            shift
            ;;
        --simulated)
            SIMULATED=1
            shift
            ;;
        --logs)
            LOGS=1
            shift
            ;;
        -h|--help)
            show_help
            exit 0
            ;;
        *)
            echo -e "${RED}[ERROR] 未知参数: $1${NC}"
            show_help
            exit 1
            ;;
    esac
done

# ========== 路径 ==========
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

PROJECT_NAME="weather-monitor"
LOG_FILE="$SCRIPT_DIR/server.log"
PID_FILE="$SCRIPT_DIR/server.pid"

if [[ "$MODE" == "dev" || "$MODE" == "debug" ]]; then
    PROFILE="debug"
else
    PROFILE="release"
fi

EXE_PATH="$SCRIPT_DIR/target/$PROFILE/$PROJECT_NAME"
FALLBACK_EXE="$SCRIPT_DIR/../target/release/$PROJECT_NAME"

# ========== 查找可执行文件 ==========
find_executable() {
    if [[ -f "$EXE_PATH" ]]; then
        echo "$EXE_PATH"
    elif [[ -f "$FALLBACK_EXE" ]]; then
        echo "$FALLBACK_EXE"
    else
        echo ""
    fi
}

# ========== 状态查看 ==========
if [[ $STATUS -eq 1 ]]; then
    PID=$(pgrep -f "$PROJECT_NAME" | head -1 || true)
    if [[ -n "$PID" ]]; then
        echo -e "${GREEN}[状态] 服务正在运行${NC}"
        echo -e "       PID: $PID"
        if command -v ps >/dev/null 2>&1; then
            START_TIME=$(ps -o lstart= -p "$PID" 2>/dev/null || echo "未知")
            echo -e "       启动时间: $START_TIME"
        fi
        if command -v curl >/dev/null 2>&1; then
            RESP=$(curl -s --max-time 3 "http://localhost:8080/api/summary" 2>/dev/null || true)
            if [[ -n "$RESP" ]]; then
                echo -e "${GREEN}       API: 正常响应${NC}"
                echo -e "       数据: $RESP"
            else
                echo -e "${YELLOW}       API: 未响应 (可能正在初始化)${NC}"
            fi
        fi
    else
        echo -e "${YELLOW}[状态] 服务未运行${NC}"
    fi
    exit 0
fi

# ========== 停止服务 ==========
if [[ $STOP -eq 1 ]]; then
    PID=$(pgrep -f "$PROJECT_NAME" | head -1 || true)
    if [[ -n "$PID" ]]; then
        echo -e "${YELLOW}[停止] 正在结束 PID $PID...${NC}"
        kill -TERM "$PID" 2>/dev/null || kill -KILL "$PID" 2>/dev/null || true
        sleep 1
        echo -e "${GREEN}[停止] 服务已停止${NC}"
    else
        echo -e "${YELLOW}[停止] 未找到运行中的服务${NC}"
    fi
    rm -f "$PID_FILE"
    exit 0
fi

# ========== 查看日志 ==========
if [[ $LOGS -eq 1 ]]; then
    if [[ -f "$LOG_FILE" ]]; then
        echo -e "${CYAN}[日志] 最近 50 行日志 ($LOG_FILE):${NC}"
        echo "----------------------------------------"
        tail -n 50 "$LOG_FILE"
    else
        echo -e "${YELLOW}[日志] 日志文件不存在: $LOG_FILE${NC}"
    fi
    exit 0
fi

# ========== 横幅 ==========
echo -e "${CYAN}==========================================${NC}"
echo -e "${CYAN}  气象站数据监控系统启动脚本${NC}"
echo -e "${CYAN}==========================================${NC}"
echo ""

# ========== 检查 config.toml ==========
if [[ ! -f "config.toml" ]]; then
    if [[ -f "config.toml.example" ]]; then
        echo -e "${YELLOW}[INFO] config.toml 不存在，从模板创建...${NC}"
        cp "config.toml.example" "config.toml"
        echo -e "${YELLOW}[INFO] 已创建 config.toml，生产环境请编辑数据库配置${NC}"
        echo ""
    fi
fi

# ========== 强制模拟模式 ==========
if [[ $SIMULATED -eq 1 ]]; then
    export STCHCK_SIMULATED="1"
    echo -e "${YELLOW}[INFO] 已强制启用模拟模式 (不连接真实数据库)${NC}"
fi

# ========== 端口覆盖 ==========
if [[ $PORT -gt 0 ]]; then
    export STCHCK_PORT="$PORT"
    echo -e "${YELLOW}[INFO] 覆盖端口为: $PORT${NC}"
fi

# ========== 编译 ==========
EXE=$(find_executable)
if [[ -z "$EXE" || $REBUILD -eq 1 ]]; then
    if ! command -v cargo &> /dev/null; then
        # 尝试常见的 cargo 路径
        for CARGO_TRY in "$HOME/.cargo/bin/cargo" "/usr/local/bin/cargo" "/usr/bin/cargo"; do
            if [[ -x "$CARGO_TRY" ]]; then
                export PATH="$(dirname "$CARGO_TRY"):$PATH"
                break
            fi
        done
    fi

    if ! command -v cargo &> /dev/null; then
        echo -e "${RED}[ERROR] 未找到 cargo。请先安装 Rust 工具链。${NC}"
        echo -e "       下载地址: https://rustup.rs/${NC}"
        exit 1
    fi

    echo -e "${YELLOW}[INFO] 正在编译 ($MODE 模式)...${NC}"
    if [[ "$MODE" == "release" ]]; then
        cargo build --release --bin "$PROJECT_NAME"
    else
        cargo build --bin "$PROJECT_NAME"
    fi

    echo -e "${GREEN}[INFO] 编译完成${NC}"
    echo ""
    EXE=$(find_executable)
fi

if [[ -z "$EXE" || ! -f "$EXE" ]]; then
    echo -e "${RED}[ERROR] 找不到可执行文件${NC}"
    exit 1
fi

chmod +x "$EXE"

# ========== 检查已有实例 ==========
EXISTING=$(pgrep -f "$PROJECT_NAME" | head -1 || true)
if [[ -n "$EXISTING" && $FOREGROUND -eq 0 ]]; then
    echo -e "${YELLOW}[WARN] 检测到已有实例在运行 (PID: $EXISTING)${NC}"
    echo -e "${YELLOW}       如需重启请先执行: ./start.sh --stop${NC}"
    exit 1
fi

# ========== 启动 ==========
ACTUAL_PORT=$([[ $PORT -gt 0 ]] && echo "$PORT" || echo "8080")

if [[ $FOREGROUND -eq 1 ]]; then
    echo -e "${GREEN}[INFO] 前台运行模式 — 按 Ctrl+C 停止服务${NC}"
    echo -e "${GREEN}[INFO] 访问地址: http://localhost:$ACTUAL_PORT${NC}"
    echo ""

    trap 'echo ""; echo -e "${YELLOW}[INFO] 服务已停止${NC}"; exit 0' INT
    "$EXE"

    EXIT_CODE=$?
    if [[ $EXIT_CODE -ne 0 ]]; then
        echo ""
        echo -e "${RED}[ERROR] 程序异常退出 (退出码: $EXIT_CODE)${NC}"
    fi
else
    # 后台运行
    echo -e "${GREEN}[INFO] 后台运行模式${NC}"
    echo -e "${GREEN}[INFO] 访问地址: http://localhost:$ACTUAL_PORT${NC}"
    echo -e "${GREEN}[INFO] 日志文件: $LOG_FILE${NC}"
    echo -e "${GREEN}[INFO] 停止命令: ./start.sh --stop${NC}"
    echo ""

    nohup "$EXE" > "$LOG_FILE" 2>&1 &
    NEW_PID=$!
    echo $NEW_PID > "$PID_FILE"

    sleep 3

    # 健康检查
    if command -v curl >/dev/null 2>&1; then
        RESP=$(curl -s --max-time 5 "http://localhost:$ACTUAL_PORT/api/summary" 2>/dev/null || true)
        if [[ -n "$RESP" ]]; then
            echo -e "${GREEN}[INFO] 服务启动成功! API 响应正常${NC}"
            echo -e "       $RESP"
        else
            echo -e "${YELLOW}[WARN] 服务可能仍在初始化，API 尚未响应${NC}"
            echo -e "       3 秒后再次检查，或查看日志: ./start.sh --logs"
        fi
    else
        echo -e "${GREEN}[INFO] 服务已启动 (PID: $NEW_PID)${NC}"
    fi
fi
