# CI/CD 流水线说明

## 工作流: `CI/CD - Un-tested to Tested`

### 触发条件
当推送到任何以 `hw/un-tested-` 开头的分支时自动触发。

### 执行步骤

| 步骤 | 名称 | 说明 |
|------|------|------|
| 1 | Checkout repository | 拉取代码 |
| 2 | Setup Rust | 安装 Rust stable 工具链 |
| 3 | Cache cargo registry | 缓存依赖加速构建 |
| 4 | Check formatting | 检查代码格式 (`cargo fmt`) |
| 5 | Run Clippy | 静态代码检查 (`cargo clippy`) |
| 6 | Build | 编译项目 |
| 7 | Run tests | 运行所有单元测试 (`cargo test`) |
| 8 | Promote to hw/tested | 测试通过则推送到 `hw/tested` 分支 |
| 9 | Notify on failure | 测试失败则阻止推送并提示 |

### 分支流转

```
hw/un-tested-YYYY-MM-DD  ──测试通过──>  hw/tested
        │
        └── 测试失败 ──X──> (不合并)
```

### 测试覆盖范围

当前测试文件位于 `src/tests.rs`，覆盖：

- `get_alarm()` - 告警文本解析
- `is_kit()` - 设备项识别
- `parse_st_packet()` - ST 数据包解析
- `get_station_index()` - 站点索引查询
- `generate_simulated_data()` - 模拟数据生成
- `calculate_risk_score()` - 风险评分计算
- `risk_level()` - 风险等级判定

### 添加新测试

在 `src/tests.rs` 的 `mod tests` 块中添加新的 `#[test]` 函数：

```rust
#[test]
fn test_your_feature() {
    // 你的测试代码
    assert_eq!(actual, expected);
}
```

### 本地测试

```bash
# 运行所有测试
cargo test

# 运行特定测试
cargo test test_get_alarm

# 运行并显示输出
cargo test -- --nocapture

# 检查格式
cargo fmt --all -- --check

# 运行 clippy
cargo clippy --all-targets --all-features -- -D warnings
```

### 注意事项

1. `hw/tested` 分支是**受保护**的测试通过代码线
2. 只有通过全部测试的代码才会被合并到 `hw/tested`
3. 测试失败时，请修复问题后重新推送触发流水线
