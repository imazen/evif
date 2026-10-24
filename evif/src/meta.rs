//! JSON chunks: `srce`, `knob`, `scor` (SPEC section 7).

use alloc::collections::BTreeMap;
use alloc::string::String;
use alloc::vec::Vec;

use serde::{Deserialize, Serialize};
use serde_json::Value;

use crate::Error;

/// `srce`: where the reference image came from.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Source {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub corpus: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    /// Fields this version does not know, kept for rewriting.
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// `knob`: the recipe that produced the distortion.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Recipe {
    /// Stress family that produced this file, such as `"zenjpeg-expert"`.
    pub family: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub codec_commit: Option<String>,
    /// The metric the search optimized for, if any.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<String>,
    #[serde(default)]
    pub params: Value,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

/// Which direction of a score means more similar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Better {
    Lower,
    Higher,
}

/// One metric score of reference vs. distorted.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Score {
    pub metric: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub implementation: Option<String>,
    pub value: f64,
    pub better: Better,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Score {
    pub fn new(metric: impl Into<String>, value: f64, better: Better) -> Self {
        Self {
            metric: metric.into(),
            variant: None,
            version: None,
            implementation: None,
            value,
            better,
            extra: BTreeMap::new(),
        }
    }

    pub fn variant(mut self, v: impl Into<String>) -> Self {
        self.variant = Some(v.into());
        self
    }

    pub fn version(mut self, v: impl Into<String>) -> Self {
        self.version = Some(v.into());
        self
    }

    pub fn implementation(mut self, v: impl Into<String>) -> Self {
        self.implementation = Some(v.into());
        self
    }

    /// `metric` or `metric/variant`.
    pub fn key(&self) -> String {
        match &self.variant {
            Some(v) => alloc::format!("{}/{}", self.metric, v),
            None => self.metric.clone(),
        }
    }
}

/// `scor`: every score computed on this file's rendition.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
#[non_exhaustive]
pub struct Scores {
    pub scores: Vec<Score>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, Value>,
}

impl Scores {
    pub fn new(scores: Vec<Score>) -> Self {
        Self {
            scores,
            extra: BTreeMap::new(),
        }
    }

    /// First score whose [`Score::key`] equals `key`.
    pub fn get(&self, key: &str) -> Option<&Score> {
        self.scores.iter().find(|s| s.key() == key)
    }
}

impl Recipe {
    pub fn new(family: impl Into<String>, params: Value) -> Self {
        Self {
            family: family.into(),
            params,
            ..Self::default()
        }
    }
}

pub(crate) fn to_json<T: Serialize>(v: &T) -> Result<Vec<u8>, Error> {
    serde_json::to_vec(v).map_err(|_| Error::Json)
}

pub(crate) fn from_json<'a, T: Deserialize<'a>>(b: &'a [u8]) -> Result<T, Error> {
    serde_json::from_slice(b).map_err(|_| Error::Json)
}
