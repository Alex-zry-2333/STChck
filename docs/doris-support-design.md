# Doris 数据库支持 — 设计文档

> 角色产出：系统架构师（依据 `docs/roles/architect.md`）
> 日期：2026-08-07

## 1. 背景与目标

气象站监控系统的真实模式目前仅支持 MySQL（主库 `cammoc_w` + 云库 `cammoc_cloud_w`）。生产侧计划将 `data_st` 明细数据迁移到 Apache Doris（OLAP）。目标是：**在不改变现有 MySQL / 模拟模式行为的前提下**，新增 Doris 作为可选数据源，通过配置切换。

范围限定：

- Doris 仅承载主库职责（`data_st` 明细查询），云库（站点元数据 `station_params`、`device_type`）仍走 MySQL。
- 模拟模式（SQLite / 纯内存）不受影响。
- 图表接口（`/api/chart/*`）当前全部使用模拟数据，不在本次范围内。

## 2. 现状分析（证据）

| 项 | 证据 | 结论 |
|---|---|---|
| 驱动 | `Cargo.toml:28` sqlx 启用 `mysql`/`sqlite` | Doris 走 MySQL 协议，可复用现有驱动，无需新依赖 |
| 配置 | `src/config.rs:6-14` 仅 `database`/`cloud_database` | 需新增可选 `[doris]` 段与数据源开关 |
| 数据源选择 | `src/main.rs:182-187` 仅 simulation 二选一 | 需新增 Doris 分支 |
| 密码注入 | `src/main.rs:172-177` 仅 DB_PASSWORD / CLOUD_DB_PASSWORD | 需新增 DORIS_DB_PASSWORD |
| SQL 协议 | `src/db.rs` 全部查询使用 `.bind()` | sqlx 带参数时走 COM_STMT_PREPARE 预处理协议；Doris 预处理支持较弱，Doris 路径需改为文本协议（不带 bind 的查询走 COM_QUERY） |
| SQL 方言 | `NOW() - INTERVAL ? MINUTE`、`COUNT(IF(...))`、`COUNT(DISTINCT CONCAT(...))`、反引号、LIMIT | Doris 全部支持，SQL 文本无需重写，仅需参数内联 |
| 类型映射 | `chrono::NaiveDateTime`、`i64`、`String` | Doris 经 MySQL 协议返回的类型可正常解码 |
| 公开 API 脱敏 | `src/main.rs:109-113` PublicConfig 仅含 server/monitor/stations | 新增 doris 配置不得加入 PublicConfig |

## 3. 方案设计

### 3.1 配置结构

`src/config.rs`：

- `MonitorConfig` 新增字段 `data_source: String`，`#[serde(default = "default_data_source")]`，默认值 `"mysql"`。取值：`mysql` | `doris`。仅在 `simulation_mode = false` 时生效。
- `Config` 新增字段 `doris: Option<DatabaseConfig>`，`#[serde(default)]`，配置文件中为可选 `[doris]` 段。

`config.toml.example` 新增：

```toml
[monitor]
check_interval_minutes = 5
simulation_mode = true
# data_source = "mysql"   # mysql | doris，仅真实模式生效，默认 mysql

# 真实模式且 data_source = "doris" 时必填；Doris FE MySQL 协议查询端口默认 9030
# [doris]
# host = "10.10.1.60"
# port = 9030
# user = "root"
# password = "${DORIS_DB_PASSWORD}"
# db = "cammoc_w"
```

`src/main.rs`：读取 `DORIS_DB_PASSWORD` 环境变量覆盖 `cfg.doris.password`。

### 3.2 代码改动点

**`src/db.rs`**：

1. `DbService` 新增字段 `is_doris: bool`（默认 false）。
2. `DbService::new(cfg, cloud_cfg)` 保持签名不变（向后兼容），内部委托给新构造函数 `DbService::new_with_source(cfg, cloud_cfg, doris_cfg: Option<&DatabaseConfig>, data_source: &str)`：
   - `data_source == "doris"` 且配置了 `[doris]`：用 `mysql://user:pass@host:port/db` 连接 Doris FE（端口由配置给出，9030），日志标注"Doris 主库"。连接失败同样降级模拟模式。
   - 其他情况走原 MySQL 逻辑。
3. 新增辅助函数：
   - `fn sql_escape(s: &str) -> String`：转义 `\`、`'`（Doris 文本协议安全内联）。
   - `fn quote_ids(ids: &[String]) -> String`：生成 `'a','b','c'` 形式的已转义 IN 列表。
   - `fn valid_interval(n: i32) -> i32`：将间隔钳制到 `1..=1440`，保证内联的是受控整数。
4. `query_monitor_data`：当 `is_doris` 时，四条 SQL（basic / device / arrival / st）改为内联参数版本（不带 `.bind()`，走文本协议）。MySQL 路径保持参数化不变。
5. `get_station_devices` MySQL 路径（`src/db.rs:1012` 之后）：`is_doris` 时使用内联版本（station_id 转义内联）。
6. `load_station_meta` MySQL 主库回退路径：云库仍走 MySQL 参数化；若 Doris 库也承载 `station_params`（作为云库不可用时的回退），内联版本同上。

**`src/main.rs`**：

- `main()` 中数据源选择改为三分支：simulation → `new_simulation()`；否则调用 `new_with_source(...)`，内部按 `data_source` 决定 Doris/MySQL。

### 3.3 安全约束

- Doris 密码仅走 `DORIS_DB_PASSWORD` 环境变量；配置文件用占位符。
- Doris 路径的 SQL 内联仅限：受控整数（钳制）+ 经 `sql_escape` 转义的字符串（站点号来自 `config.toml`，仍按不可信输入处理）。
- `PublicConfig` 不含 `doris` 字段；`/api/config` 返回的 `monitor.data_source` 仅为模式标识，无敏感信息。
- `config.toml` 仍在 `.gitignore` 中。

## 4. 任务拆分清单

| # | 任务 | 产出 | 验证方式 | 依赖 |
|---|------|------|----------|------|
| T1 | `config.rs`：新增 `MonitorConfig.data_source`（默认 mysql）与 `Config.doris: Option<DatabaseConfig>` | 配置结构 | `cargo check` | — |
| T2 | `db.rs`：新增 `is_doris` 字段、`sql_escape`/`quote_ids`/`valid_interval` 辅助函数 | 辅助代码 | `cargo check` | — |
| T3 | `db.rs`：新增 `new_with_source`，`new` 委托之；Doris 连接分支 + 日志 | 连接逻辑 | `cargo check` | T1, T2 |
| T4 | `db.rs`：`query_monitor_data` Doris 内联查询分支 | 查询逻辑 | `cargo check` | T3 |
| T5 | `db.rs`：`get_station_devices`、`load_station_meta` 主库回退的 Doris 分支 | 查询逻辑 | `cargo check` | T3 |
| T6 | `main.rs`：DORIS_DB_PASSWORD 注入 + 数据源三分支选择 | 启动逻辑 | `cargo check` | T1, T3 |
| T7 | `config.toml.example`：新增 `[doris]` 段与 `data_source` 注释 | 配置模板 | 人工核对 | T1 |
| T8 | 文档同步：`README.md`（配置说明）、`AGENTS.md`（第 5 节配置字段） | 文档 | 人工核对 | T1–T7 |
| T9 | 构建验证：`cargo build` 零错误 + 模拟模式冒烟 | 验证报告 | QA 执行 | 全部 |

## 5. 验收标准

| # | 验收项 |
|---|--------|
| A1 | `cargo build` 零错误；新增警告为零或可说明 |
| A2 | 不配置 `[doris]` 且不设置 `data_source` 时，现有模拟模式 / MySQL 模式行为完全不变（向后兼容） |
| A3 | `simulation_mode = false` + `data_source = "doris"` + `[doris]` 配置完整时，启动日志显示连接 Doris 主库（host:9030/db） |
| A4 | Doris 连接失败时自动降级模拟模式，与 MySQL 失败行为一致 |
| A5 | Doris 路径所有 SQL 无 `.bind()`，整数经钳制、字符串经转义，无注入面 |
| A6 | `/api/config` 响应不含 `doris` 段任何字段 |
| A7 | `config.toml.example` 密码为 `${DORIS_DB_PASSWORD}` 占位符；文档已同步 |

## 6. 风险与回退

| 风险 | 缓解 |
|------|------|
| Doris 版本间 MySQL 协议兼容差异 | 仅使用 Doris 官方声明支持的函数集；文本协议避开预处理差异 |
| Doris 表结构与 MySQL 不一致（列类型/编码） | 真实联调时以 `data_st` 实际建表语句核对；失败自动降级模拟模式并在日志中给出原因 |
| 回退 | 删除/注释 `[doris]` 段或将 `data_source` 改回 `mysql` 即恢复原行为；代码层面 MySQL 路径零改动 |

---

## 附录 A：真实 Doris 环境适配决策（2026-08-07 联调）

拿到测试 Doris（10.10.1.67:9030）的真实 DDL 后，确认以下差异并作出决策：

| # | 差异 | 决策 |
|---|------|------|
| D1 | ST 表为 `ods_iws.ods_data_st`（按 `data_date` 天级动态分区） | 表名写入配置 `[doris].st_table`，默认 `ods_data_st`；时间过滤仍用 `create_time`/`data_time` 列，Doris 分区裁剪自动生效 |
| D2 | 入库时间列名为 `create_time`（非 `receive_time`） | Doris 路径所有 `receive_time` 替换为 `create_time`（语义一致：数据入库时间） |
| D3 | `ods_data_st` 无自增 `id` 列 | Doris 路径设备查询 ORDER BY 去掉 `id DESC`，以 `data_time DESC, create_time DESC` 定序 |
| D4 | 台站表为 `ai_isos.station_info`，仅 `station_num/station_name/supply`，无省份/经纬度/海拔 | 表名写入配置 `[doris].station_table`，默认 `ai_isos.station_info`（支持 `db.table` 全限定名）；元数据查询后省份/经纬度用内置回退表补齐（保证地图可用），其余字段置空 |
| D5 | Doris 部署无 MySQL 云库（`device_type`、`station_params`） | Doris 模式下站点元数据直接查 Doris `station_info`；`device_type` 名称表加载失败时沿用现有降级（显示设备编码） |
| D6 | 跨库查询（连接 `ods_iws`，查 `ai_isos.station_info`） | Doris 同 catalog 内支持 `db.table` 全限定名，连接库配置为 `ods_iws` |
| D7 | sqlx 连接初始化发送 `SET sql_mode=(SELECT CONCAT(...)), time_zone=...`，Doris 报 1105「Set statement does't support non-constant expr」 | 用 `MySqlConnectOptions` 关闭：`pipes_as_concat(false)`、`no_engine_substitution(false)`、`timezone(None)`（仅 Doris 连接；MySQL 路径不变） |
| D8 | sqlx 对带 bind 参数的查询走 COM_STMT_PREPARE；Doris FE 返回 10 字节 PrepareOk（标准 12 字节，缺 warnings 字段），sqlx 严格解析失败「PrepareOk expected 12 bytes but got 10 bytes」。备选方案 mysql_async 实测 COM_QUERY 挂起，弃用 | **vendor 补丁**：`Cargo.toml [patch.crates-io] sqlx-mysql = { path = "vendor/sqlx-mysql" }`，补丁 `prepare_ok.rs` 使 warnings 字段可选。补丁后预处理与文本协议均验证通过。vendor 目录必须随仓库提交 |
| D9 | Doris 列定义元数据导致 sqlx 按名取列失败（`ColumnNotFound`，`row.columns()` 名称正常但名称索引查找失败；位置索引正常） | **Doris 路径一律使用位置元组解码**（`query_as::<_, (A, B, ...)>`），不使用 `#[derive(FromRow)]` 按名映射。MySQL/SQLite 路径不受影响，保持原样 |
| D10 | ~~联调实测：Doris 测试数据为分钟级频率（5 分钟窗口内记录数 ≈ 5×设备数），原 tm.c 在线判定 `recent_5min == devices && devices > 20` 不触发，全部站点显示离线、ST 告警不检查~~ | **已关闭（2026-08-07 业务确认）**：分钟级数据频率符合预期。在线判定放宽为「最近窗口（`data_time` 在最近 6 分钟内）有数据即在线」，并同步执行 ST 包检查；MySQL / Doris 两条路径一致生效 |

### 联调验证记录（2026-08-07，Doris 10.10.1.67:9030）

- 探针（`examples/doris_probe.rs`，密码经 `DORIS_URL` 环境变量传入）：连接、聚合、`datetime` 解码、跨库 `station_info`、bind 参数全部通过。
- 全链路：`/api/summary` 返回真实记录数（3642 条/5 分钟窗口）；`/api/map/stations` 32 个站点（`station_info` 名称 + 内置地理回退）；`/api/station/50936/devices` 返回 21 台真实设备（YACRA00/YCLOD00/YEVAP00 等）及 ST 包解析状态。
- 已知现象：全部站点 online=false（见 D10）；设备在线判定 `data_time == 当前分钟` 在分钟级数据下滞后 1 分钟（同 tm.c 原逻辑）。

配置结构变更：`[doris]` 段由复用 `DatabaseConfig` 改为独立 `DorisConfig`，新增两个可选字段（带默认值，向后兼容）：

```toml
[doris]
host = "10.10.1.67"
port = 9030
user = "root"
password = "${DORIS_DB_PASSWORD}"
db = "ods_iws"                          # 连接库（ST 明细所在库）
# st_table = "ods_data_st"              # ST 明细表名（默认）
# station_table = "ai_isos.station_info" # 台站信息表（默认，支持 db.table）
```
