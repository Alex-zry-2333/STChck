use chrono::{DateTime, Local, NaiveDateTime};
use serde::{Deserialize, Serialize};

/// CMA API 通用响应结构
#[derive(Debug, Clone, Deserialize)]
pub struct CmaApiResponse<T> {
    #[serde(rename = "returnCode")]
    pub return_code: String,
    #[serde(rename = "returnMessage")]
    pub return_message: String,
    #[serde(rename = "rowCount")]
    pub row_count: String,
    #[serde(rename = "colCount")]
    pub col_count: String,
    #[serde(rename = "fieldNames")]
    pub field_names: String,
    #[serde(rename = "fieldUnits")]
    pub field_units: String,
    #[serde(default)]
    pub ds: Vec<T>,
}

/// 地面气象站逐小时观测数据 — CMA 返回的单条记录
#[derive(Debug, Clone, Default, Deserialize, Serialize)]
pub struct CmaSurfaceData {
    #[serde(rename = "Station_Id_C")]
    pub station_id: String,
    #[serde(rename = "Year")]
    pub year: String,
    #[serde(rename = "Mon")]
    pub mon: String,
    #[serde(rename = "Day")]
    pub day: String,
    #[serde(rename = "Hour")]
    pub hour: String,

    // 气温 (℃)
    #[serde(rename = "TEM", default)]
    pub tem: Option<String>,
    // 气压 (hPa)
    #[serde(rename = "PRS", default)]
    pub prs: Option<String>,
    // 相对湿度 (%)
    #[serde(rename = "RHU", default)]
    pub rhu: Option<String>,
    // 1小时降水量 (mm)
    #[serde(rename = "PRE_1h", default)]
    pub pre_1h: Option<String>,
    // 2分钟平均风速 (m/s)
    #[serde(rename = "WIN_S_Avg_2mi", default)]
    pub win_s_avg_2mi: Option<String>,
    // 2分钟平均风向 (角度)
    #[serde(rename = "WIN_D_Avg_2mi", default)]
    pub win_d_avg_2mi: Option<String>,
    // 能见度 (m)
    #[serde(rename = "VIS", default)]
    pub vis: Option<String>,
}

impl CmaSurfaceData {
    /// 将年月日时拼接为 DateTime<Local>
    pub fn datetime(&self) -> Option<DateTime<Local>> {
        let s = format!(
            "{}-{:0>2}-{:0>2} {:0>2}:00:00",
            self.year, self.mon, self.day, self.hour
        );
        NaiveDateTime::parse_from_str(&s, "%Y-%m-%d %H:%M:%S")
            .ok()
            .and_then(|naive| naive.and_local_timezone(Local).single())
    }

    pub fn tem_f64(&self) -> Option<f64> {
        self.tem.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn prs_f64(&self) -> Option<f64> {
        self.prs.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn rhu_f64(&self) -> Option<f64> {
        self.rhu.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn pre_1h_f64(&self) -> Option<f64> {
        self.pre_1h.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn win_s_f64(&self) -> Option<f64> {
        self.win_s_avg_2mi.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn win_d_f64(&self) -> Option<f64> {
        self.win_d_avg_2mi.as_ref().and_then(|v| v.parse().ok())
    }
    pub fn vis_f64(&self) -> Option<f64> {
        self.vis.as_ref().and_then(|v| v.parse().ok())
    }
}

/// 前端展示用的实况数据（单站点最新值聚合）
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceOverview {
    pub station_id: String,
    pub station_name: String,
    pub data_time: String,
    pub tem: Option<f64>,
    pub tem_unit: String,
    pub prs: Option<f64>,
    pub prs_unit: String,
    pub rhu: Option<f64>,
    pub rhu_unit: String,
    pub pre_1h: Option<f64>,
    pub pre_unit: String,
    pub win_s: Option<f64>,
    pub win_s_unit: String,
    pub win_d: Option<f64>,
    pub win_d_unit: String,
    pub vis: Option<f64>,
    pub vis_unit: String,
}

/// 历史趋势数据点
#[derive(Debug, Clone, Serialize)]
pub struct SurfaceHistoryPoint {
    pub time: String,
    pub tem: Option<f64>,
    pub prs: Option<f64>,
    pub rhu: Option<f64>,
    pub pre_1h: Option<f64>,
    pub win_s: Option<f64>,
    pub win_d: Option<f64>,
    pub vis: Option<f64>,
}

/// 本站 vs CMA 数据对比结果
#[derive(Debug, Clone, Serialize)]
pub struct DataCompareResult {
    pub station_id: String,
    pub station_name: String,
    pub data_time: String,
    pub comparisons: Vec<ElementCompare>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ElementCompare {
    pub code: String,
    pub name: String,
    pub unit: String,
    pub local_value: Option<f64>,
    pub cma_value: Option<f64>,
    pub deviation: Option<f64>,     // 绝对偏差
    pub deviation_pct: Option<f64>, // 相对偏差 %
    pub is_abnormal: bool,
    pub threshold: f64,
}

/// 气象要素告警项
#[derive(Debug, Clone, Serialize)]
pub struct CmaAlert {
    pub station_id: String,
    pub station_name: String,
    pub alert_time: String,
    pub element_code: String,
    pub element_name: String,
    pub value: f64,
    pub threshold: f64,
    pub alert_type: String, // "HIGH" | "LOW"
    pub message: String,
}

/// 配置中定义的气象要素
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CmaElementConfig {
    pub code: String,
    pub name: String,
    pub unit: String,
    #[serde(default)]
    pub threshold_high: Option<f64>,
    #[serde(default)]
    pub threshold_low: Option<f64>,
    #[serde(default)]
    pub deviation_threshold: Option<f64>, // 数据质量对比用
}

#[cfg(test)]
mod tests {
    use super::*;
    use chrono::{Datelike, Timelike};

    fn sample_data() -> CmaSurfaceData {
        CmaSurfaceData {
            station_id: "50936".to_string(),
            year: "2024".to_string(),
            mon: "8".to_string(),
            day: "27".to_string(),
            hour: "9".to_string(),
            tem: Some("25.5".to_string()),
            prs: Some("1013.2".to_string()),
            rhu: Some("65".to_string()),
            pre_1h: Some("0.0".to_string()),
            win_s_avg_2mi: Some("3.2".to_string()),
            win_d_avg_2mi: Some("180".to_string()),
            vis: Some("10000".to_string()),
        }
    }

    #[test]
    fn test_datetime_parsing() {
        let data = sample_data();
        let dt = data.datetime().unwrap();
        assert_eq!(dt.year(), 2024);
        assert_eq!(dt.month(), 8);
        assert_eq!(dt.day(), 27);
        assert_eq!(dt.hour(), 9);
    }

    #[test]
    fn test_datetime_invalid() {
        let mut data = sample_data();
        data.year = "not_a_year".to_string();
        assert!(data.datetime().is_none());
    }

    #[test]
    fn test_tem_f64() {
        let data = sample_data();
        assert_eq!(data.tem_f64(), Some(25.5));
    }

    #[test]
    fn test_tem_f64_none() {
        let mut data = sample_data();
        data.tem = None;
        assert_eq!(data.tem_f64(), None);
    }

    #[test]
    fn test_tem_f64_invalid() {
        let mut data = sample_data();
        data.tem = Some("invalid".to_string());
        assert_eq!(data.tem_f64(), None);
    }

    #[test]
    fn test_all_fields_parsing() {
        let data = sample_data();
        assert_eq!(data.prs_f64(), Some(1013.2));
        assert_eq!(data.rhu_f64(), Some(65.0));
        assert_eq!(data.pre_1h_f64(), Some(0.0));
        assert_eq!(data.win_s_f64(), Some(3.2));
        assert_eq!(data.win_d_f64(), Some(180.0));
        assert_eq!(data.vis_f64(), Some(10000.0));
    }

    #[test]
    fn test_cma_api_response_deserialization() {
        let json = r#"{
            "returnCode": "0",
            "returnMessage": "success",
            "rowCount": "1",
            "colCount": "11",
            "fieldNames": "Station_Id_C,Year,Mon,Day,Hour,TEM,PRS,RHU,PRE_1h,WIN_S_Avg_2mi,WIN_D_Avg_2mi,VIS",
            "fieldUnits": "-,Year,Mon,Day,Hour,℃,hPa,%,mm,m/s,°,m",
            "ds": [
                {
                    "Station_Id_C": "50936",
                    "Year": "2024",
                    "Mon": "8",
                    "Day": "27",
                    "Hour": "9",
                    "TEM": "25.5",
                    "PRS": "1013.2",
                    "RHU": "65",
                    "PRE_1h": "0.0",
                    "WIN_S_Avg_2mi": "3.2",
                    "WIN_D_Avg_2mi": "180",
                    "VIS": "10000"
                }
            ]
        }"#;
        let resp: CmaApiResponse<CmaSurfaceData> = serde_json::from_str(json).unwrap();
        assert_eq!(resp.return_code, "0");
        assert_eq!(resp.ds.len(), 1);
        assert_eq!(resp.ds[0].station_id, "50936");
        assert_eq!(resp.ds[0].tem_f64(), Some(25.5));
    }

    #[test]
    fn test_cma_api_response_empty_ds() {
        let json = r#"{
            "returnCode": "0",
            "returnMessage": "success",
            "rowCount": "0",
            "colCount": "0",
            "fieldNames": "",
            "fieldUnits": "",
            "ds": []
        }"#;
        let resp: CmaApiResponse<CmaSurfaceData> = serde_json::from_str(json).unwrap();
        assert!(resp.ds.is_empty());
    }

    #[test]
    fn test_cma_surface_data_serialization_roundtrip() {
        let data = sample_data();
        let json = serde_json::to_string(&data).unwrap();
        let decoded: CmaSurfaceData = serde_json::from_str(&json).unwrap();
        assert_eq!(decoded.station_id, data.station_id);
        assert_eq!(decoded.tem, data.tem);
        assert_eq!(decoded.tem_f64(), data.tem_f64());
    }
}
