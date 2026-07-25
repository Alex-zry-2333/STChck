//! License Generator CLI Tool for STChck
//!
//! Usage:
//!   cargo run --bin license-gen -- \
//!     --to "某某气象局" \
//!     --days 365 \
//!     --key "your-secret-key" \
//!     --output license.toml
//!
//! With hardware binding:
//!   cargo run --bin license-gen -- \
//!     --to "某某气象局" \
//!     --days 365 \
//!     --key "your-secret-key" \
//!     --hardware \
//!     --fingerprint "a1b2c3d4e5f67890" \
//!     --output license.toml

use chrono::{DateTime, Utc, Duration};
use clap::{Parser, Subcommand};
use serde::{Deserialize, Serialize};
use std::fs;
use std::process;

#[derive(Parser)]
#[command(name = "license-gen")]
#[command(about = "STChck License Generator")]
#[command(version = "1.0")]
struct Cli {
    #[command(subcommand)]
    command: Option<Commands>,

    /// Licensee name
    #[arg(short, long, default_value = "Evaluation")]
    to: String,

    /// Validity period in days
    #[arg(short, long, default_value_t = 30)]
    days: i64,

    /// Secret key for signing
    #[arg(short, long, env = "STCHCK_LICENSE_KEY")]
    key: String,

    /// Enable hardware binding
    #[arg(long)]
    hardware: bool,

    /// Hardware fingerprint(s) to allow (comma-separated)
    #[arg(long, value_delimiter = ',')]
    fingerprint: Vec<String>,

    /// Output file path
    #[arg(short, long, default_value = "license.toml")]
    output: String,

    /// License version
    #[arg(long, default_value = "1.0")]
    version: String,
}

#[derive(Subcommand)]
enum Commands {
    /// Show this machine's hardware fingerprint
    Fingerprint,
}

#[derive(Debug, Serialize, Deserialize)]
struct License {
    version: String,
    issued_to: String,
    issued_at: DateTime<Utc>,
    expiry: DateTime<Utc>,
    #[serde(default)]
    hardware: HardwareBinding,
    #[serde(default)]
    features: LicenseFeatures,
    signature: String,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct HardwareBinding {
    #[serde(default)]
    enforce: bool,
    #[serde(default)]
    allowed_fingerprints: Vec<String>,
}

#[derive(Debug, Default, Serialize, Deserialize)]
struct LicenseFeatures {
    #[serde(default = "default_degraded_mode")]
    degraded_mode: bool,
    #[serde(default = "default_reminder_interval_hours")]
    reminder_interval_hours: u64,
}

fn default_degraded_mode() -> bool { true }
fn default_reminder_interval_hours() -> u64 { 24 }

fn main() {
    let cli = Cli::parse();

    match cli.command {
        Some(Commands::Fingerprint) => {
            show_fingerprint();
            return;
        }
        None => {}
    }

    if cli.key.is_empty() || cli.key == "your-secret-key" {
        eprintln!("❌ Error: Secret key is required. Set --key or STCHCK_LICENSE_KEY env var.");
        eprintln!("   Example: --key \"stchck-prod-key-2026-xxxxxxxx\"");
        process::exit(1);
    }

    let issued_at = Utc::now();
    let expiry = issued_at + Duration::days(cli.days);

    let hardware = if cli.hardware {
        if cli.fingerprint.is_empty() {
            eprintln!("⚠️  Warning: --hardware enabled but no --fingerprint provided.");
            eprintln!("   Use 'license-gen fingerprint' to get this machine's fingerprint.");
        }
        HardwareBinding {
            enforce: true,
            allowed_fingerprints: cli.fingerprint.clone(),
        }
    } else {
        HardwareBinding::default()
    };

    let license = License {
        version: cli.version,
        issued_to: cli.to.clone(),
        issued_at,
        expiry,
        hardware,
        features: LicenseFeatures {
            degraded_mode: true,
            reminder_interval_hours: 24,
        },
        signature: String::new(), // will be filled
    };

    let signed_license = sign_license(license, &cli.key);

    let toml_content = format_license_toml(&signed_license);

    match fs::write(&cli.output, &toml_content) {
        Ok(_) => {
            println!("✅ License generated successfully!");
            println!("   File: {}", cli.output);
            println!("   Issued to: {}", cli.to);
            println!("   Issued at: {}", issued_at.format("%Y-%m-%d %H:%M:%S UTC"));
            println!("   Expires:   {} ({} days)", expiry.format("%Y-%m-%d %H:%M:%S UTC"), cli.days);
            if cli.hardware {
                println!("   Hardware bound: {} fingerprint(s)", cli.fingerprint.len());
            }
        }
        Err(e) => {
            eprintln!("❌ Failed to write license file: {}", e);
            process::exit(1);
        }
    }
}

fn sign_license(mut license: License, secret_key: &str) -> License {
    use hmac::{Hmac, Mac};
    use sha2::Sha256;

    let payload = format!(
        "{}|{}|{}|{}|{}|{}",
        license.version,
        license.issued_to,
        license.issued_at.to_rfc3339(),
        license.expiry.to_rfc3339(),
        license.hardware.enforce,
        license.hardware.allowed_fingerprints.join(",")
    );

    type HmacSha256 = Hmac<Sha256>;
    let mut mac = HmacSha256::new_from_slice(secret_key.as_bytes())
        .expect("HMAC initialization failed");
    mac.update(payload.as_bytes());
    let result = mac.finalize();
    let signature = hex::encode(result.into_bytes());

    license.signature = format!("sha256={}", signature);
    license
}

fn format_license_toml(license: &License) -> String {
    let hw_fingerprints = if license.hardware.allowed_fingerprints.is_empty() {
        String::new()
    } else {
        license.hardware.allowed_fingerprints.iter()
            .map(|f| format!("    \"{}\"", f))
            .collect::<Vec<_>>()
            .join(",\n")
    };

    let hw_section = if license.hardware.enforce && !license.hardware.allowed_fingerprints.is_empty() {
        format!(
            "\n[hardware]\nenforce = true\nallowed_fingerprints = [\n{}\n]\n",
            hw_fingerprints
        )
    } else {
        String::new()
    };

    format!(
        r#"# STChck License File
# Generated: {}
# DO NOT EDIT — signature will become invalid

[license]
version = "{}"
issued_to = "{}"
issued_at = "{}"
expiry = "{}"
{}{}
[features]
degraded_mode = true
reminder_interval_hours = 24

[signature]
value = "{}"
"#,
        Utc::now().to_rfc3339(),
        license.version,
        license.issued_to,
        license.issued_at.to_rfc3339(),
        license.expiry.to_rfc3339(),
        hw_section,
        if hw_section.is_empty() { "" } else { "\n" },
        license.signature,
    )
}

fn show_fingerprint() {
    println!("🔍 Computing hardware fingerprint for this machine...\n");

    let mut parts = Vec::new();

    // Hostname
    if let Ok(hostname) = gethostname::gethostname().into_string() {
        println!("  Hostname: {}", hostname);
        parts.push(hostname);
    }

    // Machine ID / UUID
    #[cfg(target_os = "linux")]
    {
        if let Ok(id) = fs::read_to_string("/etc/machine-id") {
            let id = id.trim();
            println!("  Machine ID: {}", id);
            parts.push(id.to_string());
        }
        if let Ok(uuid) = fs::read_to_string("/sys/class/dmi/id/board_uuid") {
            let uuid = uuid.trim();
            if !uuid.is_empty() && !uuid.contains("Not") {
                println!("  Board UUID: {}", uuid);
                parts.push(uuid.to_string());
            }
        }
    }

    #[cfg(target_os = "macos")]
    {
        if let Ok(output) = std::process::Command::new("ioreg")
            .args(&["-rd1", "-c", "IOPlatformExpertDevice"])
            .output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if line.contains("IOPlatformUUID") {
                    if let Some(uuid) = line.split('"').nth(1) {
                        println!("  Platform UUID: {}", uuid);
                        parts.push(uuid.to_string());
                    }
                }
            }
        }
    }

    #[cfg(target_os = "windows")]
    {
        if let Ok(output) = std::process::Command::new("wmic")
            .args(&["csproduct", "get", "UUID", "/value"])
            .output() {
            let stdout = String::from_utf8_lossy(&output.stdout);
            for line in stdout.lines() {
                if let Some(val) = line.split('=').nth(1) {
                    let val = val.trim();
                    if !val.is_empty() && val != "FFFFFFFF-FFFF-FFFF-FFFF-FFFFFFFFFFFF" {
                        println!("  UUID: {}", val);
                        parts.push(val.to_string());
                    }
                }
            }
        }
    }

    // MAC address
    if let Ok(mac) = get_primary_mac() {
        println!("  MAC Address: {}", mac);
        parts.push(mac);
    }

    if parts.is_empty() {
        eprintln!("\n❌ Could not determine any hardware identifiers.");
        process::exit(1);
    }

    use sha2::{Sha256, Digest};
    let combined = parts.join("|");
    let mut hasher = Sha256::new();
    hasher.update(combined.as_bytes());
    let hash = format!("{:x}", hasher.finalize());
    let fingerprint = &hash[..16];

    println!("\n═══════════════════════════════════════════════════");
    println!("  Hardware Fingerprint: {}", fingerprint);
    println!("═══════════════════════════════════════════════════");
    println!("\nUse this fingerprint when generating a bound license:");
    println!("  license-gen --to \"Client\" --days 365 --key \"xxx\" --hardware --fingerprint {}", fingerprint);
}

fn get_primary_mac() -> Result<String, String> {
    #[cfg(target_os = "linux")]
    {
        let entries = fs::read_dir("/sys/class/net")
            .map_err(|e| format!("Cannot list interfaces: {}", e))?;

        for entry in entries.flatten() {
            let name = entry.file_name();
            let name_str = name.to_string_lossy();

            if name_str.starts_with("lo")
                || name_str.starts_with("docker")
                || name_str.starts_with("veth")
                || name_str.starts_with("br-")
                || name_str.starts_with("virbr")
                || name_str.starts_with("dummy")
                || name_str.starts_with("tun")
                || name_str.starts_with("tap")
            {
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

    Err("No physical MAC found".to_string())
}
