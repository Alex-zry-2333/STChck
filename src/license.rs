//! License verification module for STChck
//!
//! Compile-time controlled via `license-check` feature:
//! - Debug/dev builds: feature disabled → no verification overhead
//! - Release builds: `--features license-check` → full verification active
//!
//! Behavior on expiry:
//! - Does NOT refuse to start (graceful degradation)
//! - Logs warning, shows banner in web UI
//! - Periodic runtime reminders

#[cfg(feature = "license-check")]
pub mod license {
    use chrono::{DateTime, Utc};
    use serde::{Deserialize, Serialize};
    use std::collections::HashSet;
    use std::fs;
    use std::sync::atomic::{AtomicBool, Ordering};
    use std::sync::Arc;
    use tracing;

    /// Global flag: true if license has expired (but we keep running in degraded mode)
    static LICENSE_EXPIRED: AtomicBool = AtomicBool::new(false);

    /// Global flag: true if license file is missing or invalid
    static LICENSE_INVALID: AtomicBool = AtomicBool::new(false);

    #[derive(Debug, Clone, Serialize, Deserialize)]
    pub struct License {
        pub version: String,
        pub issued_to: String,
        pub issued_at: DateTime<Utc>,
        pub expiry: DateTime<Utc>,
        #[serde(default)]
        pub hardware: HardwareBinding,
        #[serde(default)]
        pub features: LicenseFeatures,
        pub signature: String,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct HardwareBinding {
        /// If true, verify machine fingerprint matches
        #[serde(default)]
        pub enforce: bool,
        /// List of allowed hardware fingerprints
        #[serde(default)]
        pub allowed_fingerprints: Vec<String>,
    }

    #[derive(Debug, Clone, Default, Serialize, Deserialize)]
    pub struct LicenseFeatures {
        /// If true, allow full functionality even after expiry (just warn)
        #[serde(default = "default_degraded_mode")]
        pub degraded_mode: bool,
        /// Hours between runtime reminder logs
        #[serde(default = "default_reminder_interval_hours")]
        pub reminder_interval_hours: u64,
    }

    fn default_degraded_mode() -> bool { true }
    fn default_reminder_interval_hours() -> u64 { 24 }

    #[derive(Debug, Clone)]
    pub struct LicenseState {
        pub valid: bool,
        pub expired: bool,
        pub invalid: bool,
        pub hardware_mismatch: bool,
        pub license_info: Option<License>,
        pub message: String,
    }

    impl Default for LicenseState {
        fn default() -> Self {
            Self {
                valid: false,
                expired: false,
                invalid: false,
                hardware_mismatch: false,
                license_info: None,
                message: "No license".to_string(),
            }
        }
    }

    // ============================================================
    // Main public API
    // ============================================================

    /// Verify license at startup. Returns state object.
    /// On expiry: returns degraded state, does NOT panic/exit.
    pub fn verify() -> LicenseState {
        let license_path = find_license_file();

        let license = match license_path {
            Some(ref path) => match fs::read_to_string(path) {
                Ok(content) => match toml::from_str::<License>(&content) {
                    Ok(lic) => lic,
                    Err(e) => {
                        tracing::error!("License file parse error at {}: {}", path, e);
                        LICENSE_INVALID.store(true, Ordering::SeqCst);
                        return LicenseState {
                            valid: false,
                            invalid: true,
                            message: format!("License file invalid: {}", e),
                            ..Default::default()
                        };
                    }
                },
                Err(e) => {
                    tracing::error!("Cannot read license file at {}: {}", path, e);
                    LICENSE_INVALID.store(true, Ordering::SeqCst);
                    return LicenseState {
                        valid: false,
                        invalid: true,
                        message: format!("Cannot read license file: {}", e),
                        ..Default::default()
                    };
                }
            },
            None => {
                tracing::warn!("No license.toml found. Running in evaluation mode.");
                LICENSE_INVALID.store(true, Ordering::SeqCst);
                return LicenseState {
                    valid: false,
                    invalid: true,
                    message: "No license file found".to_string(),
                    ..Default::default()
                };
            }
        };

        // Check signature
        if let Err(e) = verify_signature(&license) {
            tracing::error!("License signature verification failed: {}", e);
            return LicenseState {
                valid: false,
                invalid: true,
                message: format!("License signature invalid: {}", e),
                license_info: Some(license),
                ..Default::default()
            };
        }

        // Check expiry
        let now = Utc::now();
        let expired = now > license.expiry;

        if expired {
            LICENSE_EXPIRED.store(true, Ordering::SeqCst);
            let days_overdue = (now - license.expiry).num_days();
            let msg = format!(
                "License EXPIRED on {} ({} days overdue). Degraded mode active.",
                license.expiry.format("%Y-%m-%d"),
                days_overdue
            );
            tracing::warn!("{}", msg);

            // Hardware check still runs even if expired
            let hw_result = check_hardware(&license);

            return LicenseState {
                valid: false,
                expired: true,
                hardware_mismatch: !hw_result,
                message: msg,
                license_info: Some(license),
            };
        }

        // Check hardware binding (only if not expired)
        let hw_ok = check_hardware(&license);
        if !hw_ok {
            tracing::warn!(
                "Hardware fingerprint mismatch. License bound to: {:?}, this machine: {}",
                license.hardware.allowed_fingerprints,
                get_machine_fingerprint().unwrap_or_else(|| "unknown".to_string())
            );
            return LicenseState {
                valid: false,
                hardware_mismatch: true,
                message: "Hardware fingerprint mismatch".to_string(),
                license_info: Some(license),
                ..Default::default()
            };
        }

        let days_remaining = (license.expiry - now).num_days();
        let msg = format!(
            "License valid until {} ({} days remaining). Issued to: {}",
            license.expiry.format("%Y-%m-%d"),
            days_remaining,
            license.issued_to
        );
        tracing::info!("{}", msg);

        LicenseState {
            valid: true,
            message: msg,
            license_info: Some(license),
            ..Default::default()
        }
    }

    /// Check if license is currently expired (for runtime checks)
    pub fn is_expired() -> bool {
        LICENSE_EXPIRED.load(Ordering::SeqCst)
    }

    /// Check if license is invalid/missing (for runtime checks)
    pub fn is_invalid() -> bool {
        LICENSE_INVALID.load(Ordering::SeqCst)
    }

    /// Get license status summary for API/UI
    pub fn get_status(state: &LicenseState) -> serde_json::Value {
        serde_json::json!({
            "valid": state.valid,
            "expired": state.expired,
            "invalid": state.invalid,
            "hardware_mismatch": state.hardware_mismatch,
            "message": state.message,
            "issued_to": state.license_info.as_ref().map(|l| l.issued_to.clone()),
            "expiry": state.license_info.as_ref().map(|l| l.expiry.to_rfc3339()),
            "days_remaining": state.license_info.as_ref().map(|l| {
                let days = (l.expiry - Utc::now()).num_days();
                if days < 0 { 0 } else { days as u64 }
            }),
        })
    }

    // ============================================================
    // License file discovery
    // ============================================================

    fn find_license_file() -> Option<String> {
        let candidates = [
            "license.toml",
            "License.toml",
            "LICENSE.toml",
            "/etc/stchck/license.toml",
            "/opt/stchck/license.toml",
        ];

        for path in &candidates {
            if fs::metadata(path).is_ok() {
                return Some(path.to_string());
            }
        }

        // Also check next to the binary
        if let Ok(exe_path) = std::env::current_exe() {
            if let Some(exe_dir) = exe_path.parent() {
                let license_next_to_exe = exe_dir.join("license.toml");
                if license_next_to_exe.exists() {
                    return Some(license_next_to_exe.to_string_lossy().to_string());
                }
            }
        }

        None
    }

    // ============================================================
    // Signature verification (HMAC-SHA256)
    // ============================================================

    fn verify_signature(license: &License) -> Result<(), String> {
        use hmac::{Hmac, Mac};
        use sha2::Sha256;

        // Reconstruct the signed payload (everything except signature field)
        let payload = format!(
            "{}|{}|{}|{}|{}|{}",
            license.version,
            license.issued_to,
            license.issued_at.to_rfc3339(),
            license.expiry.to_rfc3339(),
            license.hardware.enforce,
            license.hardware.allowed_fingerprints.join(",")
        );

        // Secret key: embedded in binary at build time via env var
        // Production builds should set STCHCK_LICENSE_KEY
        let secret_key = std::env::var("STCHCK_LICENSE_KEY")
            .unwrap_or_else(|_| {
                // Fallback: use a default key (NOT for production!)
                tracing::warn!("STCHCK_LICENSE_KEY not set, using default key — INSECURE!");
                "stchck-default-dev-key-2026".to_string()
            });

        type HmacSha256 = Hmac<Sha256>;
        let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
            .map_err(|e| format!("HMAC init error: {}", e))?;
        mac.update(payload.as_bytes());
        let result = mac.finalize();
        let expected = hex::encode(result.into_bytes());

        // Signature in license file may be prefixed with "sha256="
        let sig_value = license.signature.strip_prefix("sha256=").unwrap_or(&license.signature);

        if expected != sig_value {
            return Err(format!(
                "Signature mismatch. Expected: {}, Got: {}",
                &expected[..32.min(expected.len())],
                &sig_value[..32.min(sig_value.len())]
            ));
        }

        Ok(())
    }

    // ============================================================
    // Hardware fingerprint
    // ============================================================

    fn check_hardware(license: &License) -> bool {
        // If hardware binding not enforced, always pass
        if !license.hardware.enforce || license.hardware.allowed_fingerprints.is_empty() {
            return true;
        }

        match get_machine_fingerprint() {
            Some(fp) => {
                let allowed: HashSet<String> = license.hardware.allowed_fingerprints.iter()
                    .map(|s| s.to_lowercase())
                    .collect();
                allowed.contains(&fp.to_lowercase())
            }
            None => {
                // Cannot determine fingerprint — fail safe
                tracing::warn!("Cannot determine machine fingerprint, but hardware binding is enforced");
                false
            }
        }
    }

    /// Get a cross-platform machine fingerprint
    /// Combines multiple identifiers into a stable hash
    pub fn get_machine_fingerprint() -> Option<String> {
        let mut parts = Vec::new();

        // 1. Hostname (always available)
        if let Ok(hostname) = gethostname::gethostname().into_string() {
            parts.push(hostname);
        }

        // 2. Machine ID / system UUID
        #[cfg(target_os = "linux")]
        {
            // /etc/machine-id is systemd's persistent machine ID
            if let Ok(id) = fs::read_to_string("/etc/machine-id") {
                parts.push(id.trim().to_string());
            }
            // DMI board UUID (may require root)
            if let Ok(uuid) = fs::read_to_string("/sys/class/dmi/id/board_uuid") {
                let uuid = uuid.trim();
                if !uuid.is_empty() && !uuid.contains("Not") {
                    parts.push(uuid.to_string());
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            // macOS: use IOPlatformUUID
            if let Ok(output) = std::process::Command::new("ioreg")
                .args(&["-rd1", "-c", "IOPlatformExpertDevice"])
                .output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("IOPlatformUUID") {
                        if let Some(uuid) = line.split('"').nth(1) {
                            parts.push(uuid.to_string());
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            // Windows: use wmic
            if let Ok(output) = std::process::Command::new("wmic")
                .args(&["csproduct", "get", "UUID", "/value"])
                .output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if let Some(val) = line.split('=').nth(1) {
                        let val = val.trim();
                        if !val.is_empty() && val != "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF" {
                            parts.push(val.to_string());
                        }
                    }
                }
            }
        }

        // 3. Primary MAC address (filter virtual interfaces)
        if let Ok(mac) = get_primary_mac() {
            parts.push(mac);
        }

        if parts.is_empty() {
            return None;
        }

        // Combine and hash
        use sha2::{Sha256, Digest};
        let combined = parts.join("|");
        let mut hasher = Sha256::new();
        hasher.update(combined.as_bytes());
        let hash = format!("{:x}", hasher.finalize());
        Some(hash[..16].to_string())
    }

    fn get_primary_mac() -> Result<String, String> {
        #[cfg(target_os = "linux")]
        {
            let entries = fs::read_dir("/sys/class/net")
                .map_err(|e| format!("Cannot list net interfaces: {}", e))?;

            for entry in entries.flatten() {
                let name = entry.file_name();
                let name_str = name.to_string_lossy();

                // Skip virtual interfaces
                if is_virtual_interface(&name_str) {
                    continue;
                }

                let mac_path = entry.path().join("address");
                if let Ok(mac) = fs::read_to_string(&mac_path) {
                    let mac = mac.trim();
                    if mac.len() == 17 && mac != "00:00:00:00:00:00" {
                        return Ok(mac.to_lowercase());
                    }
                }
            }
        }

        #[cfg(target_os = "macos")]
        {
            if let Ok(output) = std::process::Command::new("ifconfig")
                .arg("en0")
                .output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines() {
                    if line.contains("ether") {
                        if let Some(mac) = line.split_whitespace().nth(1) {
                            return Ok(mac.to_lowercase());
                        }
                    }
                }
            }
        }

        #[cfg(target_os = "windows")]
        {
            if let Ok(output) = std::process::Command::new("getmac")
                .args(&["/fo", "csv", "/v"])
                .output() {
                let stdout = String::from_utf8_lossy(&output.stdout);
                for line in stdout.lines().skip(1) {
                    let parts: Vec<&str> = line.split(',').collect();
                    if parts.len() >= 3 {
                        let mac = parts[2].trim_matches('"');
                        if mac.len() == 17 {
                            return Ok(mac.to_lowercase());
                        }
                    }
                }
            }
        }

        Err("No physical MAC address found".to_string())
    }

    fn is_virtual_interface(name: &str) -> bool {
        name.starts_with("lo")          // loopback
            || name.starts_with("docker")
            || name.starts_with("veth")
            || name.starts_with("br-")
            || name.starts_with("virbr")
            || name.starts_with("wlan") // could be physical, but often virtual in cloud
            || name.starts_with("dummy")
            || name.starts_with("tun")
            || name.starts_with("tap")
    }
}

// ============================================================
// No-op implementation when feature disabled
// ============================================================

#[cfg(not(feature = "license-check"))]
pub mod license {
    use serde_json;

    #[derive(Debug, Clone, Default)]
    pub struct LicenseState {
        pub valid: bool,
        pub expired: bool,
        pub invalid: bool,
        pub hardware_mismatch: bool,
        pub message: String,
    }

    /// No-op: always returns valid
    pub fn verify() -> LicenseState {
        LicenseState {
            valid: true,
            message: "License check disabled (debug build)".to_string(),
            ..Default::default()
        }
    }

    pub fn is_expired() -> bool { false }
    pub fn is_invalid() -> bool { false }

    pub fn get_status(_state: &LicenseState) -> serde_json::Value {
        serde_json::json!({
            "valid": true,
            "expired": false,
            "invalid": false,
            "hardware_mismatch": false,
            "message": "License check disabled",
        })
    }

    pub fn get_machine_fingerprint() -> Option<String> {
        None
    }
}
