# Nothing Happens

第一阶段：RON 指标、Immediate 传播、Annual 结算和原生 Bevy Debug UI。

进入开发环境后运行：

```bash
nix develop
cargo fmt
cargo check
cargo test
cargo run
```

窗口中的 `-5` / `+5` 修改对应指标，`Next Month` 推进一个月。崩溃度按钮增加 25 或 100；达到 100 后全部指标和时间恢复初始状态。Esc 或关闭窗口退出。

指标定义位于 `assets/data/metrics.metric.ron`。正式运行时由 Bevy `AssetServer` 异步加载，`MetricCatalogLoader` 使用 RON 解析并校验数据；加载完成后再生成 Metric Entity，并插入 `MetricReady`。

Metric 模块按职责拆分：

- `src/metrics.rs`：`MetricPlugin`、Asset 到 Entity 的组装。
- `src/metrics/asset.rs`：RON 静态定义、AssetLoader、定义校验。
- `src/metrics/components.rs`：Metric 运行时 ECS Components。
- `src/metrics/simulation.rs`：Immediate/Annual、时间和崩溃重置逻辑。

每个指标生成一个带 `Metric` 标记的 Entity，其身份、说明、当前值、初始值、边界和影响关系存放在独立组件中。`min_value`、`max_value`、`influence` 可省略。

`Immediate` 按实际变化传播：先计算并限制源/目标值，再将目标的实际 delta 继续向下传播。初始值和重置值不触发传播。重复 id、缺失 source 和 Immediate 环依赖会拒绝该 Asset；数值和系数必须有限，初始值必须在边界内。

`Annual` 在年末将值替换为各 `source_current_value * factor` 之和并限制边界，再将实际 delta 送入 Immediate 传播。Annual 的 `source_metric` 必须是非 Annual 指标。

时间从 `Year 1 / Month 1` 开始，第 12 次推进到 `Year 2 / Month 1` 并执行年末结算。指标 id `collapse` 保留给崩溃度；任何操作或传播使其达到 100 都立即重置这一局。

其他系统可直接通过 `Query<(&MetricId, &mut MetricValue), With<Metric>>` 修改当前值，并将写入系统安排在 `SimulationSet::ObserveChanges` 之前。
