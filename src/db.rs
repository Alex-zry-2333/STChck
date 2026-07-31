use crate::config::{DatabaseConfig, StationConfig};
use crate::models::{
    DeviceStatusInfo, DeviceStatusItem, MonitorData, MonitorSummary, StationDevicesResponse,
    StationMeta, StationStatus,
};
use crate::monitor::parse_st_packet;
use chrono::{Local, Timelike};
use sqlx::mysql::MySqlPoolOptions;
use sqlx::sqlite::SqlitePoolOptions;
use sqlx::MySqlPool;
use std::collections::{HashMap, HashSet};
use std::time::Duration;

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

/// 内置站点校正表：省份 + 近似坐标（城市级）。
/// 生产库 station_params 存在脏数据（例如 52737 被错挂为"华云-重庆酉阳基准站"、
/// 53817 省份错写为"云南"），当数据库行与配置冲突或字段缺失时用本表兜底。
fn builtin_station_meta(id: &str) -> Option<(&'static str, f64, f64)> {
    let m = match id {
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
        _ => return None,
    };
    Some(m)
}

/// 规范化省份名便于比较（去掉 省/市/自治区 等后缀）
fn normalize_province(p: &str) -> String {
    let p = p.trim();
    for suffix in ["壮族自治区", "回族自治区", "维吾尔自治区", "自治区", "省", "市"] {
        if let Some(x) = p.strip_suffix(suffix) {
            return x.to_string();
        }
    }
    p.to_string()
}

/// 判断配置名称与数据库名称是否冲突：
/// 取配置名的全部 2 字子串，数据库名一个都不包含则视为冲突
/// （如 配置"青海德令哈" vs 库"华云-重庆酉阳基准站"）。
fn names_conflict(config_name: &str, db_name: &str) -> bool {
    let chars: Vec<char> = config_name.chars().collect();
    if chars.len() < 2 || db_name.trim().is_empty() {
        return false;
    }
    !chars
        .windows(2)
        .any(|w| db_name.contains(&w.iter().collect::<String>()))
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
                tracing::info!(
                    "MySQL 云库连接成功: {}:{}/{}",
                    cloud_cfg.host,
                    cloud_cfg.port,
                    cloud_cfg.db
                );
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

        let rows: Vec<DeviceTypeRow> =
            match sqlx::query_as::<_, DeviceTypeRow>("SELECT code, name FROM device_type")
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
            self.device_type_names
                .insert(row.code, row.name.unwrap_or_default());
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
                )",
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
                )",
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
                ("YTHWS", "N01"),
                ("YTHWS", "N02"),
                ("YWIND", "N01"),
                ("YPREC", "N01"),
                ("YVISI", "N01"),
                ("YPRSS", "N01"),
                ("YSRAD", "N01"),
                ("YGRND", "N01"),
                ("YCO2", "N01"),
                ("YPOWR", "N01"),
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

            tracing::info!(
                "SQLite 测试数据初始化完成: {} 个站点, {} 台设备",
                stations.len(),
                stations.len() * device_types.len()
            );
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
                        latitude: row
                            .latitude
                            .map(|v| parse_dms_coord(&v.to_string()))
                            .unwrap_or(0.0),
                        longitude: row
                            .longitude
                            .map(|v| parse_dms_coord(&v.to_string()))
                            .unwrap_or(0.0),
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
                for st in station_ids {
                    let meta = builtin_station_meta(&st.id).unwrap_or(("未知省份", 0.0, 0.0));
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
        let rows: Vec<StationParamsRow> = match query.fetch_all(pool).await {
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
            let province = if province.is_empty() {
                "未知省份".to_string()
            } else {
                province.to_string()
            };
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

        // 用配置与内置校正表修正 station_params 脏数据
        for st in station_ids {
            let builtin = builtin_station_meta(&st.id);
            if let Some(meta) = result.get_mut(&st.id) {
                if meta.station_name.is_empty() {
                    meta.station_name = st.name.clone();
                } else if names_conflict(&st.name, &meta.station_name) {
                    // 名称冲突：DB 行属于别的站（错挂），该行坐标/海拔/省份均不可信
                    tracing::warn!(
                        "站点 {} 元数据与配置冲突：配置='{}' 库='{}'，采用配置名称与内置校正坐标",
                        st.id,
                        st.name,
                        meta.station_name
                    );
                    meta.station_name = st.name.clone();
                    meta.altitude = 0.0;
                    if let Some((prov, lat, lon)) = builtin {
                        meta.province = prov.to_string();
                        if lat != 0.0 {
                            meta.latitude = lat;
                        }
                        if lon != 0.0 {
                            meta.longitude = lon;
                        }
                    }
                } else if let Some((prov, lat, lon)) = builtin {
                    // 省份缺失或规范化后与校正表不一致（如 固原被写成 云南）→ 以校正表为准
                    let dbp = normalize_province(&meta.province);
                    if dbp.is_empty() || dbp == "未知省份" || dbp != normalize_province(prov) {
                        meta.province = prov.to_string();
                    }
                    // 坐标完全缺失时用校正表近似坐标
                    if meta.latitude == 0.0 && meta.longitude == 0.0 && lat != 0.0 {
                        meta.latitude = lat;
                        meta.longitude = lon;
                    }
                }
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

        // 单站聚合查询 + 连接池并发执行。
        // 生产表 3400 万行、索引 (station_num, device_type, device_nid, data_time)：
        // IN-list 分组查询需对全部站点做索引扫描（实测 15~44s/次，多段查询累计数分钟）；
        // 单站查询约 1s，配合连接池并发可将总耗时压缩到 10s 级。
        let agg_sql = "SELECT COUNT(*), \
             COUNT(IF(data_time > (NOW() - INTERVAL 6 MINUTE), 1, NULL)), \
             MIN(data_time), MAX(data_time), \
             COUNT(DISTINCT device_type, device_nid) \
             FROM data_st \
             WHERE station_num = ? AND data_time > (NOW() - INTERVAL ? MINUTE)";

        type AggRow = (
            i64,
            Option<i64>,
            Option<chrono::NaiveDateTime>,
            Option<chrono::NaiveDateTime>,
            i64,
        );

        let mut join_set: tokio::task::JoinSet<(String, Result<AggRow, sqlx::Error>)> =
            tokio::task::JoinSet::new();
        for id in &station_ids {
            let pool = pool.clone();
            let id = id.clone();
            join_set.spawn(async move {
                let result = sqlx::query_as::<_, AggRow>(agg_sql)
                    .bind(&id)
                    .bind(check_interval_minutes)
                    .fetch_one(&pool)
                    .await;
                (id, result)
            });
        }

        let mut rows_map: HashMap<String, AggRow> = HashMap::new();
        let mut agg_errors = 0usize;
        while let Some(res) = join_set.join_next().await {
            match res {
                Ok((id, Ok(row))) => {
                    rows_map.insert(id, row);
                }
                Ok((id, Err(e))) => {
                    agg_errors += 1;
                    tracing::warn!("站点 {} 聚合查询失败: {}", id, e);
                }
                Err(e) => {
                    agg_errors += 1;
                    tracing::warn!("聚合查询任务异常: {}", e);
                }
            }
        }
        if !station_ids.is_empty() && agg_errors == station_ids.len() {
            tracing::error!("全部站点聚合查询失败，降级为模拟数据");
            return crate::monitor::generate_simulated_data(stations);
        }

        let mut stations_out = Vec::new();
        let mut total_alarms = 0usize;
        let mut total_checked = 0usize;
        let mut online_count = 0usize;
        let mut total_records = 0i64;

        for (station_id, row) in rows_map {
            let r1 = row.0;
            let r2 = row.1.unwrap_or(0);
            let r5 = row.4;
            let min_time = row
                .2
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            let max_time = row
                .3
                .map(|t| t.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();

            total_records += r1;
            let info = stations.iter().find(|s| s.id == station_id);
            let mut alarms = Vec::new();
            // 高频到报数据（每设备每分钟数百条）下，旧的 r2==r5 判定恒为 false，
            // 会导致真实模式全部站点显示离线；改为最近 6 分钟窗口内有数据即在线
            let is_online = r2 > 0;
            let needs_st_check = is_online;

            if needs_st_check && !min_time.is_empty() {
                // Query ST packet for this station - port of sqlProST
                let st_sql = "SELECT data FROM data_st \
                     WHERE station_num = ? AND data_time = ? \
                     AND data_time > (NOW() - INTERVAL 10 MINUTE) \
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

            // 最后到达时间与到报率均由本次窗口聚合结果推导。
            // 真正的 24h 到报率需要扫描全表（3400 万行），成本过高；
            // 此处到报率为窗口近似值：窗口内有到报记 100%，无到报记 0%。
            let last_arrival = row
                .3
                .map(|dt| dt.format("%Y-%m-%d %H:%M:%S").to_string())
                .unwrap_or_default();
            let arrival_rate = if is_online { 100.0 } else { 0.0 };

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
        tracing::info!(
            "query_monitor_data 完成: {} 个站点, 耗时 {:?}",
            stations_out.len(),
            duration
        );

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

            let now_minute = chrono::Local::now()
                .naive_local()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            let _prev_minute = now_minute - chrono::Duration::minutes(1);

            let mut seen: HashSet<(String, String)> = HashSet::new();
            let mut devices: Vec<DeviceStatusInfo> = Vec::new();

            for row in rows {
                let key = (row.device_type.clone(), row.device_nid.clone());
                if seen.contains(&key) {
                    continue;
                }
                seen.insert(key.clone());

                let parsed = parse_st_packet(&row.data);
                let (common_status, specific_status) =
                    classify_device_status(&parsed, &row.device_type);

                let data_time = row.data_time;
                let is_online = data_time >= now_minute - chrono::Duration::minutes(5);
                let is_fallback =
                    !is_online && data_time >= now_minute - chrono::Duration::minutes(60);

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
                    if info.is_online {
                        0
                    } else if info.is_fallback {
                        1
                    } else {
                        2
                    }
                }
                order(a)
                    .cmp(&order(b))
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
            let now_minute = now
                .naive_local()
                .with_second(0)
                .unwrap()
                .with_nanosecond(0)
                .unwrap();
            let prev_minute = now_minute - chrono::Duration::minutes(1);

            let device_types = [
                ("YTHWS", "N01"),
                ("YTHWS", "N02"),
                ("YWIND", "N01"),
                ("YPREC", "N01"),
                ("YVISI", "N01"),
                ("YPRSS", "N01"),
                ("YSRAD", "N01"),
                ("YGRND", "N01"),
                ("YCO2", "N01"),
                ("YPOWR", "N01"),
            ];

            for (dtype, nid) in &device_types {
                let is_online = rng.gen_range(0.0..1.0) > 0.2;
                let is_fallback = !is_online && rng.gen_range(0.0..1.0) > 0.3;
                let data_time = if is_online {
                    now_minute
                } else if is_fallback {
                    prev_minute
                } else {
                    now_minute - chrono::Duration::minutes(rng.gen_range(5..60))
                };

                // 随机生成一些异常状态，让前端测试异常展示
                let (z_v, ya_v, waa_v, xea_v, tfa_v, ta_v, sa_v) = if rng.gen_range(0.0..1.0) > 0.6
                {
                    ("0", "0", "0", "0", "0", "0", "0")
                } else {
                    let idx = rng.gen_range(0..6);
                    match idx {
                        0 => ("1", "0", "0", "0", "0", "0", "0"),
                        1 => ("0", "2", "0", "0", "0", "0", "0"),
                        2 => ("0", "0", "3", "0", "0", "0", "0"),
                        3 => ("0", "0", "0", "4", "0", "0", "0"),
                        4 => ("0", "0", "0", "0", "0", "1", "0"),
                        _ => ("0", "0", "0", "0", "0", "0", "1"),
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
                    if info.is_online {
                        0
                    } else if info.is_fallback {
                        1
                    } else {
                        2
                    }
                }
                order(a)
                    .cmp(&order(b))
                    .then_with(|| a.device_type.cmp(&b.device_type))
            });

            return result;
        }

        let pool = match self.pool.as_ref() {
            Some(p) => p,
            None => return result,
        };

        // 设备大多每 5 分钟在整五分钟内批量上报一次，仅查“当前分钟”会漏掉绝大多数设备。
        // 窗口放宽到最近 6 分钟（覆盖一个完整上报周期）。
        // 注意：高频设备（如 YACRA00）单分钟可有上百条记录且按字母序排在最前，
        // 简单 LIMIT 会把其他设备全部截断，因此先用子查询按设备取 MAX(data_time)，
        // 再回表取对应记录；同一分钟内的多条由 ORDER BY + Rust 侧去重保留最新接收的一条。
        let sql = "SELECT t.device_type, t.device_nid, t.data_time, t.`data` \
             FROM data_st t \
             INNER JOIN ( \
                 SELECT device_type, device_nid, MAX(data_time) AS max_dt \
                 FROM data_st \
                 WHERE station_num = ? AND data_time > (NOW() - INTERVAL 6 MINUTE) \
                 GROUP BY device_type, device_nid \
             ) m ON t.device_type = m.device_type \
                AND t.device_nid = m.device_nid \
                AND t.data_time = m.max_dt \
             WHERE t.station_num = ? \
             ORDER BY t.device_type, t.device_nid, t.receive_time DESC, t.id DESC";

        #[derive(sqlx::FromRow)]
        struct StRow {
            device_type: String,
            device_nid: String,
            data_time: chrono::NaiveDateTime,
            data: String,
        }

        let rows: Vec<StRow> = match sqlx::query_as::<_, StRow>(&sql)
            .bind(station_id)
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

        let now_minute = chrono::Local::now()
            .naive_local()
            .with_second(0)
            .unwrap()
            .with_nanosecond(0)
            .unwrap();

        let mut seen: HashSet<(String, String)> = HashSet::new();
        let mut devices: Vec<DeviceStatusInfo> = Vec::new();

        for row in rows {
            let key = (row.device_type.clone(), row.device_nid.clone());
            if seen.contains(&key) {
                continue;
            }
            seen.insert(key);

            // 最近 1 分钟内有数据视为在线；窗口内（6 分钟）有数据但稍旧的标记为 fallback
            let is_online = row.data_time >= now_minute - chrono::Duration::minutes(1);
            let is_fallback = !is_online;
            let parsed = parse_st_packet(&row.data);
            let (common_status, specific_status) =
                classify_device_status(&parsed, &row.device_type);

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
                if info.is_online {
                    0
                } else if info.is_fallback {
                    1
                } else {
                    2
                }
            }
            order(a)
                .cmp(&order(b))
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
        let is_common = matches!(
            first_char,
            'z' | 'x' | 'w' | 'v' | 'u' | 't' | 's' | 'r' | 'q'
        );
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
            if is_specific_applicable(&code, device_type) {
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
    if code == "yA" || code == "yB" || code == "yN" || code == "aCF" || code == "aDOOR" {
        return prefix.starts_with("YACID");
    }

    // Tipping bucket / rain gauge specific
    if matches!(
        code.as_bytes().get(0..2),
        Some(b"yC") | Some(b"yD") | Some(b"yE") | Some(b"yF") | Some(b"yG") | Some(b"yH")
    ) {
        return prefix.starts_with("YPRES");
    }

    // Pump status, particle sensor
    if code == "yI" || code == "yJ" {
        return prefix.starts_with("YPRES");
    }

    // Camera specific
    if matches!(
        code.as_bytes().get(0..2),
        Some(b"yK") | Some(b"yL") | Some(b"yM")
    ) {
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
    // 名称统一由 monitor 模块的附录C状态项总表提供（单一数据源）
    crate::monitor::status_item_name(code)
        .map(|s| s.to_string())
        .unwrap_or_else(|| code.to_string())
}
