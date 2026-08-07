# 时间段监察功能 — 设计文档

> 角色：系统架构师 | 依据：`docs/time-range-inspection-requirements.md`（D1–D10 已全部确认）
> 日期：2026-08-07

---

## 1. 背景与目标

为系统增加"时间段监察"能力：用户指定起止时间（默认最近 1 小时，单次最长 7 天），系统对该时段历史数据做回顾性分析，输出**到报率/缺报时段**（D1a）与**告警时间线**（D1b）。覆盖 Doris / MySQL 真实模式与 SQLite 模拟降级（D5），独立页面 `/inspection`（D4），站点级可下钻设备级（D3），时间口径为 `data_time`（D8）。长时段走键集分页（D9），ST 解析走定点扫描（D10）。

## 2. 现状分析（证据）

- 页面与 API 统一经 `auth_middleware` 保护（`src/main.rs:441,1170`），新增路由自动获得鉴权，无需额外处理。
- 页面以 `include_str!` 内嵌模板（`src/main.rs:507-529`），新增 `templates/inspection.html` 遵循同模式。
- Doris/MySQL 的 `data_time` 列为 DATETIME（解码为 `NaiveDateTime`，字面量格式 `%Y-%m-%d %H:%M:%S`，见 `src/db.rs:1148,1167`）；SQLite 模拟库为 TEXT 紧凑格式 `%Y%m%d%H%M%S`（`src/db.rs:369,449`）。两条路径的时间字面量格式必须分别处理。
- Doris 查询约定：表名经配置注入、参数 `sql_escape` 内联、位置元组解码（`src/db.rs:108-125,1024-1047`）；MySQL 用 `?` + `bind`（`src/db.rs:898-907`）。
- 现有 `parse_st_packet`（`src/monitor.rs:597`）先 `split(',').collect()` 再遍历——定点解析（D10）即去掉全字段物化，惰性迭代 + 跳过 7 字段帧头 + 仅保留 `is_kit` 项，语义与原函数保持一致以便对拍。
- 现有图表 API 为模拟数据（`src/main.rs:969-1039`），本功能不复用，独立新增端点。

## 3. 方案设计

### 3.1 API 契约（新增，均自动纳入鉴权中间件）

| 端点 | 参数 | 说明 |
|------|------|------|
| `GET /inspection` | — | 监察页面 HTML |
| `GET /api/inspection/overview` | `start`,`end`,`station?` | 到报率总览。`station` 缺省=全部站点（站点级聚合）；指定=该站下钻到设备级 |
| `GET /api/inspection/alarms` | `start`,`end`,`station`(必填),`cursor?`,`limit?` | 告警时间线，键集分页 |

时间参数接受 `YYYY-MM-DDTHH:MM`（datetime-local）与 `YYYY-MM-DD HH:MM[:SS]`。校验：`end > start`；跨度 ≤ 7 天（D2），超出返回 400 风格错误 JSON；缺省 = 最近 1 小时。`limit` 默认 200、上限 500（D6）。

**键集游标**（D9）：`cursor` = Base64 编码的 `data_time|device_type|device_nid` 复合键；查询条件 `(data_time > t) OR (data_time = t AND (device_type > dt OR (device_type = dt AND device_nid > dn)))`，`ORDER BY data_time, device_type, device_nid LIMIT n+1`，取到 n+1 条则回传 `next_cursor`。不用 OFFSET。

### 3.2 数据模型（`src/models.rs` 新增）

- `InspectionOverviewResponse { start, end, expected_minutes, stations: Vec<StationInspection> }`
- `StationInspection { station_id, station_name, actual_count, expected_count, arrival_rate, first_data_time, last_data_time, device_count, gaps: Vec<GapInterval>, devices: Vec<DeviceInspection> }`（`devices` 仅在下钻时非空）
- `DeviceInspection { device_type, device_nid, device_name, actual_count, expected_count, arrival_rate }`
- `GapInterval { start, end, minutes }`（连续缺报分钟合并区间）
- `InspectionAlarmsResponse { station_id, events: Vec<InspectionAlarmEvent>, next_cursor: Option<String>, parsed_packets: usize }`
- `InspectionAlarmEvent { data_time, device_type, device_nid, device_name, item, value, alarm }`

### 3.3 数据查询层（`src/db.rs` 新增，`is_doris` 三分支）

1. `query_inspection_overview(stations, start, end) -> 原始聚合行`：
   - Doris/MySQL：`SELECT station_num, device_type, device_nid, COUNT(*), MIN(data_time), MAX(data_time) FROM <表> WHERE data_time >= <s> AND data_time < <e> AND station_num IN (...) GROUP BY station_num, device_type, device_nid`。Doris 内联转义字面量（`%Y-%m-%d %H:%M:%S`）+ 元组解码；MySQL `?` 绑定。
   - SQLite 模拟：返回空集，由 monitor 层合成数据（R3）。
2. `query_station_data_times(station, start, end) -> Vec<NaiveDateTime>`：`SELECT DISTINCT data_time ... ORDER BY data_time`（仅时间列，不取 `data`，7 天单站 ≤ 10080 行，量级可控）。逐站调用，用于缺报区间计算。
3. `fetch_st_packets_page(station, start, end, cursor, limit) -> Vec<(NaiveDateTime, String, String, String)>`：取 `data_time, device_type, device_nid, data`，键集条件 + `ORDER BY + LIMIT n+1`。

时间字面量：Doris/MySQL 用 `%Y-%m-%d %H:%M:%S`；SQLite 合成路径不查库，无格式问题。

### 3.4 业务逻辑层（`src/monitor.rs` 新增）

1. `parse_st_alarms_fast(data: &str) -> Vec<CheckItem>`（D10）：`split(',')` 惰性迭代，`.enumerate()` 跳过下标 < 7，按 (偶=项, 奇=值) 配对，仅 `is_kit` 项且值非 `N/C/-//` 单字符时产出，异常判定与 `get_alarm` 调用同原函数。**语义与 `parse_st_packet` 等价**，附 `#[cfg(test)]` 对拍测试（合成帧 + 边界帧）。
2. `merge_gap_intervals(present: &[NaiveDateTime], start, end) -> Vec<GapInterval>`：以分钟网格比对，连续缺报合并；单站 7 天 10080 分钟，O(n) 可接受。
3. `generate_inspection_overview_sim(...)` / `generate_inspection_alarms_sim(...)`：模拟模式合成结果（R3），到报率 90–100% 随机、少量缺报区间、少量告警事件，保证页面可演示。

### 3.5 API 层（`src/main.rs`）

- 查询结构体 `InspectionQuery { start, end, station }`、`InspectionAlarmsQuery { start, end, station, cursor, limit }`；时间解析容忍上述两种格式，错误返回 `Json` 错误对象 + `StatusCode::BAD_REQUEST`。
- 处理器 `api_inspection_overview` / `api_inspection_alarms` / `inspection_page_handler`；注册路由 `/inspection`、`/api/inspection/overview`、`/api/inspection/alarms`。
- 调试日志前缀 `[时段监察]`（遵循全链路日志约定）。

### 3.6 前端（`templates/inspection.html`）

- 风格对齐 `devices.html`；中文 UI；ECharts 非必需，以表格为主。
- 元素：`datetime-local` 起止选择（默认最近 1 小时）、站点下拉（取自 `/api/stations`，含"全部站点"）、查询按钮。
- 总览表：站点/设备、应有条数、实到条数、到报率、首末到报时间、缺报区间（可展开）。到报率 < 100% 标红。
- 告警时间线：选择单站后展示，"加载更多"按 `next_cursor` 翻页；显示解析包数。

### 3.7 安全约束

- Doris 路径全部经 `sql_escape` 内联；`limit`/跨度为钳制整数；游标解码失败即 400。
- MySQL 路径全部 `?` + `bind`，严禁拼接。
- 不新增任何配置项，无密码/连接信息暴露面变化；`/api/config` 不受影响。

## 4. 任务拆分清单

| # | 任务 | 产出 | 验证 | 依赖 |
|---|------|------|------|------|
| T1 | `models.rs` 新增监察数据结构 | 结构体定义 | `cargo build` | — |
| T2 | `monitor.rs` 定点解析 `parse_st_alarms_fast` + 对拍测试 + `merge_gap_intervals` | 函数 + `#[cfg(test)]` | `cargo test` 通过 | T1 |
| T3 | `db.rs` 三个查询方法（Doris/MySQL/模拟三分支） | 查询函数 | `cargo build` | T1 |
| T4 | `monitor.rs` 模拟模式监察数据合成 | 合成函数 | `cargo build` | T1 |
| T5 | `main.rs` 查询结构体、时间解析校验、三个处理器、路由注册 | API 可用 | curl 200/400 行为正确 | T2,T3,T4 |
| T6 | `templates/inspection.html` 页面 | 页面 | 浏览器验证 | T5 |
| T7 | QA：Doris 实测（overview/alarms/分页/边界）、模拟模式验证 | 验收记录 | 见 §5 | T5,T6 |
| T8 | CHANGELOG、README API 表、AGENTS.md API 表更新；归档推送 | 文档 + commit | git log/远程确认 | T7 |

## 5. 验收标准（QA 执行）

1. `cargo build --release` 通过，`cargo test` 通过（定点解析与原解析对拍一致）。
2. Doris 实测（10.10.1.67）：`/api/inspection/overview?start=<1小时前>&end=<现在>` 返回 32 站，到报率与手工 SQL 抽查一致（抽 1 站 `SELECT COUNT(*) ...` 比对）。
3. 指定 `station` 下钻返回设备级行数与 `SELECT ... GROUP BY device_type, device_nid` 抽查一致。
4. 缺报区间：人工挑一个缺报分钟，确认出现在 `gaps` 中。
5. `/api/inspection/alarms` 单站分页：第一页有 `next_cursor` 时，带游标请求返回后续事件且无重复、无遗漏（抽查相邻页边界分钟）。
6. 跨度 > 7 天、start ≥ end、非法时间格式、非法游标 → 400 错误 JSON。
7. 模拟模式（不连 Doris）：页面可打开，overview/alarms 返回合成数据。
8. `/inspection` 页面在浏览器完成一次完整查询与翻页。
9. 日志含 `[时段监察]` 前缀的关键环节记录。

## 6. 风险与回退

- **R1 量级**：三层防线（聚合不取 `data` / 键集分页 / 定点解析 + 500 上限）；若 Doris 实测仍慢，首回退是缩默认范围至 30 分钟并降低 limit 上限，不影响已交付接口形态。
- **R2 方言**：时间比较统一用字面量字符串（Doris/MySQL 均支持 DATETIME 与 `'YYYY-MM-DD HH:MM:SS'` 比较），不使用日期函数，规避方言差异。
- **R3 模拟**：合成数据仅用于演示，响应中带 `simulation: true` 标识（overview 响应加该字段），避免误解为真实监察结论。
- **R4 YPOWR00**：本期维持现有行为（通用键值解析），后续单独讨论。
- **回退**：本功能全部为新增文件/新增函数/新增路由，不改动既有函数行为；回退 = 还原新增路由注册与模板即可。
