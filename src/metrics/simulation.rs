use std::collections::VecDeque;

use bevy::prelude::*;

use super::{MONTH_METRIC_ID, MetricError, TERM_METRIC_ID, YEAR_METRIC_ID, components::*};

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct MetricChange {
    pub target: Entity,
    pub delta: f64,
}

#[derive(Resource, Debug, Default)]
pub struct PendingMetricChanges(pub VecDeque<MetricChange>);

/// Time requests are separate from metric deltas.
#[derive(Resource, Debug, Default)]
pub struct PendingMonthAdvances(pub usize);

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    ApplyChanges,
    AdvanceTime,
}

pub(crate) fn configure_simulation(app: &mut App) {
    app.init_resource::<PendingMetricChanges>()
        .init_resource::<PendingMonthAdvances>()
        .configure_sets(
            Update,
            (SimulationSet::ApplyChanges, SimulationSet::AdvanceTime).chain(),
        )
        .add_systems(
            Update,
            (
                apply_pending_metric_changes.in_set(SimulationSet::ApplyChanges),
                advance_time.in_set(SimulationSet::AdvanceTime),
            )
                .run_if(resource_exists::<MetricReady>),
        );
}

type MetricValues<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static MetricId,
        &'static MetricBounds,
        &'static InitialValue,
        Option<&'static CollapseMetric>,
        &'static mut MetricValue,
    ),
    With<Metric>,
>;

type ImmediateInfluences<'w, 's> =
    Query<'w, 's, (Entity, &'static MetricOrder, &'static ImmediateInfluence), With<Metric>>;

fn apply_pending_metric_changes(
    mut values: MetricValues,
    influences: ImmediateInfluences,
    mut pending: ResMut<PendingMetricChanges>,
    mut months: ResMut<PendingMonthAdvances>,
) {
    process_metric_changes(&mut values, &influences, &mut pending, &mut months);
}

/// Consume requested and propagated deltas from the same FIFO queue.
/// Return true when collapse resets the run and discards its pending requests.
fn process_metric_changes(
    values: &mut MetricValues,
    influences: &ImmediateInfluences,
    pending: &mut PendingMetricChanges,
    months: &mut PendingMonthAdvances,
) -> bool {
    while let Some(change) = pending.0.pop_front() {
        let (actual_delta, collapsed) = match apply_metric_change(values, change) {
            Ok(result) => result,
            Err(error) => {
                warn!("Metric change failed: {error}");
                continue;
            }
        };
        if collapsed {
            restart_game(values);
            pending.0.clear();
            months.0 = 0;
            return true;
        }
        if actual_delta == 0.0 {
            continue;
        }

        let mut targets: Vec<_> = influences.iter().collect();
        targets.sort_by_key(|(_, order, _)| order.0);
        for (target, _, immediate) in targets {
            for input in &immediate.0 {
                if input.source == change.target {
                    pending.0.push_back(MetricChange {
                        target,
                        delta: actual_delta * input.factor,
                    });
                }
            }
        }
    }
    false
}

fn apply_metric_change(
    values: &mut MetricValues,
    change: MetricChange,
) -> Result<(f64, bool), MetricError> {
    if !change.delta.is_finite() {
        return Err(MetricError::NonFiniteDelta);
    }
    let (_, id, bounds, _, collapse, mut value) = values
        .get_mut(change.target)
        .map_err(|_| MetricError::SimulationNotReady)?;
    let new = bounded_value(bounds, &id.0, value.0 + change.delta)?;
    let actual_delta = checked_delta(&id.0, new, value.0)?;
    if actual_delta != 0.0 {
        value.0 = new;
    }
    Ok((actual_delta, collapse.is_some() && new >= 100.0))
}

fn advance_time(
    mut values: MetricValues,
    immediate: ImmediateInfluences,
    annual: Query<(Entity, &MetricOrder, &AnnualInfluence), With<Metric>>,
    mut pending: ResMut<PendingMetricChanges>,
    mut months: ResMut<PendingMonthAdvances>,
) {
    while months.0 > 0 {
        months.0 -= 1;
        let Some((month, current)) = find_metric(&values, MONTH_METRIC_ID) else {
            warn!("Month metric is unavailable");
            months.0 = 0;
            return;
        };
        let new_year = current + 1.0 > 12.0;
        if new_year {
            let Some((year, _)) = find_metric(&values, YEAR_METRIC_ID) else {
                warn!("Year metric is unavailable");
                months.0 = 0;
                return;
            };
            pending.0.push_back(MetricChange {
                target: month,
                delta: 1.0 - current,
            });
            pending.0.push_back(MetricChange {
                target: year,
                delta: 1.0,
            });
        } else {
            pending.0.push_back(MetricChange {
                target: month,
                delta: 1.0,
            });
        }
        if process_metric_changes(&mut values, &immediate, &mut pending, &mut months) {
            return;
        }
        if new_year {
            if let Err(error) = enqueue_annual_changes(&values, &annual, &mut pending) {
                warn!("Annual settlement failed: {error}");
            }
            if process_metric_changes(&mut values, &immediate, &mut pending, &mut months) {
                return;
            }
        }
    }
}

fn find_metric(values: &MetricValues, id: &str) -> Option<(Entity, f64)> {
    values
        .iter()
        .find_map(|(entity, metric_id, _, _, _, value)| {
            (metric_id.0 == id).then_some((entity, value.0))
        })
}

fn enqueue_annual_changes(
    values: &MetricValues,
    influences: &Query<(Entity, &MetricOrder, &AnnualInfluence), With<Metric>>,
    pending: &mut PendingMetricChanges,
) -> Result<(), MetricError> {
    // Snapshot all Annual inputs before applying any settlement delta.
    let mut changes = Vec::new();
    for (target, order, annual) in influences.iter() {
        let mut new_value = 0.0;
        for input in &annual.0 {
            let (_, _, _, _, _, source) = values
                .get(input.source)
                .map_err(|_| MetricError::SimulationNotReady)?;
            new_value += source.0 * input.factor;
        }
        let (_, id, _, _, _, current) = values
            .get(target)
            .map_err(|_| MetricError::SimulationNotReady)?;
        changes.push((
            order.0,
            MetricChange {
                target,
                delta: checked_delta(&id.0, new_value, current.0)?,
            },
        ));
    }
    changes.sort_by_key(|(order, _)| *order);
    pending
        .0
        .extend(changes.into_iter().map(|(_, change)| change));
    Ok(())
}

fn bounded_value(bounds: &MetricBounds, id: &str, mut value: f64) -> Result<f64, MetricError> {
    // f64::max/min ignore NaN; reject it before applying valid bounds.
    if value.is_nan() {
        return Err(MetricError::InvalidValue { id: id.into() });
    }
    if let Some(min) = bounds.min {
        value = value.max(min);
    }
    if let Some(max) = bounds.max {
        value = value.min(max);
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err(MetricError::InvalidValue { id: id.into() })
    }
}

fn checked_delta(id: &str, new: f64, old: f64) -> Result<f64, MetricError> {
    let delta = new - old;
    if delta.is_finite() {
        Ok(delta)
    } else {
        Err(MetricError::InvalidValue { id: id.into() })
    }
}

fn restart_game(values: &mut MetricValues) {
    // Restore initial state without propagating reset deltas.
    for (_, id, _, initial, _, mut value) in values.iter_mut() {
        value.0 = match id.0.as_str() {
            TERM_METRIC_ID => value.0 + 1.0,
            YEAR_METRIC_ID | MONTH_METRIC_ID => 1.0,
            _ => initial.0,
        };
    }
}
