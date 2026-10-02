use std::{
    collections::{HashMap, VecDeque},
    fmt,
    fs,
    path::Path,
};

use bevy::prelude::*;
use serde::{Deserialize, Serialize};

pub const COLLAPSE_METRIC_ID: &str = "collapse";

/// Static metric data, loaded from a RON list before the app starts.
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

#[derive(Component, Clone, Debug)]
pub struct Metric {
    pub definition: MetricDefinition,
    pub value: f64,
}

/// Preserves RON order for the UI and maps ids to their metric entities.
#[derive(Resource, Clone, Debug, Default)]
pub struct MetricEntities {
    pub ordered: Vec<Entity>,
    by_id: HashMap<String, Entity>,
}

impl MetricEntities {
    pub fn entity(&self, id: &str) -> Option<Entity> {
        self.by_id.get(id).copied()
    }
}

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

#[derive(SystemSet, Clone, Debug, PartialEq, Eq, Hash)]
pub enum SimulationSet {
    ApplyActions,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricError {
    DuplicateId(String),
    MissingSource { target: String, source: String },
    ImmediateCycle(Vec<String>),
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
            Self::InvalidDefinition { id, reason } => {
                write!(f, "invalid metric {id}: {reason}")
            }
            Self::Load { path, reason } => write!(f, "cannot load metrics from {path}: {reason}"),
            Self::InvalidRon(reason) => write!(f, "invalid metric RON: {reason}"),
            Self::UnknownMetric(id) => write!(f, "unknown metric: {id}"),
            Self::NonFiniteDelta => write!(f, "metric changes must have a finite delta"),
            Self::InvalidValue { id } => write!(f, "calculation for metric {id} is not finite"),
            Self::SimulationNotReady => write!(f, "metric entities have not been initialized"),
            Self::YearOverflow => write!(f, "game year exceeds the supported range"),
        }
    }
}

impl std::error::Error for MetricError {}

#[derive(Resource, Clone)]
struct MetricRules {
    definitions: Vec<MetricDefinition>,
    indices: HashMap<String, usize>,
    immediate: Vec<Vec<(usize, f64)>>,
    collapse: Option<usize>,
}

impl MetricRules {
    fn new(definitions: Vec<MetricDefinition>) -> Result<Self, MetricError> {
        validate_definitions(&definitions)?;
        let indices: HashMap<_, _> = definitions
            .iter()
            .enumerate()
            .map(|(index, definition)| (definition.id.clone(), index))
            .collect();
        let mut immediate = vec![Vec::new(); definitions.len()];
        for (target, definition) in definitions.iter().enumerate() {
            if let Some(Influence::Immediate(terms)) = &definition.influence {
                for term in terms {
                    immediate[indices[&term.source_metric]].push((target, term.factor));
                }
            }
        }
        let collapse = indices.get(COLLAPSE_METRIC_ID).copied();
        Ok(Self {
            definitions,
            indices,
            immediate,
            collapse,
        })
    }

    fn collapsed(&self, values: &[f64]) -> bool {
        self.collapse.is_some_and(|index| values[index] >= 100.0)
    }
}

pub struct MetricPlugin {
    rules: MetricRules,
}

impl MetricPlugin {
    pub fn new(definitions: Vec<MetricDefinition>) -> Result<Self, MetricError> {
        Ok(Self {
            rules: MetricRules::new(definitions)?,
        })
    }

    pub fn from_ron(contents: &str) -> Result<Self, MetricError> {
        let definitions = ron::from_str(contents)
            .map_err(|error| MetricError::InvalidRon(error.to_string()))?;
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
        app.insert_resource(self.rules.clone())
            .init_resource::<MetricEntities>()
            .init_resource::<GameDate>()
            .init_resource::<GameStatus>()
            .init_resource::<PendingActions>()
            .add_systems(Startup, spawn_metrics)
            .add_systems(
                Update,
                apply_pending_actions.in_set(SimulationSet::ApplyActions),
            );
    }
}

fn spawn_metrics(
    mut commands: Commands,
    rules: Res<MetricRules>,
    mut entities: ResMut<MetricEntities>,
) {
    for definition in &rules.definitions {
        let entity = commands
            .spawn(Metric {
                definition: definition.clone(),
                value: definition.initial_value,
            })
            .id();
        entities.ordered.push(entity);
        entities.by_id.insert(definition.id.clone(), entity);
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
            if matches!(&definition.influence, Some(Influence::Immediate(_))) {
                edges[source].push(target);
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

fn apply_pending_actions(world: &mut World) {
    loop {
        let action = world.resource_mut::<PendingActions>().0.pop_front();
        let Some(action) = action else {
            break;
        };
        if let Err(error) = apply_action(world, action) {
            world.resource_mut::<GameStatus>().last_action = format!("Action failed: {error}");
        }
    }
}

/// Applies a complete action atomically; an invalid numeric calculation leaves
/// the current run unchanged. Collapse aborts propagation and clears old clicks.
pub fn apply_action(world: &mut World, action: GameAction) -> Result<(), MetricError> {
    let entities = world
        .get_resource::<MetricEntities>()
        .ok_or(MetricError::SimulationNotReady)?
        .ordered
        .clone();
    let mut values: Vec<f64> = entities
        .iter()
        .map(|&entity| {
            world
                .get::<Metric>(entity)
                .map(|metric| metric.value)
                .ok_or(MetricError::SimulationNotReady)
        })
        .collect::<Result<_, _>>()?;
    let mut date = *world
        .get_resource::<GameDate>()
        .ok_or(MetricError::SimulationNotReady)?;
    let rules = world
        .get_resource::<MetricRules>()
        .ok_or(MetricError::SimulationNotReady)?;
    if entities.len() != rules.definitions.len() {
        return Err(MetricError::SimulationNotReady);
    }

    let (restart, message) = match action {
        GameAction::ChangeMetric { id, delta } => {
            if !delta.is_finite() {
                return Err(MetricError::NonFiniteDelta);
            }
            let &index = rules
                .indices
                .get(&id)
                .ok_or_else(|| MetricError::UnknownMetric(id.clone()))?;
            let actual_delta = change_value(rules, &mut values, index, delta)?;
            let restart = rules.collapsed(&values)
                || propagate(rules, &mut values, VecDeque::from([(index, actual_delta)]))?;
            (restart, format!("{id}: {actual_delta:+.2}"))
        }
        GameAction::NextMonth => {
            if date.month < 12 {
                date.month += 1;
                (false, "Advanced one month".into())
            } else {
                date.year = date.year.checked_add(1).ok_or(MetricError::YearOverflow)?;
                date.month = 1;
                (
                    settle_annual(rules, &mut values)?,
                    "Annual settlement complete".into(),
                )
            }
        }
    };

    if restart {
        for (value, definition) in values.iter_mut().zip(&rules.definitions) {
            *value = definition.initial_value;
        }
        date = GameDate::default();
    }
    for (&entity, value) in entities.iter().zip(values) {
        world.get_mut::<Metric>(entity).unwrap().value = value;
    }
    *world.resource_mut::<GameDate>() = date;
    let mut status = world.resource_mut::<GameStatus>();
    if restart {
        status.restarts = status.restarts.saturating_add(1);
        status.last_action = "Collapse reached 100. New game started.".into();
        world.resource_mut::<PendingActions>().0.clear();
    } else {
        status.last_action = message;
    }
    Ok(())
}

fn bounded_value(definition: &MetricDefinition, mut value: f64) -> Result<f64, MetricError> {
    if let Some(min) = definition.min_value {
        value = value.max(min);
    }
    if let Some(max) = definition.max_value {
        value = value.min(max);
    }
    if value.is_finite() {
        Ok(value)
    } else {
        Err(MetricError::InvalidValue {
            id: definition.id.clone(),
        })
    }
}

fn change_value(
    rules: &MetricRules,
    values: &mut [f64],
    index: usize,
    delta: f64,
) -> Result<f64, MetricError> {
    let value = bounded_value(&rules.definitions[index], values[index] + delta)?;
    let actual_delta = value - values[index];
    if !actual_delta.is_finite() {
        return Err(MetricError::InvalidValue {
            id: rules.definitions[index].id.clone(),
        });
    }
    values[index] = value;
    Ok(actual_delta)
}

fn propagate(
    rules: &MetricRules,
    values: &mut [f64],
    mut changes: VecDeque<(usize, f64)>,
) -> Result<bool, MetricError> {
    while let Some((source, delta)) = changes.pop_front() {
        if delta == 0.0 {
            continue;
        }
        for &(target, factor) in &rules.immediate[source] {
            let actual_delta = change_value(rules, values, target, delta * factor)?;
            if rules.collapsed(values) {
                return Ok(true);
            }
            if actual_delta != 0.0 {
                changes.push_back((target, actual_delta));
            }
        }
    }
    Ok(false)
}

fn settle_annual(rules: &MetricRules, values: &mut [f64]) -> Result<bool, MetricError> {
    // Read every Annual source from the same end-of-year snapshot. Apply all
    // Annual baselines before propagating deltas so later baselines cannot
    // overwrite an Immediate change made during this settlement.
    let snapshot = values.to_vec();
    let annual_values: Vec<(usize, f64)> = rules
        .definitions
        .iter()
        .enumerate()
        .filter_map(|(index, definition)| {
            if let Some(Influence::Annual(terms)) = &definition.influence {
                let value = terms
                    .iter()
                    .map(|term| snapshot[rules.indices[&term.source_metric]] * term.factor)
                    .sum();
                Some(bounded_value(definition, value).map(|value| (index, value)))
            } else {
                None
            }
        })
        .collect::<Result<_, _>>()?;
    let mut changes = VecDeque::new();
    for (index, value) in annual_values {
        let actual_delta = value - values[index];
        if !actual_delta.is_finite() {
            return Err(MetricError::InvalidValue {
                id: rules.definitions[index].id.clone(),
            });
        }
        values[index] = value;
        if rules.collapsed(values) {
            return Ok(true);
        }
        if actual_delta != 0.0 {
            changes.push_back((index, actual_delta));
        }
    }
    propagate(rules, values, changes)
}
