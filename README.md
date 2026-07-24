# 气象站数据监控系统 (Weather Station Monitor)

基于 Rust + Axum + ECharts 的气象站数据监控可视化平台，监控逻辑完整移植自 `tm.c` 业务代码。

## 功能特性

- **实时监控**: 支持 32+ 气象站状态监控（在线/离线/报警）
- **双模式运行**: 真实 MySQL 数据库模式 / 模拟数据模式（自动降级）
- **交互式图表**: ECharts 折线图，支持缩放、悬停提示、图例切换、阈值标线
- **实时推送**: SSE 服务器推送，数据更新即时同步到浏览器
- **可配置化**: 通过 `config.toml` 配置端口、数据库、站台列表、监控间隔等
- **跨平台**: 支持 Windows / Ubuntu / macOS

## 系统要求

- **Rust**: >= 1.70.0
- **MySQL** (可选): 5.7+ 或 8.0+（仅真实模式需要）
- **操作系统**: Windows 10+, Ubuntu 20.04+, macOS 12+

## 安装

### 1. 克隆/下载项目

```bash
cd weather-monitor
```

### 2. 编译

```bash
# 开发模式
cargo build

# 生产模式（推荐）
cargo build --release
```

编译完成后，二进制文件位于：
- **Windows**: `target/release/weather-monitor.exe`
- **Linux/macOS**: `target/release/weather-monitor`

## 配置

编辑项目根目录的 `config.toml`：

```toml
[server]
port = 8080                    # Web 服务端口（最大 65535）
refresh_interval_secs = 30     # 数据刷新间隔（秒）

[database]
host = "10.10.1.59"           # MySQL 主机
port = 3306                   # MySQL 端口
user = "root"                 # MySQL 用户名
password = "root"             # MySQL 密码
db = "cammoc_w"               # 数据库名

[monitor]
check_interval_minutes = 10   # 查询最近 N 分钟的数据
simulation_mode = true        # true=模拟模式, false=连接真实数据库

[[stations]]                  # 站台列表（可自由增删）
id = "50936"
name = "吉林白城"
vendor = "华云"
```

### 模式说明

| 模式 | 配置项 | 说明 |
|------|--------|------|
| 模拟模式 | `simulation_mode = true` | 无需数据库，生成随机模拟数据 |
| 真实模式 | `simulation_mode = false` | 连接 MySQL 查询 `data_st` 表 |

## 使用方法

### 方式一：使用启动脚本（推荐）

#### Windows

```powershell
# PowerShell
.\start.ps1

# 或 CMD
start.bat
```

#### Linux / macOS

```bash
chmod +x start.sh
./start.sh
```

### 方式二：直接运行

```bash
# Windows
.\target\release\weather-monitor.exe

# Linux / macOS
./target/release/weather-monitor
```

### 方式三：开发运行

```bash
cargo run --release
```

## 访问监控界面

启动后打开浏览器访问：

```
http://localhost:8080
```

### 界面功能

1. **报警趋势图**: 顶部区域，展示各站历史报警次数折线
   - 支持时间范围切换（1h/6h/24h/3d/7d）
   - 鼠标滚轮缩放、拖拽平移
   - 点击图例切换站点显示

2. **站台卡片**: 中部区域，展示 32+ 站实时状态
   - 搜索框：按站号/站名/厂商筛选
   - 状态筛选：全部/在线/离线/报警
   - 点击卡片可跳转到数值曲线图

3. **设备数值曲线**: 底部区域，展示具体监测项的时间序列
   - 选择站点 + 监测项（温度/电压/电流等）
   - 阈值标线自动标注上下限
   - 面积图 + 平滑曲线

## API 接口

| 端点 | 方法 | 说明 |
|------|------|------|
| `GET /` | - | 监控仪表盘 HTML |
| `GET /api/status` | - | 全量监控状态 JSON |
| `GET /api/summary` | - | 汇总统计 JSON |
| `GET /api/stations` | - | 站台列表 JSON |
| `GET /api/config` | - | 当前配置 JSON |
| `GET /api/chart/alarms?hours=24` | - | 报警趋势图数据 |
| `GET /api/chart/values?station=50936&item=wA&hours=6` | - | 设备数值曲线数据 |
| `GET /api/events` | - | SSE 实时推送流 |

### API 示例

```bash
# 获取状态
curl http://localhost:8080/api/status

# 获取 24 小时报警趋势
curl "http://localhost:8080/api/chart/alarms?hours=24"

# 获取站点 50936 的温度(wA) 6 小时曲线
curl "http://localhost:8080/api/chart/values?station=50936&item=wA&hours=6"
```

## 项目结构

```
weather-monitor/
├── Cargo.toml              # Rust 依赖配置
├── config.toml             # 应用配置文件
├── README.md               # 本文件
├── start.bat               # Windows CMD 启动脚本
├── start.ps1               # Windows PowerShell 启动脚本
├── start.sh                # Linux/macOS 启动脚本
├── src/
│   ├── main.rs             # axum Web 服务器 + 路由
│   ├── config.rs           # TOML 配置解析
│   ├── db.rs               # sqlx MySQL 数据库服务
│   ├── monitor.rs          # 核心业务逻辑（tm.c 移植）
│   └── models.rs           # 数据结构定义
└── templates/
    └── dashboard.html      # ECharts 交互仪表盘
```

## 业务逻辑移植对照

| tm.c 函数 | Rust 实现 | 文件 |
|-----------|-----------|------|
| `mSTA[]` 30+站 | `config.toml` + `StationConfig` | `config.rs` |
| `getALM(I, V)` | `get_alarm(item, value)` | `monitor.rs` |
| `isKIT(m)` | `is_kit(item)` | `monitor.rs` |
| `parse_st_packet()` | `parse_st_packet(data)` | `monitor.rs` |
| `sqlProST()` | `DbService::query_monitor_data()` | `db.rs` |
| `getSid()` | `stations.iter().find()` | `db.rs` |

## 跨平台编译

### Windows 本地编译

```bash
cargo build --release
# 输出: target/release/weather-monitor.exe
```

### Ubuntu 编译

```bash
# 安装依赖
sudo apt-get update
sudo apt-get install -y build-essential libssl-dev pkg-config

# 编译
cargo build --release
# 输出: target/release/weather-monitor
```

### 交叉编译（Windows → Linux）

```bash
# 安装交叉编译工具链
rustup target add x86_64-unknown-linux-gnu

# 编译
cargo build --release --target x86_64-unknown-linux-gnu
```

## 常见问题

### Q: 端口 114514 无法使用？
A: TCP 端口最大值为 65535。若配置中写 114514，程序会自动降级到 8080 并打印警告。请修改 `config.toml` 中的 `port` 为有效值。

### Q: MySQL 连接失败怎么办？
A: 程序会自动降级到模拟模式。检查 `config.toml` 中的数据库配置，或设置 `simulation_mode = true` 使用模拟数据。

### Q: 如何添加新的监控站点？
A: 在 `config.toml` 中添加新的 `[[stations]]` 段落即可：

```toml
[[stations]]
id = "NEW01"
name = "新站点名称"
vendor = "厂商名"
```

## 技术栈

- **后端**: Rust + Axum + Tokio + sqlx
- **前端**: Vanilla JS + ECharts 5
- **数据库**: MySQL (可选)
- **协议**: HTTP/1.1 + SSE (Server-Sent Events)

## License

MIT
