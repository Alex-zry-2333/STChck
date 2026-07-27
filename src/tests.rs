#[cfg(test)]
mod tests {
    use crate::config::StationConfig;
    use crate::models::StationStatus;
    use crate::monitor::{
        calculate_risk_score, generate_simulated_data, get_alarm, get_station_index, is_kit,
        parse_st_packet, risk_level,
    };

    // === get_alarm tests ===

    #[test]
    fn test_get_alarm_empty() {
        assert_eq!(get_alarm("", "0"), "");
        assert_eq!(get_alarm("aCF", ""), "");
    }

    #[test]
    fn test_get_alarm_storage_card() {
        assert_eq!(get_alarm("aCF", "0"), "存储卡:正常");
        assert_eq!(get_alarm("aCF", "1"), "存储卡:无卡");
        assert_eq!(get_alarm("aCF", "2"), "存储卡:故障");
        assert_eq!(get_alarm("aCF", "9"), "[?aCF=9]");
    }

    #[test]
    fn test_get_alarm_door() {
        assert_eq!(get_alarm("aDOOR", "0"), "机箱门:正常");
        assert_eq!(get_alarm("aDOOR", "1"), "机箱门:异常");
    }

    #[test]
    fn test_get_alarm_single_char() {
        assert_eq!(get_alarm("a", "0"), "其他工作:正常");
        assert_eq!(get_alarm("a", "1"), "其他工作:异常");
        assert_eq!(get_alarm("t", "0"), "通讯状态:正常");
        assert_eq!(get_alarm("t", "1"), "通讯状态:异常");
        assert_eq!(get_alarm("w", "0"), "温度状态:正常");
        assert_eq!(get_alarm("x", "0"), "供电状态:正常");
        assert_eq!(get_alarm("s", "0"), "污染状态:正常");
    }

    #[test]
    fn test_get_alarm_two_char() {
        assert_eq!(get_alarm("yA", "0"), "测量部分自检:正常");
        assert_eq!(get_alarm("yB", "0"), "辅助设备自检:正常");
        assert_eq!(get_alarm("uA", "0"), "设备通风:正常");
        assert_eq!(get_alarm("uB", "0"), "发射器通风:正常");
    }

    #[test]
    fn test_get_alarm_temperature() {
        assert_eq!(get_alarm("wA", "25.5"), "电路板温度:25.5℃");
        assert_eq!(get_alarm("wB", "30.0"), "探测器温度:30.0℃");
    }

    #[test]
    fn test_get_alarm_power() {
        assert_eq!(get_alarm("xA", "1"), "供电类型:1");
        assert_eq!(get_alarm("xB", "220"), "外接电源电压:220伏");
        assert_eq!(get_alarm("xC", "12"), "蓄电池电压:12伏");
    }

    #[test]
    fn test_get_alarm_communication() {
        assert_eq!(get_alarm("tA", "0"), "设备到智能集成处理器通信状态:正常");
        assert_eq!(get_alarm("tB", "0"), "总线状态:正常");
        assert_eq!(get_alarm("tC", "1"), "串口通信状态:故障");
        assert_eq!(get_alarm("tD", "0"), "网口通信状态:正常");
    }

    #[test]
    fn test_get_alarm_pollution() {
        assert_eq!(get_alarm("sA", "0"), "窗口:正常");
        assert_eq!(get_alarm("sB", "1"), "探测器:一般污染");
        assert_eq!(get_alarm("sC", "2"), "镜头:严重污染");
    }

    #[test]
    fn test_get_alarm_unknown() {
        assert_eq!(get_alarm("unknown", "0"), "[?unknown=0]");
        assert_eq!(get_alarm("xyz", "0"), "[?xyz=0]");
    }

    // === is_kit tests ===

    #[test]
    fn test_is_kit_single_char() {
        assert!(is_kit("a"));
        assert!(is_kit("q"));
        assert!(is_kit("r"));
        assert!(is_kit("s"));
        assert!(is_kit("t"));
        assert!(is_kit("u"));
        assert!(is_kit("v"));
        assert!(is_kit("w"));
        assert!(is_kit("x"));
        assert!(is_kit("y"));
        assert!(is_kit("z"));
        assert!(!is_kit("b"));
        assert!(!is_kit("c"));
    }

    #[test]
    fn test_is_kit_two_char() {
        assert!(is_kit("yA"));
        assert!(is_kit("yM"));
        assert!(is_kit("sA"));
        assert!(is_kit("sH"));
        assert!(is_kit("qA"));
        assert!(is_kit("qE"));
        assert!(is_kit("uA"));
        assert!(is_kit("uC"));
        assert!(is_kit("tA"));
        assert!(!is_kit("tD")); // tD is 3-char
        assert!(!is_kit("yN"));
        assert!(!is_kit("zA"));
    }

    #[test]
    fn test_is_kit_three_char() {
        assert!(is_kit("xEA"));
        assert!(is_kit("xFA"));
        assert!(is_kit("wAA"));
        assert!(is_kit("wCA"));
        assert!(is_kit("vAA"));
        assert!(is_kit("vKA"));
        assert!(is_kit("uDA"));
        assert!(is_kit("uEA"));
        assert!(is_kit("tDA"));
        assert!(is_kit("tDC"));
        assert!(is_kit("tFC"));
        assert!(!is_kit("xAB"));
        assert!(!is_kit("wBA"));
    }

    #[test]
    fn test_is_kit_named_alarms() {
        assert!(is_kit("aCF"));
        assert!(is_kit("aDOOR"));
        assert!(is_kit("aLID"));
        assert!(is_kit("aLEVEL"));
        assert!(is_kit("aSWITCHA"));
        assert!(!is_kit("aSWITCH"));
        assert!(!is_kit("unknown"));
    }

    #[test]
    fn test_is_kit_empty() {
        assert!(!is_kit(""));
    }

    // === parse_st_packet tests ===

    #[test]
    fn test_parse_st_packet_empty() {
        let result = parse_st_packet("");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_st_packet_too_short() {
        let result = parse_st_packet("ST,001,001,2024,01,01,00");
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_st_packet_basic() {
        let data = "ST,001,001,2024,01,01,00,wA,25.0,xB,220,tA,0,sA,0,aCF,0,aDOOR,1";
        let result = parse_st_packet(data);

        // wA=25.0 is a kit, value != 0 so abnormal
        let wa = result.iter().find(|r| r.item == "wA");
        assert!(wa.is_some());
        let wa = wa.unwrap();
        assert!(wa.abnormal); // 25.0 != 0
        assert!(!wa.alarm.is_empty());

        // tA=0 is a kit, value == 0 so normal
        let ta = result.iter().find(|r| r.item == "tA");
        assert!(ta.is_some());
        let ta = ta.unwrap();
        assert!(!ta.abnormal);
        assert!(ta.alarm.is_empty());

        // aDOOR=1 is abnormal
        let door = result.iter().find(|r| r.item == "aDOOR");
        assert!(door.is_some());
        assert!(door.unwrap().abnormal);
    }

    #[test]
    fn test_parse_st_packet_skip_special_values() {
        // Values N, C, -, / should be skipped
        let data = "ST,001,001,2024,01,01,00,wA,N,xB,C,tA,-,sA,/";
        let result = parse_st_packet(data);
        assert!(result.is_empty());
    }

    #[test]
    fn test_parse_st_packet_non_kit() {
        // Items that are not kits should be skipped
        let data = "ST,001,001,2024,01,01,00,unknown,1,other,2";
        let result = parse_st_packet(data);
        assert!(result.is_empty());
    }

    // === get_station_index tests ===

    #[test]
    fn test_get_station_index_found() {
        let stations = vec![
            StationConfig {
                id: "001".to_string(),
                name: "Station 1".to_string(),
                vendor: "A".to_string(),
            },
            StationConfig {
                id: "002".to_string(),
                name: "Station 2".to_string(),
                vendor: "B".to_string(),
            },
        ];
        let found = get_station_index(&stations, "001");
        assert!(found.is_some());
        assert_eq!(found.unwrap().name, "Station 1");
    }

    #[test]
    fn test_get_station_index_not_found() {
        let stations = vec![StationConfig {
            id: "001".to_string(),
            name: "Station 1".to_string(),
            vendor: "A".to_string(),
        }];
        let found = get_station_index(&stations, "999");
        assert!(found.is_none());
    }

    // === generate_simulated_data tests ===

    #[test]
    fn test_generate_simulated_data() {
        let stations = vec![
            StationConfig {
                id: "001".to_string(),
                name: "Station 1".to_string(),
                vendor: "A".to_string(),
            },
            StationConfig {
                id: "002".to_string(),
                name: "Station 2".to_string(),
                vendor: "B".to_string(),
            },
        ];
        let data = generate_simulated_data(&stations);

        assert_eq!(data.stations.len(), 2);
        assert_eq!(data.summary.total, 2);
        assert!(data.summary.online <= 2);
        assert!(data.last_update.len() > 0);
        assert!(data.error.is_none());
    }

    #[test]
    fn test_generate_simulated_data_empty() {
        let stations: Vec<StationConfig> = vec![];
        let data = generate_simulated_data(&stations);

        assert!(data.stations.is_empty());
        assert_eq!(data.summary.total, 0);
        assert_eq!(data.summary.online, 0);
        assert_eq!(data.summary.alarms, 0);
        assert_eq!(data.summary.avg_arrival_rate, 0.0);
    }

    // === risk score tests ===

    #[test]
    fn test_calculate_risk_score_online_no_alarms() {
        let status = StationStatus {
            id: "001".to_string(),
            name: "Test".to_string(),
            vendor: "A".to_string(),
            province: "Test".to_string(),
            records: 100,
            recent_5min: 5,
            min_time: "2024-01-01 00:00:00".to_string(),
            max_time: "2024-01-01 23:59:59".to_string(),
            devices: 3,
            online: true,
            alarms: vec![],
            alarm_count: 0,
            last_arrival_time: "2024-01-01 23:00:00".to_string(),
            arrival_rate_24h: 95.0,
        };
        let score = calculate_risk_score(&status);
        assert!(score < 0.35); // Should be low risk
    }

    #[test]
    fn test_calculate_risk_score_offline() {
        let status = StationStatus {
            id: "001".to_string(),
            name: "Test".to_string(),
            vendor: "A".to_string(),
            province: "Test".to_string(),
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
        };
        let score = calculate_risk_score(&status);
        assert!(score >= 0.35); // Should be at least medium risk
    }

    #[test]
    fn test_calculate_risk_score_many_alarms() {
        let status = StationStatus {
            id: "001".to_string(),
            name: "Test".to_string(),
            vendor: "A".to_string(),
            province: "Test".to_string(),
            records: 100,
            recent_5min: 5,
            min_time: "2024-01-01 00:00:00".to_string(),
            max_time: "2024-01-01 23:59:59".to_string(),
            devices: 3,
            online: true,
            alarms: vec![
                "alarm1".to_string(),
                "alarm2".to_string(),
                "alarm3".to_string(),
                "alarm4".to_string(),
            ],
            alarm_count: 4,
            last_arrival_time: "2024-01-01 23:00:00".to_string(),
            arrival_rate_24h: 80.0,
        };
        let score = calculate_risk_score(&status);
        assert!(score >= 0.65); // Should be high risk
    }

    #[test]
    fn test_risk_level() {
        assert_eq!(risk_level(0.0), "低");
        assert_eq!(risk_level(0.34), "低");
        assert_eq!(risk_level(0.35), "中");
        assert_eq!(risk_level(0.64), "中");
        assert_eq!(risk_level(0.65), "高");
        assert_eq!(risk_level(1.0), "高");
    }
}
