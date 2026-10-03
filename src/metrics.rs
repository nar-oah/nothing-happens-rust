mod asset;
mod components;
mod simulation;

use std::{collections::HashMap, fmt};

use bevy::{asset::AssetApp, prelude::*};

use asset::MetricCatalogLoader;
pub use asset::{Influence, InfluenceTerm, MetricCatalog, MetricDefinition, validate_definitions};
pub use components::*;
pub use simulation::*;

pub const COLLAPSE_METRIC_ID: &str = "collapse";
const METRIC_CATALOG_PATH: &str = "data/metrics.metric.ron";

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum MetricError {
    DuplicateId(String),
    MissingSource { target: String, source: String },
    ImmediateCycle(Vec<String>),
    AnnualSource { target: String, source: String },
    InvalidDefinition { id: String, reason: String },
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
                write!(
                    f,
                    "Annual metric {target} cannot reference Annual source {source}"
                )
            }
            Self::InvalidDefinition { id, reason } => {
                write!(f, "invalid metric {id}: {reason}")
            }
            Self::UnknownMetric(id) => write!(f, "unknown metric: {id}"),
            Self::NonFiniteDelta => write!(f, "metric changes must have a finite delta"),
            Self::InvalidValue { id } => write!(f, "calculation for metric {id} is not finite"),
            Self::SimulationNotReady => write!(f, "a required metric entity is unavailable"),
            Self::YearOverflow => write!(f, "game year exceeds the supported range"),
        }
    }
}

impl std::error::Error for MetricError {}

#[derive(Resource, Clone)]
struct MetricCatalogHandle(Handle<MetricCatalog>);

#[derive(Default)]
pub struct MetricPlugin {
    definitions: Option<Vec<MetricDefinition>>,
}

impl MetricPlugin {
    /// Direct definitions are kept for tests and tools that do not run an AssetServer.
    pub fn new(definitions: Vec<MetricDefinition>) -> Result<Self, MetricError> {
        validate_definitions(&definitions)?;
        Ok(Self {
            definitions: Some(definitions),
        })
    }
}

impl Plugin for MetricPlugin {
    fn build(&self, app: &mut App) {
        simulation::configure_simulation(app);

        if let Some(definitions) = &self.definitions {
            spawn_metric_entities(app.world_mut(), definitions);
            app.world_mut().insert_resource(MetricReady);
            return;
        }

        app.init_asset::<MetricCatalog>()
            .init_asset_loader::<MetricCatalogLoader>()
            .add_systems(Startup, request_metric_catalog)
            .add_systems(
                Update,
                spawn_metrics_when_loaded.before(SimulationSet::ObserveChanges),
            );
    }
}

fn request_metric_catalog(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle: Handle<MetricCatalog> = asset_server.load(METRIC_CATALOG_PATH);
    commands.insert_resource(MetricCatalogHandle(handle));
}

fn spawn_metrics_when_loaded(world: &mut World) {
    if world.contains_resource::<MetricReady>() {
        return;
    }

    let Some(handle) = world.get_resource::<MetricCatalogHandle>().cloned() else {
        return;
    };

    let definitions = {
        let catalogs = world.resource::<Assets<MetricCatalog>>();
        catalogs
            .get(&handle.0)
            .map(|catalog| catalog.metrics.clone())
    };

    let Some(definitions) = definitions else {
        return;
    };

    spawn_metric_entities(world, &definitions);
    world.insert_resource(MetricReady);
}

fn spawn_metric_entities(world: &mut World, definitions: &[MetricDefinition]) {
    let mut entities = HashMap::new();

    for (order, definition) in definitions.iter().enumerate() {
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

    for definition in definitions {
        let resolve = |terms: &[InfluenceTerm]| {
            terms
                .iter()
                .map(|term| MetricInput {
                    source: entities[term.source_metric.as_str()],
                    factor: term.factor,
                })
                .collect()
        };

        let mut entity = world.entity_mut(entities[definition.id.as_str()]);
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
}
