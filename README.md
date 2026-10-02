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

指标定义位于 `assets/metrics.ron`，启动时读取并校验。每个指标生成一个带 `Metric` 标记的 Entity，其身份、说明、当前值、初始值、边界和影响关系存放在独立组件中。`min_value`、`max_value`、`influence` 可省略；包含时使用 RON 的 `Some(...)`：

```ron
[
    (
        id: "source",
        name: "源指标",
        description: "手动修改的测试指标",
        initial_value: 10.0,
    ),
    (
        id: "target",
        name: "目标指标",
        description: "立即受源指标影响",
        initial_value: 0.0,
        min_value: Some(0.0),
        max_value: Some(100.0),
        influence: Some(Immediate([
            (source_metric: "source", factor: 2.0),
        ])),
    ),
]
```

`Immediate` 按实际变化传播：先计算并限制源/目标值，再将目标的实际 delta 继续向下传播。初始值和重置值不触发传播。重复 id、缺失 source 和 Immediate 环依赖会阻止启动，并显示错误；数值和系数必须有限，初始值必须在边界内。

`Annual` 使用相同的项结构，年末将值替换为各 `source_current_value * factor` 之和并限制边界，再将实际 delta 送入 Immediate 传播。Annual 的 `source_metric` 必须是非 Annual 指标；引用 Annual 指标、自引用和 Annual 环依赖都会在启动时被拒绝。所有年末结果先从结算前的组件值计算完成，避免 Immediate 传播改变其他 Annual 的输入。

时间从 `Year 1 / Month 1` 开始，第 12 次推进到 `Year 2 / Month 1` 并执行年末结算。指标 id `collapse` 保留给崩溃度；任何操作或传播使其达到 100 都立即终止当前操作、重置这一局并清空尚未处理的旧操作。

逻辑位于 `src/metrics.rs`，UI 位于 `src/debug_ui.rs`；测试使用 Bevy `MinimalPlugins`，无需窗口和显卡即可验证模拟规则。

其他系统可直接通过 `Query<(&MetricId, &mut MetricValue), With<Metric>>` 修改当前值，并将写入系统安排在 `SimulationSet::ObserveChanges` 之前。模拟系统通过每个 Entity 的上次已传播值计算实际 delta，应用边界并触发 Immediate 传播。重置通过 `InitialValue` 恢复指标；`World` 中不保留 RON 定义副本或全局指标数值数组。
