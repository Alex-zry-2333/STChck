# 气象站数据监控系统 - 代理开发指南

> 本文档面向 AI 编码代理。阅读者被假定为对项目一无所知。以下内容均基于项目实际文件整理，不做假设性推广。

---

## 1. 项目概述

本项目是一个气象站数据监控可视化平台，后端主要使用 Rust + Axum 实现，前端使用原生 JavaScript + ECharts 5。监控业务逻辑完整移植自同目录下的 `tm.c`。

项目支持两种运行模式：

- **模拟模式**（默认）：不连接生产数据库，使用本地 SQLite 数据库 `test_data.db` 和内存/随机数据生成，用于本地测试和前端功能验证。
- **真实模式**：连接 MySQL 主库 `cammoc_w` 和云库 `cammoc_cloud_w`，查询 `data_st`、`station_params`、`device_type` 等表。

运行时会在后台定期刷新监控数据，并通过 SSE（Server-Sent Events）推送给前端浏览器。

---

## 2. 技术栈

| 层级 | 技术 |
|------|------|
| 后端 | Rust 2021 edition |
| Web 框架 | Axum 0.8 |
| 异步运行时 | Tokio 1（full features） |
| 数据库访问 | sqlx 0.8（MySQL + SQLite） |
| 序列化/配置 | serde、serde_json、toml |
| 日志 | tracing、tracing-subscriber |
| 静态文件/模板 | tower-http（fs feature，但实际模板直接 `include_str!` 嵌入） |
| 前端 | 原生 JS + ECharts 5（CDN 引入） |
| 可选数据库 | MySQL 5.7+ / 8.0+ |
| 本地测试数据库 | SQLite（`test_data.db`） |

---

## 3. 目录结构

```
weather-monitor/
├── Cargo.toml              # Rust 依赖与包配置
├── config.toml             # 应用运行时配置（默认不存在，首次启动从 .example 复制）
├── config.toml.example     # 配置模板，密码使用 ${DB_PASSWORD} 占位
├── README.md               # 人类可读的项目说明
├── CHANGELOG.md            # 版本与安全修复记录
├── AGENTS.md               # 本文件
├── start.bat               # Windows CMD 一键启动脚本
├── start.ps1               # Windows PowerShell / 跨平台启动脚本
├── start.sh                # Linux/macOS Bash 启动脚本
├── monitor_app.py          # 早期 Python/Flask 原型实现（保留参考，非主入口）
├── tm.c                      # C 语言原始业务逻辑参考实现
├── src/
│   ├── main.rs             # Axum 路由、SSE、后台任务、启动逻辑
│   ├── config.rs           # TOML 配置结构体与加载
│   ├── db.rs               # 数据库服务：MySQL / Doris / SQLite / 模拟数据
│   ├── monitor.rs          # 业务逻辑核心：ST 包解析、告警文本、模拟数据
│   └── models.rs           # 全部数据结构定义（JSON 响应/SSE 负载）
├── vendor/
│   └── sqlx-mysql/         # sqlx-mysql 0.8.6 本地补丁（[patch.crates-io]），
│                           # 兼容 Doris FE 的 10 字节 PrepareOk，必须随仓库提交
├── examples/
│   └── doris_probe.rs      # Doris 连接诊断探针（DORIS_URL 环境变量传入连接串）
├── docs/
│   ├── roles/              # 角色提示词（架构师/数据库工程师/测试工程师工作流）
│   └── doris-support-design.md  # Doris 支持设计文档与联调记录
├── templates/
│   ├── dashboard.html      # 主监控仪表盘（ECharts + SSE）
│   └── devices.html        # 站内设备状态详情页
└── target/                 # cargo 编译输出
```

---

## 4. 构建与运行命令

### 4.1 环境要求

- Rust >= 1.70.0
- MySQL（仅真实模式需要；模拟模式会自动创建 SQLite 文件）
- 操作系统：Windows 10+ / Ubuntu 20.04+ / macOS 12+

### 4.2 开发构建

```bash
cargo build
```

### 4.3 生产构建

```bash
cargo build --release
```

输出路径：

- Windows：`target\release\weather-monitor.exe`
- Linux/macOS：`target/release/weather-monitor`

### 4.4 直接运行

```bash
cargo run --release
```

### 4.5 使用启动脚本（推荐）

Windows：

```powershell
.\start.ps1
# 或
start.bat
```

Linux/macOS：

```bash
chmod +x start.sh
./start.sh
```

启动脚本行为：

1. 若 `config.toml` 不存在，从 `config.toml.example` 复制一份。
2. 若编译后的二进制不存在，自动执行 `cargo build`（debug 模式）。
3. 运行二进制。

---

## 5. 配置说明

配置文件为 TOML 格式，加载顺序：

1. 当前工作目录的 `config.toml`
2. 若不存在，尝试加载可执行文件同目录的 `config.toml`

### 5.1 配置字段

```toml
[server]
port = 8080                    # Web 服务端口，最大值 65535
refresh_interval_secs = 120    # 监控数据后台刷新间隔（秒）

[database]
host = "10.10.1.59"
port = 3306
user = "root"
password = "${DB_PASSWORD}"    # 会被 DB_PASSWORD 环境变量覆盖
db = "cammoc_w"

[cloud_database]
host = "10.10.1.59"
port = 3306
user = "root"
password = "${CLOUD_DB_PASSWORD}"  # 会被 CLOUD_DB_PASSWORD 环境变量覆盖
db = "cammoc_cloud_w"

# 可选：Doris 数据源（仅真实模式且 data_source = "doris" 时生效）
# [doris]
# host = "10.10.1.60"
# port = 9030                    # Doris FE MySQL 协议查询端口
# user = "root"
# password = "${DORIS_DB_PASSWORD}"  # 会被 DORIS_DB_PASSWORD 环境变量覆盖
# db = "cammoc_w"

[monitor]
check_interval_minutes = 5     # 查询最近 N 分钟的数据
simulation_mode = true         # true=模拟模式，false=真实数据库模式
data_source = "mysql"          # 真实模式数据源：mysql（默认）或 doris

[[stations]]                   # 可重复，定义监控站点列表
id = "50936"
name = "吉林白城"
vendor = "华云"
```

### 5.2 环境变量覆盖

出于安全考虑，数据库密码**不应**直接写入 `config.toml`：

```bash
# Linux/macOS
export DB_PASSWORD="实际密码"
export CLOUD_DB_PASSWORD="实际密码"
export DORIS_DB_PASSWORD="实际密码"   # 仅使用 Doris 数据源时需要

# Windows PowerShell
$env:DB_PASSWORD="实际密码"
$env:CLOUD_DB_PASSWORD="实际密码"
$env:DORIS_DB_PASSWORD="实际密码"     # 仅使用 Doris 数据源时需要
```

`src/main.rs` 启动时会读取这些环境变量并覆盖配置文件中的密码。

### 5.3 模式切换

- 模拟模式：`simulation_mode = true`（默认），会创建 `test_data.db`，不连接生产 MySQL。
- 真实模式（MySQL）：`simulation_mode = false`，`data_source = "mysql"`（默认），并正确设置数据库环境变量。
- 真实模式（Doris）：`simulation_mode = false`，`data_source = "doris"`，配置 `[doris]` 段并设置 `DORIS_DB_PASSWORD`。Doris 走 MySQL 协议（FE 查询端口默认 9030），SQL 参数经校验/转义后内联走文本协议；连接失败自动降级为模拟模式。

---

## 6. 代码组织与模块职责

### 6.1 `src/config.rs`

- 定义 `Config`、`ServerConfig`、`DatabaseConfig`、`MonitorConfig`、`StationConfig` 结构体。
- 提供 `Config::load(path)`，从 TOML 文件加载配置，支持回退到可执行文件同目录。
- 无测试代码，结构简单直接。

### 6.2 `src/models.rs`

- 集中定义所有用于 JSON 序列化的数据结构：
  - `MonitorData`、`MonitorSummary`、`StationStatus`
  - `StationMeta`、`StationDetail`、`RegionStats`、`TopLists`
  - `ChartAlarmResponse`、`ChartValueResponse`
  - `DeviceStatusInfo`、`DeviceStatusItem`、`CheckItem`
- 所有结构体均派生 `Serialize`/`Deserialize`。

### 6.3 `src/monitor.rs`

业务核心，移植自 `tm.c`：

- `get_alarm(item, value)`：根据状态项编码生成中文告警文本。
- `is_kit(item)`：判断状态项是否为需要监控的项。
- `parse_st_packet(data)`：解析 DATADICK ST 数据包，提取异常项。
- `generate_simulated_data(stations)`：模拟模式下的监控概览数据。
- `generate_alarm_trend(...)` / `generate_value_trend(...)`：图表趋势模拟数据。

### 6.4 `src/db.rs`

数据库访问层：

- `DbService::new()`：连接 MySQL 主库和云库。
- `DbService::new_simulation()`：创建 SQLite `test_data.db`，初始化测试数据。
- `load_station_meta()`：加载站点元数据（省份、经纬度等）。
- `query_monitor_data()`：查询最近数据并生成 `MonitorData`。
- `get_station_devices()`：查询某站点的设备列表和状态。
- `classify_device_status()` / `get_status_item_name()`：状态项分类与中文化。

注意：

- SQLite 优先用于模拟模式。
- 所有用户输入均使用 `?` 占位符 + `bind()` 参数化，已修复 SQL 注入问题。
- 真实模式下，若 MySQL 连接失败会自动降级到模拟模式。

### 6.5 `src/main.rs`

应用入口与 Web 层：

- 加载配置，注入环境变量密码。
- 初始化 `DbService`、`AppState`（含 `RwLock` 数据缓存、broadcast SSE 通道）。
- 启动后台任务：
  - 监控数据刷新：首次延迟 500ms，之后按 `refresh_interval_secs` 循环。
  - 站点设备缓存预热与刷新：启动立即执行一次，之后每 5 分钟刷新。
- 定义 Axum 路由。
- Windows 下会尝试清理已有的 `weather-monitor.exe` 进程以释放端口。

---

## 7. HTTP API 端点

| 端点 | 说明 |
|------|------|
| `GET /` | 主仪表盘 HTML |
| `GET /station/{id}/devices` | 站内设备详情页 HTML |
| `GET /api/status` | 全量监控状态 JSON |
| `GET /api/summary` | 汇总统计 JSON |
| `GET /api/stations` | 站点状态列表 JSON |
| `GET /api/config` | 公开配置（已脱敏，不含数据库字段） |
| `GET /api/regions` | 按省份分组的统计 |
| `GET /api/station/{id}` | 单个站点详情 |
| `GET /api/top?limit=N` | 各类 Top N 站点 |
| `GET /api/map/stations` | 地图展示所需站点元数据与状态 |
| `GET /api/station/{id}/devices` | 站点设备状态 |
| `GET /api/devices/events` | 设备状态 SSE 流 |
| `GET /api/chart/alarms?hours=24` | 告警趋势图数据 |
| `GET /api/chart/values?station=50936&item=wA&hours=6` | 设备数值曲线数据 |
| `GET /api/events` | 监控数据 SSE 流 |

---

## 8. 开发与编码约定

### 8.1 语言与注释

- 源码注释、文档、UI 文本、告警文案主要使用**中文**。
- 修改代码时，保持中文注释风格；新增告警文本应符合 `monitor.rs` 中的中文命名习惯。

### 8.2 状态项编码规则

气象 ST 包中的状态项有固定前缀：

- `a*`：其他工作/酸雨盖/机箱门/存储卡/水位等
- `q*`：分钟数据质量
- `r*`：采样数据
- `s*`：污染状态
- `t*`：通信状态
- `u*`：通风部件
- `v*`：加热部件
- `w*`：温度状态
- `x*`：供电状态
- `y*`：测量仪状态
- `z`：设备自检

修改告警解析逻辑时，需同步更新 `monitor.rs::get_alarm`、`db.rs::get_status_item_name`、`db.rs::classify_device_status` 三处。

### 8.3 数据库查询

- **严禁**字符串拼接 SQL。所有动态参数必须使用 `?` 占位符 + `bind()`。
- `IN` 列表的动态长度通过循环 `bind` 实现，参考 `db.rs::load_station_meta` 和 `query_monitor_data`。
- **Doris 路径例外**（见 `docs/doris-support-design.md` 附录 A）：Doris 查询参数经校验/转义后内联（`valid_interval_minutes` 钳制整数、`sql_escape` 转义字符串），且一律使用**位置元组解码**（Doris 列元数据与 sqlx 按名取列不兼容）；连接必须用 `pipes_as_concat(false)` + `no_engine_substitution(false)` + `timezone(None)`。
- Doris 兼容性依赖 `vendor/sqlx-mysql` 本地补丁（PrepareOk 10 字节兼容），**不得删除 `vendor/` 目录或 `[patch.crates-io]` 配置**。

### 8.4 密码与敏感信息

- `config.toml` 已被 `.gitignore` 忽略，不可提交真实密码。
- 不要在代码中硬编码密码、数据库主机等敏感信息。
- `/api/config` 已脱敏，只返回 `server`、`monitor`、`stations`。

---

## 9. 测试策略

项目**没有正式的单元测试或集成测试文件**（无 `tests/` 目录或 `#[cfg(test)]` 模块）。验证功能的主要方式：

1. **模拟模式启动**：`cargo run --release`，访问 `http://localhost:8080` 检查仪表盘、图表、设备页、SSE 推送。
2. **真实模式测试**：设置 `DB_PASSWORD` / `CLOUD_DB_PASSWORD`，关闭 `simulation_mode`。
3. **API 手动验证**：使用 curl 调用各 `/api/*` 端点。
4. **安全回归**：检查 `/api/config` 不返回 `database` 字段；确认 SQL 参数化。

---

## 10. 安全注意事项

项目历史上曾进行过安全修复（详见 `CHANGELOG.md`），编码代理应特别注意：

1. **密码管理**：仅通过环境变量传入；配置文件使用占位符。
2. **SQL 注入**：所有 SQL 查询必须参数化。
3. **配置脱敏**：公开 API 不得暴露数据库连接信息。
4. **缓冲区安全**：参考 `tm.c` 的修复，Rust 代码中避免不安全的字符串操作（目前代码均使用安全 Rust）。
5. **端口绑定**：程序会校验端口 <= 65535，并在被占用时自动尝试后续 10 个端口。

---

## 11. 部署与启动流程

### 11.1 本地开发

```bash
cargo run --release
```

### 11.2 生产部署

1. 复制 `config.toml.example` 为 `config.toml`。
2. 修改 `simulation_mode = false` 并配置正确的数据库信息。
3. 设置环境变量：
   ```bash
   export DB_PASSWORD="xxx"
   export CLOUD_DB_PASSWORD="xxx"
   ```
4. 构建并运行：
   ```bash
   cargo build --release
   ./target/release/weather-monitor
   ```

### 11.3 跨平台编译

参考 `README.md`：

```bash
# Ubuntu 依赖
sudo apt-get install -y build-essential libssl-dev pkg-config

# Windows → Linux 交叉编译
rustup target add x86_64-unknown-linux-gnu
cargo build --release --target x86_64-unknown-linux-gnu
```

---

## 12. 对 AI 代理的特别提示

1. **不要修改 `tm.c`**：它是原始业务逻辑参考，除非明确收到安全修复需求。
2. **不要修改 `monitor_app.py` 的核心业务逻辑**：它是旧版 Python 原型，保持与 Rust 版本逻辑一致即可。
3. **新增状态项告警时**，同步修改 `monitor.rs`、`db.rs::get_status_item_name`、`db.rs::classify_device_status`。
4. **修改配置结构时**，同步更新 `config.toml.example` 和 `src/main.rs::PublicConfig`。
5. **修改数据库查询时**，必须参数化；如新增动态 `IN` 列表，参考现有实现循环 `bind`。
6. **新增 API 端点时**，在 `src/main.rs` 中注册路由，并在 `README.md` 的 API 表格中补充说明。
7. **保持中文注释与 UI 文本**：新增页面文案优先使用中文。

---

*本文件最后基于项目实际内容生成。修改项目结构、配置、构建方式或安全策略时，请同步更新本文件。*
