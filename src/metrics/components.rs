use bevy::prelude::*;

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

#[derive(Resource, Clone, Copy, Debug, Default)]
pub struct MetricReady;
