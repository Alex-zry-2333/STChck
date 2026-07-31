use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationStatus {
    pub id: String,
    pub name: String,
    pub vendor: String,
    pub province: String,
    pub records: i64,
    pub recent_5min: i64,
    pub min_time: String,
    pub max_time: String,
    pub devices: i64,
    pub online: bool,
    pub alarms: Vec<String>,
    pub alarm_count: usize,
    pub last_arrival_time: String,
    pub arrival_rate_24h: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorSummary {
    pub total: usize,
    pub online: usize,
    pub alarms: usize,
    pub checked: usize,
    pub records: i64,
    pub avg_arrival_rate: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorData {
    pub stations: Vec<StationStatus>,
    pub summary: MonitorSummary,
    pub last_update: String,
    pub error: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartAlarmPoint {
    pub time: String,
    pub count: usize,
    pub station_id: String,
    pub station_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartAlarmResponse {
    pub time: Vec<String>,
    pub series: Vec<ChartSeries>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartSeries {
    pub name: String,
    pub data: Vec<usize>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartValuePoint {
    pub time: String,
    pub value: f64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ChartValueResponse {
    pub time: Vec<String>,
    pub values: Vec<f64>,
    pub unit: String,
    pub item_name: String,
    pub station_name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CheckItem {
    pub item: String,
    pub value: String,
    pub alarm: String,
    pub abnormal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationMeta {
    pub station_id: String,
    pub station_name: String,
    pub province: String,
    pub address: String,
    pub latitude: f64,
    pub longitude: f64,
    pub altitude: f32,
    pub station_type: String,
    pub is_acid_rain: bool,
    pub is_reference_radiation: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CategoryStat {
    pub total: usize,
    pub abnormal: usize,
    pub items: Vec<CheckItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationDetail {
    pub meta: StationMeta,
    pub status: StationStatus,
    pub categories: std::collections::HashMap<String, CategoryStat>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RegionStats {
    pub province: String,
    pub total: usize,
    pub online: usize,
    pub offline: usize,
    pub avg_arrival_rate: f64,
    pub alarm_count: usize,
    pub low_rate_count: usize,
    pub offline_stations: Vec<String>,
    pub alarm_stations: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopStation {
    pub id: String,
    pub name: String,
    pub province: String,
    pub value: f64,
    pub label: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TopLists {
    pub lowest_arrival_rate: Vec<TopStation>,
    pub longest_offline: Vec<TopStation>,
    pub most_alarms: Vec<TopStation>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceTypeInfo {
    pub code: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatusItem {
    pub code: String,
    pub name: String,
    pub value: String,
    pub alarm_text: String,
    pub abnormal: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DeviceStatusInfo {
    pub device_type: String,
    pub device_nid: String,
    pub device_name: String,
    pub last_data_time: String,
    pub is_online: bool,
    pub is_fallback: bool,
    /// 原始 ST 数据包（用于追溯状态解析是否正确）
    pub raw_data: String,
    pub common_status: Vec<DeviceStatusItem>,
    pub specific_status: Vec<DeviceStatusItem>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationDevicesResponse {
    pub station_id: String,
    pub station_name: String,
    pub devices: Vec<DeviceStatusInfo>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastOverview {
    pub station_id: String,
    pub station_name: String,
    pub risk_level: String,
    pub risk_score: f64,
    pub summary: String,
    pub highlight: String,
    pub advice: String,
    pub updated_at: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ForecastDetail {
    pub station_id: String,
    pub station_name: String,
    pub risk_level: String,
    pub risk_score: f64,
    pub summary: String,
    pub highlight: String,
    pub predicted_state: String,
    pub risk_factors: Vec<String>,
    pub key_triggers: Vec<String>,
    pub maintenance_advice: Vec<String>,
    pub confidence: String,
    pub generated_at: String,
}
