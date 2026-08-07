// Doris 连接诊断探针：验证 sqlx + vendor 补丁对 Doris FE 的兼容性。
// 用法：设置环境变量后运行 `cargo run --example doris_probe`
//   DORIS_URL   - 连接串，如 mysql://user:pass@host:9030/db（密码只走环境变量）
// 结果写入 probe-result.txt（本环境 exe 控制台输出不可见，故落盘）。
use sqlx::mysql::{MySqlConnectOptions, MySqlPoolOptions};
use std::time::Duration;

fn report_append(s: &str) {
    use std::io::Write;
    let mut f = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open("probe-result.txt")
        .unwrap();
    writeln!(f, "{}", s).unwrap();
}

#[tokio::main]
async fn main() {
    std::fs::write("probe-result.txt", "STEP: start\n").unwrap();

    let url = match std::env::var("DORIS_URL") {
        Ok(u) => u,
        Err(_) => {
            report_append("缺少环境变量 DORIS_URL（形如 mysql://user:pass@host:9030/db）");
            return;
        }
    };

    // Doris FE 不接受 sqlx 默认的非常量 SET 初始化语句，需关闭：
    // pipes_as_concat / no_engine_substitution / timezone
    let opts: MySqlConnectOptions = match url.parse::<MySqlConnectOptions>() {
        Ok(o) => o
            .pipes_as_concat(false)
            .no_engine_substitution(false)
            .timezone(None),
        Err(e) => {
            report_append(&format!("URL 解析失败: {}", e));
            return;
        }
    };

    let pool = match MySqlPoolOptions::new()
        .max_connections(2)
        .min_connections(1)
        .acquire_timeout(Duration::from_secs(10))
        .connect_with(opts)
        .await
    {
        Ok(p) => {
            report_append("CONNECT OK");
            p
        }
        Err(e) => {
            report_append(&format!("CONNECT FAIL: {}", e));
            return;
        }
    };

    // 1. 聚合查询（COUNT + INTERVAL + IN 内联）
    match sqlx::query_scalar::<_, i64>(
        "SELECT COUNT(*) FROM ods_data_st WHERE create_time > (NOW() - INTERVAL 10 MINUTE)",
    )
    .fetch_one(&pool)
    .await
    {
        Ok(n) => report_append(&format!("ods_data_st 最近10分钟记录数: {}", n)),
        Err(e) => report_append(&format!("ods_data_st 查询失败: {}", e)),
    }

    // 2. datetime 解码
    match sqlx::query_scalar::<_, Option<chrono::NaiveDateTime>>(
        "SELECT MAX(data_time) FROM ods_data_st WHERE station_num = '50936'",
    )
    .fetch_one(&pool)
    .await
    {
        Ok(t) => report_append(&format!("MAX(data_time): {:?}", t)),
        Err(e) => report_append(&format!("datetime 解码失败: {}", e)),
    }

    // 3. station_info 跨库查询
    match sqlx::query_as::<_, (String, Option<String>)>(
        "SELECT station_num, station_name FROM ai_isos.station_info \
         WHERE station_num IN ('50936','50968')",
    )
    .fetch_all(&pool)
    .await
    {
        Ok(rows) => report_append(&format!("station_info 查询 OK，{} 行", rows.len())),
        Err(e) => report_append(&format!("station_info 查询失败: {}", e)),
    }

    report_append("DONE");
}
