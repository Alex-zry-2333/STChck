# Weather Monitor Changelog

## 版本跟踪记录

> 本文档用于记录项目所有修改，便于回溯和版本管理。

---

## 2026-08-07 功能：时间段监察 + 全链路调试日志

### 1. 时间段监察（time-range inspection）

**影响**: 新增按起止时间回看历史数据的监察能力（与实时窗口互补）。需求与决策记录见 `docs/time-range-inspection-requirements.md`（D1–D10 业务已确认），设计见 `docs/time-range-inspection-design.md`。

| 文件 | 修改内容 |
|------|----------|
| `src/models.rs` | 新增监察响应结构：`InspectionOverviewResponse` / `StationInspection` / `DeviceInspection` / `GapInterval` / `InspectionAlarmsResponse` / `InspectionAlarmEvent` |
| `src/monitor.rs` | 新增 `parse_st_alarms_fast`（D10 定点解析：惰性迭代、跳过固定 7 字段帧头、仅解码监控项，语义与 `parse_st_packet` 等价）；`merge_gap_intervals`（缺报分钟合并为区间）；模拟模式合成函数；`#[cfg(test)]` 对拍与边界测试 |
| `src/db.rs` | 新增 `query_inspection_device_stats` / `query_station_data_times` / `fetch_st_packets_page`，均 Doris（转义内联+元组解码）/ MySQL（`?` 绑定）双路径，模拟模式返回空由上层合成 |
| `src/main.rs` | 新增路由 `GET /inspection`（页面）、`GET /api/inspection/overview`、`GET /api/inspection/alarms`；时间参数校验（默认最近 1 小时、跨度 ≤ 7 天、非法输入 400）；键集分页游标 `data_time\|device_type\|device_nid`（D9，不用 OFFSET）；`[时段监察]` 日志前缀 |
| `templates/inspection.html` | 新增监察页面：起止时间选择、站点下拉（全部/单站下钻）、到报率总览表、缺报区间展示、告警时间线翻页 |

QA 验收（Doris 实测 10.10.1.67，测试数据时段 16:30–17:36）：32 站总览与手工 SQL 抽查一致（56739 实有分钟 52=52、54333 设备数 29=29）；缺报分钟 16:41 正确出现在 gaps；告警时间线 6 页键集分页累计 1072 包与 SQL COUNT 一致、无重复无遗漏；5 类非法输入均 400；模拟模式降级返回合成数据（`simulation: true` 标识）；`cargo test` 31 项通过（含定点解析对拍）。

注意：YPOWR00 智能电源自定义格式本期维持现有解析行为，后续单独讨论；帧头 7 字段为业务确认的强约定。

### 2. 全链路调试日志

**影响**: 便于后台跟踪每个环节执行步骤。

| 文件 | 修改内容 |
|------|----------|
| `src/main.rs` / `src/db.rs` | 日志同时输出控制台与每日滚动文件 `logs/weather-monitor.log.YYYY-MM-DD`；级别由 `RUST_LOG` 控制（如 `weather_monitor=debug`）；启动/元数据/Doris 查询/监控刷新/设备缓存等环节带前缀标记 |
| `Cargo.toml` | 新增 `tracing-appender` 依赖 |

### 3. 启动脚本与安全

| 文件 | 修改内容 |
|------|----------|
| `start.ps1` / `start.bat` | 已配置 Doris 但未设 `DORIS_DB_PASSWORD` 时给出明确警告；bat 纯 ASCII 化、ps1 加 BOM 修复中文乱码；提示调试日志用法 |
| `.gitignore` | 忽略 `start-doris.ps1` / `start-doris.bat`（含 Doris 密码，严禁提交）与 `logs/` |

---

## 2026-08-07 功能：Apache Doris 数据源支持（含联调修复）

### 1. Doris 数据源接入

**影响**: 真实模式下可将主库切换为 Apache Doris（经 MySQL 协议连接 FE 查询端口 9030），云库与模拟模式行为不变；已通过测试 Doris 环境全链路联调。

| 文件 | 修改内容 |
|------|----------|
| `src/config.rs` | 新增 `DorisConfig`（含 `st_table` / `station_table` 可选表名配置）；`Config` 新增可选 `[doris]` 段；`MonitorConfig` 新增 `data_source`（`mysql` 默认 / `doris`） |
| `src/db.rs` | `DbService` 新增 `is_doris` 与 Doris 表名字段；新增 `new_with_source` 构造函数；Doris 路径查询适配（表名 `ods_data_st`、入库时间列 `create_time`、无自增 id 的 ORDER BY）；站点元数据改查 `station_info`（无地理列，用内置回退表补齐省份/经纬度） |
| `src/main.rs` | 新增 `DORIS_DB_PASSWORD` 环境变量注入；数据源三分支选择（模拟 / MySQL / Doris） |
| `config.toml.example` | 新增 `[doris]` 段模板与 `data_source` 注释 |

### 2. Doris 协议兼容性修复（联调中发现）

**影响**: sqlx 默认行为与 Doris FE 存在三层不兼容，直连全部失败。

| 问题 | 修复 |
|------|------|
| 连接初始化 `SET sql_mode=(SELECT CONCAT(...))` 被 Doris 拒绝（1105 非常量表达式） | Doris 连接使用 `pipes_as_concat(false)` + `no_engine_substitution(false)` + `timezone(None)` |
| Doris FE 返回 10 字节 PrepareOk（标准 12 字节，缺 warnings 字段），sqlx 严格解析失败 | `vendor/sqlx-mysql` 本地补丁（warnings 字段可选），`Cargo.toml` 以 `[patch.crates-io]` 挂载；**vendor 目录必须随仓库提交** |
| Doris 列元数据导致 sqlx 按名取列失败（ColumnNotFound） | Doris 路径一律使用位置元组解码，不用 `FromRow` 按名映射；MySQL/SQLite 路径保持原样 |
| （弃用方案）mysql_async 文本协议 | 实测 COM_QUERY 挂起，已回滚，未引入该依赖 |

Doris 路径 SQL 参数处理：整数经 `valid_interval_minutes` 钳制、字符串经 `sql_escape` 转义后内联（无注入面）。

### 3. 在线判定规则放宽（业务确认）

**影响**: 原 tm.c 移植规则 `recent_5min == devices && devices > 20` 假设 5 分钟级数据频率，分钟级数据下所有站点误报离线、ST 告警不触发。

| 文件 | 修改内容 |
|------|----------|
| `src/db.rs` | `query_monitor_data` 与 `query_monitor_data_doris` 在线判定放宽为「最近 6 分钟窗口有数据即在线」并执行 ST 包检查；MySQL / Doris 两条路径一致 |

联调实测（分钟级测试数据）：在线站点 0/32 → 27/32，ST 检查正常执行，设备状态页正常。

### 4. 文档与工具

| 文件 | 修改内容 |
|------|----------|
| `docs/roles/` | 新增角色提示词（系统架构师 / 数据库工程师 / 测试工程师）与「先设计后执行」工作流说明 |
| `docs/doris-support-design.md` | Doris 支持设计文档：现状分析、任务拆分、验收标准、D1–D10 决策记录与联调验证记录 |
| `docs/doris/ods_data_st_ddl.txt` | Doris 真实建表语句存档 |
| `examples/doris_probe.rs` | Doris 连接诊断探针（连接串经 `DORIS_URL` 环境变量传入，密码不落盘） |
| `README.md` | 新增 Doris 模式说明与配置示例 |
| `AGENTS.md` | 配置字段、模式切换、目录结构、数据库查询约定（Doris 例外条款与 vendor 补丁警示） |

## 修改文件列表

### 功能/数据
- `src/config.rs`（DorisConfig 与数据源开关）
- `src/db.rs`（Doris 连接与查询路径、在线判定放宽）
- `src/main.rs`（环境变量与数据源选择）
- `Cargo.toml` / `Cargo.lock`（[patch.crates-io] vendor 补丁挂载）
- `vendor/sqlx-mysql/`（新增，PrepareOk 兼容补丁）

### 配置/文档/工具
- `config.toml.example`、`README.md`、`AGENTS.md`
- `docs/`（新增，角色提示词与设计文档）
- `examples/doris_probe.rs`（新增，诊断工具）

## 未修改文件（保持原样）
- `tm.c`（原始业务逻辑参考）
- `monitor_app.py`（旧版 Python 原型）
- `src/monitor.rs`、`src/models.rs`（业务逻辑与数据结构未变）
- `templates/`（前端页面未变）

## 使用方式变更

Doris 模式（真实模式）配置：

```toml
[monitor]
simulation_mode = false
data_source = "doris"

[doris]
host = "doris-fe-host"
port = 9030
user = "root"
password = "${DORIS_DB_PASSWORD}"
db = "ods_iws"
# st_table = "ods_data_st"               # 默认值
# station_table = "ai_isos.station_info" # 默认值
```

```powershell
$env:DORIS_DB_PASSWORD = "实际密码"
.\start.ps1
```

注意事项：

1. `vendor/sqlx-mysql/` 是构建必需的本地补丁，不可删除；升级 sqlx 版本时需同步评估补丁。
2. 未配置 `[doris]` 或 `data_source` 缺省时行为与之前完全一致（向后兼容）。
3. Doris 连接失败时自动降级为模拟模式，与 MySQL 失败行为一致。

---

## 2025-06-16 安全修复（P0 优先级）

### 1. 密码泄露修复

**影响**: 数据库密码以明文硬编码在多处源码中，极易泄露。

| 文件 | 修改内容 |
|------|----------|
| `.gitignore` | 新增 `config.toml`，防止真实密码被误提交到 Git |
| `config.toml.example` | 新建模板文件，密码字段用 `${DB_PASSWORD}` 占位符 |
| `src/main.rs` | 加载配置后，用 `DB_PASSWORD` / `CLOUD_DB_PASSWORD` 环境变量覆盖文件中的密码 |
| `tm.c` | 数据库连接信息改为从 `DB_HOST`、`DB_USER`、`DB_PASSWORD`、`DB_NAME` 环境变量读取 |

### 2. `/api/config` 接口脱敏

**影响**: 访问 `http://localhost:8080/api/config` 即可获取完整数据库密码。

| 文件 | 修改内容 |
|------|----------|
| `src/main.rs` | 新增 `PublicConfig` 结构体，排除 `database` / `cloud_database` 字段；`api_config` 仅返回 `server`、`monitor`、`stations` |

### 3. SQL 注入修复（参数化查询）

**影响**: 用户可通过 URL 构造恶意 SQL 注入。

| 文件 | 修改点 |
|------|--------|
| `src/db.rs` `get_station_devices` | 用户输入的 `station_id` 改为 `?` 占位符绑定 |
| `src/db.rs` `query_monitor_data` | `basic_sql`、`device_sql`、`arrival_sql`、`st_sql` 全部使用 `?` 占位符 + `bind()`；动态 `IN` 列表通过 `?` 占位符循环绑定 |
| `src/db.rs` `load_station_meta` | `IN` 列表也改为参数化绑定 |
| `monitor_app.py` | 主查询和 ST 包查询均使用 `%s` 占位符 + `cursor.execute(sql, params)` |
| `tm.c` | `sqlProST` 使用 `mysql_real_escape_string` 转义；`main` 中每个站号用 `mysql_real_escape_string` 转义后 `strncat` 拼接 |

### 4. C 工具额外加固

| 文件 | 修改内容 |
|------|----------|
| `tm.c` | `tD` 增加边界校验（1–1440）；所有 `sprintf` 改为 `snprintf` / `strncat` |

---

## 2025-06-16 启动脚本改进

**目标**: 一键启动，无需手动创建配置或手动编译。

| 文件 | 修改内容 |
|------|----------|
| `start.bat` | 自动检查 `config.toml`（从模板复制）、自动检查编译后的二进制（未编译则自动 `cargo build`） |
| `start.ps1` | 同上，支持 PowerShell 跨平台 |
| `start.sh` | 同上，支持 Linux/macOS |

---

## 2025-06-16 测试数据增强

**目标**: 模拟模式下可完整测试前端所有功能。

| 文件 | 修改内容 |
|------|----------|
| `src/db.rs` `new_simulation()` | 预填充 20 种设备类型名称（温湿度仪、能见度仪、风仪、降水仪等） |
| `src/db.rs` `load_station_meta` | 模拟模式提供 32 个站点的真实省份 + 近似经纬度（支持地图展示） |
| `src/db.rs` `get_station_devices` | 模拟模式生成 10 台设备/站点，含在线/离线/异常状态随机分布 |
| `src/monitor.rs` | `generate_simulated_data` 保持原有（监控概览 + 告警数据） |

---

## 2025-06-16 SQLite 本地数据库 + 数据流修复

**目标**: 测试模式完全隔离 `10.10.1.59` 生产数据库；前端能实时获取数据。

### 1. SQLite 本地数据库

| 文件 | 修改内容 |
|------|----------|
| `Cargo.toml` | `sqlx` 特性添加 `sqlite`（支持 `sqlite::SqlitePoolOptions`） |
| `src/db.rs` | 结构体新增 `sqlite_pool: Option<sqlx::SqlitePool>` 字段 |
| `src/db.rs` `new_simulation()` | 改为 `async`，创建 `test_data.db` SQLite 文件；初始化 `data_st` 表和 `station_params` 表；插入 32 个站点、320 台设备测试数据 |
| `src/db.rs` `load_station_meta` | 优先使用 SQLite 查询；MySQL 仅作为 fallback |
| `src/db.rs` `get_station_devices` | 优先使用 SQLite 查询；SQLite 不可用时回退到纯内存模拟 |
| `src/main.rs` | 初始化 `DbService` 时调用 `new_simulation().await`（`async` 变更） |

### 2. 数据流修复

| 文件 | 修改内容 |
|------|----------|
| `src/main.rs` `api_station_devices` | **缓存缺失时不再返回空数组**，改为直接查询数据并返回，同时更新缓存和推送 SSE |
| `src/main.rs` 后台任务 | 启动时**立即预热设备缓存**（移除原来等待 300 秒的首个 tick），之后每 5 分钟刷新 |

---

## 修改文件列表（按类别）

### 安全相关
- `.gitignore`
- `config.toml.example`（新增）
- `src/main.rs`（密码注入 + api_config 脱敏）
- `src/db.rs`（SQL 注入修复）
- `monitor_app.py`（SQL 注入修复）
- `tm.c`（SQL 注入 + 缓冲区溢出修复）

### 启动脚本
- `start.bat`
- `start.ps1`
- `start.sh`

### 测试数据与本地数据库
- `Cargo.toml`（添加 sqlite 特性）
- `src/db.rs`（SQLite 数据库 + 模拟数据 + 站点元数据增强）
- `src/main.rs`（数据流修复 + 缓存预热）

---

## 未修改文件（保持原样）

- `src/config.rs`
- `src/monitor.rs`（原有代码未改动，仅被引用）
- `src/models.rs`（结构体未修改）
- `templates/dashboard.html`
- `templates/devices.html`
- `dashboard_check.html`
- `monitor_app.py`（Python 部分已在安全修复中修改）

---

## 当前测试模式使用方式

```powershell
# Windows 一键启动
.\start.bat

# PowerShell 推荐
.\start.ps1

# Linux/macOS
./start.sh
```

程序会自动创建 `test_data.db` SQLite 本地数据库，完全不触碰 `10.10.1.59` 的 MySQL。

## 生产环境切换方式

```powershell
$env:DB_PASSWORD = "实际密码"
$env:CLOUD_DB_PASSWORD = "实际密码"
.\start.ps1
```

同时在 `config.toml` 中将 `simulation_mode = true` 改为 `false`。

---

*最后更新: 2025-06-16*
