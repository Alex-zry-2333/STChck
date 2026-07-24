use serde::{Deserialize, Serialize};
use std::fs;
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Config {
    pub server: ServerConfig,
    pub database: DatabaseConfig,
    pub cloud_database: DatabaseConfig,
    pub monitor: MonitorConfig,
    pub stations: Vec<StationConfig>,
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
            let exe_path = std::env::current_exe().ok()
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
