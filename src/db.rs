use crate::config::{DatabaseConfig, StationConfig};
use crate::models::{StationMeta, StationStatus, MonitorSummary, MonitorData, DeviceStatusInfo, DeviceStatusItem, StationDevicesResponse};
use crate::monitor::{parse_st_packet};
use chrono::{Local, Timelike};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::MySqlPool;
use std::collections::{HashMap, HashSet};
use std::time::Duration;
use tracing;

fn parse_dms_coord(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() {
        return 0.0;
    }

    // Try plain decimal first
    if let Ok(v) = s.parse::<f64>() {
        // If already decimal degrees, return directly
        // Decimal degrees are typically small (|v| < 180) and may have a decimal point
        if v.abs() <= 180.0 && s.contains('.') {
            return v;
        }
        // Large integers like 1224741 are DMS, not decimal
    }

    // DMS string: DDMMSS or DDDMMSS, possibly with leading zeros
    let digits: String = s.chars().filter(|c| c.is_ascii_digit()).collect();
    if digits.len() < 4 {
        return 0.0;
    }

    let len = digits.len();
    let ss = digits[len - 2..].parse::<f64>().unwrap_or(0.0);
    let mm = digits[len - 4..len - 2].parse::<f64>().unwrap_or(0.0);
    let dd = digits[..len - 4].parse::<f64>().unwrap_or(0.0);

    dd + mm / 60.0 + ss / 3600.0
}

#[derive(sqlx::FromRow)]
struct StationParamsRow {
    #[sqlx(rename = "station_id")]
    station_id: String,
    #[sqlx(rename = "station_name")]
    station_name: Option<String>,
    #[sqlx(rename = "province_name")]
    province_name: Option<String>,
    #[sqlx(rename = "station_address")]
    station_address: Option<String>,
    #[sqlx(rename = "latitude")]
    latitude: Option<String>,
    #[sqlx(rename = "longitude")]
    longitude: Option<String>,
    #[sqlx(rename = "observation_field_altitude")]
    observation_field_altitude: Option<f32>,
    #[sqlx(rename = "auto_station_type_id")]
    auto_station_type_id: Option<String>,
    #[sqlx(rename = "acid_rain_station")]
    acid_rain_station: Option<i8>,
    #[sqlx(rename = "reference_radiation_station")]
    reference_radiation_station: Option<i8>,
}

pub struct DbService {
    pub pool: Option<MySqlPool>,
    pub cloud_pool: Option<MySqlPool>,
    pub sqlite_pool: Option<sqlx::SqlitePool>,
    pub simulation_mode: bool,
    device_type_names: HashMap<String, String>,
}

impl DbService {
    pub async fn new(cfg: &DatabaseConfig, cloud_cfg: &DatabaseConfig) -> Self {
        let db_url = format!(
            "mysql://{}:{}@{}:{}/{}",
            cfg.user, cfg.password, cfg.host, cfg.port, cfg.db
        );

        let main_pool = match MySqlPoolOptions::new()
            .max_connections(10)
            .min_connections(2)
            .acquire_timeout(Duration::from_secs(10))
            .connect(&db_url)
            .await
        {
            Ok(pool) => {
                tracing::info!("MySQL 主库连接成功: {}:{}/{}", cfg.host, cfg.port, cfg.db);
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("MySQL 主库连接失败: {}，切换到模拟模式", e);
                None
            }
        };

        let cloud_db_url = format!(
            "mysql://{}:{}@{}:{}/{}",
            cloud_cfg.user, cloud_cfg.password, cloud_cfg.host, cloud_cfg.port, cloud_cfg.db
        );

        let cloud_pool = match MySqlPoolOptions::new()
            .max_connections(2)
            .connect(&cloud_db_url)
            .await
        {
            Ok(pool) => {
                tracing::info!("MySQL 云库连接成功: {}:{}/{}", cloud_cfg.host, cloud_cfg.port, cloud_cfg.db);
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("MySQL 云库连接失败: {}，站点元数据将不可用", e);
                None
            }
        };

        let is_main_none = main_pool.is_none();

        let mut svc = Self {
            pool: main_pool,
            cloud_pool,
            sqlite_pool: None,
            simulation_mode: is_main_none,
            device_type_names: HashMap::new(),
        };

        svc.load_device_type_names().await;
        svc
    }

    async fn load_device_type_names(&mut self) {
        let pool = match self.cloud_pool.as_ref() {
            Some(p) => p,
            None => return,
        };

        #[derive(sqlx::FromRow)]
        struct DeviceTypeRow {
            code: String,
            name: Option<String>,
        }

        let rows: Vec<DeviceTypeRow> = match sqlx::query_as::<_, DeviceTypeRow>(
            "SELECT code, name FROM device_type"
        )
        .fetch_all(pool)
        .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("加载 device_type 失败: {}", e);
                return;
            }
        };

        for row in rows {
            self.device_type_names.insert(row.code, row.name.unwrap_or_default());
        }
        tracing::info!("已加载 {} 个设备类型名称", self.device_type_names.len());
    }

    pub fn get_device_type_names(&self) -> HashMap<String, String> {
        self.device_type_names.clone()
    }

    pub fn get_device_type_name(&self, code: &str) -> String {
        self.device_type_names
            .get(code)
            .cloned()
            .filter(|s| !s.is_empty())
            .unwrap_or_else(|| code.to_string())
    }

    pub async fn new_simulation() -> Self {
        let mut device_type_names = HashMap::new();
        device_type_names.insert("YTHWS".to_string(), "温湿度仪".to_string());
        device_type_names.insert("YVISI".to_string(), "能见度仪".to_string());
        device_type_names.insert("YWIND".to_string(), "风仪".to_string());
        device_type_names.insert("YPREC".to_string(), "降水仪".to_string());
        device_type_names.insert("YPRES".to_string(), "降水现象仪".to_string());
        device_type_names.insert("YACID".to_string(), "酸雨自动观测仪".to_string());
        device_type_names.insert("YCLOD".to_string(), "云仪".to_string());
        device_type_names.insert("YEVAP".to_string(), "蒸发仪".to_string());
        device_type_names.insert("YSRAD".to_string(), "太阳辐射仪".to_string());
        device_type_names.insert("YCO2".to_string(), "二氧化碳分析仪".to_string());
        device_type_names.insert("YPOWR".to_string(), "智能电源".to_string());
        device_type_names.insert("YGNSS".to_string(), "GNSS".to_string());
        device_type_names.insert("YSNOW".to_string(), "雪深仪".to_string());
        device_type_names.insert("YGRND".to_string(), "地温仪".to_string());
        device_type_names.insert("YPRSS".to_string(), "气压仪".to_string());
        device_type_names.insert("YUVRA".to_string(), "紫外辐射仪".to_string());
        device_type_names.insert("YPHOT".to_string(), "光合有效辐射仪".to_string());
        device_type_names.insert("YBLIZ".to_string(), "闪电定位仪".to_string());
        device_type_names.insert("YNEPH".to_string(), "云高仪".to_string());
        device_type_names.insert("YMPAR".to_string(), "多参数仪".to_string());
        
        // 创建本地 SQLite 测试数据库
        let sqlite_pool = match SqlitePoolOptions::new()
            .max_connections(1)
            .connect("sqlite:test_data.db")
            .await
        {
            Ok(pool) => {
                tracing::info!("SQLite 测试数据库创建成功: test_data.db");
                Some(pool)
            }
            Err(e) => {
                tracing::warn!("SQLite 测试数据库创建失败: {}，使用纯内存模拟", e);
                None
            }
        };
        
        if let Some(pool) = &sqlite_pool {
            // 初始化 data_st 表
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS data_st (
                    id INTEGER PRIMARY KEY AUTOINCREMENT,
                    station_num TEXT,
                    device_type TEXT,
                    device_nid TEXT,
                    data_time TEXT,
                    receive_time TEXT,
                    data TEXT
                )"
            )
            .execute(pool)
            .await
            .ok();
            
            // 初始化 station_params 表
            sqlx::query(
                "CREATE TABLE IF NOT EXISTS station_params (
                    station_id TEXT PRIMARY KEY,
                    station_name TEXT,
                    province_name TEXT,
                    station_address TEXT,
                    latitude REAL,
                    longitude REAL,
                    observation_field_altitude REAL,
                    auto_station_type_id TEXT,
                    acid_rain_station INTEGER,
                    reference_radiation_station INTEGER
                )"
            )
            .execute(pool)
            .await
            .ok();
            
            // 站点数据
            let stations = [
                ("50936", "吉林白城", "吉林", 45.6, 122.8),
                ("50968", "黑龙江尚志", "黑龙江", 45.2, 127.9),
                ("53399", "河北张北", "河北", 41.1, 114.7),
                ("53942", "陕西洛川", "陕西", 35.7, 109.4),
                ("54333", "辽宁新民", "辽宁", 41.9, 122.8),
                ("54416", "北京密云", "北京", 40.4, 116.8),
                ("54808", "山东莘县", "山东", 36.2, 115.6),
                ("56173", "四川红原", "四川", 32.8, 102.5),
                ("56312", "西藏林芝", "西藏", 29.6, 94.3),
                ("57633", "重庆酉阳", "重庆", 28.8, 108.7),
                ("57958", "广西雁山", "广西", 25.0, 110.3),
                ("58005", "河南商丘", "河南", 34.4, 115.6),
                ("58457", "浙江杭州", "浙江", 30.2, 120.1),
                ("58737", "福建建瓯", "福建", 27.0, 118.3),
                ("52983", "甘肃榆中", "天津", 35.8, 104.1),
                ("53817", "宁夏固原", "天津", 36.0, 106.2),
                ("51358", "新疆乌兰乌苏", "无锡", 44.2, 85.9),
                ("52754", "青海刚察", "无锡", 37.3, 100.1),
                ("52856", "青海共和", "无锡", 36.3, 100.6),
                ("53963", "山西侯马", "无锡", 35.6, 111.3),
                ("56739", "云南腾冲", "无锡", 25.0, 98.4),
                ("57251", "湖北郧西", "无锡", 33.2, 110.4),
                ("57793", "江西宜春", "无锡", 27.8, 114.3),
                ("57832", "贵州三穗", "无锡", 26.6, 108.6),
                ("57874", "湖南常宁", "无锡", 26.4, 112.3),
                ("58141", "江苏淮安", "无锡", 33.6, 119.0),
                ("58362", "上海宝山", "无锡", 31.4, 121.4),
                ("58437", "安徽黄山", "无锡", 30.1, 118.1),
                ("59758", "海南海口", "无锡", 20.0, 110.3),
                ("59294", "广州增城", "广东", 23.2, 113.6),
                ("52737", "青海德令哈", "无锡", 37.3, 97.3),
                ("57914", "贵州花溪", "无锡", 26.4, 106.6),
            ];
            
            // 插入站点参数
            for (id, name, province, lat, lon) in &stations {
                sqlx::query("INSERT OR REPLACE INTO station_params (station_id, station_name, province_name, latitude, longitude) VALUES (?, ?, ?, ?, ?)")
                    .bind(id)
                    .bind(name)
                    .bind(province)
                    .bind(lat)
                    .bind(lon)
                    .execute(pool)
                    .await
                    .ok();
            }
            
            // 插入设备数据
            let now = chrono::Local::now();
            let now_str = now.format("%Y-%m-%d %H:%M:%S").to_string();
            let data_time_compact = now.format("%Y%m%d%H%M%S").to_string();
            let device_types = [
                ("YTHWS", "N01"), ("YTHWS", "N02"),
                ("YWIND", "N01"), ("YPREC", "N01"),
                ("YVISI", "N01"), ("YPRSS", "N01"),
                ("YSRAD", "N01"), ("YGRND", "N01"),
                ("YCO2", "N01"), ("YPOWR", "N01"),
            ];
            
            for station in &stations {
                for (dtype, nid) in &device_types {
                    let data = format!(
                        "DATADICK,V202201,{},{}{},{},ST,{},z,0,yA,0,yB,0,wA,25.0,wAA,0,xB,220,xC,12.6,xE,12.6,xEA,0,xF,37,xFA,0,tA,0,sA,0,rA,0,qA,0,vA,0,uD,0",
                        station.0, dtype, nid, nid, data_time_compact
                    );
                    sqlx::query("INSERT INTO data_st (station_num, device_type, device_nid, data_time, receive_time, data) VALUES (?, ?, ?, ?, ?, ?)")
                        .bind(station.0)
                        .bind(dtype)
                        .bind(nid)
                        .bind(&now_str)
                        .bind(&now_str)
                        .bind(&data)
                        .execute(pool)
                        .await
                        .ok();
                }
            }
            
            tracing::info!("SQLite 测试数据初始化完成: {} 个站点, {} 台设备", stations.len(), stations.len() * device_types.len());
        }
        
        Self {
            pool: None,
            cloud_pool: None,
            sqlite_pool,
            simulation_mode: true,
            device_type_names,
        }
    }

    pub async fn load_station_meta(
        &self,
        station_ids: &[StationConfig],
    ) -> HashMap<String, StationMeta> {
        let mut result = HashMap::new();
        
        // 优先使用 SQLite 本地数据库（测试模式）
        if let Some(pool) = self.sqlite_pool.as_ref() {
            let ids_placeholders = station_ids
                .iter()
                .map(|_| "?")
                .collect::<Vec<_>>()
                .join(",");
            
            let sql = format!(
                "SELECT station_id, station_name, province_name, station_address, \
                 latitude, longitude, observation_field_altitude, \
                 auto_station_type_id, acid_rain_station, reference_radiation_station \
                 FROM station_params \
                 WHERE station_id IN ({})",
                ids_placeholders
            );
            
            let mut query = sqlx::query_as::<_, StationParamsRow>(&sql);
            for s in station_ids {
                query = query.bind(&s.id);
            }
            let rows: Vec<StationParamsRow> = match query.fetch_all(pool).await {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!("SQLite 加载站点元数据失败: {}", e);
                    return result;
                }
            };
            
            for row in rows {
                result.insert(
                    row.station_id.clone(),
                    StationMeta {
                        station_id: row.station_id,
                        station_name: row.station_name.unwrap_or_default(),
                        province: row.province_name.unwrap_or_default(),
                        address: row.station_address.unwrap_or_default(),
                        latitude: row.latitude.map(|v| parse_dms_coord(&v.to_string())).unwrap_or(0.0),
                        longitude: row.longitude.map(|v| parse_dms_coord(&v.to_string())).unwrap_or(0.0),
                        altitude: row.observation_field_altitude.unwrap_or(0.0),
                        station_type: row.auto_station_type_id.unwrap_or_default(),
                        is_acid_rain: row.acid_rain_station.unwrap_or(0) != 0,
                        is_reference_radiation: row.reference_radiation_station.unwrap_or(0) != 0,
                    },
                );
            }
            return result;
        }
        
        let pool = match (self.cloud_pool.as_ref(), self.pool.as_ref()) {
            (Some(p), _) => p,
            (None, Some(p)) => p,
            (None, None) => {
                // Fallback: create meta from config with approximate coordinates
                fn station_sim_meta(id: &str) -> (&'static str, f64, f64) {
                    match id {
                        "50936" => ("吉林", 45.6, 122.8),
                        "50968" => ("黑龙江", 45.2, 127.9),
                        "53399" => ("河北", 41.1, 114.7),
                        "53942" => ("陕西", 35.7, 109.4),
                        "54333" => ("辽宁", 41.9, 122.8),
                        "54416" => ("北京", 40.4, 116.8),
                        "54808" => ("山东", 36.2, 115.6),
                        "56173" => ("四川", 32.8, 102.5),
                        "56312" => ("西藏", 29.6, 94.3),
                        "57633" => ("重庆", 28.8, 108.7),
                        "57958" => ("广西", 25.0, 110.3),
                        "58005" => ("河南", 34.4, 115.6),
                        "58457" => ("浙江", 30.2, 120.1),
                        "58737" => ("福建", 27.0, 118.3),
                        "52983" => ("甘肃", 35.8, 104.1),
                        "53817" => ("宁夏", 36.0, 106.2),
                        "51358" => ("新疆", 44.2, 85.9),
                        "52754" => ("青海", 37.3, 100.1),
                        "52856" => ("青海", 36.3, 100.6),
                        "53963" => ("山西", 35.6, 111.3),
                        "56739" => ("云南", 25.0, 98.4),
                        "57251" => ("湖北", 33.2, 110.4),
                        "57793" => ("江西", 27.8, 114.3),
                        "57832" => ("贵州", 26.6, 108.6),
                        "57874" => ("湖南", 26.4, 112.3),
                        "58141" => ("江苏", 33.6, 119.0),
                        "58362" => ("上海", 31.4, 121.4),
                        "58437" => ("安徽", 30.1, 118.1),
                        "59758" => ("海南", 20.0, 110.3),
                        "59294" => ("广东", 23.2, 113.6),
                        "52737" => ("青海", 37.3, 97.3),
                        "57914" => ("贵州", 26.4, 106.6),
                        _ => ("未知省份", 0.0, 0.0),
                    }
                }
                for st in station_ids {
                    let meta = station_sim_meta(&st.id);
                    result.insert(
                        st.id.clone(),
                        StationMeta {
                            station_id: st.id.clone(),
                            station_name: st.name.clone(),
                            province: meta.0.to_string(),
                            address: String::new(),
                            latitude: meta.1,
                            longitude: meta.2,
                            altitude: 0.0,
                            station_type: String::new(),
                            is_acid_rain: false,
                            is_reference_radiation: false,
                        },
                    );
                }
                return result;
            }
        };

        let ids_placeholders = station_ids
            .iter()
            .map(|_| "?")
            .collect::<Vec<_>>()
            .join(",");

        let sql = format!(
            "SELECT station_id, station_name, province_name, station_address, \
             latitude, longitude, observation_field_altitude, \
             auto_station_type_id, acid_rain_station, reference_radiation_station \
             FROM station_params \
             WHERE station_id IN ({})",
            ids_placeholders
        );

        let mut query = sqlx::query_as::<_, StationParamsRow>(&sql);
        for s in station_ids {
            query = query.bind(&s.id);
        }
        let rows: Vec<StationParamsRow> = match query.fetch_all(pool).await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("加载 station_params 失败: {}", e);
                return result;
            }
        };

        let mut found_ids = std::collections::HashSet::new();
        for row in rows {
            found_ids.insert(row.station_id.clone());
            let province = row.province_name.as_deref().unwrap_or("未知省份").trim();
            let province = if province.is_empty() { "未知省份".to_string() } else { province.to_string() };
            let station_type = row
                .auto_station_type_id
                .as_deref()
                .unwrap_or("")
                .trim()
                .to_string();
            result.insert(
                row.station_id.clone(),
                StationMeta {
                    station_id: row.station_id.clone(),
                    station_name: row.station_name.unwrap_or_default(),
                    province,
                    address: row.station_address.unwrap_or_default(),
                    latitude: parse_dms_coord(&row.latitude.unwrap_or_default()),
                    longitude: parse_dms_coord(&row.longitude.unwrap_or_default()),
                    altitude: row.observation_field_altitude.unwrap_or(0.0),
                    station_type,
                    is_acid_rain: row.acid_rain_station.unwrap_or(0) != 0,
                    is_reference_radiation: row.reference_radiation_station.unwrap_or(0) != 0,
                },
            );
        }

        // Fill missing stations with unknown province
        for st in station_ids {
            if !found_ids.contains(&st.id) {
                tracing::warn!("站点 {} 在 station_params 中未找到", st.id);
                result.insert(
                    st.id.clone(),
                    StationMeta {
                        station_id: st.id.clone(),
                        station_name: st.name.clone(),
                        province: "未知省份".to_string(),
                        address: String::new(),
                        latitude: 0.0,
                        longitude: 0.0,
                        altitude: 0.0,
                        station_type: String::new(),
                        is_acid_rain: false,
                        is_reference_radiation: false,
                    },
                );
            }
        }

        result
    }

    pub async fn query_monitor_data(
        &self,
        stations: &[StationConfig],
        check_interval_minutes: i32,
    ) -> MonitorData {
        if self.simulation_mode || self.pool.is_none() {
            return crate::monitor::generate_simulated_data(stations);
        }

        let start = std::time::Instant::now();
        let pool = self.pool.as_ref().unwrap();
        let station_ids: Vec<String> = stations.iter().map(|s| s.id.clone()).collect();
        let ids_placeholders = station_ids.iter().map(|_| "?").collect::<Vec<_>>().join(",");

        // Main query - keep as grouped queries but split basic aggregates
        // and distinct device count. On the 15 GB table this is much faster
        // than per-station queries because each query scans the small recent
        // window only once.
        let basic_sql = format!(
            "SELECT station_num, COUNT(*), \
             COUNT(IF(data_time > (NOW() - INTERVAL 6 MINUTE), 1, NULL)), \
             MIN(data_time), MAX(data_time) \
             FROM data_st \
             WHERE receive_time > (NOW() - INTERVAL ? MINUTE) \
             AND station_num IN ({}) \
             GROUP BY station_num ORDER BY station_num",
            ids_placeholders
        );

        let mut basic_query = sqlx::query_as::<_, (String, i64, Option<i64>, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>)>(&basic_sql);
        basic_query = basic_query.bind(check_interval_minutes);
        for id in &station_ids {
            basic_query = basic_query.bind(id);
        }
        let basic_rows = match basic_query.fetch_all(pool).await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::error!("主聚合查询失败: {}" , e);
                return crate::monitor::generate_simulated_data(stations);
            }
        };

        let device_sql = format!(
            "SELECT station_num, COUNT(DISTINCT CONCAT(device_type, device_nid)) \
             FROM data_st \
             WHERE receive_time > (NOW() - INTERVAL ? MINUTE) \
             AND station_num IN ({}) \
             GROUP BY station_num ORDER BY station_num",
            ids_placeholders
        );

        let mut device_query = sqlx::query_as::<_, (String, i64)>(&device_sql);
        device_query = device_query.bind(check_interval_minutes);
        for id in &station_ids {
            device_query = device_query.bind(id);
        }
        let device_rows: std::collections::HashMap<String, i64> = match device_query.fetch_all(pool).await
        {
            Ok(rows) => rows.into_iter().collect(),
            Err(e) => {
                tracing::warn!("设备数查询失败: {}", e);
                std::collections::HashMap::new()
            }
        };

        let mut rows_map: std::collections::HashMap<String, (i64, Option<i64>, Option<chrono::NaiveDateTime>, Option<chrono::NaiveDateTime>, i64)> = HashMap::new();
        for row in basic_rows {
            let device_count = device_rows.get(&row.0).copied().unwrap_or(0);
            rows_map.insert(row.0, (row.1, row.2, row.3, row.4, device_count));
        }

        let mut stations_out = Vec::new();
        let mut total_alarms = 0usize;
        let mut total_checked = 0usize;
        let mut online_count = 0usize;
        let mut total_records = 0i64;

        // Query last arrival time and approximate arrival rate per station
        // Avoid scanning the full 24h window on huge tables:
        // 1) Get latest receive_time per station from index
        // 2) Count recent records in a small window (30 min) as a proxy for activity
        let arrival_sql = format!(
            "SELECT station_num, \
             MAX(receive_time) as last_arrival, \
             COUNT(IF(receive_time > (NOW() - INTERVAL 30 MINUTE), 1, NULL)) as cnt_recent \
             FROM data_st \
             WHERE station_num IN ({}) \
             AND receive_time > (NOW() - INTERVAL 2 HOUR) \
             GROUP BY station_num",
            ids_placeholders
        );

        let mut arrival_query = sqlx::query_as::<_, (String, Option<chrono::NaiveDateTime>, i64)>(&arrival_sql);
        for id in &station_ids {
            arrival_query = arrival_query.bind(id);
        }
        let arrival_rows: std::collections::HashMap<String, (Option<chrono::NaiveDateTime>, i64)> = 
            match arrival_query.fetch_all(pool).await
            {
                Ok(rows) => rows.into_iter().map(|r| (r.0, (r.1, r.2))).collect(),
                Err(_) => std::collections::HashMap::new(),
            };

        for (station_id, row) in rows_map {
            let r1 = row.0;
            let r2 = row.1.unwrap_or(0);
            let r5 = row.4;
            let min_time = row.2.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();
            let max_time = row.3.map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string()).unwrap_or_default();

            total_records += r1;
            let info = stations.iter().find(|s| s.id == station_id);
            let mut alarms = Vec::new();
            let needs_st_check = r2 == r5 && r5 > 20;

            if needs_st_check && !min_time.is_empty() {
                // Query ST packet for this station - port of sqlProST
                let st_sql = "SELECT data FROM data_st \
                     WHERE station_num = ? AND data_time = ? \
                     AND receive_time > (NOW() - INTERVAL 10 MINUTE) \
                     LIMIT 1";

                if let Ok(st_row) = sqlx::query_scalar::<_, String>(&st_sql)
                    .bind(&station_id)
                    .bind(&min_time)
                    .fetch_optional(pool)
                    .await
                {
                    if let Some(data_str) = st_row {
                        let parsed = parse_st_packet(&data_str);
                        for p in parsed {
                            if p.abnormal {
                                alarms.push(p.alarm);
                                total_alarms += 1;
                            }
                            total_checked += 1;
                        }
                    }
                }
            }

            let is_online = needs_st_check;
            if is_online {
                online_count += 1;
            }

            // Get last arrival time and recent arrival rate (30 min proxy)
            let (last_arrival, arrival_rate) = arrival_rows.get(&station_id)
                .map(|(t, cnt)| {
                    let last = t.map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                        .unwrap_or_default();
                    // 30 minutes has 6 expected 5-minute buckets; scale to 100%
                    let rate = (*cnt as f64 / 6.0 * 100.0 * 10.0).round() / 10.0;
                    (last, rate.min(100.0))
                })
                .unwrap_or((String::new(), 0.0));

            stations_out.push(StationStatus {
                id: station_id.clone(),
                name: info.map(|s| s.name.clone()).unwrap_or_default(),
                vendor: info.map(|s| s.vendor.clone()).unwrap_or_default(),
                province: String::new(),
                records: r1,
                recent_5min: r2,
                min_time,
                max_time,
                devices: r5,
                online: is_online,
                alarms: alarms.clone(),
                alarm_count: alarms.len(),
                last_arrival_time: last_arrival,
                arrival_rate_24h: arrival_rate,
            });
        }

        // Add stations that have no data
        for st in stations {
            if !stations_out.iter().any(|s| s.id == st.id) {
                stations_out.push(StationStatus {
                    id: st.id.clone(),
                    name: st.name.clone(),
                    vendor: st.vendor.clone(),
                    province: String::new(),
                    records: 0,
                    recent_5min: 0,
                    min_time: String::new(),
                    max_time: String::new(),
                    devices: 0,
                    online: false,
                    alarms: vec![],
                    alarm_count: 0,
                    last_arrival_time: String::new(),
                    arrival_rate_24h: 0.0,
                });
            }
        }

        let avg_rate = if !stations_out.is_empty() {
            stations_out.iter().map(|s| s.arrival_rate_24h).sum::<f64>() / stations_out.len() as f64
        } else {
            0.0
        };

        let duration = start.elapsed();
        tracing::info!("query_monitor_data 完成: {} 个站点, 耗时 {:?}", stations_out.len(), duration);

        MonitorData {
            summary: MonitorSummary {
                total: stations.len(),
                online: online_count,
                alarms: total_alarms,
                checked: total_checked,
                records: total_records,
                avg_arrival_rate: (avg_rate * 10.0).round() / 10.0,
            },
            stations: stations_out,
            last_update: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            error: None,
        }
    }

    pub async fn get_station_devices(
        &self,
        station_id: &str,
        station_name: &str,
    ) -> StationDevicesResponse {
        let mut result = StationDevicesResponse {
            station_id: station_id.to_string(),
            station_name: station_name.to_string(),
            devices: Vec::new(),
        };

        // 优先使用 SQLite 本地数据库（测试模式）
        if let Some(pool) = self.sqlite_pool.as_ref() {
            let sql = "SELECT device_type, device_nid, data_time, data 
                 FROM data_st 
                 WHERE station_num = ? 
                 AND receive_time > datetime('now', '-1 day', 'localtime')
                 ORDER BY device_type, device_nid, data_time DESC, receive_time DESC, id DESC
                 LIMIT 100";

            #[derive(sqlx::FromRow)]
            struct StRow {
                device_type: String,
                device_nid: String,
                data_time: chrono::NaiveDateTime,
                data: String,
            }

            let rows: Vec<StRow> = match sqlx::query_as::<_, StRow>(&sql)
                .bind(station_id)
                .fetch_all(pool)
                .await
            {
                Ok(rows) => rows,
                Err(e) => {
                    tracing::warn!("SQLite 查询站点设备状态失败 {}: {}", station_id, e);
                    return result;
                }
            };

            let now_minute = chrono::Local::now().naive_local()
                .with_second(0).unwrap()
                .with_nanosecond(0).unwrap();
            let prev_minute = now_minute - chrono::Duration::minutes(1);

            let mut seen: HashSet<(String, String)> = HashSet::new();
            let mut devices: Vec<DeviceStatusInfo> = Vec::new();

            for row in rows {
                let key = (row.device_type.clone(), row.device_nid.clone());
                if seen.contains(&key) { continue; }
                seen.insert(key.clone());

                let parsed = parse_st_packet(&row.data);
                let (common_status, specific_status) = classify_device_status(&parsed, &row.device_type);

                let data_time = row.data_time;
                let is_online = data_time >= now_minute - chrono::Duration::minutes(5);
                let is_fallback = !is_online && data_time >= now_minute - chrono::Duration::minutes(60);

                devices.push(DeviceStatusInfo {
                    device_type: row.device_type,
                    device_nid: row.device_nid,
                    device_name: self.get_device_type_name(&key.0),
                    last_data_time: data_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    is_online,
                    is_fallback,
                    common_status,
                    specific_status,
                });
            }

            devices.sort_by(|a, b| {
                fn order(info: &DeviceStatusInfo) -> u8 {
                    if info.is_online { 0 }
                    else if info.is_fallback { 1 }
                    else { 2 }
                }
                order(a).cmp(&order(b))
                    .then_with(|| a.device_type.cmp(&b.device_type))
            });

            result.devices = devices;
            return result;
        }

        // 纯内存模拟（SQLite 不可用时的回退）
        if self.simulation_mode {
            use rand::Rng;
            let mut rng = rand::thread_rng();
            let now = chrono::Local::now();
            let now_minute = now.naive_local().with_second(0).unwrap().with_nanosecond(0).unwrap();
            let prev_minute = now_minute - chrono::Duration::minutes(1);
            
            let device_types = [
                ("YTHWS", "N01"), ("YTHWS", "N02"),
                ("YWIND", "N01"), ("YPREC", "N01"),
                ("YVISI", "N01"), ("YPRSS", "N01"),
                ("YSRAD", "N01"), ("YGRND", "N01"),
                ("YCO2", "N01"), ("YPOWR", "N01"),
            ];
            
            for (dtype, nid) in &device_types {
                let is_online = rng.gen_range(0.0..1.0) > 0.2;
                let is_fallback = !is_online && rng.gen_range(0.0..1.0) > 0.3;
                let data_time = if is_online { now_minute } 
                    else if is_fallback { prev_minute } 
                    else { now_minute - chrono::Duration::minutes(rng.gen_range(5..60)) };
                
                // 随机生成一些异常状态，让前端测试异常展示
                let (z_v, ya_v, waa_v, xea_v, tfa_v, ta_v, sa_v) = if rng.gen_range(0.0..1.0) > 0.6 {
                    ("0","0","0","0","0","0","0")
                } else {
                    let idx = rng.gen_range(0..6);
                    match idx {
                        0 => ("1","0","0","0","0","0","0"),
                        1 => ("0","2","0","0","0","0","0"),
                        2 => ("0","0","3","0","0","0","0"),
                        3 => ("0","0","0","4","0","0","0"),
                        4 => ("0","0","0","0","0","1","0"),
                        _ => ("0","0","0","0","0","0","1"),
                    }
                };
                
                let data = format!(
                    "DATADICK,V202201,{},{}{},{},ST,{},z,{},yA,{},yB,0,wA,25.0,wAA,{},xB,220,xC,12.6,xE,12.6,xEA,{},xF,37,xFA,{},tA,{},sA,{},rA,0,qA,0,vA,0,uD,0",
                    station_id, dtype, nid, nid, data_time.format("%Y%m%d%H%M%S"),
                    z_v, ya_v, waa_v, xea_v, tfa_v, ta_v, sa_v
                );
                
                let parsed = parse_st_packet(&data);
                let (common_status, specific_status) = classify_device_status(&parsed, dtype);
                
                result.devices.push(DeviceStatusInfo {
                    device_type: dtype.to_string(),
                    device_nid: nid.to_string(),
                    device_name: self.get_device_type_name(dtype),
                    last_data_time: data_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                    is_online,
                    is_fallback,
                    common_status,
                    specific_status,
                });
            }
            
            result.devices.sort_by(|a, b| {
                fn order(info: &DeviceStatusInfo) -> u8 {
                    if info.is_online { 0 }
                    else if info.is_fallback { 1 }
                    else { 2 }
                }
                order(a).cmp(&order(b))
                    .then_with(|| a.device_type.cmp(&b.device_type))
            });
            
            return result;
        }

        let pool = match self.pool.as_ref() {
            Some(p) => p,
            None => return result,
        };

        // For each station, get the latest rows per device within a short window.
        // Use a simple ORDER BY ... LIMIT query (fast with ix_station_receivetime)
        // and deduplicate the latest per (device_type, device_nid) in Rust.
        let sql = "SELECT device_type, device_nid, data_time, `data` \
             FROM data_st \
             WHERE station_num = ? AND receive_time > (NOW() - INTERVAL 5 MINUTE) \
             ORDER BY device_type, device_nid, data_time DESC, receive_time DESC, id DESC \
             LIMIT 100";

        #[derive(sqlx::FromRow)]
        struct StRow {
            device_type: String,
            device_nid: String,
            data_time: chrono::NaiveDateTime,
            data: String,
        }

        let rows: Vec<StRow> = match sqlx::query_as::<_, StRow>(&sql)
            .bind(station_id)
            .fetch_all(pool)
            .await
        {
            Ok(rows) => rows,
            Err(e) => {
                tracing::warn!("查询站点设备状态失败 {}: {}", station_id, e);
                return result;
            }
        };

        let now_minute = chrono::Local::now().naive_local()
            .with_second(0).unwrap()
            .with_nanosecond(0).unwrap();
        let prev_minute = now_minute - chrono::Duration::minutes(1);

        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut devices: Vec<DeviceStatusInfo> = Vec::new();

        for row in rows {
            let key = (row.device_type.clone(), row.device_nid.clone());
            if seen.contains(&key) { continue; }
            seen.insert(key);

            let is_online = row.data_time == now_minute;
            let is_fallback = row.data_time == prev_minute;
            let parsed = parse_st_packet(&row.data);
            let (common_status, specific_status) = classify_device_status(
                &parsed,
                &row.device_type,
            );

            devices.push(DeviceStatusInfo {
                device_type: row.device_type.clone(),
                device_nid: row.device_nid.clone(),
                device_name: self.get_device_type_name(&row.device_type),
                last_data_time: row.data_time.format("%Y-%m-%d %H:%M:%S").to_string(),
                is_online,
                is_fallback,
                common_status,
                specific_status,
            });
        }

        devices.sort_by(|a, b| {
            fn order(info: &DeviceStatusInfo) -> u8 {
                if info.is_online { 0 }
                else if info.is_fallback { 1 }
                else { 2 }
            }
            order(a).cmp(&order(b))
                .then_with(|| a.device_type.cmp(&b.device_type))
        });

        result.devices = devices;
        result
    }
}

fn classify_device_status(
    parsed: &[crate::models::CheckItem],
    device_type: &str,
) -> (Vec<DeviceStatusItem>, Vec<DeviceStatusItem>) {
    let mut common = Vec::new();
    let mut specific = Vec::new();

    for item in parsed {
        let code = item.item.clone();
        let first_char = code.chars().next().unwrap_or(' ');
        let is_common = matches!(first_char, 'z' | 'x' | 'w' | 'v' | 'u' | 't' | 's' | 'r' | 'q');
        let is_specific = matches!(first_char, 'y' | 'a');

        if is_common {
            common.push(DeviceStatusItem {
                code: code.clone(),
                name: get_status_item_name(&code),
                value: item.value.clone(),
                alarm_text: item.alarm.clone(),
                abnormal: item.abnormal,
            });
        } else if is_specific {
            if is_specific_applicable(&code,
                device_type,
            ) {
                specific.push(DeviceStatusItem {
                    code: code.clone(),
                    name: get_status_item_name(&code),
                    value: item.value.clone(),
                    alarm_text: item.alarm.clone(),
                    abnormal: item.abnormal,
                });
            }
        }
    }

    (common, specific)
}

fn is_specific_applicable(code: &str, device_type: &str) -> bool {
    let upper = device_type.to_uppercase();
    let prefix = upper.chars().take(5).collect::<String>();

    // yA, yB, yN are general measurement/auxiliary/power-on status
    if code == "yA" || code == "yB" || code == "yN" ||
       code == "aCF" || code == "aDOOR" {
        return prefix.starts_with("YACID");
    }

    // Tipping bucket / rain gauge specific
    if matches!(code.as_bytes().get(0..2), Some(b"yC") | Some(b"yD") | Some(b"yE") | Some(b"yF") | Some(b"yG") | Some(b"yH")) {
        return prefix.starts_with("YPRES");
    }

    // Pump status, particle sensor
    if code == "yI" || code == "yJ" {
        return prefix.starts_with("YPRES");
    }

    // Camera specific
    if matches!(code.as_bytes().get(0..2), Some(b"yK") | Some(b"yL") | Some(b"yM")) {
        return prefix.starts_with("YCLOD");
    }

    // Beidou tilt
    if code == "aTILT" {
        return prefix.starts_with("YFROS");
    }

    // Evaporation water level / switch
    if code == "aLEVEL" || code == "aSWITCH1" || code == "aSWITCHA" {
        return prefix.starts_with("YEVAP");
    }

    // Acid rain lid
    if code == "aLID" {
        return prefix.starts_with("YACID") || prefix.starts_with("YACID");
    }

    // Unknown prefix: still show to avoid hiding data
    true
}

fn get_status_item_name(code: &str) -> String {
    let s = code.to_lowercase();
    match s.as_str() {
        "z" => "设备自检状态".into(),
        "y" => "测量仪总工作状态".into(),
        "ya" => "测量仪测量部分自检".into(),
        "yb" => "测量仪辅助设施自检".into(),
        "yc" => "翻斗式雨量工作状态".into(),
        "yd" => "雨量筒筒口堵塞监测".into(),
        "ye" => "雨量筒上翻斗状态".into(),
        "yf" => "计数翻斗状态".into(),
        "yg" => "雨量计数翻斗1状态".into(),
        "yg1" => "雨量计数翻斗1状态".into(),
        "yh" => "雨量计数翻斗2状态".into(),
        "yh1" => "雨量计数翻斗2状态".into(),
        "yi" => "泵状态".into(),
        "yj" => "颗粒物数谱传感器状态".into(),
        "yk" => "鱼眼摄像机工作状态".into(),
        "yl" => "普通摄像机1工作状态".into(),
        "ym" => "普通摄像机2工作状态".into(),
        "yn" => "智能电源开启状态".into(),
        "x" => "供电部分自检状态".into(),
        "xa" => "供电类型".into(),
        "xb" => "外接电源电压".into(),
        "xc" => "蓄电池电压".into(),
        "xd" => "设备供电电压".into(),
        "xe" => "主板电压".into(),
        "xea" => "主板电压状态".into(),
        "xf" => "工作电流".into(),
        "xfa" => "工作电流状态".into(),
        "xg" => "加热电源电压".into(),
        "xga" => "加热电源电压状态".into(),
        "xh" => "蓄电池电量".into(),
        "w" => "温度部分自检状态".into(),
        "wa" => "设备/主采主板温度".into(),
        "waa" => "内部电路温度状态".into(),
        "wb" => "探测器温度".into(),
        "wc" => "腔体温度".into(),
        "wca" => "腔体温度状态".into(),
        "wd" => "恒温器温度".into(),
        "we" => "机箱温度".into(),
        "v" => "加热部件自检状态".into(),
        "va" => "设备加热开关".into(),
        "vaa" => "设备加热状态".into(),
        "vb" => "发射器加热开关".into(),
        "vba" => "发射器加热状态".into(),
        "vc" => "接收器加热开关".into(),
        "vca" => "接收器加热状态".into(),
        "vd" => "相机加热开关".into(),
        "vda" => "相机加热状态".into(),
        "ve" => "鱼眼摄像机加热开关".into(),
        "vea" => "鱼眼摄像机加热状态".into(),
        "vf" => "普通摄像机1加热开关".into(),
        "vfa" => "普通摄像机1加热状态".into(),
        "vg" => "普通摄像机2加热开关".into(),
        "vga" => "普通摄像机2加热状态".into(),
        "vh" => "风速加热开关".into(),
        "vha" => "风速加热状态".into(),
        "vi" => "风向加热开关".into(),
        "via" => "风向加热状态".into(),
        "vj" => "降水现象仪加热通道1开关".into(),
        "vja" => "降水现象仪通道1加热状态".into(),
        "vk" => "降水现象仪加热通道2开关".into(),
        "vka" => "降水现象仪通道2加热状态".into(),
        "u" => "通风部件类自检状态".into(),
        "ua" => "设备通风".into(),
        "ub" => "发射器通风状态".into(),
        "uc" => "接收器通风状态".into(),
        "ud" => "通风罩通风速度".into(),
        "uda" => "通风罩通风状态".into(),
        "ue" => "通风罩转速".into(),
        "uea" => "通风罩转速状态".into(),
        "t" => "通信部件自检状态".into(),
        "ta" => "设备到智能集成处理器通信状态".into(),
        "tb" => "总线状态".into(),
        "tc" => "串口通信状态".into(),
        "td" => "网口通信状态".into(),
        "tda" => "鱼眼摄像机网口通信状态".into(),
        "tdb" => "普通摄像机1网口通信状态".into(),
        "tdc" => "普通摄像机2网口通信状态".into(),
        "te" => "卫星通信状态".into(),
        "tf" => "无线通信状态".into(),
        "tfa" => "无线信号强度".into(),
        "tfb" => "无线信号强度状态".into(),
        "tfc" => "无线连接状态".into(),
        "tg" => "光纤通信状态".into(),
        "s" => "污染类自检状态".into(),
        "sa" => "窗口污染情况".into(),
        "sb" => "探测器污染情况".into(),
        "sc" => "相机镜头污染情况".into(),
        "sd" => "鱼眼摄像机镜头污染情况".into(),
        "se" => "普通摄像机1镜头污染情况".into(),
        "sf" => "普通摄像机2镜头污染情况".into(),
        "sg" => "降水现象仪窗口1污染情况".into(),
        "sh" => "降水现象仪窗口2污染情况".into(),
        "r" => "采样数据状态自检".into(),
        "ra" => "当前分钟采样值超上限次数".into(),
        "rb" => "当前分钟采样值超下限次数".into(),
        "rc" => "当前分钟采样值变化率超限次数".into(),
        "q" => "设备输出分钟数据状态自检".into(),
        "qa" => "当前设备输出分钟数据超上限".into(),
        "qb" => "当前设备输出分钟数据超下限".into(),
        "qc" => "当前设备输出分钟数据变化率超错误变化率".into(),
        "qd" => "当前设备输出分钟数据变化率超存疑变化率".into(),
        "qe" => "当前设备输出分钟数据不满足小时最小变化率".into(),
        "acf" => "存储卡状态".into(),
        "adoor" => "机箱门状态".into(),
        "alid" => "酸雨盖状态".into(),
        "alevel" => "称重降水、蒸发水位状态".into(),
        "aswitch1" => "称重降水、蒸发加排水开关状态".into(),
        "aswitcha" => "称重降水、蒸发加排水状态".into(),
        "atilt" => "北斗设备倾斜角".into(),
        _ => format!("状态项({})", code),
    }
}
