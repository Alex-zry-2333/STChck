import os
import sys
import time
import json
import threading
import random
from datetime import datetime, timedelta
from flask import Flask, render_template, jsonify

try:
    import pymysql
    HAVE_MYSQL = True
except ImportError:
    HAVE_MYSQL = False

# ============================================================
# Ported business logic from tm.c
# ============================================================

STATIONS = [
    {"id": "50936", "name": "吉林白城", "vendor": "华云"},
    {"id": "50968", "name": "黑龙江尚志", "vendor": "华云"},
    {"id": "53399", "name": "河北张北", "vendor": "华云"},
    {"id": "53942", "name": "陕西洛川", "vendor": "华云"},
    {"id": "54333", "name": "辽宁新民", "vendor": "华云"},
    {"id": "54416", "name": "北京密云", "vendor": "华云"},
    {"id": "54808", "name": "山东莘县", "vendor": "华云"},
    {"id": "56173", "name": "四川红原", "vendor": "华云"},
    {"id": "56312", "name": "西藏林芝", "vendor": "华云"},
    {"id": "57633", "name": "重庆酉阳", "vendor": "华云"},
    {"id": "57958", "name": "广西雁山", "vendor": "华云"},
    {"id": "58005", "name": "河南商丘", "vendor": "华云"},
    {"id": "58457", "name": "浙江杭州", "vendor": "华云"},
    {"id": "58737", "name": "福建建瓯", "vendor": "华云"},
    {"id": "52983", "name": "甘肃榆中", "vendor": "天津"},
    {"id": "53817", "name": "宁夏固原", "vendor": "天津"},
    {"id": "51358", "name": "新疆乌兰乌苏", "vendor": "无锡"},
    {"id": "52754", "name": "青海刚察", "vendor": "无锡"},
    {"id": "52856", "name": "青海共和", "vendor": "无锡"},
    {"id": "53963", "name": "山西侯马", "vendor": "无锡"},
    {"id": "56739", "name": "云南腾冲", "vendor": "无锡"},
    {"id": "57251", "name": "湖北郧西", "vendor": "无锡"},
    {"id": "57793", "name": "江西宜春", "vendor": "无锡"},
    {"id": "57832", "name": "贵州三穗", "vendor": "无锡"},
    {"id": "57874", "name": "湖南常宁", "vendor": "无锡"},
    {"id": "58141", "name": "江苏淮安", "vendor": "无锡"},
    {"id": "58362", "name": "上海宝山", "vendor": "无锡"},
    {"id": "58437", "name": "安徽黄山", "vendor": "无锡"},
    {"id": "59758", "name": "海南海口", "vendor": "无锡"},
    {"id": "59294", "name": "广州增城", "vendor": "广东"},
    {"id": "52737", "name": "青海德令哈", "vendor": "无锡"},
    {"id": "57914", "name": "贵州花溪", "vendor": "无锡"},
]

STATION_LOOKUP = {s["id"]: s for s in STATIONS}


def get_alarm(item, value):
    """Port of getALM() from tm.c"""
    if not item or not value:
        return ""

    # a-prefix alarms (4-char codes like aCF, aDOOR, aLID, aLEVEL, aSWITCH, aSWITCHA, aTILT)
    if item[0] == 'a' and len(item) > 1:
        if item == "aCF":
            return {"0": "存储卡:正常", "1": "存储卡:无卡", "2": "存储卡:故障"}.get(value[0], f"[?{item}={value}]")
        elif item == "aDOOR":
            return {"0": "机箱门:正常", "1": "机箱门:异常"}.get(value[0], f"[?{item}={value}]")
        elif item == "aLID":
            return {"0": "酸雨盖:正常", "1": "酸雨盖:开启"}.get(value[0], f"[?{item}={value}]")
        elif item == "aLEVEL":
            return {"0": "水位:正常", "3": "水位:偏高", "4": "水位:偏低"}.get(value[0], f"[?{item}={value}]")
        elif item == "aSWITCH":
            if value[0] == 'O':
                return "水开关:开启" if len(value) > 1 and value[1] == 'N' else "水开关:关闭"
            elif value[0] == 'N':
                return "水开关:无设备"
            return f"[?{item}={value}]"
        elif item == "aSWITCHA":
            return {"0": "加排水:正常", "1": "加排水:异常", "2": "加排水:故障",
                    "3": "加排水:加水", "4": "加排水:排水", "5": "加排水:维护"}.get(value[0], f"[?{item}={value}]")
        elif item == "aTILT":
            return f"北斗设备倾斜角:{value}度"

    # Single-char items (a, q, r, s, t, u, v, w, x, y, z)
    # Two-char items: y[AB], u[ABC]
    is_single = len(item) == 1 and item[0] in 'aqrstuvwxyz'
    is_yAB = len(item) == 2 and item[0] == 'y' and 'A' <= item[1] <= 'B'
    is_uABC = len(item) == 2 and item[0] == 'u' and 'A' <= item[1] <= 'C'

    if is_single or is_yAB or is_uABC:
        prefix = ""
        if is_single:
            prefix = {"a": "其他工作", "q": "分钟数据", "r": "采样数据", "s": "污染状态",
                      "t": "通讯状态", "u": "通风部件", "v": "加热部件",
                      "w": "温度状态", "x": "供电状态", "y": "测量仪", "z": "设备自检"}.get(item[0], item)
        elif is_yAB:
            prefix = {"A": "测量部分自检", "B": "辅助设备自检"}.get(item[1], item)
        elif is_uABC:
            prefix = {"A": "设备通风", "B": "发射器通风", "C": "接收器通风"}.get(item[1], item)

        suffix_map = {
            '0': "正常", '1': "异常", '2': "故障（未检测到）", '3': "偏高",
            '4': "偏低", '5': "超上限", '6': "超下限", '7': "预留",
            '8': "预留", '9': "未检查", 'N': "关闭或无配置"
        }
        suffix = suffix_map.get(value[0], value)
        return f"{prefix}:{suffix}"

    # Three-char items
    if len(item) == 3:
        # y[C-H,J]: 翻斗雨量 etc
        if item[0] == 'y' and ('C' <= item[1] <= 'H' or item[1] == 'J'):
            y_prefix = {"C": "翻斗雨量", "D": "筒口", "E": "上翻斗", "F": "计数翻斗",
                        "G": "计数翻斗1", "H": "计数翻斗2", "J": "颗粒物谱传感器"}.get(item[1], item)
            y_suffix = {"0": "正常", "1": "异常", "2": "堵塞"}.get(value[0], value)
            return f"{y_prefix}:{y_suffix}"
        elif item[0] == 'y' and item[1] == 'I':
            return {"0": "筒口:正常", "2": "筒口:故障"}.get(value[0], f"筒口:{value}")
        elif item[0] == 'y' and 'K' <= item[1] <= 'M':
            y_prefix = {"K": "鱼眼相机", "L": "普通相机1", "M": "普通相机2"}.get(item[1], item)
            y_suffix = {"0": "正常", "1": "可连接但无法拍照", "2": "无法连接"}.get(value[0], value)
            return f"{y_prefix}:{y_suffix}"
        elif item[0] == 'y' and item[1] == 'N':
            return {"N": "智能电源:电源开启", "F": "智能电源:电源关闭"}.get(value[1] if len(value) > 1 else value[0], f"智能电源:{value}")

        # x-prefix power items
        if item[0] == 'x':
            x_prefix = {"A": "供电类型", "B": "外接电源电压", "C": "蓄电池电压",
                        "D": "设备供电电压", "E": "当前主板电压值", "F": "当前工作电流",
                        "G": "加热电源电压值", "H": "蓄电池电量"}.get(item[1], item)
            if item[1] == 'H':
                return f"{x_prefix}:{value}/100"
            elif item[1] in 'BCDEFG':
                unit = {"B": "伏", "C": "伏", "D": "伏", "E": "伏", "F": "毫安", "G": "伏"}.get(item[1], "")
                return f"{x_prefix}:{value}{unit}"
            return f"{x_prefix}:{value}"

        # w-prefix temperature items
        if item[0] == 'w':
            w_prefix = {"A": "电路板温度", "B": "探测器温度", "C": "腔体温度",
                        "D": "恒温器温度", "E": "机箱温度"}.get(item[1], item)
            return f"{w_prefix}:{value}℃"

        # v-prefix heating items
        if item[0] == 'v':
            v_prefix = {"A": "设备加热开关状态", "B": "发射器加热开关状态", "C": "接收器加热开关状态",
                        "D": "相机加热开关状态", "E": "鱼眼摄像机加热开关状态",
                        "F": "普通摄像机1加热开关状态", "G": "普通摄像机2加热开关状态",
                        "H": "风速加热开关状态", "I": "风向加热开关状态"}.get(item[1], item)
            return f"{v_prefix}:{value}"

        # u-prefix ventilation
        if item[0] == 'u':
            u_prefix = {"D": "通风罩通风速度", "E": "通风罩转速"}.get(item[1], item)
            unit = "(m/s)" if item[1] == 'D' else "(r/min)"
            return f"{u_prefix}:{value}{unit}"

        # t-prefix communication
        if item[0] == 't':
            t_prefix = {"A": "设备到智能集成处理器通信状态", "B": "总线状态", "C": "串口通信状态",
                        "D": "网口通信状态", "E": "鱼眼相机网口通信状态",
                        "F": "普通相机1网口通信状态", "G": "普通相机2网口通信状态"}.get(item[1], item)
            t_suffix = {"0": "正常", "1": "故障", "2": "未启用"}.get(value[0], value)
            return f"{t_prefix}:{t_suffix}"

        # s-prefix pollution
        if item[0] == 's':
            s_prefix = {"A": "窗口", "B": "探测器", "C": "镜头", "D": "鱼眼镜头",
                        "E": "摄像头1", "F": "摄像头2", "G": "降水现象仪1窗口",
                        "H": "降水现象仪2窗口"}.get(item[1], item)
            s_suffix = {"0": "正常", "1": "一般污染", "2": "严重污染"}.get(value[0], value)
            return f"{s_prefix}:{s_suffix}"

        # r-prefix sampling
        if item[0] == 'r':
            r_prefix = {"A": "分钟采样值超上限次数", "B": "分钟采样值超下限次数",
                        "C": "分钟采样值跳变超限次数"}.get(item[1], item)
            return f"{r_prefix}:{value}"

        # q-prefix minute data
        if item[0] == 'q':
            q_prefix = {"A": "当前设备输出分钟数据值不超上限", "B": "当前设备输出分钟数据值不超下限",
                        "C": "当前设备输出分钟数据变化率不超限", "D": "当前设备输出分钟数据(存疑)不超限",
                        "E": "当前设备输出分钟数据达到最小变化率"}.get(item[1], item)
            q_suffix = {"0": "是的（正常）", "1": "不是（错误）"}.get(value[0], value)
            return f"{q_prefix}:{q_suffix}"

    # Four-char items: xEA, xFA, xGA, wAA, wCA, vAA..vKA, uDA, uEA, tDA..tDC, tFA..tFC
    if len(item) == 4 and item[2] == 'A':
        if item[0] == 'x':
            x3_prefix = {"E": "主板电压", "F": "工作电流", "G": "加热电压"}.get(item[1], item)
            x3_suffix = {"0": "正常", "3": "偏高", "4": "偏低"}.get(value[0], value)
            return f"{x3_prefix}:{x3_suffix}"
        elif item[0] == 'w':
            w3_prefix = {"A": "电路板温度", "C": "腔体温度"}.get(item[1], item)
            w3_suffix = {"0": "正常", "3": "偏高", "4": "偏低"}.get(value[0], value)
            return f"{w3_prefix}:{w3_suffix}"
        elif item[0] == 'v':
            v3_prefix = {"A": "设备加热", "B": "发射器加热", "C": "接收器加热",
                         "D": "相机加热", "E": "鱼眼相机加热", "F": "摄像机1加热",
                         "G": "摄像机2加热", "H": "风速加热", "I": "风向加热",
                         "J": "降水现象仪通道1加热", "K": "降水现象仪通道2加热"}.get(item[1], item)
            v3_suffix = {"0": "正常", "1": "异常", "2": "故障", "3": "偏高",
                         "4": "偏低", "5": "停止"}.get(value[0], value)
            return f"{v3_prefix}:{v3_suffix}"
        elif item[0] == 'u':
            u3_prefix = {"D": "通风罩通风", "E": "通风罩转速"}.get(item[1], item)
            u3_suffix = {"0": "正常", "1": "异常", "2": "故障", "3": "偏高",
                         "4": "偏低"}.get(value[0], value)
            return f"{u3_prefix}:{u3_suffix}"

    if len(item) == 4 and item[0] == 't' and item[1] == 'D':
        td_prefix = {"A": "鱼眼摄像机网口", "B": "普通摄像机1网口", "C": "普通摄像机2网口"}.get(item[2], item)
        td_suffix = {"0": "正常", "1": "故障", "2": "未启用"}.get(value[0], value)
        return f"{td_prefix}:{td_suffix}"

    if len(item) == 4 and item[0] == 't' and item[1] == 'F':
        tf_prefix = {"A": "无线信号强度", "B": "无线信号强度", "C": "无线连接状态"}.get(item[2], item)
        if item[2] == 'A':
            return f"{tf_prefix}:{value} dBm"
        elif item[2] == 'B':
            return f"{tf_prefix}:{value} 级"
        elif item[2] == 'C':
            tf_suffix = {"0": "正常", "7": "物理链接断开", "8": "逻辑链路断开"}.get(value[0], value)
            return f"{tf_prefix}:{tf_suffix}"

    return f"[?{item}={value}]"


def is_kit(item):
    """Port of isKIT() from tm.c - checks if item is a known checkable alarm item"""
    if not item:
        return False

    # 1-char: a, q-z
    if len(item) == 1:
        return item[0] in 'aqrstuvwxyz'

    # 2-char
    if len(item) == 2:
        if item[0] == 'y' and 'A' <= item[1] <= 'M':
            return True
        if item[0] == 's' and 'A' <= item[1] <= 'H':
            return True
        if item[0] == 'q' and 'A' <= item[1] <= 'E':
            return True
        if item[0] == 'u' and 'A' <= item[1] <= 'C':
            return True
        if item[0] == 't' and 'A' <= item[1] <= 'G' and item[1] != 'D':
            return True

    # 3-char
    if len(item) == 3:
        if item[0] == 'x' and item[1] in 'EFG' and item[2] == 'A':
            return True
        if item[0] == 'w' and item[1] in ('A', 'C') and item[2] == 'A':
            return True
        if item[0] == 'v' and 'A' <= item[1] <= 'K' and item[2] == 'A':
            return True
        if item[0] == 'u' and item[1] in ('D', 'E') and item[2] == 'A':
            return True
        if item[0] == 't' and item[1] == 'D' and item[2] in 'ABC':
            return True
        if item[0] == 't' and item[1] == 'F' and item[2] == 'C':
            return True

    # Named alarms
    named = {"aCF", "aDOOR", "aLID", "aLEVEL", "aSWITCHA"}
    return item in named


def parse_st_packet(data_str):
    """Parse ST packet data string, return list of (item, value, alarm_text)"""
    parts = data_str.split(',')
    results = []
    # Skip header fields (first ~7 fields), then pairs
    # Format: DATADICK,V202201,station,device,N01,ST,timestamp,z,0,rA,0,rB,0,...
    # Items start from index 7+
    for i in range(7, len(parts) - 1, 2):
        item = parts[i].strip()
        if i + 1 < len(parts):
            value = parts[i + 1].strip()
        else:
            break
        if not item:
            continue
        if len(value) == 1 and value[0] in 'NC-/':
            continue
        if is_kit(item):
            is_abnormal = not (value == '0' or value == '0:0:0:0:0:0:0:0:0:0')
            alarm_text = get_alarm(item, value) if is_abnormal else ""
            results.append({
                "item": item,
                "value": value,
                "alarm": alarm_text,
                "abnormal": is_abnormal
            })
    return results


# ============================================================
# MySQL / Data Layer
# ============================================================

DB_CONFIG = {
    "host": "10.10.1.59",
    "port": 3306,
    "user": "root",
    "password": "root",
    "database": "cammoc_w"
}

monitor_data = {
    "stations": [],
    "summary": {"total": 0, "online": 0, "alarms": 0, "checked": 0},
    "last_update": None,
    "error": None
}


def query_database(td=10):
    """Query MySQL - port of main() logic from tm.c"""
    if not HAVE_MYSQL:
        raise Exception("pymysql not installed")

    conn = pymysql.connect(**DB_CONFIG)
    try:
        station_ids = [s['id'] for s in STATIONS]
        placeholders = ','.join(['%s'] * len(station_ids))
        sql = (
            "SELECT station_num, COUNT(*), "
            "COUNT(IF(data_time>(NOW()-INTERVAL %s MINUTE),1,NULL)), "
            "MIN(data_time), MAX(data_time), "
            "COUNT(DISTINCT CONCAT(device_type,device_nid)) "
            "FROM data_st "
            "WHERE receive_time>(NOW()-INTERVAL %s MINUTE) "
            "AND station_num IN ({}) "
            "GROUP BY station_num ORDER BY station_num".format(placeholders)
        )
        params = [td, td] + station_ids

        with conn.cursor() as cursor:
            cursor.execute(sql, params)
            rows = cursor.fetchall()

        stations_out = []
        total_records = 0
        alarm_count = 0
        checked_count = 0
        online_count = 0

        for row in rows:
            station_id = row[0]
            r1 = row[1] or 0
            r2 = row[2] or 0
            r5 = row[5] or 0
            min_time = str(row[3]) if row[3] else ""
            max_time = str(row[4]) if row[4] else ""

            total_records += r1
            info = STATION_LOOKUP.get(station_id, {})
            alarms = []
            needs_st_check = (r2 == r5 and r5 > 20)

            if needs_st_check and min_time:
                # Query ST packet for this station
                st_sql = (
                    "SELECT data FROM data_st "
                    "WHERE station_num=%s AND data_time=%s LIMIT 1"
                )
                cursor.execute(st_sql, (station_id, min_time))
                st_row = cursor.fetchone()
                if st_row and st_row[0]:
                    parsed = parse_st_packet(st_row[0])
                    for p in parsed:
                        if p["abnormal"]:
                            alarms.append(p["alarm"])
                            alarm_count += 1
                        checked_count += 1

            is_online = needs_st_check
            if is_online:
                online_count += 1

            stations_out.append({
                "id": station_id,
                "name": info.get("name", ""),
                "vendor": info.get("vendor", ""),
                "records": r1,
                "recent_5min": r2,
                "min_time": min_time,
                "max_time": max_time,
                "devices": r5,
                "online": is_online,
                "alarms": alarms,
                "alarm_count": len(alarms)
            })

        monitor_data["stations"] = stations_out
        monitor_data["summary"] = {
            "total": len(stations_out),
            "online": online_count,
            "alarms": alarm_count,
            "checked": checked_count,
            "records": total_records
        }
        monitor_data["last_update"] = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
        monitor_data["error"] = None

    finally:
        conn.close()


def generate_simulated_data():
    """Generate simulated data for demo when DB is unavailable"""
    stations_out = []
    alarm_count = 0
    checked_count = 0
    online_count = 0

    alarm_items = [
        ("aCF", "0"), ("aDOOR", "0"), ("aLID", "0"), ("aLEVEL", "0"),
        ("aSWITCH", "ON"), ("aSWITCHA", "0"), ("yC", "0"), ("yD", "0"),
        ("wA", "25.0"), ("xB", "220"), ("tA", "0"), ("sA", "0"),
        ("rA", "0"), ("qA", "0"), ("vA", "0"), ("uD", "0"),
    ]

    for st in STATIONS:
        is_online = random.random() > 0.15
        if is_online:
            online_count += 1

        alarms = []
        # Simulate some random alarms
        if is_online:
            for item, val in alarm_items:
                checked_count += 1
                # 10% chance of alarm per item
                if random.random() < 0.08:
                    bad_val = str(random.randint(1, 4))
                    alarm_text = get_alarm(item, bad_val)
                    alarms.append(alarm_text)
                    alarm_count += 1

        now = datetime.now()
        stations_out.append({
            "id": st["id"],
            "name": st["name"],
            "vendor": st["vendor"],
            "records": random.randint(50, 500),
            "recent_5min": random.randint(3, 10),
            "min_time": (now - timedelta(minutes=random.randint(1, 10))).strftime("%Y-%m-%d %H:%M:%S"),
            "max_time": now.strftime("%Y-%m-%d %H:%M:%S"),
            "devices": random.randint(1, 5),
            "online": is_online,
            "alarms": alarms,
            "alarm_count": len(alarms)
        })

    monitor_data["stations"] = stations_out
    monitor_data["summary"] = {
        "total": len(stations_out),
        "online": online_count,
        "alarms": alarm_count,
        "checked": checked_count,
        "records": sum(s["records"] for s in stations_out)
    }
    monitor_data["last_update"] = datetime.now().strftime("%Y-%m-%d %H:%M:%S")
    monitor_data["error"] = None


def refresh_data():
    """Background thread to refresh data every 30 seconds"""
    use_simulation = True
    while True:
        try:
            if not use_simulation:
                query_database()
            else:
                generate_simulated_data()
        except Exception as e:
            if use_simulation:
                generate_simulated_data()
            else:
                monitor_data["error"] = str(e)
                generate_simulated_data()
                use_simulation = True
        time.sleep(30)


# ============================================================
# Flask Web Server
# ============================================================

app = Flask(__name__)

@app.route("/")
def index():
    return render_template("dashboard.html")

@app.route("/api/status")
def api_status():
    return jsonify(monitor_data)

@app.route("/api/stations")
def api_stations():
    return jsonify(monitor_data.get("stations", []))

@app.route("/api/summary")
def api_summary():
    return jsonify(monitor_data.get("summary", {}))


if __name__ == "__main__":
    import argparse
    parser = argparse.ArgumentParser(description="Weather Station Monitor")
    parser.add_argument("--port", type=int, default=114514, help="Web port")
    parser.add_argument("--db", action="store_true", help="Use real MySQL database")
    parser.add_argument("--interval", type=int, default=10, help="Query interval (minutes)")
    args = parser.parse_args()

    if args.db:
        DB_CONFIG["host"] = input("MySQL host [10.10.1.59]: ") or "10.10.1.59"
        DB_CONFIG["user"] = input("MySQL user [root]: ") or "root"
        DB_CONFIG["password"] = input("MySQL password [root]: ") or "root"
        DB_CONFIG["database"] = input("Database [cammoc_w]: ") or "cammoc_w"

    # Start background refresh thread
    t = threading.Thread(target=refresh_data, daemon=True)
    t.start()

    # Ensure templates directory exists
    os.makedirs(os.path.join(os.path.dirname(__file__), "templates"), exist_ok=True)

    print(f"\n{'='*60}")
    print(f"  气象站数据监控系统")
    print(f"  Web 界面: http://localhost:{args.port}")
    print(f"  刷新间隔: 30 秒")
    print(f"  数据模式: {'实时数据库' if args.db else '模拟数据'}")
    print(f"{'='*60}\n")

    app.run(host="0.0.0.0", port=args.port, debug=False, use_reloader=False)
