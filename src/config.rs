use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

fn default_data_source() -> String {
    "mysql".to_string()
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub cloud_database: DatabaseConfig,
    /// 可选的 Doris 数据源配置；仅在 monitor.data_source = "doris" 且非模拟模式时生效
    #[serde(default)]
    pub doris: Option<DorisConfig>,
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    pub stations: Vec<StationConfig>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct AuthConfig {
    pub enabled: bool,
    pub username: String,
    pub password: String,
    pub allowed_origins: Vec<String>,
    pub rate_limit_per_second: u64,
}

impl Default for AuthConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            username: "admin".to_string(),
            password: "admin".to_string(),
            allowed_origins: Vec::new(),
            rate_limit_per_second: 20,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ServerConfig {
    pub port: u32,
    pub refresh_interval_secs: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DatabaseConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub db: String,
}

fn default_st_table() -> String {
    "ods_data_st".to_string()
}

fn default_station_table() -> String {
    "ai_isos.station_info".to_string()
}

/// Doris 数据源配置（FE MySQL 协议查询端口默认 9030）
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DorisConfig {
    pub host: String,
    pub port: u16,
    pub user: String,
    pub password: String,
    pub db: String,
    /// ST 明细表名（支持 db.table 全限定名）
    #[serde(default = "default_st_table")]
    pub st_table: String,
    /// 台站信息表名（支持 db.table 全限定名）
    #[serde(default = "default_station_table")]
    pub station_table: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub check_interval_minutes: u32,
    pub simulation_mode: bool,
    /// 真实模式下的数据源：mysql（默认）或 doris
    #[serde(default = "default_data_source")]
    pub data_source: String,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StationConfig {
    pub id: String,
    pub name: String,
    pub vendor: String,
}

impl Config {
    pub fn load(path: &str) -> Self {
        let path = Path::new(path);
        let content = if path.exists() {
            fs::read_to_string(path).expect("Failed to read config.toml")
        } else {
            // Try next to the binary
            let exe_path = std::env::current_exe()
                .ok()
                .and_then(|p| p.parent().map(|d| d.join("config.toml")))
                .filter(|p| p.exists());
            if let Some(p) = exe_path {
                fs::read_to_string(p).expect("Failed to read config.toml")
            } else {
                panic!("config.toml not found");
            }
        };
        toml::from_str(&content).expect("Failed to parse config.toml")
    }
}
