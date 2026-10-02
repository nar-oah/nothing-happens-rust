use bevy::prelude::*;
use nothing_happens::metrics::{
    AnnualInfluence, COLLAPSE_METRIC_ID, CollapseMetric, GameAction, GameDate, ImmediateInfluence,
    Influence, InfluenceTerm, InitialValue, Metric, MetricBounds, MetricDefinition, MetricError,
    MetricId, MetricMetadata, MetricOrder, MetricPlugin, MetricValue, PendingActions, SimulationSet,
};

fn definition(id: &str, value: f64, influence: Option<Influence>) -> MetricDefinition {
    MetricDefinition {
        id: id.to_owned(),
        name: id.to_owned(),
        description: format!("Test metric {id}"),
        initial_value: value,
        min_value: None,
        max_value: None,
        influence,
    }
}

fn bounded(mut metric: MetricDefinition, min: f64, max: f64) -> MetricDefinition {
    metric.min_value = Some(min);
    metric.max_value = Some(max);
    metric
}

fn term(source: &str, factor: f64) -> InfluenceTerm {
    InfluenceTerm {
        source_metric: source.to_owned(),
        factor,
    }
}

fn immediate(source: &str, factor: f64) -> Option<Influence> {
    Some(Influence::Immediate(vec![term(source, factor)]))
}

fn annual(source: &str, factor: f64) -> Option<Influence> {
    Some(Influence::Annual(vec![term(source, factor)]))
}

fn app_with(definitions: Vec<MetricDefinition>) -> App {
    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .add_plugins(MetricPlugin::new(definitions).expect("valid metric definitions"));
    app.update();
    app
}

fn act(app: &mut App, actions: impl IntoIterator<Item = GameAction>) {
    app.world_mut()
        .resource_mut::<PendingActions>()
        .0
        .extend(actions);
    app.update();
}

fn change(app: &mut App, id: &str, delta: f64) {
    act(
        app,
        [GameAction::ChangeMetric {
            id: id.to_owned(),
            delta,
        }],
    );
}

fn advance_months(app: &mut App, months: usize) {
    act(app, (0..months).map(|_| GameAction::NextMonth));
}

fn entity(app: &mut App, id: &str) -> Entity {
    let world = app.world_mut();
    let mut query = world.query_filtered::<(Entity, &MetricId), With<Metric>>();
    query
        .iter(world)
        .find_map(|(entity, metric_id)| (metric_id.0 == id).then_some(entity))
        .expect("metric entity exists")
}

fn value(app: &mut App, id: &str) -> f64 {
    let entity = entity(app, id);
    app.world()
        .get::<MetricValue>(entity)
        .expect("entity has a MetricValue component")
        .0
}

fn assert_value(app: &mut App, id: &str, expected: f64) {
    let actual = value(app, id);
    assert!(
        (actual - expected).abs() < 1e-10,
        "{id}: expected {expected}, got {actual}"
    );
}

fn assert_date(app: &App, year: u32, month: u8) {
    let date = app.world().resource::<GameDate>();
    assert_eq!((date.year, date.month), (year, month));
}

#[derive(Resource, Default)]
struct DirectEdits(Vec<(&'static str, f64)>);

fn apply_direct_edits(
    mut metrics: Query<(&MetricId, &mut MetricValue), With<Metric>>,
    mut edits: ResMut<DirectEdits>,
) {
    for (id, delta) in edits.0.drain(..) {
        let (_, mut value) = metrics
            .iter_mut()
            .find(|(metric_id, _)| metric_id.0 == id)
            .expect("direct edit refers to a metric");
        value.0 += delta;
    }
}

#[derive(Resource, Default)]
struct ChangedMetrics(Vec<String>);

fn record_changed_metrics(
    metrics: Query<&MetricId, (With<Metric>, Changed<MetricValue>)>,
    mut changed: ResMut<ChangedMetrics>,
) {
    changed.0 = metrics.iter().map(|id| id.0.clone()).collect();
    changed.0.sort();
}

#[test]
fn startup_spawns_each_metric_without_propagating_initial_values() {
    let mut app = app_with(vec![
        definition("source", 50.0, None),
        definition("target", 3.0, immediate("source", 2.0)),
    ]);

    assert_date(&app, 1, 1);
    assert_value(&mut app, "source", 50.0);
    assert_value(&mut app, "target", 3.0);
    let world = app.world_mut();
    let mut query = world.query_filtered::<(&MetricId, &MetricOrder), With<Metric>>();
    let mut metrics: Vec<_> = query
        .iter(world)
        .map(|(id, order)| (id.0.clone(), order.0))
        .collect();
    metrics.sort_by_key(|(_, order)| *order);
    assert_eq!(
        metrics,
        [("source".to_owned(), 0), ("target".to_owned(), 1)]
    );
}

#[test]
fn startup_exposes_metric_data_and_entity_influences_as_distinct_components() {
    let mut app = app_with(vec![
        bounded(definition("source", 10.0, None), 0.0, 100.0),
        definition("immediate", 2.0, immediate("source", 2.0)),
        definition("annual", 3.0, annual("immediate", 3.0)),
        bounded(definition(COLLAPSE_METRIC_ID, 0.0, None), 0.0, 100.0),
    ]);

    let world = app.world_mut();
    let mut query = world.query_filtered::<
        (
            &MetricId,
            &MetricMetadata,
            &MetricValue,
            &InitialValue,
            &MetricBounds,
            &MetricOrder,
            Has<ImmediateInfluence>,
            Has<AnnualInfluence>,
            Has<CollapseMetric>,
        ),
        With<Metric>,
    >();
    let mut metrics = Vec::new();
    for (id, metadata, value, initial, bounds, order, immediate, annual, collapse) in
        query.iter(world)
    {
        assert_eq!(metadata.name, id.0);
        assert_eq!(metadata.description, format!("Test metric {}", id.0));
        assert_eq!(value.0, initial.0);
        if id.0 == "source" || id.0 == COLLAPSE_METRIC_ID {
            assert_eq!((bounds.min, bounds.max), (Some(0.0), Some(100.0)));
        } else {
            assert_eq!((bounds.min, bounds.max), (None, None));
        }
        metrics.push((id.0.clone(), order.0, immediate, annual, collapse));
    }
    metrics.sort_by_key(|(_, order, _, _, _)| *order);
    assert_eq!(
        metrics,
        [
            ("source".to_owned(), 0, false, false, false),
            ("immediate".to_owned(), 1, true, false, false),
            ("annual".to_owned(), 2, false, true, false),
            (COLLAPSE_METRIC_ID.to_owned(), 3, false, false, true),
        ]
    );

    let source = entity(&mut app, "source");
    let immediate = entity(&mut app, "immediate");
    let annual = entity(&mut app, "annual");
    let inputs = &app.world().get::<ImmediateInfluence>(immediate).unwrap().0;
    assert_eq!(inputs.len(), 1);
    assert_eq!((inputs[0].source, inputs[0].factor), (source, 2.0));
    let inputs = &app.world().get::<AnnualInfluence>(annual).unwrap().0;
    assert_eq!(inputs.len(), 1);
    assert_eq!((inputs[0].source, inputs[0].factor), (immediate, 3.0));
}

#[test]
fn a_runtime_query_write_propagates_immediate_changes_once() {
    let mut app = app_with(vec![
        bounded(definition("source", 10.0, None), 0.0, 12.0),
        definition("middle", 5.0, immediate("source", 2.0)),
        definition("target", 1.0, immediate("middle", 3.0)),
    ]);
    app.insert_resource(DirectEdits(vec![("source", 20.0)]))
        .add_systems(
            Update,
            apply_direct_edits.before(SimulationSet::ObserveChanges),
        );

    app.update();

    assert_value(&mut app, "source", 12.0);
    assert_value(&mut app, "middle", 9.0);
    assert_value(&mut app, "target", 13.0);

    app.update();

    assert_value(&mut app, "middle", 9.0);
    assert_value(&mut app, "target", 13.0);
}

#[test]
fn simultaneous_runtime_query_writes_propagate_each_external_delta_once() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("middle", 5.0, immediate("source", 2.0)),
        definition("target", 1.0, immediate("middle", 3.0)),
    ]);
    app.insert_resource(DirectEdits(vec![("source", 2.0), ("middle", 4.0)]))
        .add_systems(
            Update,
            apply_direct_edits.before(SimulationSet::ObserveChanges),
        );

    app.update();

    assert_value(&mut app, "source", 12.0);
    assert_value(&mut app, "middle", 13.0);
    assert_value(&mut app, "target", 25.0);

    app.update();
    assert_value(&mut app, "target", 25.0);
}

#[test]
fn idle_and_unrelated_actions_do_not_mark_untouched_metric_values_changed() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("target", 5.0, immediate("source", 2.0)),
        definition("untouched", 42.0, None),
    ]);
    app.init_resource::<ChangedMetrics>()
        .add_systems(Update, record_changed_metrics.after(SimulationSet::ApplyActions));
    app.update();
    app.update();
    assert!(app.world().resource::<ChangedMetrics>().0.is_empty());

    change(&mut app, "source", 1.0);

    assert_eq!(
        app.world().resource::<ChangedMetrics>().0,
        ["source".to_owned(), "target".to_owned()]
    );
    assert_value(&mut app, "untouched", 42.0);

    change(&mut app, "source", 0.0);
    assert!(app.world().resource::<ChangedMetrics>().0.is_empty());
}

#[test]
fn immediate_single_layer_propagates_the_source_delta() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("target", 5.0, immediate("source", 2.0)),
    ]);

    change(&mut app, "source", 3.0);

    assert_value(&mut app, "source", 13.0);
    assert_value(&mut app, "target", 11.0);
}

#[test]
fn immediate_multiple_layers_propagate_each_actual_delta() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("middle", 20.0, immediate("source", 2.0)),
        definition("target", 3.0, immediate("middle", 0.5)),
    ]);

    change(&mut app, "source", 4.0);

    assert_value(&mut app, "source", 14.0);
    assert_value(&mut app, "middle", 28.0);
    assert_value(&mut app, "target", 7.0);
}

#[test]
fn source_maximum_limits_the_delta_sent_to_dependents() {
    let mut app = app_with(vec![
        bounded(definition("source", 10.0, None), 0.0, 12.0),
        definition("target", 5.0, immediate("source", 3.0)),
    ]);

    change(&mut app, "source", 20.0);
    assert_value(&mut app, "source", 12.0);
    assert_value(&mut app, "target", 11.0);

    change(&mut app, "source", 20.0);
    assert_value(&mut app, "target", 11.0);
}

#[test]
fn target_maximum_limits_the_delta_propagated_to_the_next_layer() {
    let mut app = app_with(vec![
        definition("source", 3.0, None),
        bounded(
            definition("middle", 9.0, immediate("source", 2.0)),
            0.0,
            10.0,
        ),
        definition("target", 1.0, immediate("middle", 4.0)),
    ]);

    change(&mut app, "source", 3.0);

    assert_value(&mut app, "middle", 10.0);
    assert_value(&mut app, "target", 5.0);
}

#[test]
fn source_and_target_minimums_limit_negative_propagation() {
    let mut app = app_with(vec![
        bounded(definition("source", 4.0, None), 0.0, 100.0),
        bounded(
            definition("middle", 3.0, immediate("source", 1.0)),
            2.0,
            100.0,
        ),
        definition("target", 10.0, immediate("middle", 2.0)),
    ]);

    change(&mut app, "source", -10.0);

    assert_value(&mut app, "source", 0.0);
    assert_value(&mut app, "middle", 2.0);
    assert_value(&mut app, "target", 8.0);
}

#[test]
fn immediate_fan_in_adds_changes_from_both_paths() {
    let mut app = app_with(vec![
        definition("source", 0.0, None),
        definition("left", 0.0, immediate("source", 2.0)),
        definition("right", 0.0, immediate("source", 3.0)),
        definition(
            "target",
            10.0,
            Some(Influence::Immediate(vec![
                term("left", 4.0),
                term("right", -1.0),
            ])),
        ),
    ]);

    change(&mut app, "source", 2.0);

    assert_value(&mut app, "target", 20.0);
}

#[test]
fn annual_recalculation_replaces_the_value() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("target", 999.0, annual("source", 1.5)),
    ]);

    change(&mut app, "source", 2.0);
    assert_value(&mut app, "target", 999.0);
    advance_months(&mut app, 12);

    assert_value(&mut app, "target", 18.0);
    assert_date(&app, 2, 1);
}

#[test]
fn annual_recalculation_sums_multiple_sources_and_applies_bounds() {
    let mut app = app_with(vec![
        definition("left", 10.0, None),
        definition("right", 4.0, None),
        bounded(
            definition(
                "target",
                0.0,
                Some(Influence::Annual(vec![
                    term("left", 2.0),
                    term("right", -0.5),
                ])),
            ),
            0.0,
            15.0,
        ),
    ]);

    advance_months(&mut app, 12);
    assert_value(&mut app, "target", 15.0);

    change(&mut app, "left", -20.0);
    advance_months(&mut app, 12);
    assert_value(&mut app, "target", 0.0);
}

#[test]
fn annual_actual_delta_triggers_immediate_propagation() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("annual", 40.0, annual("source", 2.0)),
        definition("target", 100.0, immediate("annual", 0.5)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "annual", 20.0);
    assert_value(&mut app, "target", 90.0);
}

#[test]
fn bounded_annual_result_propagates_its_actual_delta_through_multiple_layers() {
    let mut app = app_with(vec![
        definition("source", 30.0, None),
        bounded(definition("annual", 5.0, annual("source", 2.0)), 0.0, 10.0),
        definition("middle", 20.0, immediate("annual", 2.0)),
        definition("target", 1.0, immediate("middle", 0.5)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "annual", 10.0);
    assert_value(&mut app, "middle", 30.0);
    assert_value(&mut app, "target", 6.0);
}

#[test]
fn unchanged_annual_result_does_not_change_immediate_dependents() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("annual", 20.0, annual("source", 2.0)),
        definition("target", 7.0, immediate("annual", 0.5)),
    ]);

    advance_months(&mut app, 24);

    assert_value(&mut app, "target", 7.0);
}

#[test]
fn annual_metrics_use_the_same_source_snapshot_in_either_definition_order() {
    let source = definition("source", 10.0, None);
    let first = definition("first", 7.0, annual("source", 2.0));
    let second = definition("second", 2.0, annual("first", 3.0));

    for definitions in [
        vec![source.clone(), first.clone(), second.clone()],
        vec![second, first, source],
    ] {
        let mut app = app_with(definitions);

        advance_months(&mut app, 12);

        assert_value(&mut app, "first", 20.0);
        assert_value(&mut app, "second", 21.0);
    }
}

#[test]
fn annual_cycles_are_allowed_and_read_pre_settlement_values() {
    let mut app = app_with(vec![
        definition("left", 2.0, annual("right", 2.0)),
        definition("right", 3.0, annual("left", 4.0)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "left", 6.0);
    assert_value(&mut app, "right", 8.0);
}

#[test]
fn annual_inputs_are_snapshotted_before_settlement_propagation() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("first", 0.0, annual("source", 2.0)),
        definition("intermediate", 5.0, immediate("first", 1.0)),
        definition("second", 0.0, annual("intermediate", 3.0)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "intermediate", 25.0);
    assert_value(&mut app, "second", 15.0);
}

#[test]
fn annual_settlement_occurs_only_when_month_twelve_wraps() {
    let mut app = app_with(vec![
        definition("source", 20.0, None),
        definition("annual", 0.0, annual("source", 2.0)),
    ]);

    advance_months(&mut app, 11);
    assert_date(&app, 1, 12);
    assert_value(&mut app, "annual", 0.0);

    advance_months(&mut app, 1);
    assert_date(&app, 2, 1);
    assert_value(&mut app, "annual", 40.0);

    advance_months(&mut app, 1);
    assert_date(&app, 2, 2);
}

#[test]
fn duplicate_metric_ids_are_rejected_before_startup() {
    let result = MetricPlugin::new(vec![
        definition("duplicate", 0.0, None),
        definition("duplicate", 1.0, None),
    ]);

    assert!(matches!(result, Err(MetricError::DuplicateId(id)) if id == "duplicate"));
}

#[test]
fn missing_immediate_source_is_rejected_before_startup() {
    let result = MetricPlugin::new(vec![definition("target", 0.0, immediate("missing", 1.0))]);

    assert!(matches!(
        result,
        Err(MetricError::MissingSource { target, source })
            if target == "target" && source == "missing"
    ));
}

#[test]
fn missing_annual_source_is_rejected_before_startup() {
    let result = MetricPlugin::new(vec![definition("target", 0.0, annual("missing", 1.0))]);

    assert!(matches!(
        result,
        Err(MetricError::MissingSource { target, source })
            if target == "target" && source == "missing"
    ));
}

#[test]
fn immediate_cycle_is_rejected_before_startup() {
    let result = MetricPlugin::new(vec![
        definition("left", 0.0, immediate("right", 1.0)),
        definition("right", 0.0, immediate("left", 1.0)),
    ]);

    assert!(matches!(result, Err(MetricError::ImmediateCycle(_))));
}

#[test]
fn immediate_self_dependency_is_rejected_before_startup() {
    let result = MetricPlugin::new(vec![definition("self", 0.0, immediate("self", 1.0))]);

    assert!(matches!(result, Err(MetricError::ImmediateCycle(_))));
}

#[test]
fn collapse_at_one_hundred_resets_every_metric_and_the_date() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("annual", 0.0, annual("source", 2.0)),
        definition("target", 7.0, immediate("annual", 0.5)),
        bounded(definition(COLLAPSE_METRIC_ID, 0.0, None), 0.0, 100.0),
    ]);
    change(&mut app, "source", 5.0);
    advance_months(&mut app, 12);
    assert_value(&mut app, "target", 22.0);
    assert_date(&app, 2, 1);

    change(&mut app, COLLAPSE_METRIC_ID, 99.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 99.0);
    assert_date(&app, 2, 1);

    change(&mut app, COLLAPSE_METRIC_ID, 1.0);

    assert_value(&mut app, "source", 10.0);
    assert_value(&mut app, "annual", 0.0);
    assert_value(&mut app, "target", 7.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_date(&app, 1, 1);
}

#[test]
fn collapse_triggered_by_immediate_propagation_resets_the_run() {
    let mut app = app_with(vec![
        definition("source", 0.0, None),
        bounded(
            definition(COLLAPSE_METRIC_ID, 0.0, immediate("source", 10.0)),
            0.0,
            100.0,
        ),
        definition("target", 5.0, immediate(COLLAPSE_METRIC_ID, 2.0)),
    ]);

    change(&mut app, "source", 15.0);

    assert_value(&mut app, "source", 0.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_value(&mut app, "target", 5.0);
    assert_date(&app, 1, 1);
}

#[test]
fn collapse_triggered_by_annual_settlement_resets_the_run() {
    let mut app = app_with(vec![
        definition("source", 50.0, None),
        bounded(
            definition(COLLAPSE_METRIC_ID, 0.0, annual("source", 2.0)),
            0.0,
            100.0,
        ),
        definition("target", 7.0, immediate(COLLAPSE_METRIC_ID, 0.5)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "source", 50.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_value(&mut app, "target", 7.0);
    assert_date(&app, 1, 1);
}

#[test]
fn collapse_triggered_by_an_annual_delta_stops_remaining_settlement_work() {
    let mut app = app_with(vec![
        definition("source", 50.0, None),
        definition("annual", 0.0, annual("source", 1.0)),
        bounded(
            definition(COLLAPSE_METRIC_ID, 0.0, immediate("annual", 2.0)),
            0.0,
            100.0,
        ),
        definition("later_annual", 7.0, annual("source", 3.0)),
        definition("target", 5.0, immediate("later_annual", 1.0)),
    ]);

    advance_months(&mut app, 12);

    assert_value(&mut app, "source", 50.0);
    assert_value(&mut app, "annual", 0.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_value(&mut app, "later_annual", 7.0);
    assert_value(&mut app, "target", 5.0);
    assert_date(&app, 1, 1);
}

#[test]
fn collapse_discards_remaining_actions_from_the_previous_run() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        bounded(definition(COLLAPSE_METRIC_ID, 0.0, None), 0.0, 100.0),
    ]);

    act(
        &mut app,
        [
            GameAction::ChangeMetric {
                id: "source".to_owned(),
                delta: 5.0,
            },
            GameAction::ChangeMetric {
                id: COLLAPSE_METRIC_ID.to_owned(),
                delta: 100.0,
            },
            GameAction::ChangeMetric {
                id: "source".to_owned(),
                delta: 50.0,
            },
            GameAction::NextMonth,
        ],
    );

    assert_value(&mut app, "source", 10.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_date(&app, 1, 1);
    assert!(app.world().resource::<PendingActions>().0.is_empty());
}

#[test]
fn ron_supports_omitted_optional_fields_and_both_influence_kinds() {
    let definitions: Vec<MetricDefinition> = ron::from_str(
        r#"[
            (
                id: "source",
                name: "来源",
                description: "无需可选字段",
                initial_value: 10.0,
            ),
            (
                id: "immediate",
                name: "即时",
                description: "包含边界",
                initial_value: 0.0,
                min_value: Some(0.0),
                max_value: Some(100.0),
                influence: Some(Immediate([
                    (source_metric: "source", factor: 2.0),
                ])),
            ),
            (
                id: "annual",
                name: "年度",
                description: "年末重算",
                initial_value: 0.0,
                influence: Some(Annual([
                    (source_metric: "immediate", factor: 3.0),
                ])),
            ),
        ]"#,
    )
    .expect("RON definitions deserialize");
    assert!(definitions[0].min_value.is_none());
    assert!(definitions[0].max_value.is_none());
    assert!(definitions[0].influence.is_none());
    let mut app = app_with(definitions);

    change(&mut app, "source", 2.0);
    advance_months(&mut app, 12);

    assert_value(&mut app, "immediate", 4.0);
    assert_value(&mut app, "annual", 12.0);
}

#[test]
fn repository_ron_asset_loads_and_runs_the_example_loop() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("assets/metrics.ron");
    let plugin = MetricPlugin::from_path(path).expect("repository RON asset is valid");
    let mut app = App::new();
    app.add_plugins(MinimalPlugins).add_plugins(plugin);
    app.update();

    assert_value(&mut app, "productivity", 10.0);
    assert_value(&mut app, "output", 20.0);
    change(&mut app, "productivity", 5.0);
    assert_value(&mut app, "output", 30.0);
    advance_months(&mut app, 12);
    assert_value(&mut app, "income", 45.0);
    assert_value(&mut app, "reserves", 72.5);
    assert_date(&app, 2, 1);
}
