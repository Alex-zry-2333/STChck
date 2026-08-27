pub mod client;
pub mod models;

use crate::cma::client::{CmaClient, CmaClientError};
use crate::cma::models::{
    CmaAlert, CmaSurfaceData, DataCompareResult, ElementCompare, SurfaceHistoryPoint,
    SurfaceOverview,
};
use crate::config::{CmaConfig, CmaElementConfig, StationConfig};
use crate::db::DbService;
use chrono::Local;
use std::collections::HashMap;
use std::sync::Arc;

/// CMA 数据服务：封装后台拉取、缓存读写、数据质量分析
#[derive(Clone)]
pub struct CmaService {
    client: Option<CmaClient>,
    config: CmaConfig,
    stations: Vec<StationConfig>,
}

impl CmaService {
    pub fn new(config: &CmaConfig, stations: &[StationConfig]) -> Self {
        let client = if config.enabled {
            Some(CmaClient::new(
                config.api_user_id.clone(),
                config.api_password.clone(),
            ))
        } else {
            None
        };
        Self {
            client,
            config: config.clone(),
            stations: stations.to_vec(),
        }
    }

    /// 后台定时拉取任务入口
    pub async fn run_refresh_task(&self, db: Arc<DbService>) {
        let client = match self.client.as_ref() {
            Some(c) => c,
            None => {
                tracing::info!("CMA 功能未启用，跳过后台拉取任务");
                return;
            }
        };

        let station_ids: Vec<String> = self.stations.iter().map(|s| s.id.clone()).collect();
        if station_ids.is_empty() {
            return;
        }

        // 拉取最近 3 小时数据（覆盖缓存刷新窗口）
        match client.fetch_recent_surface_data(&station_ids, 3).await {
            Ok(data) => {
                tracing::info!("CMA 数据拉取成功: {} 条记录", data.len());
                if let Err(e) = db.save_cma_surface_data(&data).await {
                    tracing::warn!("CMA 数据缓存写入失败: {}", e);
                } else {
                    tracing::info!("CMA 数据已缓存到本地 SQLite");
                }
            }
            Err(e) => {
                tracing::warn!("CMA 数据拉取失败: {}", e);
            }
        }
    }

    /// 获取单站点最新实况数据（从缓存读）
    pub async fn get_surface_overview(
        &self,
        db: &DbService,
        station_id: &str,
    ) -> Option<SurfaceOverview> {
        let records = db.query_cma_surface_latest(station_id, 1).await.ok()?;
        let latest = records.into_iter().next()?;
        let station_name = self
            .stations
            .iter()
            .find(|s| s.id == station_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        Some(SurfaceOverview {
            station_id: station_id.to_string(),
            station_name,
            data_time: latest
                .datetime()
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default(),
            tem: latest.tem_f64(),
            tem_unit: "℃".to_string(),
            prs: latest.prs_f64(),
            prs_unit: "hPa".to_string(),
            rhu: latest.rhu_f64(),
            rhu_unit: "%".to_string(),
            pre_1h: latest.pre_1h_f64(),
            pre_unit: "mm".to_string(),
            win_s: latest.win_s_f64(),
            win_s_unit: "m/s".to_string(),
            win_d: latest.win_d_f64(),
            win_d_unit: "°".to_string(),
            vis: latest.vis_f64(),
            vis_unit: "m".to_string(),
        })
    }

    /// 获取单站点历史趋势数据
    pub async fn get_surface_history(
        &self,
        db: &DbService,
        station_id: &str,
        hours: i64,
    ) -> Vec<SurfaceHistoryPoint> {
        let limit = (hours * 2) as i64; // 每小时最多 1 条，留点余量
        let records = match db.query_cma_surface_latest(station_id, limit).await {
            Ok(r) => r,
            Err(_) => return Vec::new(),
        };

        let cutoff = Local::now() - chrono::Duration::hours(hours);
        records
            .into_iter()
            .filter(|r| r.datetime().map(|d| d >= cutoff).unwrap_or(false))
            .map(|r| SurfaceHistoryPoint {
                time: r
                    .datetime()
                    .map(|d| d.format("%m-%d %H:%M").to_string())
                    .unwrap_or_default(),
                tem: r.tem_f64(),
                prs: r.prs_f64(),
                rhu: r.rhu_f64(),
                pre_1h: r.pre_1h_f64(),
                win_s: r.win_s_f64(),
                win_d: r.win_d_f64(),
                vis: r.vis_f64(),
            })
            .collect()
    }

    /// 数据质量对比：本站设备数据 vs CMA 基准数据
    /// 注意：当前 STChck 的设备数据中不包含 TEM/PRS 等气象要素值，
    /// 仅包含设备状态码。因此对比功能需要后续在 data_st 中补充
    /// 实际观测值字段，或从其他来源获取。
    /// 这里先实现框架，对比逻辑在数据可用时自动生效。
    pub async fn compare_data(
        &self,
        _db: &DbService,
        station_id: &str,
    ) -> Option<DataCompareResult> {
        let station_name = self
            .stations
            .iter()
            .find(|s| s.id == station_id)
            .map(|s| s.name.clone())
            .unwrap_or_default();

        // TODO: 当本地数据库中有设备观测的 TEM/PRS 等实际值时，
        // 查询 CMA 同期数据，计算偏差。
        // 当前返回占位结构，提示用户数据尚未接入。
        Some(DataCompareResult {
            station_id: station_id.to_string(),
            station_name,
            data_time: Local::now().format("%Y-%m-%d %H:%M").to_string(),
            comparisons: vec![ElementCompare {
                code: "TEM".to_string(),
                name: "气温".to_string(),
                unit: "℃".to_string(),
                local_value: None,
                cma_value: None,
                deviation: None,
                deviation_pct: None,
                is_abnormal: false,
                threshold: self.deviation_threshold("TEM"),
            }],
        })
    }

    /// 生成气象要素告警（基于 CMA 实况数据与配置阈值）
    pub async fn generate_alerts(&self, db: &DbService) -> Vec<CmaAlert> {
        let mut alerts = Vec::new();
        if !self.config.enabled {
            return alerts;
        }

        for st in &self.stations {
            let records = match db.query_cma_surface_latest(&st.id, 1).await {
                Ok(r) => r,
                Err(_) => continue,
            };
            let Some(data) = records.into_iter().next() else {
                continue;
            };

            let data_time = data
                .datetime()
                .map(|d| d.format("%Y-%m-%d %H:%M").to_string())
                .unwrap_or_default();

            // 逐个要素检查阈值
            self.check_element_alert(
                &data,
                &st.id,
                &st.name,
                &data_time,
                "TEM",
                data.tem_f64(),
                &mut alerts,
            );
            self.check_element_alert(
                &data,
                &st.id,
                &st.name,
                &data_time,
                "PRE_1h",
                data.pre_1h_f64(),
                &mut alerts,
            );
            self.check_element_alert(
                &data,
                &st.id,
                &st.name,
                &data_time,
                "WIN_S_Avg_2mi",
                data.win_s_f64(),
                &mut alerts,
            );
        }

        alerts
    }

    fn check_element_alert(
        &self,
        _data: &CmaSurfaceData,
        station_id: &str,
        station_name: &str,
        data_time: &str,
        code: &str,
        value: Option<f64>,
        alerts: &mut Vec<CmaAlert>,
    ) {
        let Some(value) = value else { return };
        let Some(cfg) = self.config.elements.iter().find(|e| e.code == code) else {
            return;
        };

        if let Some(th_high) = cfg.threshold_high {
            if value > th_high {
                alerts.push(CmaAlert {
                    station_id: station_id.to_string(),
                    station_name: station_name.to_string(),
                    alert_time: data_time.to_string(),
                    element_code: code.to_string(),
                    element_name: cfg.name.clone(),
                    value,
                    threshold: th_high,
                    alert_type: "HIGH".to_string(),
                    message: format!(
                        "{} {} 达到 {:.1}{}，超过阈值 {:.1}{}",
                        station_name, cfg.name, value, cfg.unit, th_high, cfg.unit
                    ),
                });
            }
        }
        if let Some(th_low) = cfg.threshold_low {
            if value < th_low {
                alerts.push(CmaAlert {
                    station_id: station_id.to_string(),
                    station_name: station_name.to_string(),
                    alert_time: data_time.to_string(),
                    element_code: code.to_string(),
                    element_name: cfg.name.clone(),
                    value,
                    threshold: th_low,
                    alert_type: "LOW".to_string(),
                    message: format!(
                        "{} {} 降至 {:.1}{}，低于阈值 {:.1}{}",
                        station_name, cfg.name, value, cfg.unit, th_low, cfg.unit
                    ),
                });
            }
        }
    }

    fn deviation_threshold(&self, code: &str) -> f64 {
        self.config
            .elements
            .iter()
            .find(|e| e.code == code)
            .and_then(|e| e.deviation_threshold)
            .unwrap_or_else(|| self.config.deviation_threshold)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn test_config() -> CmaConfig {
        CmaConfig {
            enabled: true,
            api_user_id: "test".to_string(),
            api_password: "test".to_string(),
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
                    name: "1小时降水".to_string(),
                    unit: "mm".to_string(),
                    threshold_high: Some(50.0),
                    threshold_low: None,
                    deviation_threshold: None,
                },
            ],
        }
    }

    fn test_stations() -> Vec<StationConfig> {
        vec![StationConfig {
            id: "50936".to_string(),
            name: "测试站".to_string(),
            vendor: "TEST".to_string(),
        }]
    }

    #[test]
    fn test_cma_service_new_disabled() {
        let mut cfg = test_config();
        cfg.enabled = false;
        let service = CmaService::new(&cfg, &test_stations());
        assert!(service.client.is_none());
    }

    #[test]
    fn test_cma_service_new_enabled() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        assert!(service.client.is_some());
    }

    #[test]
    fn test_deviation_threshold() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        assert_eq!(service.deviation_threshold("TEM"), 3.0);
        assert_eq!(service.deviation_threshold("UNKNOWN"), 5.0);
    }

    #[test]
    fn test_check_element_alert_high() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        let mut alerts = Vec::new();
        let data = CmaSurfaceData {
            station_id: "50936".to_string(),
            year: "2024".to_string(),
            mon: "8".to_string(),
            day: "27".to_string(),
            hour: "9".to_string(),
            tem: Some("45.0".to_string()),
            prs: None,
            rhu: None,
            pre_1h: None,
            win_s_avg_2mi: None,
            win_d_avg_2mi: None,
            vis: None,
        };
        service.check_element_alert(
            &data,
            "50936",
            "测试站",
            "2024-08-27 09:00",
            "TEM",
            Some(45.0),
            &mut alerts,
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "HIGH");
        assert!(alerts[0].message.contains("超过阈值"));
    }

    #[test]
    fn test_check_element_alert_low() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        let mut alerts = Vec::new();
        let data = CmaSurfaceData {
            station_id: "50936".to_string(),
            year: "2024".to_string(),
            mon: "8".to_string(),
            day: "27".to_string(),
            hour: "9".to_string(),
            tem: Some("-25.0".to_string()),
            prs: None,
            rhu: None,
            pre_1h: None,
            win_s_avg_2mi: None,
            win_d_avg_2mi: None,
            vis: None,
        };
        service.check_element_alert(
            &data,
            "50936",
            "测试站",
            "2024-08-27 09:00",
            "TEM",
            Some(-25.0),
            &mut alerts,
        );
        assert_eq!(alerts.len(), 1);
        assert_eq!(alerts[0].alert_type, "LOW");
        assert!(alerts[0].message.contains("低于阈值"));
    }

    #[test]
    fn test_check_element_alert_normal() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        let mut alerts = Vec::new();
        service.check_element_alert(
            &CmaSurfaceData {
                station_id: "50936".to_string(),
                year: "2024".to_string(),
                mon: "8".to_string(),
                day: "27".to_string(),
                hour: "9".to_string(),
                tem: None,
                prs: None,
                rhu: None,
                pre_1h: None,
                win_s_avg_2mi: None,
                win_d_avg_2mi: None,
                vis: None,
            },
            "50936",
            "测试站",
            "2024-08-27 09:00",
            "TEM",
            Some(25.0),
            &mut alerts,
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_check_element_alert_no_value() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        let mut alerts = Vec::new();
        service.check_element_alert(
            &CmaSurfaceData {
                station_id: "50936".to_string(),
                year: "2024".to_string(),
                mon: "8".to_string(),
                day: "27".to_string(),
                hour: "9".to_string(),
                tem: None,
                prs: None,
                rhu: None,
                pre_1h: None,
                win_s_avg_2mi: None,
                win_d_avg_2mi: None,
                vis: None,
            },
            "50936",
            "测试站",
            "2024-08-27 09:00",
            "TEM",
            None,
            &mut alerts,
        );
        assert!(alerts.is_empty());
    }

    #[test]
    fn test_check_element_alert_unknown_element() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        let mut alerts = Vec::new();
        service.check_element_alert(
            &CmaSurfaceData {
                station_id: "50936".to_string(),
                year: "2024".to_string(),
                mon: "8".to_string(),
                day: "27".to_string(),
                hour: "9".to_string(),
                tem: None,
                prs: None,
                rhu: None,
                pre_1h: None,
                win_s_avg_2mi: None,
                win_d_avg_2mi: None,
                vis: None,
            },
            "50936",
            "测试站",
            "2024-08-27 09:00",
            "UNKNOWN",
            Some(100.0),
            &mut alerts,
        );
        assert!(alerts.is_empty());
    }

    #[tokio::test]
    async fn test_compare_data_placeholder() {
        let cfg = test_config();
        let service = CmaService::new(&cfg, &test_stations());
        // compare_data 需要 DbService，这里传一个模拟的——
        // 但由于 DbService 结构复杂，我们只验证框架返回了占位结构
        // 实际对比逻辑等 data_st 中有观测值后再完善
        let result = service
            .compare_data(&DbService::new_simulation().await, "50936")
            .await;
        assert!(result.is_some());
        let r = result.unwrap();
        assert_eq!(r.station_id, "50936");
        assert_eq!(r.comparisons.len(), 1);
        assert_eq!(r.comparisons[0].code, "TEM");
    }
}
