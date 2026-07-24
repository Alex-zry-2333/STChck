# Weather Monitor Changelog

## 版本跟踪记录

> 本文档用于记录项目所有修改，便于回溯和版本管理。

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
