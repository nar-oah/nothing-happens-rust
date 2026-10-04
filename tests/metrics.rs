use std::{
    path::Path,
    time::{Duration, Instant},
};

use bevy::{
    asset::{
        AssetApp, AssetPlugin, LoadState,
        io::{
            AssetSourceBuilder, AssetSourceId,
            memory::{Dir, MemoryAssetReader},
        },
    },
    prelude::*,
};
use nothing_happens::metrics::{
    AnnualInfluence, COLLAPSE_METRIC_ID, CollapseMetric, ImmediateInfluence, Influence,
    InfluenceTerm, InitialValue, MONTH_METRIC_ID, Metric, MetricBounds, MetricCatalog,
    MetricChange, MetricDefinition, MetricError, MetricId, MetricMetadata, MetricOrder,
    MetricPlugin, MetricReady, MetricValue, PendingMetricChanges, PendingMonthAdvances,
    SimulationSet, TERM_METRIC_ID, YEAR_METRIC_ID, validate_definitions,
};

const METRIC_CATALOG_PATH: &str = "data/metrics.metric.ron";

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

fn catalog_app(catalog: &str) -> App {
    let dir = Dir::default();
    dir.insert_asset_text(Path::new(METRIC_CATALOG_PATH), catalog);

    let mut app = App::new();
    app.add_plugins(MinimalPlugins)
        .register_asset_source(
            AssetSourceId::Default,
            AssetSourceBuilder::new(move || Box::new(MemoryAssetReader { root: dir.clone() })),
        )
        .add_plugins((AssetPlugin::default(), MetricPlugin));
    app
}

fn update_until(app: &mut App, ready: impl Fn(&World) -> bool) {
    let deadline = Instant::now() + Duration::from_secs(5);
    loop {
        app.update();
        if ready(app.world()) {
            return;
        }
        assert!(
            Instant::now() < deadline,
            "metric catalog did not load in time"
        );
        std::thread::yield_now();
    }
}

fn app_with_catalog(catalog: &str) -> App {
    let mut app = catalog_app(catalog);
    update_until(&mut app, |world| world.contains_resource::<MetricReady>());
    app
}

fn app_with(mut definitions: Vec<MetricDefinition>) -> App {
    for (id, max) in [
        (YEAR_METRIC_ID, None),
        (MONTH_METRIC_ID, Some(12.0)),
        (TERM_METRIC_ID, None),
    ] {
        if !definitions.iter().any(|metric| metric.id == id) {
            let mut metric = definition(id, 1.0, None);
            metric.min_value = Some(1.0);
            metric.max_value = max;
            definitions.push(metric);
        }
    }
    let catalog = ron::to_string(&MetricCatalog {
        metrics: definitions,
    })
    .expect("metric catalog serializes");
    app_with_catalog(&catalog)
}

fn queue_change(app: &mut App, id: &str, delta: f64) {
    let target = entity(app, id);
    app.world_mut()
        .resource_mut::<PendingMetricChanges>()
        .0
        .push_back(MetricChange { target, delta });
}

fn change(app: &mut App, id: &str, delta: f64) {
    queue_change(app, id, delta);
    app.update();
}

fn advance_months(app: &mut App, months: usize) {
    app.world_mut().resource_mut::<PendingMonthAdvances>().0 += months;
    app.update();
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

fn assert_date(app: &mut App, year: u32, month: u8) {
    assert_value(app, YEAR_METRIC_ID, f64::from(year));
    assert_value(app, MONTH_METRIC_ID, f64::from(month));
}

#[test]
fn startup_spawns_each_metric_without_propagating_initial_values() {
    let mut app = app_with(vec![
        definition("source", 50.0, None),
        definition("target", 3.0, immediate("source", 2.0)),
    ]);

    assert_date(&mut app, 1, 1);
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
        [
            ("source".to_owned(), 0),
            ("target".to_owned(), 1),
            (YEAR_METRIC_ID.to_owned(), 2),
            (MONTH_METRIC_ID.to_owned(), 3),
            (TERM_METRIC_ID.to_owned(), 4),
        ]
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
    let mut query = world.query_filtered::<(
        &MetricId,
        &MetricMetadata,
        &MetricValue,
        &InitialValue,
        &MetricBounds,
        &MetricOrder,
        Has<ImmediateInfluence>,
        Has<AnnualInfluence>,
        Has<CollapseMetric>,
    ), With<Metric>>();
    let mut metrics = Vec::new();
    for (id, metadata, value, initial, bounds, order, immediate, annual, collapse) in
        query.iter(world)
    {
        assert_eq!(metadata.name, id.0);
        assert_eq!(metadata.description, format!("Test metric {}", id.0));
        assert_eq!(value.0, initial.0);
        if id.0 == "source" || id.0 == COLLAPSE_METRIC_ID {
            assert_eq!((bounds.min, bounds.max), (Some(0.0), Some(100.0)));
        } else if id.0 == MONTH_METRIC_ID {
            assert_eq!((bounds.min, bounds.max), (Some(1.0), Some(12.0)));
        } else if id.0 == YEAR_METRIC_ID || id.0 == TERM_METRIC_ID {
            assert_eq!((bounds.min, bounds.max), (Some(1.0), None));
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
            (YEAR_METRIC_ID.to_owned(), 4, false, false, false),
            (MONTH_METRIC_ID.to_owned(), 5, false, false, false),
            (TERM_METRIC_ID.to_owned(), 6, false, false, false),
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

#[derive(Resource, Default)]
struct MetricRequests(Vec<(&'static str, f64)>);

fn submit_metric_requests(
    metrics: Query<(Entity, &MetricId), With<Metric>>,
    mut requests: ResMut<MetricRequests>,
    mut pending: ResMut<PendingMetricChanges>,
) {
    for (id, delta) in requests.0.drain(..) {
        let target = metrics
            .iter()
            .find_map(|(entity, metric_id)| (metric_id.0 == id).then_some(entity))
            .unwrap();
        pending.0.push_back(MetricChange { target, delta });
    }
}

#[test]
fn runtime_system_requests_and_existing_queue_propagate_once_in_the_same_frame() {
    let mut app = app_with(vec![
        bounded(definition("source", 10.0, None), 0.0, 12.0),
        definition("middle", 5.0, immediate("source", 2.0)),
        definition("target", 1.0, immediate("middle", 3.0)),
    ]);
    app.insert_resource(MetricRequests(vec![("source", 20.0), ("middle", 4.0)]))
        .add_systems(
            Update,
            submit_metric_requests.before(SimulationSet::ApplyChanges),
        );

    change(&mut app, "source", 1.0);

    assert_value(&mut app, "source", 12.0);
    assert_value(&mut app, "middle", 13.0);
    assert_value(&mut app, "target", 25.0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
    app.update();
    assert_value(&mut app, "target", 25.0);
}

#[test]
fn propagated_deltas_are_appended_after_existing_requests_in_the_same_fifo() {
    let mut app = app_with(vec![
        definition("source", 0.0, None),
        bounded(
            definition("target", 10.0, immediate("source", 1.0)),
            0.0,
            10.0,
        ),
    ]);

    queue_change(&mut app, "source", 5.0);
    queue_change(&mut app, "target", -5.0);
    app.update();

    assert_value(&mut app, "source", 5.0);
    assert_value(&mut app, "target", 10.0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
}

#[test]
fn invalid_requests_are_discarded_without_blocking_valid_changes() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        definition("target", 5.0, immediate("source", 2.0)),
    ]);
    let unknown_target = app.world_mut().spawn_empty().id();
    app.world_mut()
        .resource_mut::<PendingMetricChanges>()
        .0
        .push_back(MetricChange {
            target: unknown_target,
            delta: 1.0,
        });
    queue_change(&mut app, "source", f64::NAN);
    queue_change(&mut app, "source", f64::INFINITY);
    queue_change(&mut app, "source", 3.0);
    app.update();

    assert_value(&mut app, "source", 13.0);
    assert_value(&mut app, "target", 11.0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
}

#[test]
fn time_metrics_use_bounds_and_immediate_propagation() {
    let mut app = app_with(vec![
        definition("year_target", 0.0, immediate(YEAR_METRIC_ID, 2.0)),
        definition("month_target", 0.0, immediate(MONTH_METRIC_ID, 3.0)),
        definition("annual", 0.0, annual(YEAR_METRIC_ID, 10.0)),
    ]);

    change(&mut app, YEAR_METRIC_ID, 1.0);
    change(&mut app, MONTH_METRIC_ID, 100.0);
    assert_date(&mut app, 2, 12);
    assert_value(&mut app, "year_target", 2.0);
    assert_value(&mut app, "month_target", 33.0);
    assert_value(&mut app, "annual", 0.0);
    change(&mut app, MONTH_METRIC_ID, 5.0);
    assert_value(&mut app, "month_target", 33.0);

    advance_months(&mut app, 1);
    assert_date(&mut app, 3, 1);
    assert_value(&mut app, "year_target", 4.0);
    assert_value(&mut app, "month_target", 0.0);
    assert_value(&mut app, "annual", 30.0);

    advance_months(&mut app, 1);
    assert_date(&mut app, 3, 2);
    assert_value(&mut app, "month_target", 3.0);
}

#[test]
fn batched_month_requests_settle_every_crossed_year() {
    let mut app = app_with(vec![
        definition("annual", 0.0, annual(YEAR_METRIC_ID, 10.0)),
        definition("target", 5.0, immediate("annual", 2.0)),
    ]);

    advance_months(&mut app, 24);

    assert_date(&mut app, 3, 1);
    assert_value(&mut app, "annual", 30.0);
    assert_value(&mut app, "target", 65.0);
    assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
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
    assert_date(&mut app, 2, 1);
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
fn annual_source_is_rejected_during_validation() {
    let result = validate_definitions(&[
        definition("source", 10.0, None),
        definition("first", 7.0, annual("source", 2.0)),
        definition("second", 2.0, annual("first", 3.0)),
    ]);

    assert!(matches!(
        result,
        Err(MetricError::AnnualSource { target, source })
            if target == "second" && source == "first"
    ));
}

#[test]
fn annual_source_is_rejected_even_when_declared_after_its_target() {
    let result = validate_definitions(&[
        definition("second", 2.0, annual("first", 3.0)),
        definition("first", 7.0, annual("source", 2.0)),
        definition("source", 10.0, None),
    ]);

    assert!(matches!(
        result,
        Err(MetricError::AnnualSource { target, source })
            if target == "second" && source == "first"
    ));
}

#[test]
fn annual_self_dependency_is_rejected_during_validation() {
    let result = validate_definitions(&[definition("self", 2.0, annual("self", 2.0))]);

    assert!(matches!(
        result,
        Err(MetricError::AnnualSource { target, source })
            if target == "self" && source == "self"
    ));
}

#[test]
fn annual_cycle_is_rejected_during_validation() {
    let result = validate_definitions(&[
        definition("first", 2.0, annual("third", 2.0)),
        definition("second", 3.0, annual("first", 3.0)),
        definition("third", 4.0, annual("second", 4.0)),
    ]);

    assert!(matches!(result, Err(MetricError::AnnualSource { .. })));
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
    assert_date(&mut app, 1, 12);
    assert_value(&mut app, "annual", 0.0);

    advance_months(&mut app, 1);
    assert_date(&mut app, 2, 1);
    assert_value(&mut app, "annual", 40.0);

    advance_months(&mut app, 1);
    assert_date(&mut app, 2, 2);
}

#[test]
fn duplicate_metric_ids_are_rejected_during_validation() {
    let result = validate_definitions(&[
        definition("duplicate", 0.0, None),
        definition("duplicate", 1.0, None),
    ]);

    assert!(matches!(result, Err(MetricError::DuplicateId(id)) if id == "duplicate"));
}

#[test]
fn missing_immediate_source_is_rejected_during_validation() {
    let result = validate_definitions(&[definition("target", 0.0, immediate("missing", 1.0))]);

    assert!(matches!(
        result,
        Err(MetricError::MissingSource { target, source })
            if target == "target" && source == "missing"
    ));
}

#[test]
fn missing_annual_source_is_rejected_during_validation() {
    let result = validate_definitions(&[definition("target", 0.0, annual("missing", 1.0))]);

    assert!(matches!(
        result,
        Err(MetricError::MissingSource { target, source })
            if target == "target" && source == "missing"
    ));
}

#[test]
fn immediate_cycle_is_rejected_during_validation() {
    let result = validate_definitions(&[
        definition("left", 0.0, immediate("right", 1.0)),
        definition("right", 0.0, immediate("left", 1.0)),
    ]);

    assert!(matches!(result, Err(MetricError::ImmediateCycle(_))));
}

#[test]
fn immediate_self_dependency_is_rejected_during_validation() {
    let result = validate_definitions(&[definition("self", 0.0, immediate("self", 1.0))]);

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
    assert_date(&mut app, 2, 1);

    change(&mut app, COLLAPSE_METRIC_ID, 99.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 99.0);
    assert_date(&mut app, 2, 1);

    change(&mut app, COLLAPSE_METRIC_ID, 1.0);

    assert_value(&mut app, "source", 10.0);
    assert_value(&mut app, "annual", 0.0);
    assert_value(&mut app, "target", 7.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
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
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
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
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
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
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
}

#[test]
fn collapse_discards_pending_changes_and_month_requests_from_the_previous_run() {
    let mut app = app_with(vec![
        definition("source", 10.0, None),
        bounded(definition(COLLAPSE_METRIC_ID, 0.0, None), 0.0, 100.0),
    ]);

    queue_change(&mut app, "source", 5.0);
    queue_change(&mut app, COLLAPSE_METRIC_ID, 100.0);
    queue_change(&mut app, "source", 50.0);
    app.world_mut().resource_mut::<PendingMonthAdvances>().0 = 3;
    app.update();

    assert_value(&mut app, "source", 10.0);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
    assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 0);
    app.update();
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
}

#[test]
fn collapse_increments_current_term_and_resets_time_to_one() {
    let mut app = app_with(vec![
        definition(YEAR_METRIC_ID, 5.0, None),
        definition(MONTH_METRIC_ID, 8.0, None),
        definition(TERM_METRIC_ID, 1.0, None),
        definition("source", 10.0, None),
        definition("target", 7.0, immediate("source", 2.0)),
        bounded(definition(COLLAPSE_METRIC_ID, 0.0, None), 0.0, 100.0),
    ]);
    change(&mut app, TERM_METRIC_ID, 5.0);
    change(&mut app, "source", 5.0);
    change(&mut app, COLLAPSE_METRIC_ID, 100.0);

    assert_value(&mut app, "source", 10.0);
    assert_value(&mut app, "target", 7.0);
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 7.0);

    change(&mut app, COLLAPSE_METRIC_ID, 100.0);
    assert_value(&mut app, TERM_METRIC_ID, 8.0);
    assert_date(&mut app, 1, 1);
}

#[test]
fn collapse_from_time_propagation_cancels_remaining_month_requests() {
    let mut app = app_with(vec![bounded(
        definition(COLLAPSE_METRIC_ID, 0.0, immediate(MONTH_METRIC_ID, 100.0)),
        0.0,
        100.0,
    )]);

    advance_months(&mut app, 24);

    assert_date(&mut app, 1, 1);
    assert_value(&mut app, COLLAPSE_METRIC_ID, 0.0);
    assert_value(&mut app, TERM_METRIC_ID, 2.0);
    assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 0);
    assert!(app.world().resource::<PendingMetricChanges>().0.is_empty());
}

#[test]
fn ron_supports_omitted_optional_fields_and_both_influence_kinds() {
    let catalog_ron = r#"(metrics: [
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
            (id: "year", name: "年份", description: "时间", initial_value: 1.0),
            (id: "month", name: "月份", description: "时间", initial_value: 1.0),
            (id: "term", name: "任期", description: "局次", initial_value: 1.0),
        ])"#;
    let catalog: MetricCatalog = ron::from_str(catalog_ron).expect("RON catalog deserializes");
    assert!(catalog.metrics[0].min_value.is_none());
    assert!(catalog.metrics[0].max_value.is_none());
    assert!(catalog.metrics[0].influence.is_none());
    let mut app = app_with_catalog(catalog_ron);

    change(&mut app, "source", 2.0);
    advance_months(&mut app, 12);

    assert_value(&mut app, "immediate", 4.0);
    assert_value(&mut app, "annual", 12.0);
}

#[test]
fn repository_metric_catalog_parses_and_runs_the_example_loop() {
    let mut app = app_with_catalog(include_str!("../assets/data/metrics.metric.ron"));

    assert_value(&mut app, "productivity", 10.0);
    assert_value(&mut app, "output", 20.0);
    assert_date(&mut app, 1, 1);
    assert_value(&mut app, TERM_METRIC_ID, 1.0);
    change(&mut app, "productivity", 5.0);
    assert_value(&mut app, "output", 30.0);
    advance_months(&mut app, 12);
    assert_value(&mut app, "income", 45.0);
    assert_value(&mut app, "reserves", 72.5);
    assert_date(&mut app, 2, 1);
}

#[test]
fn requests_wait_for_asset_loading_before_advancing_time() {
    let mut app = catalog_app(include_str!("../assets/data/metrics.metric.ron"));
    app.world_mut().resource_mut::<PendingMonthAdvances>().0 = 12;

    update_until(&mut app, |world| world.contains_resource::<MetricReady>());

    assert_date(&mut app, 2, 1);
    assert_value(&mut app, "income", 30.0);
    assert_value(&mut app, "reserves", 65.0);
    assert_value(&mut app, TERM_METRIC_ID, 1.0);
    assert_eq!(app.world().resource::<PendingMonthAdvances>().0, 0);
}

#[test]
fn invalid_metric_catalog_fails_to_load_without_spawning_metrics() {
    let catalog = ron::to_string(&MetricCatalog {
        metrics: vec![
            definition("duplicate", 0.0, None),
            definition("duplicate", 1.0, None),
        ],
    })
    .unwrap();
    let mut app = catalog_app(&catalog);
    let handle: Handle<MetricCatalog> = app
        .world()
        .resource::<AssetServer>()
        .load(METRIC_CATALOG_PATH);
    update_until(&mut app, |world| {
        world
            .resource::<AssetServer>()
            .load_state(handle.id())
            .is_failed()
    });

    let LoadState::Failed(error) = app
        .world()
        .resource::<AssetServer>()
        .load_state(handle.id())
    else {
        panic!("invalid metric catalog must fail to load");
    };
    assert!(error.to_string().contains("duplicate metric id: duplicate"));
    assert!(!app.world().contains_resource::<MetricReady>());
    let world = app.world_mut();
    assert_eq!(
        world
            .query_filtered::<Entity, With<Metric>>()
            .iter(world)
            .count(),
        0
    );
}
