use std::collections::VecDeque;

use bevy::prelude::*;

use super::{MetricError, components::*};

#[derive(Resource, Clone, Copy, Debug, PartialEq, Eq)]
pub struct GameDate {
    pub year: u32,
    pub month: u8,
}

impl Default for GameDate {
    fn default() -> Self {
        Self { year: 1, month: 1 }
    }
}

#[derive(Clone, Debug, PartialEq)]
pub enum GameAction {
    ChangeMetric { id: String, delta: f64 },
    NextMonth,
}

#[derive(Resource, Debug, Default)]
pub struct PendingActions(pub VecDeque<GameAction>);

#[derive(Resource, Debug)]
pub struct GameStatus {
    pub restarts: u32,
    pub last_action: String,
}

impl Default for GameStatus {
    fn default() -> Self {
        Self {
            restarts: 0,
            last_action: "New game".into(),
        }
    }
}

/// External MetricValue writers must run before ObserveChanges so their actual
/// deltas are reconciled before queued actions or Immediate propagation.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    ObserveChanges,
    ApplyActions,
}

pub(crate) fn configure_simulation(app: &mut App) {
    app.init_resource::<GameDate>()
        .init_resource::<GameStatus>()
        .init_resource::<PendingActions>()
        .configure_sets(
            Update,
            (SimulationSet::ObserveChanges, SimulationSet::ApplyActions).chain(),
        )
        .add_systems(
            Update,
            (
                observe_metric_changes.in_set(SimulationSet::ObserveChanges),
                apply_pending_actions.in_set(SimulationSet::ApplyActions),
            ),
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
        &'static mut PreviousValue,
    ),
    With<Metric>,
>;

type MetricInfluences<'w, 's> = Query<
    'w,
    's,
    (
        Entity,
        &'static MetricOrder,
        Option<&'static ImmediateInfluence>,
        Option<&'static AnnualInfluence>,
    ),
    With<Metric>,
>;

fn observe_metric_changes(
    mut values: MetricValues,
    influences: MetricInfluences,
    mut date: ResMut<GameDate>,
    mut pending: ResMut<PendingActions>,
    mut status: ResMut<GameStatus>,
) {
    let mut changed: Vec<(usize, Entity)> = values
        .iter_mut()
        .filter_map(|(entity, _, _, _, _, value, _)| {
            value.is_changed().then(|| {
                let (_, order, _, _) = influences.get(entity).unwrap();
                (order.0, entity)
            })
        })
        .collect();
    changed.sort_by_key(|&(order, _)| order);

    // Reconcile every external write before propagating. Otherwise an entity
    // directly changed alongside one of its sources could propagate twice.
    let mut deltas = VecDeque::new();
    for (_, entity) in changed {
        let (_, id, bounds, _, collapse, mut value, mut previous) = values.get_mut(entity).unwrap();
        let result = bounded_value(bounds, &id.0, value.0)
            .and_then(|new| checked_delta(&id.0, new, previous.0).map(|delta| (new, delta)));
        match result {
            Ok((new, delta)) => {
                if value.0 != new {
                    value.0 = new;
                }
                if previous.0 != new {
                    previous.0 = new;
                }
                if collapse.is_some() && new >= 100.0 {
                    restart_game(&mut values, &mut date, &mut pending, &mut status);
                    return;
                }
                if delta != 0.0 {
                    deltas.push_back((entity, delta));
                }
            }
            Err(error) => {
                // An invalid external write does not replace the last valid
                // value or send a non-finite delta through the graph.
                if value.0 != previous.0 {
                    value.0 = previous.0;
                }
                status.last_action = format!("Action failed: {error}");
            }
        }
    }
    match propagate(&mut values, &influences, deltas) {
        Ok(true) => restart_game(&mut values, &mut date, &mut pending, &mut status),
        Ok(false) => {}
        Err(error) => status.last_action = format!("Action failed: {error}"),
    }
}

fn apply_pending_actions(
    mut values: MetricValues,
    influences: MetricInfluences,
    mut date: ResMut<GameDate>,
    mut pending: ResMut<PendingActions>,
    mut status: ResMut<GameStatus>,
) {
    while let Some(action) = pending.0.pop_front() {
        match execute_action(action, &mut values, &influences, &mut date) {
            Ok((true, _)) => {
                restart_game(&mut values, &mut date, &mut pending, &mut status);
                break;
            }
            Ok((false, message)) => status.last_action = message,
            Err(error) => status.last_action = format!("Action failed: {error}"),
        }
    }
}

fn execute_action(
    action: GameAction,
    values: &mut MetricValues,
    influences: &MetricInfluences,
    date: &mut GameDate,
) -> Result<(bool, String), MetricError> {
    match action {
        GameAction::ChangeMetric { id, delta } => {
            if !delta.is_finite() {
                return Err(MetricError::NonFiniteDelta);
            }
            let (entity, current) = values
                .iter()
                .find_map(|(entity, metric_id, _, _, _, value, _)| {
                    (metric_id.0 == id).then_some((entity, value.0))
                })
                .ok_or_else(|| MetricError::UnknownMetric(id.clone()))?;
            let (actual_delta, collapsed) = write_value(values, entity, current + delta)?;
            let collapsed = collapsed
                || propagate(values, influences, VecDeque::from([(entity, actual_delta)]))?;
            Ok((collapsed, format!("{id}: {actual_delta:+.2}")))
        }
        GameAction::NextMonth => {
            if date.month < 12 {
                date.month += 1;
                Ok((false, "Advanced one month".into()))
            } else {
                let year = date.year.checked_add(1).ok_or(MetricError::YearOverflow)?;
                let collapsed = settle_annual(values, influences)?;
                date.year = year;
                date.month = 1;
                Ok((collapsed, "Annual settlement complete".into()))
            }
        }
    }
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

fn write_value(
    values: &mut MetricValues,
    entity: Entity,
    requested: f64,
) -> Result<(f64, bool), MetricError> {
    let (_, id, bounds, _, collapse, mut value, mut previous) = values
        .get_mut(entity)
        .map_err(|_| MetricError::SimulationNotReady)?;
    let new = bounded_value(bounds, &id.0, requested)?;
    let delta = checked_delta(&id.0, new, value.0)?;
    if value.0 != new {
        value.0 = new;
    }
    if previous.0 != new {
        previous.0 = new;
    }
    Ok((delta, collapse.is_some() && new >= 100.0))
}

fn propagate(
    values: &mut MetricValues,
    influences: &MetricInfluences,
    mut deltas: VecDeque<(Entity, f64)>,
) -> Result<bool, MetricError> {
    while let Some((source, delta)) = deltas.pop_front() {
        if delta == 0.0 {
            continue;
        }
        let mut targets = Vec::new();
        for (entity, order, immediate, _) in influences.iter() {
            if let Some(immediate) = immediate {
                for input in &immediate.0 {
                    if input.source == source {
                        targets.push((order.0, entity, input.factor));
                    }
                }
            }
        }
        targets.sort_by_key(|&(order, _, _)| order);
        for (_, entity, factor) in targets {
            let (_, _, _, _, _, value, _) = values
                .get(entity)
                .map_err(|_| MetricError::SimulationNotReady)?;
            let requested = value.0 + delta * factor;
            let (actual_delta, collapsed) = write_value(values, entity, requested)?;
            if collapsed {
                return Ok(true);
            }
            if actual_delta != 0.0 {
                deltas.push_back((entity, actual_delta));
            }
        }
    }
    Ok(false)
}

fn settle_annual(
    values: &mut MetricValues,
    influences: &MetricInfluences,
) -> Result<bool, MetricError> {
    // Compute only Annual targets while every input still has its pre-settlement
    // value. An Immediate intermediary can be an input even if an earlier
    // Annual result will later change it through propagation.
    let mut annual_values = Vec::new();
    for (entity, order, _, annual) in influences.iter() {
        let Some(annual) = annual else {
            continue;
        };
        let mut sum = 0.0;
        for input in &annual.0 {
            let (_, _, _, _, _, value, _) = values
                .get(input.source)
                .map_err(|_| MetricError::SimulationNotReady)?;
            sum += value.0 * input.factor;
        }
        let (_, id, bounds, _, _, old, _) = values
            .get(entity)
            .map_err(|_| MetricError::SimulationNotReady)?;
        let new = bounded_value(bounds, &id.0, sum)?;
        checked_delta(&id.0, new, old.0)?;
        annual_values.push((order.0, entity, new));
    }
    annual_values.sort_by_key(|&(order, _, _)| order);
    let mut deltas = VecDeque::new();
    for (_, entity, value) in annual_values {
        let (delta, collapsed) = write_value(values, entity, value)?;
        if collapsed {
            return Ok(true);
        }
        if delta != 0.0 {
            deltas.push_back((entity, delta));
        }
    }
    propagate(values, influences, deltas)
}

fn restart_game(
    values: &mut MetricValues,
    date: &mut GameDate,
    pending: &mut PendingActions,
    status: &mut GameStatus,
) {
    for (_, _, _, initial, _, mut value, mut previous) in values.iter_mut() {
        if value.0 != initial.0 {
            value.0 = initial.0;
        }
        if previous.0 != initial.0 {
            previous.0 = initial.0;
        }
    }
    *date = GameDate::default();
    pending.0.clear();
    status.restarts = status.restarts.saturating_add(1);
    status.last_action = "Collapse reached 100. New game started.".into();
}
