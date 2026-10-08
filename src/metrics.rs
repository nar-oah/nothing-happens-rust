mod asset;
mod components;
mod simulation;

use std::collections::HashMap;

use bevy::{asset::AssetApp, prelude::*};

use asset::MetricCatalogLoader;
pub use asset::{Influence, InfluenceTerm, MetricCatalog, MetricDefinition, validate_definitions};
pub use components::*;
pub use simulation::*;

pub const COLLAPSE_METRIC_ID: &str = "collapse";
pub const YEAR_METRIC_ID: &str = "year";
pub const MONTH_METRIC_ID: &str = "month";
pub const TERM_METRIC_ID: &str = "term";
const METRIC_CATALOG_PATH: &str = "data/metrics.metric.ron";

#[derive(Resource, Clone)]
struct MetricCatalogHandle(Handle<MetricCatalog>);

#[derive(Default)]
pub struct MetricPlugin;

impl Plugin for MetricPlugin {
    fn build(&self, app: &mut App) {
        simulation::configure_simulation(app);

        app.init_asset::<MetricCatalog>()
            .init_asset_loader::<MetricCatalogLoader>()
            .add_systems(Startup, request_metric_catalog)
            .add_systems(
                Update,
                handle_metric_catalog_events.before(SimulationSet::ApplyChanges),
            );
    }
}

fn request_metric_catalog(mut commands: Commands, asset_server: Res<AssetServer>) {
    let handle: Handle<MetricCatalog> = asset_server.load(METRIC_CATALOG_PATH);
    commands.insert_resource(MetricCatalogHandle(handle));
}

fn handle_metric_catalog_events(
    mut commands: Commands,
    handle: Res<MetricCatalogHandle>,
    catalogs: Res<Assets<MetricCatalog>>,
    mut events: MessageReader<AssetEvent<MetricCatalog>>,
) {
    for event in events.read() {
        let AssetEvent::LoadedWithDependencies { id } = event else {
            continue;
        };

        if *id != handle.0.id() {
            continue;
        }

        let catalog = catalogs
            .get(&handle.0)
            .expect("loaded metric catalog must be available");

        spawn_metric_entities(&mut commands, &catalog.metrics);
    }
}

fn spawn_metric_entities(commands: &mut Commands, definitions: &[MetricDefinition]) {
    let mut entities = HashMap::new();

    for (order, definition) in definitions.iter().enumerate() {
        let mut entity = commands.spawn((
            Metric,
            MetricId(definition.id.clone()),
            MetricMetadata {
                name: definition.name.clone(),
                description: definition.description.clone(),
            },
            MetricValue(definition.initial_value),
            InitialValue(definition.initial_value),
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

        let mut entity = commands.entity(entities[definition.id.as_str()]);
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
