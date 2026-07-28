# License 硬件绑定技术方案 — MAC 地址绑定深度分析

## 需求确认

将软件使用期限与**特定机器绑定**，防止 license 文件被复制到其他设备上运行。

---

## 方案对比：MAC 地址 vs 其他硬件标识

| 硬件标识 | 获取难度 | 稳定性 | 可伪造性 | 推荐度 |
|----------|----------|--------|----------|--------|
| **MAC 地址** | ⭐⭐ 简单 | ⚠️ 可变更 | ⚠️ 中等 | ⭐⭐⭐ |
| **CPU ID** | ⭐⭐⭐ 较复杂 | ✅ 稳定 | ⭐ 难伪造 | ⭐⭐⭐⭐ |
| **磁盘序列号** | ⭐⭐ 较简单 | ⚠️ 换盘会变 | ⚠️ 中等 | ⭐⭐⭐ |
| **主板 UUID** | ⭐⭐⭐ 较复杂 | ✅ 稳定 | ⭐ 难伪造 | ⭐⭐⭐⭐⭐ |
| **组合指纹** | ⭐⭐⭐⭐ 复杂 | ✅ 稳定 | ⭐ 难伪造 | ⭐⭐⭐⭐⭐ |

**推荐：主板 UUID + MAC 地址组合**，单一标识容易被绕过，组合指纹更可靠。

---

## 技术实现方案

### 方案一：MAC 地址绑定（基础版）

```rust
// src/hardware_id.rs

use std::process::Command;

/// 获取本机所有 MAC 地址
#[cfg(target_os = "linux")]
pub fn get_mac_addresses() -> Vec<String> {
    let output = Command::new("cat")
        .arg("/sys/class/net/")
        .output()
        .expect("Failed to read network interfaces");
    
    // 或者读取 /sys/class/net/eth0/address
    // 过滤掉虚拟网卡（docker0, lo, veth*）
}

#[cfg(target_os = "windows")]
pub fn get_mac_addresses() -> Vec<String> {
    let output = Command::new("wmic")
        .args(&["nic", "get", "MACAddress", "/value"])
        .output()
        .expect("Failed to get MAC addresses");
    // 解析输出
}

#[cfg(target_os = "macos")]
pub fn get_mac_addresses() -> Vec<String> {
    let output = Command::new("ifconfig")
        .output()
        .expect("Failed to get MAC addresses");
    // 解析 en0 等物理网卡
}
```

**问题：**
1. MAC 地址可以通过软件轻松修改（`ifconfig` / 注册表）
2. 虚拟机中 MAC 是虚拟的，可随意指定
3. Docker 容器有自己的 MAC 地址

---

### 方案二：系统级硬件指纹（推荐）

利用操作系统提供的 DMI/ACPI 信息读取主板 UUID 和序列号。

```rust
// src/hardware_id.rs

use std::fs;
use std::process::Command;

/// 获取硬件指纹（跨平台）
pub struct HardwareFingerprint {
    pub board_uuid: String,
    pub board_serial: String,
    pub cpu_id: String,
    pub primary_mac: String,
    pub disk_serial: String,
}

impl HardwareFingerprint {
    /// 生成组合哈希指纹
    pub fn combined_hash(&self) -> String {
        use sha2::{Sha256, Digest};
        let input = format!(
            "{}|{}|{}|{}",
            self.board_uuid,
            self.board_serial,
            self.cpu_id,
            self.primary_mac
        );
        let mut hasher = Sha256::new();
        hasher.update(input.as_bytes());
        format!("{:x}", hasher.finalize())[..16].to_string()
    }
}

// ========== Linux ==========
#[cfg(target_os = "linux")]
pub fn get_hardware_fingerprint() -> Result<HardwareFingerprint, String> {
    // 方法1: 读取 DMI 信息（需要 root 或 dmi-sysfs 权限）
    let board_uuid = fs::read_to_string("/sys/class/dmi/id/board_uuid")
        .map_err(|e| format!("Failed to read board UUID: {}", e))?
        .trim()
        .to_string();
    
    let board_serial = fs::read_to_string("/sys/class/dmi/id/board_serial")
        .unwrap_or_default()
        .trim()
        .to_string();
    
    let cpu_id = get_cpu_id_linux()?;
    let primary_mac = get_primary_mac_linux()?;
    let disk_serial = get_disk_serial_linux()?;
    
    Ok(HardwareFingerprint {
        board_uuid,
        board_serial,
        cpu_id,
        primary_mac,
        disk_serial,
    })
}

#[cfg(target_os = "linux")]
fn get_cpu_id_linux() -> Result<String, String> {
    // 读取 /proc/cpuinfo 中的 serial 或 physical id
    let cpuinfo = fs::read_to_string("/proc/cpuinfo")
        .map_err(|e| format!("Failed to read cpuinfo: {}", e))?;
    
    for line in cpuinfo.lines() {
        if line.starts_with("serial") || line.starts_with("Serial") {
            if let Some(val) = line.split(':').nth(1) {
                return Ok(val.trim().to_string());
            }
        }
    }
    
    // Fallback: 使用 machine id
    fs::read_to_string("/etc/machine-id")
        .map(|s| s.trim().to_string())
        .map_err(|e| format!("Failed to get machine-id: {}", e))
}

#[cfg(target_os = "linux")]
fn get_primary_mac_linux() -> Result<String, String> {
    // 读取 eth0 或第一个非虚拟网卡的 MAC
    let interfaces = fs::read_dir("/sys/class/net")
        .map_err(|e| format!("Failed to list interfaces: {}", e))?;
    
    for entry in interfaces {
        if let Ok(entry) = entry {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            
            // 跳过虚拟接口
            if name_str.starts_with("lo") 
                || name_str.starts_with("docker")
                || name_str.starts_with("veth")
                || name_str.starts_with("br-")
                || name_str.starts_with("virbr") {
                continue;
            }
            
            let mac_path = entry.path().join("address");
            if let Ok(mac) = fs::read_to_string(&mac_path) {
                let mac = mac.trim();
                if mac != "00:00:00:00:00:00" {
                    return Ok(mac.to_string());
                }
            }
        }
    }
    
    Err("No physical network interface found".to_string())
}

#[cfg(target_os = "linux")]
fn get_disk_serial_linux() -> Result<String, String> {
    // 使用 lsblk 或 hdparm 获取磁盘序列号
    let output = Command::new("lsblk")
        .args(&["-ndo", "NAME,SERIAL"])
        .output()
        .map_err(|e| format!("Failed to run lsblk: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        let parts: Vec<&str> = line.split_whitespace().collect();
        if parts.len() >= 2 && parts[0].starts_with("sd") {
            return Ok(parts[1].to_string());
        }
    }
    
    Err("No disk serial found".to_string())
}

// ========== Windows ==========
#[cfg(target_os = "windows")]
pub fn get_hardware_fingerprint() -> Result<HardwareFingerprint, String> {
    // 使用 WMIC 命令获取硬件信息
    let board_uuid = run_wmic("baseboard", "SerialNumber")?;
    let board_serial = run_wmic("baseboard", "SerialNumber")?;
    let cpu_id = run_wmic("cpu", "ProcessorId")?;
    let primary_mac = run_wmic("nic", "MACAddress")?;
    let disk_serial = run_wmic("diskdrive", "SerialNumber")?;
    
    Ok(HardwareFingerprint {
        board_uuid,
        board_serial,
        cpu_id,
        primary_mac,
        disk_serial,
    })
}

#[cfg(target_os = "windows")]
fn run_wmic(class: &str, property: &str) -> Result<String, String> {
    let output = Command::new("wmic")
        .args(&[class, "get", property, "/value"])
        .output()
        .map_err(|e| format!("WMIC failed: {}", e))?;
    
    let stdout = String::from_utf8_lossy(&output.stdout);
    for line in stdout.lines() {
        if let Some(val) = line.split('=').nth(1) {
            let val = val.trim();
            if !val.is_empty() && val != "To be filled by O.E.M." {
                return Ok(val.to_string());
            }
        }
    }
    
    Err(format!("No {}.{} found", class, property))
}

// ========== Fallback ==========
pub fn get_simple_fingerprint() -> String {
    // 最简方案：使用 hostname + machine-id
    let hostname = gethostname::gethostname()
        .to_string_lossy()
        .to_string();
    
    #[cfg(target_os = "linux")]
    let machine_id = fs::read_to_string("/etc/machine-id")
        .unwrap_or_default()
        .trim()
        .to_string();
    
    #[cfg(not(target_os = "linux"))]
    let machine_id = "unknown".to_string();
    
    format!("{}-{}", hostname, machine_id)
}
```

---

### 方案三：License 文件结构（含硬件绑定）

```toml
# license.toml
[license]
version = "1.0"
issued_to = "某某气象局"
issued_at = "2026-07-25T00:00:00Z"
expiry = "2027-07-25T00:00:00Z"

# 硬件绑定（可选）
[hardware]
# 允许绑定的硬件指纹列表
allowed_fingerprints = [
    "a1b2c3d4e5f67890",  # 服务器1
    "b2c3d4e5f6a78901",  # 服务器2
]

# 如果不绑定硬件，留空或删除 [hardware] 段
# allowed_fingerprints = []

[signature]
# HMAC-SHA256 签名，防止篡改
value = "sha256=abc123..."
```

---

### 方案四：签名验证机制

```rust
// src/license.rs

use hmac::{Hmac, Mac};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

pub fn verify_license_signature(license: &License, secret_key: &[u8]) -> Result<(), String> {
    // 将 license 内容序列化（不含 signature 字段）
    let license_content = format!(
        "{}|{}|{}|{}",
        license.issued_to,
        license.issued_at,
        license.expiry,
        license.hardware_fingerprints.join(",")
    );
    
    // 计算 HMAC
    let mut mac = HmacSha256::new_from_slice(secret_key)
        .map_err(|e| format!("HMAC init error: {}", e))?;
    mac.update(license_content.as_bytes());
    let result = mac.finalize();
    let expected_signature = hex::encode(result.into_bytes());
    
    // 对比签名
    if expected_signature != license.signature {
        return Err("License signature verification failed".to_string());
    }
    
    Ok(())
}
```

---

## 完整验证流程

```
启动 STChck
    │
    ▼
检查 license-check feature 是否启用
    │
    ├── 否 → 正常启动（开发/测试模式）
    │
    └── 是 → 读取 license.toml
              │
              ▼
         解析 license 文件
              │
              ▼
         验证 HMAC 签名
              │
              ├── 失败 → 退出，提示 "License 文件被篡改"
              │
              └── 成功 → 检查过期时间
                            │
                            ▼
                       是否过期？
                            │
                            ├── 是 → 退出，提示 "License 已过期"
                            │
                            └── 否 → 检查硬件绑定（如有）
                                          │
                                          ▼
                                     当前机器指纹是否在允许列表？
                                          │
                                          ├── 否 → 退出，提示 "未授权设备"
                                          │
                                          └── 是 → ✅ 正常启动
```

---

## 防绕过措施

| 攻击方式 | 防御措施 |
|----------|----------|
| 修改系统时间 | 可选：启动时 NTP 校验 |
| 复制 license 到其他机器 | 硬件指纹绑定 |
| 篡改 license 文件 | HMAC 签名验证 |
| 删除 license 文件 | 启动必须检测到有效 license |
| 调试绕过验证 | 条件编译，release 才启用 |
| 替换二进制为 debug 版 | 不分发 debug 二进制 |

---

## 实施建议

### 阶段一：最小可用（MVP）
- 仅实现时间限制
- 不绑定硬件
- HMAC 防篡改签名
- **工作量：0.5 天**

### 阶段二：增加硬件绑定
- 添加主板 UUID 读取
- 组合指纹生成
- license 文件支持 allowed_fingerprints
- **工作量：+0.5 天**

### 阶段三：增强安全
- NTP 时间校验
- 反调试检测（可选）
- License 服务器在线验证（可选）
- **工作量：+1 天**

---

## 推荐最终实现结构

```
STChck/
├── src/
│   ├── main.rs              # 启动时调用 license::verify()
│   ├── license.rs           # License 验证核心逻辑
│   ├── hardware_id.rs       # 硬件指纹获取
│   └── ...
├── tools/
│   └── license-gen.rs       # License 生成工具 CLI
├── Cargo.toml
└── license.toml             # 运行时读取（发布时附带）
```

---

## 关键问题确认

需要你来决定：

1. **是否必须硬件绑定？** 还是仅时间限制就够了？
2. **目标部署环境是什么？** 物理服务器 / 虚拟机 / 云服务器？
3. **License 谁来生成？** 你们内部工具还是我写一个 CLI？
4. **过期后行为？** 完全拒绝启动，还是降级运行（仅读不能写）？

这些决定后，我可以直接写完整代码。
