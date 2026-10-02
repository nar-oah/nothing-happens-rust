use std::{
    collections::{HashMap, VecDeque},
    fmt, fs,
    path::Path,
};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const COLLAPSE_METRIC_ID: &str = "collapse";

/// RON-only input; runtime state belongs to individual metric entities.
#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MetricDefinition {
    pub id: String,
    pub name: String,
    pub description: String,
    pub initial_value: f64,
    #[serde(default)]
    pub min_value: Option<f64>,
    #[serde(default)]
    pub max_value: Option<f64>,
    #[serde(default)]
    pub influence: Option<Influence>,
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub enum Influence {
    Immediate(Vec<InfluenceTerm>),
    Annual(Vec<InfluenceTerm>),
}

#[derive(Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct InfluenceTerm {
    pub source_metric: String,
    pub factor: f64,
}

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct Metric;

#[derive(Component, Clone, Debug, PartialEq, Eq)]
pub struct MetricId(pub String);

#[derive(Component, Clone, Debug)]
pub struct MetricMetadata {
    pub name: String,
    pub description: String,
}

#[derive(Component, Clone, Copy, Debug, PartialEq)]
pub struct MetricValue(pub f64);

#[derive(Component, Clone, Copy, Debug)]
pub struct InitialValue(pub f64);

#[derive(Component, Clone, Copy, Debug)]
pub struct MetricBounds {
    pub min: Option<f64>,
    pub max: Option<f64>,
}

#[derive(Component, Clone, Copy, Debug, PartialEq, Eq)]
pub struct MetricOrder(pub usize);

#[derive(Clone, Copy, Debug)]
pub struct MetricInput {
    pub source: Entity,
    pub factor: f64,
}

#[derive(Component, Clone, Debug)]
pub struct ImmediateInfluence(pub Vec<MetricInput>);

#[derive(Component, Clone, Debug)]
pub struct AnnualInfluence(pub Vec<MetricInput>);

#[derive(Component, Clone, Copy, Debug, Default)]
pub struct CollapseMetric;

/// Last value already propagated through Immediate influences. This is local
/// to an entity so other systems can write MetricValue through ordinary Queries.
#[derive(Component, Clone, Copy, Debug)]
pub struct PreviousValue(pub f64);

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

/// External value writers can run before ObserveChanges to propagate in the
/// same update. Writes made after this set are observed on the next update.
#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    ObserveChanges,
    ApplyActions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricError {
    DuplicateId(String),
    MissingSource { target: String, source: String },
    ImmediateCycle(Vec<String>),
    AnnualSource { target: String, source: String },
    InvalidDefinition { id: String, reason: String },
    Load { path: String, reason: String },
    InvalidRon(String),
    UnknownMetric(String),
    NonFiniteDelta,
    InvalidValue { id: String },
    SimulationNotReady,
    YearOverflow,
}

impl fmt::Display for MetricError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::DuplicateId(id) => write!(f, "duplicate metric id: {id}"),
            Self::MissingSource { target, source } => {
                write!(f, "metric {target} references missing source {source}")
            }
            Self::ImmediateCycle(ids) => {
                write!(f, "Immediate influence cycle: {}", ids.join(" -> "))
            }
            Self::AnnualSource { target, source } => {
                write!(f, "Annual metric {target} cannot reference Annual source {source}")
            }
            Self::InvalidDefinition { id, reason } => {
                write!(f, "invalid metric {id}: {reason}")
            }
            Self::Load { path, reason } => write!(f, "cannot load metrics from {path}: {reason}"),
            Self::InvalidRon(reason) => write!(f, "invalid metric RON: {reason}"),
            Self::UnknownMetric(id) => write!(f, "unknown metric: {id}"),
            Self::NonFiniteDelta => write!(f, "metric changes must have a finite delta"),
            Self::InvalidValue { id } => write!(f, "calculation for metric {id} is not finite"),
            Self::SimulationNotReady => write!(f, "a required metric entity is unavailable"),
            Self::YearOverflow => write!(f, "game year exceeds the supported range"),
        }
    }
}

impl std::error::Error for MetricError {}

pub struct MetricPlugin {
    definitions: Vec<MetricDefinition>,
}

impl MetricPlugin {
    pub fn new(definitions: Vec<MetricDefinition>) -> Result<Self, MetricError> {
        validate_definitions(&definitions)?;
        Ok(Self { definitions })
    }

    pub fn from_ron(contents: &str) -> Result<Self, MetricError> {
        let definitions =
            ron::from_str(contents).map_err(|error| MetricError::InvalidRon(error.to_string()))?;
        Self::new(definitions)
    }

    pub fn from_path(path: impl AsRef<Path>) -> Result<Self, MetricError> {
        let path = path.as_ref();
        let contents = fs::read_to_string(path).map_err(|error| MetricError::Load {
            path: path.display().to_string(),
            reason: error.to_string(),
        })?;
        Self::from_ron(&contents).map_err(|error| MetricError::Load {
            path: path.display().to_string(),
            reason: error.to_string(),
        })
    }
}

impl Plugin for MetricPlugin {
    fn build(&self, app: &mut App) {
        // Resolve ids once while constructing the entities. No definition or
        // value registry is retained in the runtime World.
        let world = app.world_mut();
        let mut entities = HashMap::new();
        for (order, definition) in self.definitions.iter().enumerate() {
            let mut entity = world.spawn((
                Metric,
                MetricId(definition.id.clone()),
                MetricMetadata {
                    name: definition.name.clone(),
                    description: definition.description.clone(),
                },
                MetricValue(definition.initial_value),
                InitialValue(definition.initial_value),
                PreviousValue(definition.initial_value),
                MetricBounds {
                    min: definition.min_value,
                    max: definition.max_value,
                },
                MetricOrder(order),
            ));
            if definition.id == COLLAPSE_METRIC_ID {
                entity.insert(CollapseMetric);
            }
            entities.insert(definition.id.as_str(), entity.id());
        }
        for definition in &self.definitions {
            let mut entity = world.entity_mut(entities[definition.id.as_str()]);
            let resolve = |terms: &[InfluenceTerm]| {
                terms
                    .iter()
                    .map(|term| MetricInput {
                        source: entities[term.source_metric.as_str()],
                        factor: term.factor,
                    })
                    .collect()
            };
            match &definition.influence {
                Some(Influence::Immediate(terms)) => {
                    entity.insert(ImmediateInfluence(resolve(terms)));
                }
                Some(Influence::Annual(terms)) => {
                    entity.insert(AnnualInfluence(resolve(terms)));
                }
                None => {}
            }
        }

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
}

pub fn validate_definitions(definitions: &[MetricDefinition]) -> Result<(), MetricError> {
    let mut indices = HashMap::new();
    for (index, definition) in definitions.iter().enumerate() {
        if indices.insert(definition.id.as_str(), index).is_some() {
            return Err(MetricError::DuplicateId(definition.id.clone()));
        }
        let invalid = |reason: &str| MetricError::InvalidDefinition {
            id: definition.id.clone(),
            reason: reason.into(),
        };
        if definition.id.trim().is_empty() || definition.name.trim().is_empty() {
            return Err(invalid("id and name must not be empty"));
        }
        if !definition.initial_value.is_finite()
            || definition.min_value.is_some_and(|value| !value.is_finite())
            || definition.max_value.is_some_and(|value| !value.is_finite())
        {
            return Err(invalid("initial value and bounds must be finite"));
        }
        if let (Some(min), Some(max)) = (definition.min_value, definition.max_value)
            && min > max
        {
            return Err(invalid("min_value must not exceed max_value"));
        }
        if definition
            .min_value
            .is_some_and(|min| definition.initial_value < min)
            || definition
                .max_value
                .is_some_and(|max| definition.initial_value > max)
        {
            return Err(invalid("initial_value must be within min_value/max_value"));
        }
    }

    let mut edges = vec![Vec::new(); definitions.len()];
    for (target, definition) in definitions.iter().enumerate() {
        let terms = match &definition.influence {
            Some(Influence::Immediate(terms)) | Some(Influence::Annual(terms)) => terms,
            None => continue,
        };
        for term in terms {
            let Some(&source) = indices.get(term.source_metric.as_str()) else {
                return Err(MetricError::MissingSource {
                    target: definition.id.clone(),
                    source: term.source_metric.clone(),
                });
            };
            if !term.factor.is_finite() {
                return Err(MetricError::InvalidDefinition {
                    id: definition.id.clone(),
                    reason: "influence factors must be finite".into(),
                });
            }
            match &definition.influence {
                Some(Influence::Immediate(_)) => edges[source].push(target),
                Some(Influence::Annual(_))
                    if matches!(definitions[source].influence, Some(Influence::Annual(_))) =>
                {
                    return Err(MetricError::AnnualSource {
                        target: definition.id.clone(),
                        source: term.source_metric.clone(),
                    });
                }
                _ => {}
            }
        }
    }

    let mut visited = vec![0; definitions.len()];
    let mut path = Vec::new();
    for index in 0..definitions.len() {
        visit_immediate(index, &edges, definitions, &mut visited, &mut path)?;
    }
    Ok(())
}

fn visit_immediate(
    index: usize,
    edges: &[Vec<usize>],
    definitions: &[MetricDefinition],
    visited: &mut [u8],
    path: &mut Vec<usize>,
) -> Result<(), MetricError> {
    match visited[index] {
        2 => return Ok(()),
        1 => {
            let start = path.iter().position(|&entry| entry == index).unwrap();
            let cycle = path[start..]
                .iter()
                .chain(std::iter::once(&index))
                .map(|&entry| definitions[entry].id.clone())
                .collect();
            return Err(MetricError::ImmediateCycle(cycle));
        }
        _ => {}
    }
    visited[index] = 1;
    path.push(index);
    for &target in &edges[index] {
        visit_immediate(target, edges, definitions, visited, path)?;
    }
    path.pop();
    visited[index] = 2;
    Ok(())
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
        let (_, id, bounds, _, collapse, mut value, mut previous) =
            values.get_mut(entity).unwrap();
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
                || propagate(
                    values,
                    influences,
                    VecDeque::from([(entity, actual_delta)]),
                )?;
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
