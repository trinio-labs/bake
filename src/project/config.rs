use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use validator::{Validate, ValidationError};

#[derive(Debug, Deserialize, Serialize)]
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

#[derive(Debug, Deserialize, Serialize)]
pub struct RemoteCacheConfig {
    pub s3: Option<S3CacheConfig>,
    pub gcs: Option<GcsCacheConfig>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct S3CacheConfig {
    pub bucket: String,
    pub region: Option<String>,
}

#[derive(Debug, Deserialize, Serialize)]
pub struct GcsCacheConfig {
    pub bucket: String,
}

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct CacheConfig {
    #[serde(default)]
    pub local: LocalCacheConfig,

    #[serde(default, with = "serde_yaml::with::singleton_map")]
    pub remotes: Option<RemoteCacheConfig>,

    #[validate(custom(function = "validate_order"))]
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

#[derive(Debug, Deserialize, Serialize)]
pub struct UpdateConfig {
    #[serde(default = "bool_true_default")]
    pub enabled: bool,

    #[serde(default = "update_check_interval_default")]
    pub check_interval_days: u64,

    #[serde(default)]
    pub auto_update: bool,

    #[serde(default)]
    pub prerelease: bool,
}

impl Default for UpdateConfig {
    fn default() -> Self {
        Self {
            enabled: true,
            check_interval_days: update_check_interval_default(),
            auto_update: false,
            prerelease: false,
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

#[derive(Debug, Deserialize, Serialize, Validate)]
pub struct ToolConfig {
    #[serde(default = "max_parallel_default")]
    pub max_parallel: usize,

    #[serde(default)]
    pub fast_fail: bool,

    #[serde(default)]
    pub verbose: bool,

    #[serde(default)]
    #[validate(nested)]
    pub cache: CacheConfig,

    #[serde(default)]
    pub clean_environment: bool,

    #[serde(default)]
    pub update: UpdateConfig,

    /// The minimum version of bake required to work with this project configuration.
    /// This helps detect configuration mismatches due to breaking changes.
    #[serde(default, rename = "minVersion")]
    pub min_version: Option<String>,
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_parallel: max_parallel_default(),
            fast_fail: true,
            verbose: false,
            cache: CacheConfig::default(),
            clean_environment: false,
            update: UpdateConfig::default(),
            min_version: None,
        }
    }
}

fn bool_true_default() -> bool {
    true
}

fn max_parallel_default() -> usize {
    std::thread::available_parallelism().unwrap().get() - 1
}

fn update_check_interval_default() -> u64 {
    7 // Default value, you might want to implement a more robust default logic
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_yaml;
    use std::path::PathBuf;

    #[test]
    fn test_local_cache_config_default() {
        let config = LocalCacheConfig::default();
        assert!(config.enabled);
        assert!(config.path.is_none());
    }

    #[test]
    fn test_local_cache_config_deserialization() {
        let yaml = r#"
enabled: false
path: "/custom/cache/path"
"#;
        let config: LocalCacheConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(!config.enabled);
        assert_eq!(config.path, Some(PathBuf::from("/custom/cache/path")));
    }

    #[test]
    fn test_s3_cache_config_deserialization() {
        let yaml = r#"
bucket: "my-cache-bucket"
region: "us-west-2"
"#;
        let config: S3CacheConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.bucket, "my-cache-bucket");
        assert_eq!(config.region, Some("us-west-2".to_string()));
    }

    #[test]
    fn test_gcs_cache_config_deserialization() {
        let yaml = r#"
bucket: "my-gcs-bucket"
"#;
        let config: GcsCacheConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.bucket, "my-gcs-bucket");
    }

    #[test]
    fn test_cache_config_default() {
        let config = CacheConfig::default();
        assert!(config.local.enabled);
        assert!(config.remotes.is_none());
        assert!(config.order.is_empty());
    }

    #[test]
    fn test_cache_config_with_s3_deserialization() {
        let yaml = r#"
local:
  enabled: true
  path: "/tmp/cache"
remotes:
  s3:
    bucket: "my-s3-cache"
    region: "eu-central-1"
order: ["local", "s3"]
"#;
        let config: CacheConfig = serde_yaml::from_str(yaml).unwrap();

        assert!(config.local.enabled);
        assert_eq!(config.local.path, Some(PathBuf::from("/tmp/cache")));

        let remotes = config.remotes.unwrap();
        let s3_config = remotes.s3.unwrap();
        assert_eq!(s3_config.bucket, "my-s3-cache");
        assert_eq!(s3_config.region, Some("eu-central-1".to_string()));

        assert_eq!(config.order, vec!["local", "s3"]);
    }

    #[test]
    fn test_cache_config_with_gcs_deserialization() {
        let yaml = r#"
local:
  enabled: false
remotes:
  gcs:
    bucket: "my-gcs-cache"
order: ["gcs"]
"#;
        let config: CacheConfig = serde_yaml::from_str(yaml).unwrap();

        assert!(!config.local.enabled);

        let remotes = config.remotes.unwrap();
        let gcs_config = remotes.gcs.unwrap();
        assert_eq!(gcs_config.bucket, "my-gcs-cache");

        assert_eq!(config.order, vec!["gcs"]);
    }

    #[test]
    fn test_update_config_default() {
        let config = UpdateConfig::default();
        assert!(config.enabled);
        assert_eq!(config.check_interval_days, 7);
        assert!(!config.auto_update);
        assert!(!config.prerelease);
    }

    #[test]
    fn test_update_config_deserialization() {
        let yaml = r#"
enabled: true
check_interval_days: 14
auto_update: true
prerelease: true
"#;
        let config: UpdateConfig = serde_yaml::from_str(yaml).unwrap();
        assert!(config.enabled);
        assert_eq!(config.check_interval_days, 14);
        assert!(config.auto_update);
        assert!(config.prerelease);
    }

    #[test]
    fn test_tool_config_default() {
        let config = ToolConfig::default();
        assert!(config.max_parallel > 0); // Should be calculated from available parallelism
        assert!(!config.verbose);
        assert!(config.fast_fail);
        assert!(config.min_version.is_none());

        // Check nested defaults
        assert!(config.cache.local.enabled);
        assert!(config.update.enabled);
    }

    #[test]
    fn test_tool_config_deserialization() {
        let yaml = r#"
max_parallel: 4
verbose: true
fast_fail: false
minVersion: "1.0.0"
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.max_parallel, 4);
        assert!(config.verbose);
        assert!(!config.fast_fail);
        assert_eq!(config.min_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_complex_tool_config_deserialization() {
        let yaml = r#"
max_parallel: 8
verbose: true
fast_fail: false
minVersion: "2.1.0"
cache:
  local:
    enabled: true
    path: "/opt/cache"
  remotes:
    s3:
      bucket: "build-cache"
      region: "us-east-1"
    gcs:
      bucket: "backup-cache"
  order: ["local", "s3", "gcs"]
update:
  enabled: true
  check_interval_days: 3
  auto_update: false
  prerelease: true
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();

        // Validate main config
        assert_eq!(config.max_parallel, 8);
        assert!(config.verbose);
        assert!(!config.fast_fail);
        assert_eq!(config.min_version, Some("2.1.0".to_string()));

        // Validate cache config
        assert!(config.cache.local.enabled);
        assert_eq!(config.cache.local.path, Some(PathBuf::from("/opt/cache")));

        let remotes = config.cache.remotes.as_ref().unwrap();
        let s3_config = remotes.s3.as_ref().unwrap();
        assert_eq!(s3_config.bucket, "build-cache");

        let gcs_config = remotes.gcs.as_ref().unwrap();
        assert_eq!(gcs_config.bucket, "backup-cache");

        assert_eq!(config.cache.order, vec!["local", "s3", "gcs"]);

        // Validate update config
        assert!(config.update.enabled);
        assert_eq!(config.update.check_interval_days, 3);
        assert!(!config.update.auto_update);
        assert!(config.update.prerelease);

        // Validate overall configuration
        assert!(config.validate().is_ok());
    }

    #[test]
    fn test_cache_order_validation() {
        let yaml = r#"
order: ["invalid_strategy"]
"#;
        let config: CacheConfig = serde_yaml::from_str(yaml).unwrap();
        let validation_result = config.validate();

        // This should fail because "invalid_strategy" is not a valid cache strategy
        assert!(validation_result.is_err());
    }

    #[test]
    fn test_valid_cache_order_validation() {
        let yaml = r#"
cache:
  order: ["local", "s3", "gcs"]
"#;
        let config: CacheConfig = serde_yaml::from_str(yaml).unwrap();
        let validation_result = config.validate();

        // This should pass because all strategies are valid
        assert!(validation_result.is_ok());
    }
}
