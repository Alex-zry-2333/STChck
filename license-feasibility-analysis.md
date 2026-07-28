# STChck License 使用期限控制 - 可行性分析

## 需求摘要

在 STChck 项目中增加软件使用期限控制，**测试/开发期间不启用**，仅在**发布版本**中激活，避免干扰日常调试。

---

## 方案对比分析

### 方案一：直接集成 licensecc（C++ FFI）

| 维度 | 评估 |
|------|------|
| **实现复杂度** | ⭐⭐⭐⭐⭐ 高 — licensecc 是 C++ 库，Rust 需通过 FFI/绑定引入 |
| **构建依赖** | 需安装 OpenSSL、CMake、C++ 编译器；跨平台构建复杂 |
| **二进制体积** | 增加显著（C++ 运行时 + OpenSSL） |
| **调试干扰** | 高 — 开发时也需要处理 FFI 边界和许可证文件 |
| **测试隔离** | 困难 — 需条件编译屏蔽整个 FFI 层 |
| **可维护性** | 低 — 依赖外部 C++ 项目，版本升级困难 |

**结论：不推荐**。Rust 项目引入 C++ 依赖是重量级操作，与 STChck 的轻量定位不符。

---

### 方案二：纯 Rust 自建轻量 License 模块（推荐）

**设计思路：** 不引入任何外部库，手写一个几百行的 Rust 模块，功能覆盖核心需求：

- 许可证文件（JSON / TOML 格式）含过期时间
- 启动时读取并验证
- 过期后优雅退出（提示信息 + 日志）
- 可选：简单的机器码绑定（MAC 地址 / 磁盘 ID）

| 维度 | 评估 |
|------|------|
| **实现复杂度** | ⭐⭐ 低 — 纯 Rust，无外部依赖 |
| **构建依赖** | 零额外依赖 |
| **二进制体积** | 增加 < 10KB |
| **调试干扰** | **无** — 通过编译条件彻底屏蔽 |
| **测试隔离** | ✅ 完美 — `#[cfg(feature = "license-check")]` 控制 |
| **可维护性** | 高 — 代码自研，完全可控 |

**核心实现模式：**

```rust
// src/license.rs

#[cfg(feature = "license-check")]
pub mod license {
    use chrono::{DateTime, Utc};
    use serde::Deserialize;
    use std::fs;
    
    #[derive(Deserialize)]
    pub struct License {
        pub expiry: DateTime<Utc>,
        pub issued_to: String,
        // 可选：机器绑定
        // pub machine_id: Option<String>,
    }
    
    pub fn verify() -> Result<(), String> {
        let content = fs::read_to_string("license.toml")
            .map_err(|_| "License file not found".to_string())?;
        let license: License = toml::from_str(&content)
            .map_err(|e| format!("Invalid license: {}", e))?;
        
        if Utc::now() > license.expiry {
            return Err(format!(
                "License expired on {}. Please contact support.",
                license.expiry.format("%Y-%m-%d")
            ));
        }
        
        tracing::info!("License valid until {}", license.expiry.format("%Y-%m-%d"));
        Ok(())
    }
}

#[cfg(not(feature = "license-check"))]
pub mod license {
    pub fn verify() -> Result<(), String> {
        // 测试/开发模式：直接通过，零干扰
        Ok(())
    }
}
```

**Cargo.toml 配置：**

```toml
[features]
default = []
# 发布时启用: cargo build --release --features license-check
license-check = []

[dependencies]
# license-check 启用时才需要的依赖（已有 chrono, toml, serde）
```

**启动时调用：**

```rust
// main.rs
fn main() {
    // ... setup ...
    
    if let Err(e) = license::verify() {
        tracing::error!("License verification failed: {}", e);
        eprintln!("❌ {}", e);
        std::process::exit(1);
    }
    
    // ... continue startup ...
}
```

---

### 方案三：编译模式自动区分（release-only）

利用 Rust 内置的 `debug_assertions` 条件：

```rust
#[cfg(not(debug_assertions))]
pub fn verify() { /* 实际验证逻辑 */ }

#[cfg(debug_assertions)]
pub fn verify() { /* 测试模式直接通过 */ }
```

- `cargo build` / `cargo run` → debug 模式 → **License 检查被完全编译掉**
- `cargo build --release` → release 模式 → **License 检查生效**

**优点：** 零配置，开发者无感知  
**缺点：** 灵活性低，无法手动在 debug 模式测试 License 逻辑

---

### 方案四：外部 License 验证工具（进程外）

写一个独立的 License 验证程序（Python/Rust），在打包/发布流程中调用：

```
打包流程:
1. 编译 STChck
2. 运行 license-generator 生成 license.toml
3. 将 license.toml 与二进制一起分发
4. STChck 启动时读取 license.toml
```

**优点：** 验证逻辑与主程序完全解耦  
**缺点：** 多了一个部署单元，增加运维复杂度

---

## 推荐方案：方案二 + 方案三混合

### 最终架构

```
┌─────────────────────────────────────────────────────────┐
│                    编译期条件                              │
├─────────────────────────────────────────────────────────┤
│  cargo build (debug)                                     │
│    └── license 模块被 #[cfg] 完全屏蔽                    │
│        └── 零运行时开销，零调试干扰                       │
│                                                          │
│  cargo build --release --features license-check          │
│    └── license 模块编译进二进制                           │
│        └── 启动时验证 license.toml                        │
│            └── 过期 → 优雅退出 + 提示                      │
│            └── 有效 → 正常启动                             │
└─────────────────────────────────────────────────────────┘
```

### 开发/测试/发布工作流

```bash
# 日常开发 — License 完全不介入
cargo run

# 本地测试 License 逻辑（手动启用 feature）
cargo run --features license-check

# 发布构建 — 自动启用 License
cargo build --release --features license-check
```

### 发布流程建议

1. **编译发布版本：** `cargo build --release --features license-check`
2. **生成 License 文件：** 使用独立工具或脚本生成 `license.toml`
3. **打包分发：** 二进制 + `license.toml` + 配置文件

---

## 风险评估

| 风险 | 可能性 | 影响 | 缓解措施 |
|------|--------|------|----------|
| License 文件被篡改 | 中 | 高 | 添加简单签名验证（HMAC-SHA256） |
| 系统时间被修改绕过 | 低 | 中 | 可选：NTP 时间校验 |
| Feature flag 误用 | 低 | 中 | CI/CD 中强制 `--features license-check` |
| 调试时意外启用 | 低 | 低 | 默认 feature 为空，需显式启用 |

---

## 实现成本估算

| 项 | 工作量 | 说明 |
|----|--------|------|
| License 模块开发 | 0.5 天 | 纯 Rust，约 200 行代码 |
| HMAC 签名验证 | 0.5 天 | 增加防篡改能力 |
| License 生成工具 | 0.5 天 | CLI 工具或脚本 |
| 集成到 main.rs | 0.25 天 | 启动时调用验证 |
| CI/CD 适配 | 0.25 天 | 发布流程加 `--features license-check` |
| **总计** | **~2 天** | |

---

## 决策建议

**建议采用：纯 Rust 自建 + Feature Flag 方案**

理由：
1. **零调试干扰** — 开发时 License 代码被编译器完全移除
2. **零外部依赖** — 不引入 C++/OpenSSL 等重量级依赖
3. **可控可维护** — 自研代码，适配 STChck 的具体需求
4. **轻量集成** — 2 天工作量，不阻塞主业务开发
5. **可扩展** — 后续可添加机器绑定、浮动许可等高级功能

**不建议使用 licensecc** — 对于 Rust 项目来说，引入 C++ FFI 的边际收益远低于边际成本。

---

## 下一步

如同意此方案，我可以立即：
1. 编写 `src/license.rs` 模块
2. 更新 `Cargo.toml` 添加 feature
3. 修改 `src/main.rs` 集成启动验证
4. 提供 License 生成脚本
5. 更新 CI/CD 流水线适配发布构建
