use std::{collections::HashMap, fmt};

use bevy::{
    asset::{AssetLoader, LoadContext, io::Reader},
    prelude::*,
    reflect::TypePath,
};
use serde::{Deserialize, Serialize};

use super::MetricError;

#[derive(Asset, TypePath, Clone, Debug, Deserialize, Serialize, PartialEq)]
pub struct MetricCatalog {
    pub metrics: Vec<MetricDefinition>,
}

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

#[derive(Default, TypePath)]
pub(crate) struct MetricCatalogLoader;

#[derive(Debug)]
pub(crate) enum MetricAssetError {
    Io(std::io::Error),
    Ron(ron::error::SpannedError),
    Metric(MetricError),
}

impl fmt::Display for MetricAssetError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Io(error) => write!(f, "could not read metric asset: {error}"),
            Self::Ron(error) => write!(f, "could not parse metric RON: {error}"),
            Self::Metric(error) => write!(f, "invalid metric definitions: {error}"),
        }
    }
}

impl std::error::Error for MetricAssetError {}

impl From<std::io::Error> for MetricAssetError {
    fn from(error: std::io::Error) -> Self {
        Self::Io(error)
    }
}

impl From<ron::error::SpannedError> for MetricAssetError {
    fn from(error: ron::error::SpannedError) -> Self {
        Self::Ron(error)
    }
}

impl From<MetricError> for MetricAssetError {
    fn from(error: MetricError) -> Self {
        Self::Metric(error)
    }
}

impl AssetLoader for MetricCatalogLoader {
    type Asset = MetricCatalog;
    type Settings = ();
    type Error = MetricAssetError;

    async fn load(
        &self,
        reader: &mut dyn Reader,
        _settings: &Self::Settings,
        _load_context: &mut LoadContext<'_>,
    ) -> Result<Self::Asset, Self::Error> {
        let mut bytes = Vec::new();
        reader.read_to_end(&mut bytes).await?;

        let catalog = ron::de::from_bytes::<MetricCatalog>(&bytes)?;
        validate_definitions(&catalog.metrics)?;
        Ok(catalog)
    }

    fn extensions(&self) -> &[&str] {
        &["metric.ron"]
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
