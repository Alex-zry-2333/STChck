use crate::config::StationConfig;
use crate::models::{
    CheckItem, ForecastDetail, ForecastOverview, MonitorData, MonitorSummary, StationMeta,
    StationStatus,
};
use chrono::{DateTime, Duration, Local};
use rand::Rng;
use std::collections::HashMap;

/// Port of getSid() from tm.c
pub fn get_station_index<'a>(stations: &'a [StationConfig], id: &str) -> Option<&'a StationConfig> {
    stations.iter().find(|s| s.id == id)
}

/// Port of getALM() from tm.c
/// 状态项类别（判定与展示策略），依据《地面气象要素编码与数据格式》附录C
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum StatusKind {
    /// 0=正常，非 0 异常（表C.2 通用取值或分项专用取值）
    Status,
    /// 数值型：温度/电压/电流/信号强度等，仅展示不告警
    Value,
    /// 等级型：如 tFB 无线信号强度 0~4 级，0 级最差，<=1 级判异常
    Level,
}

/// 状态项静态定义（附录C 表C.1/C.2 及各分项取值表）
pub struct StatusItemSpec {
    pub code: &'static str,
    pub name: &'static str,
    pub kind: StatusKind,
    pub unit: &'static str,
    pub texts: &'static [(&'static str, &'static str)],
}

use StatusKind::*;

/// 表C.2 通用状态取值
const GENERIC_STATUS_TEXTS: &[(&str, &str)] = &[
    ("0", "正常"),
    ("1", "异常"),
    ("2", "故障（未检测到）"),
    ("3", "偏高"),
    ("4", "偏低"),
    ("5", "超上限"),
    ("6", "超下限"),
    ("7", "预留"),
    ("8", "预留"),
    ("9", "未检查"),
    ("N", "关闭或无配置"),
];

const RAIN_TEXTS: &[(&str, &str)] = &[("0", "正常"), ("1", "异常"), ("2", "堵塞")];
const CAM_TEXTS: &[(&str, &str)] =
    &[("0", "正常"), ("1", "可连接但无法拍照"), ("2", "故障无法连接")];
const HILO_TEXTS: &[(&str, &str)] = &[("0", "正常"), ("3", "偏高"), ("4", "偏低")];
const HEAT_TEXTS: &[(&str, &str)] = &[
    ("0", "正常"),
    ("1", "加热异常"),
    ("2", "故障"),
    ("3", "加热温度偏高"),
    ("4", "加热温度偏低"),
    ("5", "加热停止"),
];
const COMM_TEXTS: &[(&str, &str)] = &[("0", "正常"), ("1", "故障"), ("2", "未启用")];
const POLLUTION_TEXTS: &[(&str, &str)] =
    &[("0", "正常"), ("1", "一般污染"), ("2", "严重污染")];

macro_rules! spec {
    ($code:expr, $name:expr, Status) => {
        StatusItemSpec { code: $code, name: $name, kind: Status, unit: "", texts: &[] }
    };
    ($code:expr, $name:expr, Status, $texts:expr) => {
        StatusItemSpec { code: $code, name: $name, kind: Status, unit: "", texts: $texts }
    };
    ($code:expr, $name:expr, Status, unit: $unit:expr) => {
        StatusItemSpec { code: $code, name: $name, kind: Status, unit: $unit, texts: &[] }
    };
    ($code:expr, $name:expr, Value, $unit:expr) => {
        StatusItemSpec { code: $code, name: $name, kind: Value, unit: $unit, texts: &[] }
    };
    ($code:expr, $name:expr, Value, $unit:expr, $texts:expr) => {
        StatusItemSpec { code: $code, name: $name, kind: Value, unit: $unit, texts: $texts }
    };
    ($code:expr, $name:expr, Level, $unit:expr) => {
        StatusItemSpec { code: $code, name: $name, kind: Level, unit: $unit, texts: &[] }
    };
}

/// 附录C 状态项总表。
/// 说明：
/// - 报批稿表C.3（y 类）中"辅助设施自检"印为 yC，与翻斗雨量 yC 冲突，
///   疑为笔误；沿用 tm.c 口径 yB 为辅助设施自检。
/// - tE/tF/tG 按报批稿为 卫星/无线/光纤通信状态（tm.c 旧版为摄像机网口，
///   摄像机网口在新版中为 tDA/tDB/tDC）。
/// - v 类开关（vA..vK ON/OFF/N）、yN、aSWITCH 等开关项暂不纳入解析。
static SPECS: &[StatusItemSpec] = &[
    // ---- 单字母类别自检（表C.1）----
    spec!("z", "设备状态自检", Status),
    spec!("y", "测量仪工作状态", Status),
    spec!("x", "供电类状态", Status),
    spec!("w", "工作温度类状态", Status),
    spec!("v", "加热部件类状态", Status),
    spec!("u", "通风部件类状态", Status),
    spec!("t", "通信类状态", Status),
    spec!("s", "污染类状态", Status),
    spec!("r", "采样数据类状态", Status),
    spec!("q", "分钟数据类状态", Status),
    spec!("a", "其他工作类状态", Status),
    // ---- y 类：测量仪工作状态 ----
    spec!("yA", "测量仪测量部分自检状态", Status),
    spec!("yB", "测量仪辅助设施自检状态", Status),
    spec!("yC", "翻斗式雨量工作状态检测", Status, RAIN_TEXTS),
    spec!("yD", "雨量筒筒口堵塞监测", Status, RAIN_TEXTS),
    spec!("yE", "雨量筒上翻斗状态监测", Status, RAIN_TEXTS),
    spec!("yF", "雨量计数翻斗状态监测", Status, RAIN_TEXTS),
    spec!("yG", "雨量计数翻斗1状态监测", Status, RAIN_TEXTS),
    spec!("yH", "雨量计数翻斗2状态监测", Status, RAIN_TEXTS),
    spec!("yG1", "雨量计数翻斗1状态监测", Status, RAIN_TEXTS),
    spec!("yH1", "雨量计数翻斗2状态监测", Status, RAIN_TEXTS),
    spec!("yI", "泵状态", Status, &[("0", "正常"), ("2", "故障")]),
    spec!("yJ", "颗粒物数谱传感器状态", Status, &[("0", "正常"), ("1", "异常")]),
    spec!("yK", "鱼眼摄像机工作状态", Status, CAM_TEXTS),
    spec!("yL", "普通摄像机1工作状态", Status, CAM_TEXTS),
    spec!("yM", "普通摄像机2工作状态", Status, CAM_TEXTS),
    // ---- x 类：供电 ----
    spec!("xA", "供电类型", Value, ""), // AC/DC
    spec!("xB", "外接电源电压", Value, "伏"),
    spec!("xC", "蓄电池电压", Value, "伏"),
    spec!("xD", "设备供电电压", Value, "伏"),
    spec!("xE", "主板电压", Value, "伏"),
    spec!("xEA", "主板电压状态", Status, HILO_TEXTS),
    spec!("xF", "工作电流", Value, "毫安"),
    spec!("xFA", "工作电流状态", Status, HILO_TEXTS),
    spec!("xG", "加热电源电压", Value, "伏"),
    spec!("xGA", "加热电源电压状态", Status, HILO_TEXTS),
    spec!("xH", "蓄电池电量", Value, "/100"),
    // ---- w 类：工作温度 ----
    spec!("wA", "内部电路温度", Value, "℃"),
    spec!("wAA", "内部电路温度状态", Status, HILO_TEXTS),
    spec!("wB", "探测器温度", Value, "℃"),
    spec!("wC", "腔体温度", Value, "℃"),
    spec!("wCA", "腔体温度状态", Status, HILO_TEXTS),
    spec!("wD", "恒温器温度", Value, "℃"),
    spec!("wE", "机箱温度", Value, "℃"),
    // ---- v 类：加热部件状态（开关项 vA..vK 暂不解析）----
    spec!("vAA", "设备加热状态", Status, HEAT_TEXTS),
    spec!("vBA", "发射器加热状态", Status, HEAT_TEXTS),
    spec!("vCA", "接收器加热状态", Status, HEAT_TEXTS),
    spec!("vDA", "相机加热状态", Status, HEAT_TEXTS),
    spec!("vEA", "鱼眼摄像机加热状态", Status, HEAT_TEXTS),
    spec!("vFA", "普通摄像机1加热状态", Status, HEAT_TEXTS),
    spec!("vGA", "普通摄像机2加热状态", Status, HEAT_TEXTS),
    spec!("vHA", "风速加热状态", Status, HEAT_TEXTS),
    spec!("vIA", "风向加热状态", Status, HEAT_TEXTS),
    spec!("vJA", "降水现象仪通道1加热状态", Status, HEAT_TEXTS),
    spec!("vKA", "降水现象仪通道2加热状态", Status, HEAT_TEXTS),
    // ---- u 类：通风部件 ----
    spec!("uA", "设备通风", Status),
    spec!("uB", "发射器通风状态", Status),
    spec!("uC", "接收器通风状态", Status),
    spec!("uD", "通风罩通风速度", Value, "m/s"),
    spec!("uDA", "通风罩通风状态", Status, &[("0", "正常"), ("1", "异常"), ("2", "故障")]),
    spec!("uE", "通风罩转速", Value, "r/min"),
    spec!("uEA", "通风罩转速状态", Status, &[("0", "正常"), ("2", "故障"), ("3", "偏高"), ("4", "偏低")]),
    // ---- t 类：通信 ----
    spec!("tA", "设备到智能集成处理器通信状态", Status, COMM_TEXTS),
    spec!("tB", "总线状态", Status, COMM_TEXTS),
    spec!("tC", "RS232/485/422通信状态", Status, COMM_TEXTS),
    spec!("tD", "RJ45/LAN通信状态", Status, COMM_TEXTS),
    spec!("tDA", "鱼眼摄像机RJ45/LAN通信状态", Status, COMM_TEXTS),
    spec!("tDB", "普通摄像机1 RJ45/LAN通信状态", Status, COMM_TEXTS),
    spec!("tDC", "普通摄像机2 RJ45/LAN通信状态", Status, COMM_TEXTS),
    spec!("tE", "卫星通信状态", Status, COMM_TEXTS),
    spec!("tF", "无线通信状态", Status, COMM_TEXTS),
    spec!("tFA", "无线信号强度", Value, "dBm"),
    spec!("tFB", "无线信号强度状态", Level, "级"),
    spec!("tFC", "无线连接状态", Status, &[("0", "正常"), ("7", "物理链接断开"), ("8", "逻辑链路断开")]),
    spec!("tG", "光纤通信状态", Status, COMM_TEXTS),
    // ---- s 类：污染 ----
    spec!("sA", "窗口污染情况", Status, POLLUTION_TEXTS),
    spec!("sB", "探测器污染情况", Status, POLLUTION_TEXTS),
    spec!("sC", "相机镜头污染情况", Status, POLLUTION_TEXTS),
    spec!("sD", "鱼眼摄像机镜头污染情况", Status, POLLUTION_TEXTS),
    spec!("sE", "普通摄像机1镜头污染情况", Status, POLLUTION_TEXTS),
    spec!("sF", "普通摄像机2镜头污染情况", Status, POLLUTION_TEXTS),
    spec!("sG", "降水现象仪窗口1污染情况", Status, POLLUTION_TEXTS),
    spec!("sH", "降水现象仪窗口2污染情况", Status, POLLUTION_TEXTS),
    // ---- r 类：采样数据（次数，非 0 视为异常提示）----
    spec!("rA", "当前分钟采样值超上限次数", Status, unit: "次"),
    spec!("rB", "当前分钟采样值超下限次数", Status, unit: "次"),
    spec!("rC", "当前分钟采样值变化率超限次数", Status, unit: "次"),
    // ---- q 类：分钟数据 ----
    spec!("qA", "当前设备输出分钟数据超上限", Status, &[("0", "正常"), ("1", "超上限")]),
    spec!("qB", "当前设备输出分钟数据超下限", Status, &[("0", "正常"), ("1", "超下限")]),
    spec!("qC", "当前设备输出分钟数据变化率超错误变化率", Status, &[("0", "正常"), ("1", "超错误变化率")]),
    spec!("qD", "当前设备输出分钟数据变化率超存疑变化率", Status, &[("0", "正常"), ("1", "超存疑变化率")]),
    spec!("qE", "当前设备输出分钟数据不满足小时最小变化率", Status, &[("0", "正常"), ("1", "不满足")]),
    // ---- a 类：其他工作 ----
    spec!("aCF", "存储卡状态", Status, &[("0", "正常"), ("1", "无卡"), ("2", "故障")]),
    spec!("aDOOR", "机箱门状态", Status, &[("0", "正常"), ("1", "异常")]),
    spec!("aLID", "酸雨盖状态", Status, &[("0", "正常"), ("1", "开启")]),
    spec!("aLEVEL", "称重降水、蒸发水位状态", Status, HILO_TEXTS),
    spec!("aSWITCHA", "称重降水、蒸发加排水状态", Status,
        &[("0", "正常"), ("1", "异常"), ("2", "故障"), ("3", "加水"), ("4", "排水"), ("5", "维护")]),
    spec!("aTILT", "北斗设备倾斜角", Value, "°"),
];

/// 按状态编码查附录C定义；未定义的编码返回 None（不解析）
pub fn status_item_spec(code: &str) -> Option<&'static StatusItemSpec> {
    SPECS.iter().find(|s| s.code == code)
}

/// 状态编码对应的标准中文名
pub fn status_item_name(code: &str) -> Option<&'static str> {
    status_item_spec(code).map(|s| s.name)
}

/// 该状态项是否纳入解析（原 tm.c isKIT 的表驱动版本）
pub fn is_kit(item: &str) -> bool {
    status_item_spec(item).is_some()
}

fn lookup_text<'a>(texts: &'a [(&'a str, &'a str)], value: &str) -> Option<&'a str> {
    let key: String = value.chars().take(1).collect();
    texts.iter().find(|(k, _)| *k == key).map(|(_, t)| *t)
}

/// 判定状态项是否异常
pub fn is_abnormal_value(spec: &StatusItemSpec, value: &str) -> bool {
    match spec.kind {
        Status => !(value == "0" || value == "0:0:0:0:0:0:0:0:0:0"),
        // 等级型（tFB）：0 级最差，<=1 级视为信号差告警
        Level => value.trim().parse::<i64>().map(|v| v <= 1).unwrap_or(false),
        Value => false,
    }
}

/// 生成状态项告警/展示文本
pub fn get_alarm(item: &str, value: &str) -> String {
    if item.is_empty() || value.is_empty() {
        return String::new();
    }

    let spec = match status_item_spec(item) {
        Some(s) => s,
        None => return format!("[?{}={}]", item, value),
    };

    match spec.kind {
        Value => format!("{}:{}{}", spec.name, value, spec.unit),
        Level => format!("{}:{}{}", spec.name, value, spec.unit),
        Status => {
            if !spec.texts.is_empty() {
                match lookup_text(spec.texts, value) {
                    Some(t) => format!("{}:{}", spec.name, t),
                    // a 类命名项未取态值时保持原有 [?code=value] 风格
                    None if item.starts_with('a') => format!("[?{}={}]", item, value),
                    None => format!("{}:{}", spec.name, value),
                }
            } else if !spec.unit.is_empty() {
                // 无取值表但带单位（如 r 类次数）：原值+单位展示
                format!("{}:{}{}", spec.name, value, spec.unit)
            } else {
                match lookup_text(GENERIC_STATUS_TEXTS, value) {
                    Some(t) => format!("{}:{}", spec.name, t),
                    None => format!("{}:{}", spec.name, value),
                }
            }
        }
    }
}

/// Parse ST packet - port of the parsing loop from tm.c
pub fn parse_st_packet(data: &str) -> Vec<CheckItem> {
    let parts: Vec<&str> = data.split(',').collect();
    let mut results = Vec::new();
    if parts.len() < 8 {
        return results;
    }

    let mut i = 7;
    while i + 1 < parts.len() {
        let item = parts[i].trim();
        let value = parts[i + 1].trim();
        if item.is_empty() {
            i += 2;
            continue;
        }

        // Skip items with single char values N, C, -, /
        if value.len() == 1 {
            match value.as_bytes()[0] {
                b'N' | b'C' | b'-' | b'/' => {
                    i += 2;
                    continue;
                }
                _ => {}
            }
        }

        if let Some(spec) = status_item_spec(item) {
            // 数值型项目（温度/电压/供电类型等）只展示数值，不参与异常判定；
            // 等级型项目（tFB）按等级阈值判定
            let is_abnormal = is_abnormal_value(spec, value);
            let alarm_text = if is_abnormal {
                get_alarm(item, value)
            } else {
                String::new()
            };
            results.push(CheckItem {
                item: item.to_string(),
                value: value.to_string(),
                alarm: alarm_text,
                abnormal: is_abnormal,
            });
        }
        i += 2;
    }
    results
}

/// Generate simulated monitoring data (for demo)
pub fn generate_simulated_data(stations: &[StationConfig]) -> MonitorData {
    let mut rng = rand::thread_rng();
    let mut stations_out = Vec::new();
    let mut total_alarms = 0usize;
    let mut total_checked = 0usize;
    let mut online_count = 0usize;

    // Standard alarm items to simulate
    let alarm_items: &[(&str, &str)] = &[
        ("aCF", "0"),
        ("aDOOR", "0"),
        ("aLID", "0"),
        ("aLEVEL", "0"),
        ("aSWITCH", "ON"),
        ("aSWITCHA", "0"),
        ("yC", "0"),
        ("yD", "0"),
        ("wA", "25.0"),
        ("xB", "220"),
        ("tA", "0"),
        ("sA", "0"),
        ("rA", "0"),
        ("qA", "0"),
        ("vA", "0"),
        ("uD", "0"),
    ];

    for st in stations {
        let is_online: bool = rng.gen_range(0.0..1.0) > 0.15;
        if is_online {
            online_count += 1;
        }

        let mut alarms = Vec::new();
        if is_online {
            for &(item, _) in alarm_items {
                total_checked += 1;
                if rng.gen_range(0.0..1.0) < 0.06 {
                    let bad_val: i32 = rng.gen_range(1..=4);
                    let bad_val = bad_val.to_string();
                    let alarm = get_alarm(item, &bad_val);
                    alarms.push(alarm);
                    total_alarms += 1;
                }
            }
        }

        let now = Local::now();
        let min_time = (now - Duration::minutes(rng.gen_range(1..10)))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();
        let max_time = now.format("%Y-%m-%d %H:%M:%S").to_string();

        // Simulate last arrival time: some recent, some older (0~120 min ago)
        let arrival_gap_min: i64 = rng.gen_range(0..=120);
        let last_arrival = (now - Duration::minutes(arrival_gap_min))
            .format("%Y-%m-%d %H:%M:%S")
            .to_string();

        // Simulate arrival rate: online stations 85~100%, offline 0~50%
        let arrival_rate = if is_online {
            let base: f64 = rng.gen_range(0.85f64..1.0f64);
            (base * 100.0 * 10.0).round() / 10.0
        } else {
            let base: f64 = rng.gen_range(0.0f64..0.5f64);
            (base * 100.0 * 10.0).round() / 10.0
        };

        stations_out.push(StationStatus {
            id: st.id.clone(),
            name: st.name.clone(),
            vendor: st.vendor.clone(),
            province: String::new(),
            records: rng.gen_range(50i64..500),
            recent_5min: rng.gen_range(3i64..10),
            min_time,
            max_time,
            devices: rng.gen_range(1i64..5),
            online: is_online,
            alarms: alarms.clone(),
            alarm_count: alarms.len(),
            last_arrival_time: last_arrival,
            arrival_rate_24h: arrival_rate,
        });
    }

    let total_records: i64 = stations_out.iter().map(|s| s.records).sum();
    let avg_rate = if !stations_out.is_empty() {
        stations_out.iter().map(|s| s.arrival_rate_24h).sum::<f64>() / stations_out.len() as f64
    } else {
        0.0
    };

    MonitorData {
        summary: MonitorSummary {
            total: stations_out.len(),
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

/// Generate simulated chart data for alarm trends
pub fn generate_alarm_trend(
    stations: &[StationConfig],
    hours: i64,
) -> Vec<(DateTime<Local>, HashMap<String, usize>)> {
    let mut rng = rand::thread_rng();
    let now = Local::now();
    let mut data = Vec::new();

    let mut t = now - Duration::hours(hours);
    while t <= now {
        let mut bucket = HashMap::new();
        for st in stations {
            // Simulate random alarm count per hour per station
            let count: usize = if rng.gen_range(0.0..1.0) > 0.3 {
                rng.gen_range(0..5)
            } else {
                0
            };
            bucket.insert(st.id.clone(), count);
        }
        data.push((t, bucket));
        t = t + Duration::hours(1);
    }
    data
}

/// Generate simulated chart data for a specific item value
pub fn generate_value_trend(
    _stations: &[StationConfig],
    _station_id: &str,
    item: &str,
    hours: i64,
) -> Vec<(DateTime<Local>, f64)> {
    let mut rng = rand::thread_rng();
    let now = Local::now();
    let mut data = Vec::new();

    // Base value depends on item type
    let (base, amp) = match item.chars().next() {
        Some('w') => (25.0, 10.0), // temperature
        Some('x') if item.len() > 1 => {
            match item.as_bytes()[1] {
                b'B' => (220.0, 30.0), // voltage
                b'F' => (100.0, 50.0), // current
                _ => (50.0, 20.0),
            }
        }
        Some('u') => (3.0, 1.5), // wind speed
        _ => (50.0, 25.0),
    };

    let mut t = now - Duration::hours(hours);
    let mut prev = base;
    while t <= now {
        prev += rng.gen_range(-amp * 0.1..amp * 0.1);
        prev = (prev as f64).clamp(base as f64 - amp, base as f64 + amp);
        data.push((t, ((prev * 10.0) as f64).round() / 10.0));
        t = t + Duration::minutes(10);
    }
    data
}

pub fn generate_forecast_overview(stations: &[StationStatus]) -> Vec<ForecastOverview> {
    stations
        .iter()
        .map(|status| {
            let score = calculate_risk_score(status);
            let level = risk_level(score);
            ForecastOverview {
                station_id: status.id.clone(),
                station_name: status.name.clone(),
                risk_level: level.to_string(),
                risk_score: (score * 100.0).round() / 10.0,
                summary: forecast_summary(status, level),
                highlight: forecast_highlight(status, level),
                advice: forecast_advice(status, level),
                updated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
            }
        })
        .collect()
}

pub fn generate_forecast_detail(
    status: &StationStatus,
    meta: Option<&StationMeta>,
) -> ForecastDetail {
    let score = calculate_risk_score(status);
    let level = risk_level(score);
    let mut triggers = Vec::new();
    let mut advice = Vec::new();

    if !status.online {
        triggers.push("站点离线，未收到最近数据".to_string());
        triggers.push("通信链路或电源可能异常".to_string());
        advice.push("优先排查供电与网络链路，确认设备是否恢复在线".to_string());
        advice.push("检查机箱门、蓄电池电压、智能电源状态".to_string());
    } else {
        if status.alarm_count > 0 {
            triggers.push(format!("当前存在 {} 条异常告警", status.alarm_count));
        }
        if status.arrival_rate_24h < 90.0 {
            triggers.push("24小时到达率低于 90%".to_string());
        }
        if status.last_arrival_time.is_empty() {
            triggers.push("最近到达时间记录缺失".to_string());
        }
        if status.alarm_count == 0 && status.arrival_rate_24h >= 90.0 {
            triggers.push("当前运行态势平稳，无明显异常".to_string());
        }
        if status.arrival_rate_24h < 95.0 {
            advice.push("检查数据上报链路和采集模块的稳定性".to_string());
        }
        if status.alarm_count > 1 {
            advice.push("根据告警类型重点巡检通信、供电和温度模块".to_string());
        }
        if status.alarm_count == 0 {
            advice.push("保持当前巡检频次，持续观察告警趋势".to_string());
        }
    }

    if advice.is_empty() {
        advice.push("建议保持日常巡检并关注后续告警变化".to_string());
    }

    let predicted_state = if !status.online {
        "离线风险: 设备可能需要现场检查".to_string()
    } else if status.alarm_count >= 3 {
        "警戒风险: 当前运行状态存在多点异常".to_string()
    } else if status.arrival_rate_24h < 85.0 {
        "关注风险: 数据上报稳定性较差".to_string()
    } else {
        "运行稳定: 继续保持常规巡检".to_string()
    };

    let confidence = if score >= 0.7 {
        "高".to_string()
    } else if score >= 0.35 {
        "中".to_string()
    } else {
        "低".to_string()
    };

    let station_name = match meta {
        Some(m) => format!("{} ({})", status.name, m.station_id),
        None => status.name.clone(),
    };

    let mut risk_factors = Vec::new();
    if status.online {
        risk_factors.push("当前在线: 设备正在上报数据".to_string());
    } else {
        risk_factors.push("当前离线: 需优先排查通信和供电".to_string());
    }
    if status.alarm_count > 0 {
        risk_factors.push(format!("告警数量: {} 条", status.alarm_count));
    } else {
        risk_factors.push("暂无活动告警".to_string());
    }
    risk_factors.push(format!("24h 到达率: {:.1}%", status.arrival_rate_24h));
    if !status.last_arrival_time.is_empty() {
        risk_factors.push(format!("最近到达时间: {}", status.last_arrival_time));
    } else {
        risk_factors.push("最近到达时间未知".to_string());
    }
    if status.arrival_rate_24h < 90.0 {
        risk_factors.push("数据到达率下降，可能存在通信/采集异常".to_string());
    }
    if status.alarm_count >= 3 {
        risk_factors.push("多点告警触发，高风险巡检优先级".to_string());
    }

    ForecastDetail {
        station_id: status.id.clone(),
        station_name,
        risk_level: level.to_string(),
        risk_score: (score * 100.0).round() / 10.0,
        summary: forecast_summary(status, level),
        highlight: forecast_highlight(status, level),
        predicted_state,
        risk_factors,
        key_triggers: triggers,
        maintenance_advice: advice,
        confidence,
        generated_at: Local::now().format("%Y-%m-%d %H:%M:%S").to_string(),
    }
}

pub fn calculate_risk_score(status: &StationStatus) -> f64 {
    let mut score = if status.online { 0.1 } else { 0.7 };
    score += (status.alarm_count as f64) * 0.12;
    if status.arrival_rate_24h < 95.0 {
        score += (95.0 - status.arrival_rate_24h) * 0.015;
    }
    if !status.last_arrival_time.is_empty() {
        if let Ok(last) =
            chrono::NaiveDateTime::parse_from_str(&status.last_arrival_time, "%Y-%m-%d %H:%M:%S")
        {
            let diff = (Local::now().naive_local() - last).num_minutes();
            if diff > 120 {
                score += 0.15;
            }
        }
    }
    score.clamp(0.0, 1.0)
}

pub fn risk_level(score: f64) -> &'static str {
    if score >= 0.65 {
        "高"
    } else if score >= 0.35 {
        "中"
    } else {
        "低"
    }
}

fn forecast_summary(status: &StationStatus, level: &str) -> String {
    if !status.online {
        "设备离线，可能存在通信或供电异常".to_string()
    } else if level == "高" {
        "当前告警集中且数据到达率下降，建议优先现场检查".to_string()
    } else if level == "中" {
        "运行态势需关注，重点检查告警模块与网络稳定性".to_string()
    } else {
        "运行稳定，继续保持常规巡检和数据观察".to_string()
    }
}

fn forecast_highlight(status: &StationStatus, level: &str) -> String {
    if !status.online {
        "离线风险".to_string()
    } else if level == "高" {
        "多点异常告警".to_string()
    } else if status.arrival_rate_24h < 90.0 {
        "到达率下降".to_string()
    } else {
        "运行稳定".to_string()
    }
}

fn forecast_advice(status: &StationStatus, level: &str) -> String {
    if !status.online {
        "优先排查供电与通信链路，及时恢复在线".to_string()
    } else if level == "高" {
        "现场巡检机箱温度、蓄电池电压及通信网口".to_string()
    } else if level == "中" {
        "关注告警趋势，检查传感器与网络稳定性".to_string()
    } else {
        "保持当前巡检频次，持续观察数据变化".to_string()
    }
}
