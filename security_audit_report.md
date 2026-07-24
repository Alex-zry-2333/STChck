# 气象站数据监控系统 — 安全审计报告

> **审计时间**: 2025-06-16  
> **审计范围**: `weather-monitor` 项目全仓库（Rust 主程序、Python 旧版、C 工具、配置文件、前端模板）  
> **风险等级说明**:  
> - **🔴 严重 (Critical)**: 可直接导致系统被入侵、数据泄露或完全失控  
> - **🟠 高危 (High)**: 可被利用造成重大安全事件，利用难度较低  
> - **🟡 中危 (Medium)**: 扩大攻击面或降低攻击门槛，需结合其他条件利用  
> - **🟢 低危 (Low/Info)**: 安全加固建议，存在潜在风险但难以直接利用

---

## 一、🔴 严重风险（Critical）

### 1. 明文硬编码数据库凭据，且配置文件未纳入版本控制忽略列表

**影响**: 数据库 `root` 密码以明文形式写入多处源码与配置，极易通过 Git 泄露、源码分发或仓库镜像暴露。

**涉及文件与位置**:

| 文件 | 位置 | 泄露内容 |
|------|------|----------|
| `config.toml` | `[database]` / `[cloud_database]` | `host=10.10.1.59`, `user=root`, `password=root`, `db=cammoc_w` / `cammoc_cloud_w` |
| `monitor_app.py` | 第 306–311 行 | 同上（`DB_CONFIG` 字典） |
| `tm.c` | 第 626 行 | `mysql_real_connect(conn,"10.10.1.59","root","root","cammoc_w",3306,NULL,0)` |

**补充问题**:
- `.gitignore`（第 1–7 行）未将 `config.toml` 加入忽略列表，存在误提交至 Git 仓库的风险。
- 用户名 `root` + 密码 `root` 属于极弱口令，若数据库对外开放端口（3306），可被直接暴力破解。

**修复建议**:
1. 立即修改数据库密码为强口令（16 位以上，含大小写+数字+特殊字符）。
2. 将 `config.toml` 加入 `.gitignore`，并创建 `config.toml.example` 作为模板（不含真实密码）。
3. 使用环境变量或本地密钥管理服务（如 Windows DPAPI、Linux keyring）注入密码，代码中仅保留占位符。
4. 数据库层面创建专用只读/监控账号，禁用 `root` 远程连接。

---

### 2. `/api/config` 接口直接暴露完整配置（含密码）

**影响**: 任何能访问 Web 服务的人无需认证即可获取全部数据库密码。

**涉及文件**: `src/main.rs` 第 293–297 行

```rust
async fn api_config(
    State(state): State<Arc<AppState>>,
) -> Json<config::Config> {
    Json(state.config.clone())   // 直接序列化整个 Config，包含 password 字段
}
```

**验证**: 访问 `http://localhost:8080/api/config` 即可得到完整 JSON，其中 `database.password` 与 `cloud_database.password` 字段为明文。

**修复建议**:
- 删除该接口，或仅返回脱敏后的公开配置（如 `port`、`refresh_interval`）。
- 若必须提供配置预览，显式构造 DTO（数据传输对象），排除 `password` 等敏感字段：
  ```rust
  pub struct PublicConfig {
      pub port: u32,
      pub refresh_interval_secs: u64,
      pub simulation_mode: bool,
      // 不含 password
  }
  ```

---

### 3. Rust 主程序存在 SQL 注入（用户输入直接拼接进查询）

**影响**: 攻击者可通过构造恶意站号 URL 路径，注入任意 SQL 语句，造成数据泄露、篡改或数据库被控。

**涉及文件**: `src/db.rs` 第 528–535 行

```rust
let sql = format!(
    "SELECT device_type, device_nid, data_time, `data` \
     FROM data_st \
     WHERE station_num = '{}' AND receive_time > (NOW() - INTERVAL 5 MINUTE) \
     ORDER BY ... LIMIT 100",
    station_id   // <-- 直接来自 URL Path 参数，未经校验或参数化
);
```

该函数被 `src/main.rs` 第 618–678 行的 `api_station_devices` 调用，其中 `station_id` 来自用户可控的 URL：

```rust
async fn api_station_devices(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,   // 用户输入
) -> Json<models::StationDevicesResponse> {
    ...
    let fresh = state_clone.db.get_station_devices(&id_clone, &station_name).await;
```

**攻击示例**:

```
GET /api/station/50936' UNION SELECT user,password,host,authentication_string FROM mysql.user-- /devices
```

由于 `sqlx` 支持参数化查询，应使用 `?` 占位符而非字符串拼接。

**修复建议**:
```rust
let sql = "SELECT device_type, device_nid, data_time, `data` \
           FROM data_st \
           WHERE station_num = ? AND receive_time > (NOW() - INTERVAL 5 MINUTE) \
           ORDER BY device_type, device_nid, data_time DESC, receive_time DESC, id DESC \
           LIMIT 100";
let rows: Vec<StRow> = sqlx::query_as::<_, StRow>(sql)
    .bind(station_id)   // 参数化绑定
    .fetch_all(pool)
    .await?;
```

---

### 4. Python 旧版程序存在 SQL 注入

**影响**: 同第 3 条，虽然 `monitor_app.py` 当前可能仅作演示/备用，但代码中多处使用 f-string 拼接 SQL。

**涉及文件**: `monitor_app.py`

- 第 329–339 行：主查询 `WHERE receive_time>(NOW()-INTERVAL {td} MINUTE)` 与 `IN ({','.join(station_ids)})`
- 第 366–370 行：ST 包查询 `WHERE station_num='{station_id}' AND data_time='{min_time}'`

**修复建议**: 使用 `pymysql` 的参数化接口（`cursor.execute(sql, (station_id, min_time))`）。

---

### 5. C 语言工具 `tm.c` 存在 SQL 注入与缓冲区溢出

**影响**: `tm.c` 作为定时任务脚本直接与数据库交互，使用 `sprintf` 拼接 SQL 字符串，存在栈溢出和 SQL 注入双重风险。

**涉及文件**: `tm.c`

- 第 510 行：`sprintf(sqlcmd,"select data from data_st where station_num='%s' and data_time='%s';",row0,row3);`
- 第 614–621 行：通过 `sprintf` 循环拼接 `station_num IN (...)`，以及 `tD` 直接拼入主查询。

**修复建议**:
- 使用 MySQL C API 的预处理语句（`mysql_stmt_prepare` / `mysql_stmt_bind_param`）。
- 对 `tD` 做严格边界校验（`tD` 仅允许 1–1440 的整数）。

---

## 二、🟠 高危风险（High）

### 6. 全系统无任何身份认证与权限控制

**影响**: 所有 API 端点、Web 界面、数据接口完全开放，任何网络可达用户均可访问与操作。

**涉及范围**:
- Rust 服务（`src/main.rs`）：所有路由（`/api/status`、`/api/stations`、`/api/regions`、`/api/chart/values`、`/api/devices/events` 等）均无认证中间件。
- Python 服务（`monitor_app.py`）：Flask 应用未集成任何登录机制。
- 服务绑定地址：`0.0.0.0`（`main.rs` 第 235 行、`monitor_app.py` 第 542 行），意味着局域网或公网可达即可直接访问。

**修复建议**:
1. 增加基于 Token 或 Session 的认证中间件（如 Axum 的 `tower-http` auth 层，或 Flask 的 `flask-login`）。
2. 若仅为内部监控，建议将绑定地址改为 `127.0.0.1` 并通过反向代理（Nginx/Apache）对外发布，由代理层统一处理认证。
3. 网络层增加防火墙规则，限制仅允许可信 IP 访问 `8080` / `114514` 端口。

---

### 7. 通信未启用 TLS/HTTPS，明文传输数据库密码与监控数据

**影响**: 中间人攻击者可嗅探流量，直接获取 `/api/config` 泄露的密码、SSE 数据流中的监控信息，以及前端与后端交互的全部内容。

**涉及范围**: Rust 与 Python 服务均使用 HTTP（非 HTTPS）。

**修复建议**:
- 生产环境使用反向代理（Nginx、Caddy、Traefik）终止 TLS，后端服务监听本地回环地址。
- 或在内网中使用私有 CA 签发证书，强制启用 HTTPS。

---

### 8. `tm.c` 存在多处缓冲区溢出风险

**影响**: 栈溢出可导致程序崩溃、信息泄露，甚至在特定条件下被利用执行任意代码。

**涉及文件**: `tm.c`

| 变量 | 大小 | 风险点 |
|------|------|--------|
| `alarmMsg[1024]` | 1024 字节 | `getALM()` 中多次 `sprintf` 未检查累计长度，超长输入可溢出 |
| `sqlcmd[1024]` | 1024 字节 | 拼接 `row0` + `row3` 时若输入超长（> 900 字节）可溢出 |
| `cmd[1024]` | 1024 字节 | 主查询字符串拼接 32 个站号，若站号被篡改可溢出 |
| `erDm[2048]` | 2048 字节 | 循环拼接报警信息，超长数据可溢出 |
| `Lbuf[10240]` | 10240 字节 | 虽较大，但 `sprintf((char *)&Lbuf[strlen(Lbuf)], ...)` 无边界检查 |
| `itm[128][64]` | 128×64 字节 | 解析 ST 包时 `nt` 可能超过 128，且 `j` 的边界检查 `if(j<63) j++` 会导致写入 `itm[nt][63]` 后停止递增，但后续不终止写入，存在逻辑缺陷 |

**修复建议**:
- 使用 `snprintf` 替代所有 `sprintf`。
- 对 `nt` 增加上限检查（`nt < 128`），对字符串长度做截断处理。
- 考虑将 `tm.c` 重写为 Rust 或 Python，利用内存安全语言消除此类风险。

---

## 三、🟡 中危风险（Medium）

### 9. 未配置 CORS，存在跨域滥用风险

**影响**: 若用户访问恶意网页，该网页可通过浏览器向监控服务发起跨域请求，读取监控数据或触发接口。

**涉及范围**: Rust 与 Python 服务均未配置 CORS 策略。

**修复建议**:
- 在 Axum 中增加 `tower-http` 的 `CorsLayer`，仅允许特定可信域名：
  ```rust
  use tower_http::cors::{Any, CorsLayer};
  let cors = CorsLayer::new()
      .allow_origin(["https://your-domain.com".parse().unwrap()]);
  ```
- 或若完全内网使用，直接禁止跨域请求。

---

### 10. 缺乏速率限制（Rate Limiting），易遭受资源耗尽攻击

**影响**: 攻击者可通过高频请求 `/api/station/{id}/devices`（该接口触发数据库查询）导致数据库 CPU/IO 飙升，造成拒绝服务。

**涉及范围**: 所有 API 端点。

**修复建议**:
- 在反向代理层（Nginx / Caddy）配置 `limit_req`。
- 或在 Axum 中增加 `tower-http` 的 `RateLimitLayer`，按 IP 限制 QPS。

---

### 11. `tm.c` 输出含 ANSI 转义序列，存在日志注入隐患

**影响**: `tm.c` 中大量使用 `\e[33m`、`\e[31m` 等 ANSI 颜色码。若日志被其他系统解析或显示，可能被利用注入虚假日志行（如伪造 `\e[0m\n[ERROR]`）。

**涉及文件**: `tm.c` 第 584–586 行等。

**修复建议**:
- 生产环境日志输出禁用 ANSI 颜色，或提供 `--no-color` 开关。
- 对日志内容进行过滤，移除不可信控制字符。

---

## 四、🟢 低危 / 信息项（Low / Info）

### 12. 错误信息通过 API 暴露给前端

**影响**: `MonitorData` 结构体包含 `error: Option<String>`，错误详情可能泄露数据库内部状态、文件路径或网络拓扑信息。

**涉及文件**: `src/models.rs` 第 36 行；`src/db.rs` 多处 `tracing::error!` 后将错误写入 `MonitorData.error`。

**修复建议**:
- 对外 API 返回通用错误信息（如 `"数据库查询失败，请联系管理员"`）。
- 详细错误日志仅写入服务端日志文件，不暴露给客户端。

---

### 13. 清理旧进程逻辑存在条件判断错误

**影响**: 在 Windows 上，`cleanup_previous_instances()` 函数中 `if parts.len() < 2 && parts[0] != "weather-monitor.exe"` 的逻辑应为 `||`；若 `parts` 为空，`parts[0]` 会导致 panic。虽然 `tasklist` 输出通常可控，但这是一个可靠性缺陷。

**涉及文件**: `src/main.rs` 第 40 行。

**修复建议**:
```rust
if parts.len() < 2 || parts[0] != "weather-monitor.exe" {
    continue;
}
```

---

### 14. 模拟模式与真实模式切换缺乏运行时保护

**影响**: `config.toml` 中 `simulation_mode = true` 可直接关闭数据库连接，若该配置被误改或恶意篡改，系统将基于假数据运行，影响运维判断。

**涉及文件**: `config.toml` 第 21 行；`src/main.rs` 第 110–115 行。

**修复建议**:
- 对 `simulation_mode` 的切换增加环境变量或命令行显式确认，避免仅通过配置文件静默切换。

---

## 五、修复优先级与行动清单

| 优先级 | 风险编号 | 行动项 | 预估工作量 |
|--------|----------|--------|------------|
| **P0** | 1 | 修改数据库密码；将 `config.toml` 移出版本控制；改用环境变量注入 | 0.5 天 |
| **P0** | 2 | 删除或脱敏 `/api/config` 接口 | 0.5 天 |
| **P0** | 3, 4, 5 | 将所有 SQL 拼接改为参数化查询（Rust `sqlx::query!` / `bind`；Python `cursor.execute(sql, params)`；C 预处理语句） | 1–2 天 |
| **P1** | 6 | 增加 API 认证（Token / Session）或改为仅本地监听 + 反向代理认证 | 1–2 天 |
| **P1** | 7 | 部署 HTTPS（反向代理 TLS 终止） | 0.5 天 |
| **P1** | 8 | 将 `tm.c` 中 `sprintf` 替换为 `snprintf`；增加边界检查 | 0.5–1 天 |
| **P2** | 9, 10 | 配置 CORS、增加速率限制 | 0.5 天 |
| **P2** | 12 | 对外错误信息脱敏 | 0.5 天 |

---

## 六、总结

本项目当前存在 **5 项严重（Critical）**、**3 项高危（High）** 安全风险，其中 **明文密码泄露** 与 **SQL 注入** 可被直接利用，**建议在 24 小时内完成 P0 项修复**。整体安全架构缺失认证、加密、参数化查询等基础防护，建议在近期进行一次以安全为重点的代码重构，优先将 `tm.c` 迁移至内存安全语言，并建立统一的 Secrets 管理与访问控制体系。
