use std::path::PathBuf;

use serde::Deserialize;

use validator::{Validate, ValidationError};

#[derive(Debug, Deserialize)]
pub struct LocalCacheConfig {
    #[serde(default = "bool_true_default")]
    pub enabled: bool,
    pub path: Option<PathBuf>,
}

impl Default for LocalCacheConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            path: None,
        }
    }
}

#[derive(Debug, Deserialize)]
pub struct RemoteCacheConfig {
    pub s3: Option<S3CacheConfig>,
    pub gcs: Option<GcsCacheConfig>,
}

#[derive(Debug, Deserialize)]
pub struct S3CacheConfig {
    #[serde(default = "bool_true_default")]
    pub enabled: bool,
    pub bucket: String,
    pub region: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct GcsCacheConfig {
    #[serde(default = "bool_true_default")]
    pub enabled: bool,
    pub bucket: String,
}

#[derive(Debug, Deserialize, Validate)]
pub struct CacheConfig {
    #[serde(default)]
    pub local: LocalCacheConfig,

    #[serde(default, with = "serde_yaml::with::singleton_map")]
    pub remotes: Option<RemoteCacheConfig>,

    #[validate(custom = "validate_order")]
    #[serde(default)]
    pub order: Vec<String>,
}

impl Default for CacheConfig {
    fn default() -> Self {
        println!("Using default cache config");
        Self {
            local: LocalCacheConfig::default(),
            remotes: None,
            order: vec![],
        }
    }
}

fn validate_order(value: &[String]) -> Result<(), ValidationError> {
    let valid = value
        .iter()
        .all(|v| matches!(v.as_str(), "local" | "s3" | "gcs"));
    if !valid {
        Err(ValidationError::new(
            "string must be one of 'local', 's3' or 'gcs'",
        ))
    } else {
        Ok(())
    }
}

#[derive(Debug, Deserialize, Validate)]
pub struct ToolConfig {
    #[serde(default = "max_parallel_default")]
    pub max_parallel: usize,

    #[serde(default)]
    pub fast_fail: bool,

    #[serde(default)]
    pub verbose: bool,

    #[serde(default)]
    #[validate]
    pub cache: CacheConfig,

    #[serde(default)]
    pub clean_environment: bool,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_parallel: max_parallel_default(),
            fast_fail: true,
            verbose: false,
            cache: CacheConfig::default(),
            clean_environment: false,
        }
    }
}

fn bool_true_default() -> bool {
    true
}

fn max_parallel_default() -> usize {
    std::thread::available_parallelism().unwrap().get() - 1
}
