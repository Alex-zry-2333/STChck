use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub cloud_database: DatabaseConfig,
    pub monitor: MonitorConfig,
    #[serde(default)]
    pub auth: AuthConfig,
    #[serde(default)]
    pub cma: CmaConfig,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MonitorConfig {
    pub check_interval_minutes: u32,
    pub simulation_mode: bool,
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

// ── CMA (中国气象数据网) 配置 ──────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct CmaConfig {
    pub enabled: bool,
    pub api_user_id: String,
    pub api_password: String,
    pub refresh_interval_minutes: u64,
    pub deviation_threshold: f64,
    pub elements: Vec<CmaElementConfig>,
}

impl Default for CmaConfig {
    fn default() -> Self {
        Self {
            enabled: false,
            api_user_id: String::new(),
            api_password: String::new(),
            refresh_interval_minutes: 30,
            deviation_threshold: 5.0,
            elements: vec![
                CmaElementConfig {
                    code: "TEM".to_string(),
                    name: "气温".to_string(),
                    unit: "℃".to_string(),
                    threshold_high: Some(40.0),
                    threshold_low: Some(-20.0),
                    deviation_threshold: Some(3.0),
                },
                CmaElementConfig {
                    code: "PRE_1h".to_string(),
                    name: "1小时降水量".to_string(),
                    unit: "mm".to_string(),
                    threshold_high: Some(50.0),
                    threshold_low: None,
                    deviation_threshold: Some(5.0),
                },
                CmaElementConfig {
                    code: "WIN_S_Avg_2mi".to_string(),
                    name: "2分钟平均风速".to_string(),
                    unit: "m/s".to_string(),
                    threshold_high: Some(24.5), // 10级风
                    threshold_low: None,
                    deviation_threshold: Some(3.0),
                },
                CmaElementConfig {
                    code: "PRS".to_string(),
                    name: "气压".to_string(),
                    unit: "hPa".to_string(),
                    threshold_high: None,
                    threshold_low: None,
                    deviation_threshold: Some(5.0),
                },
                CmaElementConfig {
                    code: "RHU".to_string(),
                    name: "相对湿度".to_string(),
                    unit: "%".to_string(),
                    threshold_high: None,
                    threshold_low: None,
                    deviation_threshold: Some(10.0),
                },
            ],
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmaElementConfig {
    pub code: String,
    pub name: String,
    pub unit: String,
    pub threshold_high: Option<f64>,
    pub threshold_low: Option<f64>,
    pub deviation_threshold: Option<f64>,
}
