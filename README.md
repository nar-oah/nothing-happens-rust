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

窗口中的 `-5` / `+5` 修改对应指标，年份、月份和任期使用 `-1` / `+1`，`Next Month` 推进一个月。崩溃度按钮增加 25 或 100；达到 100 后普通指标恢复初始值，年份和月份归 1，任期加 1。Esc 或关闭窗口退出。UI 只提交请求，不直接写入指标。

指标定义位于 `assets/data/metrics.metric.ron`。正式运行时由 Bevy `AssetServer` 异步加载，`MetricCatalogLoader` 使用 RON 解析并校验数据；加载完成后再生成 Metric Entity。Debug UI 响应 `Added<Metric>`，在 Metric Entity 创建后初始化一次；simulation 不依赖额外的就绪标记。

指标资源读取、RON 解析或定义校验失败会终止游戏，并报告资源路径和原因。运行时非法增量、非有限计算结果或缺失指标实体通过断言直接暴露。

Metric 模块按职责拆分：

- `src/metrics.rs`：`MetricPlugin`、Asset 到 Entity 的组装。
- `src/metrics/asset.rs`：RON 静态定义、AssetLoader、定义校验。
- `src/metrics/components.rs`：Metric 运行时 ECS Components。
- `src/metrics/simulation.rs`：Immediate/Annual、时间和崩溃重置逻辑。

每个指标生成一个带 `Metric` 标记的 Entity，其身份、说明、当前值、初始值、边界和影响关系存放在独立组件中。`min_value`、`max_value`、`influence` 可省略。

所有指标增量通过 `PendingMetricChanges` 队列提交，队列项 `MetricChange` 仅包含目标 Metric Entity 和 delta。处理系统消费队列，应用 min/max，计算 actual delta，并把所有 Immediate 目标的 `actual_delta * factor` 追加到同一个队列，直到队列为空。初始值和重置值不触发传播。重复 id、缺失 source 和 Immediate 环依赖会拒绝该 Asset；数值和系数必须有限，初始值必须在边界内。

`Annual` 在跨年时基于结算前的源指标值计算各 `source_current_value * factor` 之和，将其与当前值的差值加入同一个队列，由队列统一应用边界并触发 Immediate 传播。Annual 的 `source_metric` 必须是非 Annual 指标。

`year`、`month`、`term` 是 RON 中的普通 Metric，均从 1 开始。独立的时间系统消费 `PendingMonthAdvances` 请求，每次月份加 1；超过 12 时归 1，年份加 1，并执行 Annual 结算。时间增量也通过指标队列传播。第 12 次推进到 `Year 2 / Month 1`。指标 id `collapse` 保留给崩溃度；任何操作或传播使其达到 100 都立即重置这一局，并清空剩余指标变更和月份推进请求。

其他系统应向 `PendingMetricChanges` 提交目标 Entity 和 delta，并安排在 `SimulationSet::ApplyChanges` 之前。月份推进请求通过增加 `PendingMonthAdvances.0` 提交。处理顺序为指标变更、月份推进；不支持外部直接写入 `MetricValue`。
