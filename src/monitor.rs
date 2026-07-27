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
pub fn get_alarm(item: &str, value: &str) -> String {
    if item.is_empty() || value.is_empty() {
        return String::new();
    }

    // a-prefix named alarms (4-5 char codes)
    if item.starts_with('a') && item.len() > 1 {
        return match item {
            "aCF" => match value.chars().next() {
                Some('0') => "存储卡:正常".into(),
                Some('1') => "存储卡:无卡".into(),
                Some('2') => "存储卡:故障".into(),
                _ => format!("[?{}={}]", item, value),
            },
            "aDOOR" => match value.chars().next() {
                Some('0') => "机箱门:正常".into(),
                Some('1') => "机箱门:异常".into(),
                _ => format!("[?{}={}]", item, value),
            },
            "aLID" => match value.chars().next() {
                Some('0') => "酸雨盖:正常".into(),
                Some('1') => "酸雨盖:开启".into(),
                _ => format!("[?{}={}]", item, value),
            },
            "aLEVEL" => match value.chars().next() {
                Some('0') => "水位:正常".into(),
                Some('3') => "水位:偏高".into(),
                Some('4') => "水位:偏低".into(),
                _ => format!("[?{}={}]", item, value),
            },
            "aSWITCH" => {
                let c = value.chars().next().unwrap_or(' ');
                if c == 'O' {
                    if value.len() > 1 && value.as_bytes()[1] == b'N' {
                        "水开关:开启".into()
                    } else {
                        "水开关:关闭".into()
                    }
                } else if c == 'N' {
                    "水开关:无设备".into()
                } else {
                    format!("[?{}={}]", item, value)
                }
            }
            "aSWITCHA" => match value.chars().next() {
                Some('0') => "加排水:正常".into(),
                Some('1') => "加排水:异常".into(),
                Some('2') => "加排水:故障".into(),
                Some('3') => "加排水:加水".into(),
                Some('4') => "加排水:排水".into(),
                Some('5') => "加排水:维护".into(),
                _ => format!("[?{}={}]", item, value),
            },
            "aTILT" => format!("北斗设备倾斜角:{}度", value),
            _ => format!("[?{}={}]", item, value),
        };
    }

    // Single-char items (a, q, r, s, t, u, v, w, x, y, z)
    fn single_prefix(c: char) -> &'static str {
        match c {
            'a' => "其他工作",
            'q' => "分钟数据",
            'r' => "采样数据",
            's' => "污染状态",
            't' => "通讯状态",
            'u' => "通风部件",
            'v' => "加热部件",
            'w' => "温度状态",
            'x' => "供电状态",
            'y' => "测量仪",
            'z' => "设备自检",
            _ => "",
        }
    }

    fn generic_suffix(v: char) -> &'static str {
        match v {
            '0' => "正常",
            '1' => "异常",
            '2' => "故障（未检测到）",
            '3' => "偏高",
            '4' => "偏低",
            '5' => "超上限",
            '6' => "超下限",
            '7' => "预留",
            '8' => "预留",
            '9' => "未检查",
            'N' => "关闭或无配置",
            _ => "",
        }
    }

    let is_single = item.len() == 1
        && matches!(
            item.as_bytes()[0],
            b'a' | b'q' | b'r' | b's' | b't' | b'u' | b'v' | b'w' | b'x' | b'y' | b'z'
        );
    let is_yAB = item.len() == 2
        && item.as_bytes()[0] == b'y'
        && (item.as_bytes()[1] == b'A' || item.as_bytes()[1] == b'B');
    let is_uABC = item.len() == 2
        && item.as_bytes()[0] == b'u'
        && item.as_bytes()[1] >= b'A'
        && item.as_bytes()[1] <= b'C';

    if is_single || is_yAB || is_uABC {
        let prefix = if is_single {
            single_prefix(item.as_bytes()[0] as char).to_string()
        } else if is_yAB {
            match item.as_bytes()[1] {
                b'A' => "测量部分自检",
                b'B' => "辅助设备自检",
                _ => item,
            }
            .to_string()
        } else {
            match item.as_bytes()[1] {
                b'A' => "设备通风",
                b'B' => "发射器通风",
                b'C' => "接收器通风",
                _ => item,
            }
            .to_string()
        };
        let suffix = value.chars().next().map(generic_suffix).unwrap_or("");
        return format!("{}:{}", prefix, suffix);
    }

    // Three-char items
    if item.len() == 3 {
        let bytes = item.as_bytes();
        // y[C-H,J]: tipping bucket etc
        if bytes[0] == b'y' && ((bytes[1] >= b'C' && bytes[1] <= b'H') || bytes[1] == b'J') {
            let p = match bytes[1] {
                b'C' => "翻斗雨量",
                b'D' => "筒口",
                b'E' => "上翻斗",
                b'F' => "计数翻斗",
                b'G' => "计数翻斗1",
                b'H' => "计数翻斗2",
                b'J' => "颗粒物谱传感器",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "正常",
                Some('1') => "异常",
                Some('2') => "堵塞",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }
        if bytes[0] == b'y' && bytes[1] == b'I' {
            return match value.chars().next() {
                Some('0') => "筒口:正常".into(),
                Some('2') => "筒口:故障".into(),
                _ => format!("筒口:{}", value),
            };
        }
        if bytes[0] == b'y' && bytes[1] >= b'K' && bytes[1] <= b'M' {
            let p = match bytes[1] {
                b'K' => "鱼眼相机",
                b'L' => "普通相机1",
                b'M' => "普通相机2",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "正常",
                Some('1') => "可连接但无法拍照",
                Some('2') => "无法连接",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }
        if bytes[0] == b'y' && bytes[1] == b'N' {
            let v2 = value.as_bytes().get(1).copied().unwrap_or(b' ');
            return match v2 {
                b'N' => "智能电源:电源开启".into(),
                b'F' => "智能电源:电源关闭".into(),
                _ => format!("智能电源:{}", value),
            };
        }

        // x-prefix power
        if bytes[0] == b'x' {
            let (p, unit) = match bytes[1] {
                b'A' => ("供电类型", ""),
                b'B' => ("外接电源电压", "伏"),
                b'C' => ("蓄电池电压", "伏"),
                b'D' => ("设备供电电压", "伏"),
                b'E' => ("当前主板电压值", "伏"),
                b'F' => ("当前工作电流", "毫安"),
                b'G' => ("加热电源电压值", "伏"),
                b'H' => ("蓄电池电量", "/100"),
                _ => (item, ""),
            };
            return format!("{}:{}{}", p, value, unit);
        }

        // w-prefix temperature
        if bytes[0] == b'w' {
            let p = match bytes[1] {
                b'A' => "电路板温度",
                b'B' => "探测器温度",
                b'C' => "腔体温度",
                b'D' => "恒温器温度",
                b'E' => "机箱温度",
                _ => item,
            };
            return format!("{}:{}℃", p, value);
        }

        // v-prefix heating
        if bytes[0] == b'v' {
            let p = match bytes[1] {
                b'A' => "设备加热开关状态",
                b'B' => "发射器加热开关状态",
                b'C' => "接收器加热开关状态",
                b'D' => "相机加热开关状态",
                b'E' => "鱼眼摄像机加热开关状态",
                b'F' => "普通摄像机1加热开关状态",
                b'G' => "普通摄像机2加热开关状态",
                b'H' => "风速加热开关状态",
                b'I' => "风向加热开关状态",
                _ => item,
            };
            return format!("{}:{}", p, value);
        }

        // u-prefix ventilation
        if bytes[0] == b'u' {
            let (p, unit) = match bytes[1] {
                b'D' => ("通风罩通风速度", "(m/s)"),
                b'E' => ("通风罩转速", "(r/min)"),
                _ => (item, ""),
            };
            return format!("{}:{}{}", p, value, unit);
        }

        // t-prefix communication
        if bytes[0] == b't' {
            let p = match bytes[1] {
                b'A' => "设备到智能集成处理器通信状态",
                b'B' => "总线状态",
                b'C' => "串口通信状态",
                b'D' => "网口通信状态",
                b'E' => "鱼眼相机网口通信状态",
                b'F' => "普通相机1网口通信状态",
                b'G' => "普通相机2网口通信状态",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "正常",
                Some('1') => "故障",
                Some('2') => "未启用",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }

        // s-prefix pollution
        if bytes[0] == b's' {
            let p = match bytes[1] {
                b'A' => "窗口",
                b'B' => "探测器",
                b'C' => "镜头",
                b'D' => "鱼眼镜头",
                b'E' => "摄像头1",
                b'F' => "摄像头2",
                b'G' => "降水现象仪1窗口",
                b'H' => "降水现象仪2窗口",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "正常",
                Some('1') => "一般污染",
                Some('2') => "严重污染",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }

        // r-prefix sampling
        if bytes[0] == b'r' {
            let p = match bytes[1] {
                b'A' => "分钟采样值超上限次数",
                b'B' => "分钟采样值超下限次数",
                b'C' => "分钟采样值跳变超限次数",
                _ => item,
            };
            return format!("{}:{}", p, value);
        }

        // q-prefix minute data
        if bytes[0] == b'q' {
            let p = match bytes[1] {
                b'A' => "当前设备输出分钟数据值不超上限",
                b'B' => "当前设备输出分钟数据值不超下限",
                b'C' => "当前设备输出分钟数据变化率不超限",
                b'D' => "当前设备输出分钟数据(存疑)不超限",
                b'E' => "当前设备输出分钟数据达到最小变化率",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "是的（正常）",
                Some('1') => "不是（错误）",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }
    }

    // Four-char items: xEA, xFA, xGA, wAA, wCA, vAA..vKA, uDA, uEA, tDA..tDC, tFA..tFC
    if item.len() == 4 {
        let bytes = item.as_bytes();
        if bytes[2] == b'A' {
            if bytes[0] == b'x' {
                let p = match bytes[1] {
                    b'E' => "主板电压",
                    b'F' => "工作电流",
                    b'G' => "加热电压",
                    _ => item,
                };
                let s = match value.chars().next() {
                    Some('0') => "正常",
                    Some('3') => "偏高",
                    Some('4') => "偏低",
                    _ => value,
                };
                return format!("{}:{}", p, s);
            }
            if bytes[0] == b'w' {
                let p = match bytes[1] {
                    b'A' => "电路板温度",
                    b'C' => "腔体温度",
                    _ => item,
                };
                let s = match value.chars().next() {
                    Some('0') => "正常",
                    Some('3') => "偏高",
                    Some('4') => "偏低",
                    _ => value,
                };
                return format!("{}:{}", p, s);
            }
            if bytes[0] == b'v' {
                let p = match bytes[1] {
                    b'A' => "设备加热",
                    b'B' => "发射器加热",
                    b'C' => "接收器加热",
                    b'D' => "相机加热",
                    b'E' => "鱼眼相机加热",
                    b'F' => "摄像机1加热",
                    b'G' => "摄像机2加热",
                    b'H' => "风速加热",
                    b'I' => "风向加热",
                    b'J' => "降水现象仪通道1加热",
                    b'K' => "降水现象仪通道2加热",
                    _ => item,
                };
                let s = match value.chars().next() {
                    Some('0') => "正常",
                    Some('1') => "异常",
                    Some('2') => "故障",
                    Some('3') => "偏高",
                    Some('4') => "偏低",
                    Some('5') => "停止",
                    _ => value,
                };
                return format!("{}:{}", p, s);
            }
            if bytes[0] == b'u' {
                let p = match bytes[1] {
                    b'D' => "通风罩通风",
                    b'E' => "通风罩转速",
                    _ => item,
                };
                let s = match value.chars().next() {
                    Some('0') => "正常",
                    Some('1') => "异常",
                    Some('2') => "故障",
                    Some('3') => "偏高",
                    Some('4') => "偏低",
                    _ => value,
                };
                return format!("{}:{}", p, s);
            }
        }
        // tDA..tDC
        if bytes[0] == b't' && bytes[1] == b'D' {
            let p = match bytes[2] {
                b'A' => "鱼眼摄像机网口",
                b'B' => "普通摄像机1网口",
                b'C' => "普通摄像机2网口",
                _ => item,
            };
            let s = match value.chars().next() {
                Some('0') => "正常",
                Some('1') => "故障",
                Some('2') => "未启用",
                _ => value,
            };
            return format!("{}:{}", p, s);
        }
        // tFA..tFC
        if bytes[0] == b't' && bytes[1] == b'F' {
            let p = match bytes[2] {
                b'A' => "无线信号强度",
                b'B' => "无线信号强度",
                b'C' => "无线连接状态",
                _ => item,
            };
            return match bytes[2] {
                b'A' => format!("{}:{} dBm", p, value),
                b'B' => format!("{}:{} 级", p, value),
                b'C' => {
                    let s = match value.chars().next() {
                        Some('0') => "正常",
                        Some('7') => "物理链接断开",
                        Some('8') => "逻辑链路断开",
                        _ => value,
                    };
                    format!("{}:{}", p, s)
                }
                _ => format!("{}:{}", p, value),
            };
        }
    }

    format!("[?{}={}]", item, value)
}

/// Port of isKIT() from tm.c
pub fn is_kit(item: &str) -> bool {
    if item.is_empty() {
        return false;
    }
    let bytes = item.as_bytes();
    // 1-char: a, q-z
    if item.len() == 1 {
        return matches!(
            bytes[0],
            b'a' | b'q' | b'r' | b's' | b't' | b'u' | b'v' | b'w' | b'x' | b'y' | b'z'
        );
    }
    // 2-char
    if item.len() == 2 {
        if bytes[0] == b'y' && bytes[1] >= b'A' && bytes[1] <= b'M' {
            return true;
        }
        if bytes[0] == b's' && bytes[1] >= b'A' && bytes[1] <= b'H' {
            return true;
        }
        if bytes[0] == b'q' && bytes[1] >= b'A' && bytes[1] <= b'E' {
            return true;
        }
        if bytes[0] == b'u' && bytes[1] >= b'A' && bytes[1] <= b'C' {
            return true;
        }
        if bytes[0] == b't' && bytes[1] >= b'A' && bytes[1] <= b'G' && bytes[1] != b'D' {
            return true;
        }
        return false;
    }
    // 3-char
    if item.len() == 3 {
        if bytes[0] == b'x' && matches!(bytes[1], b'E' | b'F' | b'G') && bytes[2] == b'A' {
            return true;
        }
        if bytes[0] == b'w' && matches!(bytes[1], b'A' | b'C') && bytes[2] == b'A' {
            return true;
        }
        if bytes[0] == b'v' && bytes[1] >= b'A' && bytes[1] <= b'K' && bytes[2] == b'A' {
            return true;
        }
        if bytes[0] == b'u' && matches!(bytes[1], b'D' | b'E') && bytes[2] == b'A' {
            return true;
        }
        if bytes[0] == b't' && bytes[1] == b'D' && bytes[2] >= b'A' && bytes[2] <= b'C' {
            return true;
        }
        if bytes[0] == b't' && bytes[1] == b'F' && bytes[2] == b'C' {
            return true;
        }
        return false;
    }
    // Named alarms
    matches!(item, "aCF" | "aDOOR" | "aLID" | "aLEVEL" | "aSWITCHA")
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

        if is_kit(item) {
            let is_abnormal = !(value == "0" || value == "0:0:0:0:0:0:0:0:0:0");
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
