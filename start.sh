#!/bin/bash

set -e

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
CYAN='\033[0;36m'
NC='\033[0m' # No Color

echo -e "${CYAN}==========================================${NC}"
echo -e "${CYAN}  气象站数据监控系统启动脚本${NC}"
echo -e "${CYAN}==========================================${NC}"
echo ""

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
cd "$SCRIPT_DIR"

# 检查 config.toml
if [ ! -f "config.toml" ]; then
    if [ -f "config.toml.example" ]; then
        echo -e "${YELLOW}[INFO] config.toml 不存在，从模板创建...${NC}"
        cp "config.toml.example" "config.toml"
        echo -e "${YELLOW}[INFO] 已创建 config.toml，请根据环境编辑数据库配置${NC}"
        echo ""
    else
        echo -e "${YELLOW}[WARNING] config.toml 和 config.toml.example 均不存在，使用默认配置${NC}"
        echo ""
    fi
fi

# 检查可执行文件，不存在则自动编译
EXE_PATH=""
for path in "target/release/weather-monitor" "target/debug/weather-monitor"; do
    if [ -f "$path" ]; then
        EXE_PATH="$path"
        break
    fi
done

if [ -z "$EXE_PATH" ]; then
    echo -e "${YELLOW}[INFO] 可执行文件不存在，正在编译（Debug 模式）...${NC}"
    echo ""
    if command -v cargo &> /dev/null; then
        cargo build
    else
        echo -e "${RED}[ERROR] 未找到 cargo，请手动编译：cargo build --release${NC}"
        echo ""
        read -p "按 Enter 键退出"
        exit 1
    fi
    EXE_PATH="target/debug/weather-monitor"
fi

# Make executable
chmod +x "$EXE_PATH"

echo -e "${GREEN}[INFO] 启动程序: $EXE_PATH${NC}"
echo -e "${GREEN}[INFO] 访问地址: http://localhost:8080${NC}"
echo -e "${GREEN}[INFO] 按 Ctrl+C 停止服务${NC}"
echo ""

# Trap Ctrl+C
trap 'echo ""; echo -e "${YELLOW}[INFO] 服务已停止${NC}"; exit 0' INT

# Run
"$EXE_PATH"

EXIT_CODE=$?
if [ $EXIT_CODE -ne 0 ]; then
    echo ""
    echo -e "${RED}[ERROR] 程序异常退出 (退出码: $EXIT_CODE)${NC}"
    read -p "按 Enter 键关闭"
fi
