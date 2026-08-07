mod config;
mod db;
mod license;
mod models;
mod monitor;

#[cfg(test)]
mod tests;

use axum::{
    body::Body,
    extract::{Form, Path, Query, State},
    http::{header, HeaderMap, HeaderValue, Method, Request, StatusCode},
    middleware::Next,
    response::{
        sse::{Event, Sse},
        Html, IntoResponse, Json, Redirect, Response,
    },
    routing::get,
    Router,
};
use rand::{distributions::Alphanumeric, thread_rng, Rng};
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::sync::Arc;
use std::time::Duration;
use tokio::sync::{broadcast, RwLock};
use tokio_stream::wrappers::BroadcastStream;
use tokio_stream::StreamExt;
use tower_http::cors::{AllowOrigin, Any, CorsLayer};

#[cfg(target_os = "windows")]
fn cleanup_previous_instances() {
    use std::process::Command;

    let current_pid = std::process::id();
    let output = Command::new("tasklist")
        .args([
            "/FI",
            "IMAGENAME eq weather-monitor.exe",
            "/FO",
            "CSV",
            "/NH",
        ])
        .output();

    if let Ok(output) = output {
        let text = String::from_utf8_lossy(&output.stdout);
        for line in text.lines() {
            let parts: Vec<&str> = line.split(',').map(|s| s.trim_matches('"')).collect();
            if parts.len() < 2 && parts[0] != "weather-monitor.exe" {
                continue;
            }
            if let Ok(pid) = parts[1].parse::<u32>() {
                if pid != current_pid {
                    tracing::info!(
                        "发现已有 weather-monitor.exe 进程 (PID: {})，正在结束...",
                        pid
                    );
                    let _ = Command::new("taskkill")
                        .args(["/F", "/PID", &pid.to_string()])
                        .output();
                }
            }
        }
    }

    // Wait briefly for the socket to be released
    std::thread::sleep(Duration::from_millis(800));
}

#[cfg(not(target_os = "windows"))]
fn cleanup_previous_instances() {}

async fn try_bind_port(
    base_port: u32,
    max_attempts: u32,
) -> Option<(tokio::net::TcpListener, u32)> {
    for offset in 0..max_attempts {
        let port = base_port + offset;
        if port > 65535 {
            break;
        }
        let addr = format!("0.0.0.0:{}", port);
        match tokio::net::TcpListener::bind(&addr).await {
            Ok(listener) => return Some((listener, port)),
            Err(e) => {
                tracing::warn!("端口 {} 绑定失败: {}，尝试下一个端口", port, e);
                tokio::time::sleep(Duration::from_millis(200)).await;
            }
        }
    }
    None
}

struct AppState {
    config: config::Config,
    data: RwLock<models::MonitorData>,
    db: db::DbService,
    tx: broadcast::Sender<String>,
    station_meta: HashMap<String, models::StationMeta>,
    station_devices: RwLock<HashMap<String, models::StationDevicesResponse>>,
    devices_tx: broadcast::Sender<String>,
    session_tokens: RwLock<HashSet<String>>,
    license_state: std::sync::Mutex<license::license::LicenseState>,
}

#[derive(serde::Serialize)]
struct PublicConfig {
    server: config::ServerConfig,
    monitor: config::MonitorConfig,
    stations: Vec<config::StationConfig>,
}

#[derive(Deserialize)]
struct ChartQuery {
    hours: Option<i64>,
}

#[derive(Deserialize)]
struct ValueQuery {
    station: Option<String>,
    item: Option<String>,
    hours: Option<i64>,
}

#[derive(Deserialize)]
struct TopQuery {
    #[serde(default = "default_top_limit")]
    limit: usize,
}

#[derive(Deserialize)]
struct LoginForm {
    username: String,
    password: String,
}

#[derive(Deserialize)]
struct ForecastQuery {
    station: Option<String>,
}

#[tokio::main]
async fn main() {
    tracing_subscriber::fmt::init();

    // ============================================================
    // License verification (compile-time controlled)
    // ============================================================
    let license_state = license::license::verify();

    if license_state.expired {
        tracing::warn!("╔════════════════════════════════════════════════════════════╗");
        tracing::warn!("║  ⚠️  LICENSE EXPIRED — Running in DEGRADED MODE          ║");
        tracing::warn!("║  Contact support to renew your license.                  ║");
        tracing::warn!("╚════════════════════════════════════════════════════════════╝");
    } else if license_state.invalid {
        tracing::warn!("╔════════════════════════════════════════════════════════════╗");
        tracing::warn!("║  ⚠️  NO VALID LICENSE — Running in evaluation mode       ║");
        tracing::warn!("║  Please place a valid license.toml in the working dir.   ║");
        tracing::warn!("╚════════════════════════════════════════════════════════════╝");
    } else if license_state.hardware_mismatch {
        tracing::error!("╔════════════════════════════════════════════════════════════╗");
        tracing::error!("║  ❌ HARDWARE MISMATCH — License not valid for this machine ║");
        tracing::error!("╚════════════════════════════════════════════════════════════╝");
    }

    let mut cfg = config::Config::load("config.toml");

    // 环境变量覆盖数据库密码，避免明文存储在配置文件中
    if let Ok(pwd) = std::env::var("DB_PASSWORD") {
        cfg.database.password = pwd;
    }
    if let Ok(pwd) = std::env::var("CLOUD_DB_PASSWORD") {
        cfg.cloud_database.password = pwd;
    }
    if let (Ok(pwd), Some(doris)) = (std::env::var("DORIS_DB_PASSWORD"), cfg.doris.as_mut()) {
        doris.password = pwd;
    }

    let (tx, _rx) = broadcast::channel::<String>(16);
    let (devices_tx, _devices_rx) = broadcast::channel::<String>(128);

    let db = if cfg.monitor.simulation_mode {
        tracing::info!("配置为模拟模式，使用本地 SQLite 数据库");
        db::DbService::new_simulation().await
    } else {
        tracing::info!("真实模式，数据源: {}", cfg.monitor.data_source);
        db::DbService::new_with_source(
            &cfg.database,
            &cfg.cloud_database,
            cfg.doris.as_ref(),
            &cfg.monitor.data_source,
        )
        .await
    };

    // Load station metadata once at startup
    let station_meta = db.load_station_meta(&cfg.stations).await;
    tracing::info!("已加载 {} 个站点元数据", station_meta.len());

    let state = Arc::new(AppState {
        data: RwLock::new(models::MonitorData {
            stations: vec![],
            summary: models::MonitorSummary {
                total: 0,
                online: 0,
                alarms: 0,
                checked: 0,
                records: 0,
                avg_arrival_rate: 0.0,
            },
            last_update: String::new(),
            error: None,
        }),
        config: cfg,
        db,
        tx,
        station_meta,
        station_devices: RwLock::new(HashMap::new()),
        devices_tx,
        session_tokens: RwLock::new(HashSet::new()),
        license_state: std::sync::Mutex::new(license_state.clone()),
    });

    // ============================================================
    // Runtime license reminder (degraded mode)
    // ============================================================
    #[cfg(feature = "license-check")]
    {
        if license_state.expired || license_state.invalid {
            let reminder_interval = license_state
                .license_info
                .as_ref()
                .map(|l| l.features.reminder_interval_hours)
                .unwrap_or(24);

            tokio::spawn(async move {
                let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
                    reminder_interval * 3600,
                ));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
                interval.tick().await; // skip first immediate tick

                loop {
                    interval.tick().await;
                    if license::license::is_expired() {
                        tracing::warn!(
                            "╔════════════════════════════════════════════════════════════╗"
                        );
                        tracing::warn!(
                            "║  ⏰ LICENSE REMINDER: Your license has EXPIRED.           ║"
                        );
                        tracing::warn!(
                            "║  Please contact support to renew.                        ║"
                        );
                        tracing::warn!(
                            "╚════════════════════════════════════════════════════════════╝"
                        );
                    } else if license::license::is_invalid() {
                        tracing::warn!(
                            "╔════════════════════════════════════════════════════════════╗"
                        );
                        tracing::warn!(
                            "║  ⏰ LICENSE REMINDER: No valid license file found.        ║"
                        );
                        tracing::warn!(
                            "╚════════════════════════════════════════════════════════════╝"
                        );
                    }
                }
            });
        }
    }

    // Background refresh for monitor data; start immediately so HTTP comes up fast
    let state_clone = state.clone();
    tokio::spawn(async move {
        // First load after a short delay so the web server binds first
        tokio::time::sleep(tokio::time::Duration::from_millis(500)).await;
        let mut new_data = state_clone
            .db
            .query_monitor_data(
                &state_clone.config.stations,
                state_clone.config.monitor.check_interval_minutes as i32,
            )
            .await;
        enrich_station_meta(&mut new_data.stations, &state_clone.station_meta);
        if let Ok(json) = serde_json::to_string(&new_data) {
            let _ = state_clone.tx.send(json);
        }
        {
            let mut data = state_clone.data.write().await;
            *data = new_data;
        }

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(
            state_clone.config.server.refresh_interval_secs,
        ));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        loop {
            interval.tick().await;
            let mut new_data = state_clone
                .db
                .query_monitor_data(
                    &state_clone.config.stations,
                    state_clone.config.monitor.check_interval_minutes as i32,
                )
                .await;
            enrich_station_meta(&mut new_data.stations, &state_clone.station_meta);

            // Broadcast update via SSE
            if let Ok(json) = serde_json::to_string(&new_data) {
                let _ = state_clone.tx.send(json);
            }

            let mut data = state_clone.data.write().await;
            *data = new_data;
        }
    });

    // Background station devices refresh (every 5 minutes)
    let state_devices = state.clone();
    tokio::spawn(async move {
        // 启动时立即预热一次缓存，避免前端首次访问时返回空数据
        refresh_station_devices_cache(&state_devices).await;

        let mut interval = tokio::time::interval(tokio::time::Duration::from_secs(300));
        interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Skip);
        interval.tick().await;
        loop {
            interval.tick().await;
            refresh_station_devices_cache(&state_devices).await;
        }
    });

    let port = if state.config.server.port > 65535 {
        tracing::warn!(
            "端口 {} 无效（超过 65535），使用 8080 代替",
            state.config.server.port
        );
        8080u32
    } else {
        state.config.server.port
    };
    let refresh_interval = state.config.server.refresh_interval_secs;
    let is_simulation = state.db.simulation_mode;

    let cors_layer = build_cors_layer(&state.config.auth.allowed_origins);

    let auth_state = state.clone();
    let app = Router::new()
        .route("/", get(root_handler))
        .route("/overview", get(overview_handler))
        .route("/map", get(map_handler))
        .route("/forecast", get(forecast_page_handler))
        .route("/login", get(login_page_handler).post(login_handler))
        .route("/logout", get(logout_handler))
        .route("/station/{id}/devices", get(devices_page_handler))
        .route("/api/status", get(api_status))
        .route("/api/summary", get(api_summary))
        .route("/api/stations", get(api_stations))
        .route("/api/config", get(api_config))
        .route("/api/regions", get(api_regions))
        .route("/api/station/{id}", get(api_station_detail))
        .route("/api/top", get(api_top))
        .route("/api/map/stations", get(api_map_stations))
        .route("/api/station/{id}/devices", get(api_station_devices))
        .route("/api/devices/events", get(devices_sse_handler))
        .route("/api/chart/alarms", get(chart_alarms))
        .route("/api/chart/values", get(chart_values))
        .route("/api/events", get(sse_handler))
        .route("/api/forecast", get(api_forecast))
        .route("/api/forecast/{id}", get(api_forecast_detail))
        .route("/api/license", get(api_license))
        .fallback(static_handler)
        .route_layer(axum::middleware::from_fn(move |req, next| {
            let state = auth_state.clone();
            async move { auth_middleware(req, next, state).await }
        }))
        .layer(cors_layer)
        .with_state(state.clone());

    cleanup_previous_instances();

    let (listener, actual_port) = match try_bind_port(port, 10).await {
        Some(v) => v,
        None => {
            tracing::error!("无法绑定端口 {} 到 {}，请检查端口占用情况", port, port + 9);
            std::process::exit(1);
        }
    };

    let _addr = format!("0.0.0.0:{}", actual_port);
    tracing::info!("============================================================");
    tracing::info!("  气象站数据监控系统");
    tracing::info!("  Web 界面: http://localhost:{}", actual_port);
    tracing::info!("  刷新间隔: {} 秒", refresh_interval);
    tracing::info!(
        "  数据模式: {}",
        if is_simulation {
            "模拟数据"
        } else {
            "实时数据库"
        }
    );
    // License status in startup banner
    {
        let lic = state.license_state.lock().unwrap();
        if lic.expired {
            tracing::info!("  License: ⚠️  EXPIRED (degraded mode)");
        } else if lic.invalid {
            tracing::info!("  License: ⚠️  INVALID / NOT FOUND (evaluation mode)");
        } else if !lic.valid {
            tracing::info!("  License: ❌ {}", lic.message);
        } else {
            tracing::info!("  License: ✅ {}", lic.message);
        }
    }
    tracing::info!("============================================================");

    let server = axum::serve(listener, app);
    let shutdown = tokio::spawn(async move {
        if let Err(e) = tokio::signal::ctrl_c().await {
            tracing::warn!("监听 Ctrl+C 信号失败: {}", e);
        } else {
            tracing::info!("收到关闭信号，服务正在优雅退出...");
        }
    });

    tokio::select! {
        result = server => {
            if let Err(e) = result {
                tracing::error!("服务异常退出: {}", e);
            }
        }
        _ = shutdown => {
            tracing::info!("服务已关闭");
        }
    }
}

async fn map_handler() -> Html<&'static str> {
    Html(include_str!("../templates/map.html"))
}

async fn root_handler() -> Html<&'static str> {
    Html(include_str!("../templates/dashboard.html"))
}

async fn forecast_page_handler() -> Html<&'static str> {
    Html(include_str!("../templates/forecast.html"))
}

async fn login_page_handler() -> Html<&'static str> {
    Html(include_str!("../templates/login.html"))
}

async fn overview_handler() -> Html<&'static str> {
    Html(include_str!("../templates/overview.html"))
}

async fn devices_page_handler() -> Html<&'static str> {
    Html(include_str!("../templates/devices.html"))
}

async fn api_status(State(state): State<Arc<AppState>>) -> Json<models::MonitorData> {
    let data = state.data.read().await;
    Json(data.clone())
}

async fn api_summary(State(state): State<Arc<AppState>>) -> Json<models::MonitorSummary> {
    let data = state.data.read().await;
    Json(data.summary.clone())
}

async fn api_stations(State(state): State<Arc<AppState>>) -> Json<Vec<models::StationStatus>> {
    let data = state.data.read().await;
    Json(data.stations.clone())
}

async fn api_config(State(state): State<Arc<AppState>>) -> Json<PublicConfig> {
    Json(PublicConfig {
        server: state.config.server.clone(),
        monitor: state.config.monitor.clone(),
        stations: state.config.stations.clone(),
    })
}

async fn api_regions(State(state): State<Arc<AppState>>) -> Json<Vec<models::RegionStats>> {
    let data = state.data.read().await;
    let mut map: HashMap<String, models::RegionStats> = HashMap::new();

    for st in &data.stations {
        let province = state
            .station_meta
            .get(&st.id)
            .map(|m| m.province.clone())
            .unwrap_or_else(|| "未知省份".to_string());

        let entry = map.entry(province.clone()).or_insert(models::RegionStats {
            province,
            total: 0,
            online: 0,
            offline: 0,
            avg_arrival_rate: 0.0,
            alarm_count: 0,
            low_rate_count: 0,
            offline_stations: vec![],
            alarm_stations: vec![],
        });

        entry.total += 1;
        if st.online {
            entry.online += 1;
        } else {
            entry.offline += 1;
            entry.offline_stations.push(st.id.clone());
        }
        entry.avg_arrival_rate += st.arrival_rate_24h;
        entry.alarm_count += st.alarm_count;
        if st.arrival_rate_24h < 90.0 {
            entry.low_rate_count += 1;
        }
        if st.alarm_count > 0 {
            entry.alarm_stations.push(st.id.clone());
        }
    }

    let mut result: Vec<models::RegionStats> = map.into_values().collect();
    for r in &mut result {
        if r.total > 0 {
            r.avg_arrival_rate = (r.avg_arrival_rate / r.total as f64 * 10.0).round() / 10.0;
        }
    }

    result.sort_by(|a, b| b.total.cmp(&a.total));
    Json(result)
}

async fn api_station_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Option<models::StationDetail>> {
    let data = state.data.read().await;
    let status = match data.stations.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => return Json(None),
    };

    let meta = state
        .station_meta
        .get(&id)
        .cloned()
        .unwrap_or_else(|| models::StationMeta {
            station_id: id.clone(),
            station_name: status.name.clone(),
            province: "未知省份".to_string(),
            address: String::new(),
            latitude: 0.0,
            longitude: 0.0,
            altitude: 0.0,
            station_type: String::new(),
            is_acid_rain: false,
            is_reference_radiation: false,
        });

    // Re-parse current ST alarms into categories
    let mut categories: HashMap<String, models::CategoryStat> = HashMap::new();
    // Initialize all known categories so the drawer always shows them
    let cat_keys = [
        "communication",
        "power",
        "temperature",
        "heating",
        "ventilation",
        "pollution",
        "data_quality",
        "other",
    ];
    for key in cat_keys {
        categories.insert(
            key.to_string(),
            models::CategoryStat {
                total: 0,
                abnormal: 0,
                items: vec![],
            },
        );
    }

    for alarm in &status.alarms {
        let key = categorize_alarm_text(alarm);
        if let Some(entry) = categories.get_mut(key) {
            entry.total += 1;
            entry.abnormal += 1;
            entry.items.push(models::CheckItem {
                item: String::new(),
                value: String::new(),
                alarm: alarm.clone(),
                abnormal: true,
            });
        }
    }

    Json(Some(models::StationDetail {
        meta,
        status,
        categories,
    }))
}

fn categorize_alarm_text(alarm: &str) -> &'static str {
    if alarm.starts_with("通讯")
        || alarm.contains("通信")
        || alarm.contains("网口")
        || alarm.contains("串口")
        || alarm.contains("无线")
    {
        "communication"
    } else if alarm.starts_with("供电")
        || alarm.starts_with("主板")
        || alarm.starts_with("蓄电池")
        || alarm.starts_with("工作电流")
        || alarm.starts_with("加热电源")
    {
        "power"
    } else if alarm.contains("温度")
        || alarm.contains("腔体")
        || alarm.contains("恒温")
        || alarm.contains("机箱")
    {
        "temperature"
    } else if alarm.contains("加热") {
        "heating"
    } else if alarm.contains("通风") || alarm.contains("转速") {
        "ventilation"
    } else if alarm.contains("污染") || alarm.contains("镜头") || alarm.contains("窗口") {
        "pollution"
    } else if alarm.contains("分钟")
        || alarm.contains("采样")
        || alarm.contains("变化率")
        || alarm.contains("超上限")
        || alarm.contains("超下限")
    {
        "data_quality"
    } else {
        "other"
    }
}

fn default_top_limit() -> usize {
    5
}

async fn api_top(
    State(state): State<Arc<AppState>>,
    Query(q): Query<TopQuery>,
) -> Json<models::TopLists> {
    let data = state.data.read().await;
    let limit = q.limit.max(1).min(100);

    // Lowest arrival rate
    let mut lowest: Vec<&models::StationStatus> = data.stations.iter().collect();
    lowest.sort_by(|a, b| {
        a.arrival_rate_24h
            .partial_cmp(&b.arrival_rate_24h)
            .unwrap_or(std::cmp::Ordering::Equal)
    });

    // Longest offline: use last_arrival_time gap in minutes; offline stations first
    let now = chrono::Local::now();
    let mut offline_list: Vec<(&models::StationStatus, f64)> = data
        .stations
        .iter()
        .map(|s| {
            let gap_min = if s.last_arrival_time.is_empty() {
                9999.0
            } else {
                chrono::NaiveDateTime::parse_from_str(&s.last_arrival_time, "%Y-%m-%d %H:%M:%S")
                    .ok()
                    .map(|t| (now.naive_local() - t).num_minutes() as f64)
                    .unwrap_or(9999.0)
            };
            (s, gap_min)
        })
        .collect();
    offline_list.sort_by(|a, b| b.1.partial_cmp(&a.1).unwrap_or(std::cmp::Ordering::Equal));

    // Most alarms
    let mut most_alarms: Vec<&models::StationStatus> = data.stations.iter().collect();
    most_alarms.sort_by(|a, b| b.alarm_count.cmp(&a.alarm_count));

    let to_top = |s: &models::StationStatus, province: String, value: f64, label: String| {
        models::TopStation {
            id: s.id.clone(),
            name: s.name.clone(),
            province,
            value,
            label,
        }
    };

    let lowest_arrival_rate: Vec<models::TopStation> = lowest
        .iter()
        .take(limit)
        .map(|s| {
            let province = state
                .station_meta
                .get(&s.id)
                .map(|m| m.province.clone())
                .unwrap_or_default();
            to_top(
                s,
                province,
                s.arrival_rate_24h,
                format!("{:.1}%", s.arrival_rate_24h),
            )
        })
        .collect();

    let longest_offline: Vec<models::TopStation> = offline_list
        .iter()
        .take(limit)
        .map(|(s, gap)| {
            let province = state
                .station_meta
                .get(&s.id)
                .map(|m| m.province.clone())
                .unwrap_or_default();
            let label = if *gap >= 9999.0 {
                "无数据".to_string()
            } else if *gap >= 1440.0 {
                format!("{:.1}天未更新", gap / 1440.0)
            } else if *gap >= 60.0 {
                format!("{:.1}小时未更新", gap / 60.0)
            } else {
                format!("{:.0}分钟未更新", gap)
            };
            to_top(s, province, *gap, label)
        })
        .collect();

    let most_alarms_list: Vec<models::TopStation> = most_alarms
        .iter()
        .take(limit)
        .map(|s| {
            let province = state
                .station_meta
                .get(&s.id)
                .map(|m| m.province.clone())
                .unwrap_or_default();
            to_top(
                s,
                province,
                s.alarm_count as f64,
                format!("{} 个告警", s.alarm_count),
            )
        })
        .collect();

    Json(models::TopLists {
        lowest_arrival_rate,
        longest_offline,
        most_alarms: most_alarms_list,
    })
}

async fn api_map_stations(
    State(state): State<Arc<AppState>>,
) -> Json<Vec<(models::StationMeta, models::StationStatus)>> {
    let data = state.data.read().await;
    let mut result = Vec::new();
    for st in &data.stations {
        if let Some(meta) = state.station_meta.get(&st.id) {
            if meta.latitude != 0.0 && meta.longitude != 0.0 {
                result.push((meta.clone(), st.clone()));
            }
        }
    }
    Json(result)
}

fn enrich_station_meta(
    stations: &mut [models::StationStatus],
    meta: &HashMap<String, models::StationMeta>,
) {
    for st in stations {
        if let Some(m) = meta.get(&st.id) {
            st.province.clone_from(&m.province);
        } else {
            st.province = "未知省份".to_string();
        }
    }
}

async fn refresh_station_devices_cache(state: &Arc<AppState>) {
    let station_ids: Vec<String> = state.config.stations.iter().map(|s| s.id.clone()).collect();

    // Refresh stations in batches of 4 to avoid DB overload
    for chunk in station_ids.chunks(4) {
        let mut handles = Vec::new();
        for id in chunk {
            let state = state.clone();
            let id = id.clone();
            handles.push(tokio::spawn(async move {
                let station_name = state
                    .config
                    .stations
                    .iter()
                    .find(|s| s.id == id)
                    .map(|s| s.name.clone())
                    .unwrap_or_default();

                let fresh = state.db.get_station_devices(&id, &station_name).await;
                let changed = {
                    let cache = state.station_devices.read().await;
                    match cache.get(&id) {
                        Some(prev) => {
                            serde_json::to_string(prev).ok() != serde_json::to_string(&fresh).ok()
                        }
                        None => true,
                    }
                };

                if changed {
                    let mut cache = state.station_devices.write().await;
                    cache.insert(id.clone(), fresh.clone());
                    drop(cache);

                    if let Ok(json) = serde_json::to_string(&fresh) {
                        let _ = state.devices_tx.send(json);
                    }
                    tracing::debug!("站点 {} 设备状态缓存已刷新", id);
                }
            }));
        }
        for h in handles {
            let _ = h.await;
        }
    }
}

async fn api_station_devices(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<models::StationDevicesResponse> {
    {
        let cache = state.station_devices.read().await;
        if let Some(cached) = cache.get(&id) {
            return Json(cached.clone());
        }
    }

    // Cache miss: query directly, cache immediately, and return the data
    let station_name = state
        .config
        .stations
        .iter()
        .find(|s| s.id == id)
        .map(|s| s.name.clone())
        .unwrap_or_else(|| {
            state
                .station_meta
                .get(&id)
                .map(|m| m.station_name.clone())
                .unwrap_or_default()
        });

    let fresh = state.db.get_station_devices(&id, &station_name).await;

    let mut cache = state.station_devices.write().await;
    cache.insert(id.clone(), fresh.clone());
    drop(cache);

    if let Ok(json) = serde_json::to_string(&fresh) {
        let _ = state.devices_tx.send(json);
    }

    Json(fresh)
}

async fn devices_sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let rx = state.devices_tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|result| match result {
        Ok(msg) => Ok(Event::default().data(msg)),
        Err(_) => Ok(Event::default().data("{}")),
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn chart_alarms(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ChartQuery>,
) -> Json<models::ChartAlarmResponse> {
    let hours = q.hours.unwrap_or(24).max(1).min(168);
    let stations = &state.config.stations;
    let trend = monitor::generate_alarm_trend(stations, hours);

    let time_labels: Vec<String> = trend
        .iter()
        .map(|(t, _)| t.format("%m-%d %H:00").to_string())
        .collect();

    let mut series_map = std::collections::HashMap::<String, Vec<usize>>::new();
    for st in stations {
        series_map.insert(st.id.clone(), Vec::new());
    }
    for (_, bucket) in &trend {
        for st in stations {
            let count = bucket.get(&st.id).copied().unwrap_or(0);
            series_map.get_mut(&st.id).unwrap().push(count);
        }
    }

    let series: Vec<models::ChartSeries> = stations
        .iter()
        .map(|st| models::ChartSeries {
            name: format!("{} {}", st.name, st.id),
            data: series_map.get(&st.id).cloned().unwrap_or_default(),
        })
        .collect();

    Json(models::ChartAlarmResponse {
        time: time_labels,
        series,
    })
}

async fn chart_values(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ValueQuery>,
) -> Json<models::ChartValueResponse> {
    let hours = q.hours.unwrap_or(24).max(1).min(168);
    let station_id = q.station.as_deref().unwrap_or("50936");
    let item = q.item.as_deref().unwrap_or("wA");

    let trend = monitor::generate_value_trend(&state.config.stations, station_id, item, hours);

    let time_labels: Vec<String> = trend
        .iter()
        .map(|(t, _)| t.format("%m-%d %H:%M").to_string())
        .collect();
    let values: Vec<f64> = trend.iter().map(|(_, v)| *v).collect();

    let (unit, item_name) = get_item_info(item);
    let station_name = state
        .config
        .stations
        .iter()
        .find(|s| s.id == station_id)
        .map(|s| format!("{} {}", s.name, s.id))
        .unwrap_or_else(|| station_id.to_string());

    Json(models::ChartValueResponse {
        time: time_labels,
        values,
        unit,
        item_name,
        station_name,
    })
}

async fn api_forecast(
    State(state): State<Arc<AppState>>,
    Query(q): Query<ForecastQuery>,
) -> Json<Vec<models::ForecastOverview>> {
    let data = state.data.read().await;
    let mut result = monitor::generate_forecast_overview(&data.stations);
    if let Some(station_filter) = q.station.as_deref() {
        result.retain(|item| item.station_id == station_filter);
    }
    Json(result)
}

async fn api_forecast_detail(
    State(state): State<Arc<AppState>>,
    Path(id): Path<String>,
) -> Json<Option<models::ForecastDetail>> {
    let data = state.data.read().await;
    let status = match data.stations.iter().find(|s| s.id == id) {
        Some(s) => s.clone(),
        None => return Json(None),
    };

    let meta = state.station_meta.get(&id);
    let result = monitor::generate_forecast_detail(&status, meta);
    Json(Some(result))
}

async fn api_license(State(state): State<Arc<AppState>>) -> Json<serde_json::Value> {
    let license_state = state.license_state.lock().unwrap();
    Json(license::license::get_status(&license_state))
}

async fn sse_handler(
    State(state): State<Arc<AppState>>,
) -> Sse<impl tokio_stream::Stream<Item = Result<Event, axum::Error>>> {
    let rx = state.tx.subscribe();
    let stream = BroadcastStream::new(rx).map(|result| match result {
        Ok(msg) => Ok(Event::default().data(msg)),
        Err(_) => Ok(Event::default().data("{}")),
    });

    Sse::new(stream).keep_alive(
        axum::response::sse::KeepAlive::new()
            .interval(Duration::from_secs(15))
            .text("keep-alive"),
    )
}

async fn login_handler(
    State(state): State<Arc<AppState>>,
    Form(form): Form<LoginForm>,
) -> Response {
    if !state.config.auth.enabled {
        return Redirect::to("/").into_response();
    }

    if form.username == state.config.auth.username && form.password == state.config.auth.password {
        let token: String = thread_rng()
            .sample_iter(&Alphanumeric)
            .take(48)
            .map(char::from)
            .collect();
        {
            let mut tokens = state.session_tokens.write().await;
            tokens.insert(token.clone());
        }

        let cookie_value = format!("session_token={}; HttpOnly; SameSite=Lax; Path=/", token);
        let mut headers = HeaderMap::new();
        headers.insert(
            header::SET_COOKIE,
            HeaderValue::from_str(&cookie_value).unwrap(),
        );
        headers.insert(header::LOCATION, HeaderValue::from_static("/"));
        (StatusCode::FOUND, headers, "").into_response()
    } else {
        let mut headers = HeaderMap::new();
        headers.insert(header::LOCATION, HeaderValue::from_static("/login"));
        (StatusCode::FOUND, headers, "").into_response()
    }
}

async fn logout_handler(State(state): State<Arc<AppState>>, req: Request<Body>) -> Response {
    if let Some(cookie_header) = req.headers().get(header::COOKIE) {
        if let Ok(cookie_str) = cookie_header.to_str() {
            for part in cookie_str.split(';') {
                let pair = part.trim();
                if let Some((name, value)) = pair.split_once('=') {
                    if name == "session_token" {
                        let mut tokens = state.session_tokens.write().await;
                        tokens.remove(value);
                    }
                }
            }
        }
    }

    let mut headers = HeaderMap::new();
    headers.insert(
        header::SET_COOKIE,
        HeaderValue::from_static(
            "session_token=deleted; Max-Age=0; Path=/; HttpOnly; SameSite=Lax",
        ),
    );
    headers.insert(header::LOCATION, HeaderValue::from_static("/login"));
    (StatusCode::FOUND, headers, "").into_response()
}

async fn authenticate_request(cookie_header: Option<String>, state: &AppState) -> bool {
    if !state.config.auth.enabled {
        return true;
    }

    if let Some(cookie_str) = cookie_header {
        for part in cookie_str.split(';') {
            let pair = part.trim();
            if let Some((name, value)) = pair.split_once('=') {
                if name == "session_token" {
                    let tokens = state.session_tokens.read().await;
                    if tokens.contains(value) {
                        return true;
                    }
                }
            }
        }
    }
    false
}

async fn auth_middleware(req: Request<Body>, next: Next, state: Arc<AppState>) -> Response {
    let path = req.uri().path();
    if path == "/login" || path == "/logout" || req.method() == Method::OPTIONS {
        return next.run(req).await;
    }

    let cookie_header = req
        .headers()
        .get(header::COOKIE)
        .and_then(|h| h.to_str().ok())
        .map(ToOwned::to_owned);

    if authenticate_request(cookie_header, &state).await {
        next.run(req).await
    } else {
        let mut headers = HeaderMap::new();
        headers.insert(header::LOCATION, HeaderValue::from_static("/login"));
        (StatusCode::FOUND, headers, "").into_response()
    }
}

fn build_cors_layer(origins: &[String]) -> CorsLayer {
    if origins.is_empty() {
        CorsLayer::new()
            .allow_methods(Any)
            .allow_headers(Any)
            .allow_origin(Any)
    } else {
        let origin_values: Vec<HeaderValue> = origins
            .iter()
            .filter_map(|origin| HeaderValue::from_str(origin).ok())
            .collect();
        CorsLayer::new()
            .allow_methods([Method::GET, Method::POST, Method::OPTIONS])
            .allow_headers([header::CONTENT_TYPE, header::COOKIE])
            .allow_origin(AllowOrigin::list(origin_values))
    }
}

fn get_item_info(item: &str) -> (String, String) {
    match item.chars().next() {
        Some('w') => ("℃".into(), format!("温度({})", item)),
        Some('x') if item.len() > 1 => match item.as_bytes()[1] {
            b'B' => ("V".into(), format!("外接电源电压({})", item)),
            b'C' => ("V".into(), format!("蓄电池电压({})", item)),
            b'F' => ("mA".into(), format!("工作电流({})", item)),
            b'H' => ("%".into(), format!("蓄电池电量({})", item)),
            _ => ("".into(), format!("供电项({})", item)),
        },
        Some('u') if item.len() > 1 => match item.as_bytes()[1] {
            b'D' => ("m/s".into(), format!("通风速度({})", item)),
            b'E' => ("r/min".into(), format!("转速({})", item)),
            _ => ("".into(), format!("通风项({})", item)),
        },
        Some('t') => ("".into(), format!("通信状态({})", item)),
        Some('v') => ("".into(), format!("加热状态({})", item)),
        Some('s') => ("".into(), format!("污染状态({})", item)),
        _ => ("".into(), format!("监测项({})", item)),
    }
}

async fn static_handler() -> Response {
    (StatusCode::NOT_FOUND, "Not Found").into_response()
}
