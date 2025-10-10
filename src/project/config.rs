use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use validator::{Validate, ValidationError};

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct RemoteCacheConfig {
    pub s3: Option<S3CacheConfig>,
    pub gcs: Option<GcsCacheConfig>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct S3CacheConfig {
    pub bucket: String,
    pub region: Option<String>,
}

#[derive(Debug, Clone, Deserialize, Serialize)]
pub struct GcsCacheConfig {
    pub bucket: String,
}

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
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

#[derive(Debug, Clone, Deserialize, Serialize)]
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

#[derive(Debug, Clone, Deserialize, Serialize, Validate)]
pub struct ToolConfig {
    #[serde(default = "max_parallel_default", rename = "maxParallel")]
    pub max_parallel: usize,

    #[serde(default = "reserved_threads_default", rename = "reservedThreads")]
    pub reserved_threads: usize,

    #[serde(default, rename = "fastFail")]
    pub fast_fail: bool,

    #[serde(default)]
    pub verbose: bool,

    #[serde(default)]
    #[validate(nested)]
    pub cache: CacheConfig,

    #[serde(default, rename = "cleanEnvironment")]
    pub clean_environment: bool,

    #[serde(default)]
    pub update: UpdateConfig,

    /// The minimum version of bake required to work with this project configuration.
    /// This helps detect configuration mismatches due to breaking changes.
    #[serde(default, rename = "minVersion")]
    pub min_version: Option<String>,
}

impl ToolConfig {
    /// Calculate effective max_parallel based on both max_parallel and reserved_threads.
    /// This should be used at runtime to get the actual parallelism considering reserved threads.
    pub fn effective_max_parallel(&self) -> usize {
        let available = std::thread::available_parallelism().unwrap().get();
        let available_minus_reserved = available.saturating_sub(self.reserved_threads);

        // Take the minimum of user-configured max_parallel and available threads minus reserved
        std::cmp::min(self.max_parallel, available_minus_reserved)
    }
}

impl Default for ToolConfig {
    fn default() -> Self {
        Self {
            max_parallel: max_parallel_default(),
            reserved_threads: reserved_threads_default(),
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
    std::thread::available_parallelism().unwrap().get()
}

fn reserved_threads_default() -> usize {
    1
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
        assert_eq!(config.reserved_threads, 1); // Default reserved threads
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
maxParallel: 4
reservedThreads: 2
verbose: true
fastFail: false
minVersion: "1.0.0"
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.max_parallel, 4);
        assert_eq!(config.reserved_threads, 2);
        assert!(config.verbose);
        assert!(!config.fast_fail);
        assert_eq!(config.min_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_complex_tool_config_deserialization() {
        let yaml = r#"
maxParallel: 8
verbose: true
fastFail: false
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

    #[test]
    fn test_reserved_threads_zero() {
        // Test CI scenario where all threads should be used
        let yaml = r#"
reservedThreads: 0
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.reserved_threads, 0);

        // When only reserved_threads is specified, max_parallel uses the default (available threads)
        let expected_max = std::thread::available_parallelism().unwrap().get();
        assert_eq!(config.max_parallel, expected_max);

        // With reservedThreads: 0, effective should be all available threads
        assert_eq!(config.effective_max_parallel(), expected_max);
    }

    #[test]
    fn test_reserved_threads_calculation() {
        // Test that effective max_parallel calculation respects reserved_threads
        let available = std::thread::available_parallelism().unwrap().get();

        // Test with default reserved threads (1) - max_parallel defaults to available, effective is available-1
        let default_config = ToolConfig::default();
        assert_eq!(default_config.max_parallel, available);
        assert_eq!(
            default_config.effective_max_parallel(),
            available.saturating_sub(1)
        );

        // Test when both max_parallel and reserved_threads are explicitly set
        let yaml = format!(
            "maxParallel: {}\nreservedThreads: 2",
            available.saturating_sub(2)
        );
        let config: ToolConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.max_parallel, available.saturating_sub(2));
        assert_eq!(config.reserved_threads, 2);
    }

    #[test]
    fn test_reserved_threads_default_function() {
        assert_eq!(reserved_threads_default(), 1);
    }

    #[test]
    fn test_effective_max_parallel() {
        let available = std::thread::available_parallelism().unwrap().get();

        // Test with reserved_threads = 0 (CI scenario) - should equal available threads
        let yaml = r#"
reservedThreads: 0
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        // When reservedThreads is 0, we should get all available threads
        assert_eq!(config.effective_max_parallel(), available);

        // Test with reserved_threads = 2 - should take min of max_parallel and (available - 2)
        let yaml = r#"
reservedThreads: 2
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        let expected = std::cmp::min(config.max_parallel, available.saturating_sub(2));
        assert_eq!(config.effective_max_parallel(), expected);

        // Test default behavior - should take min of default max_parallel and (available - 1)
        let default_config = ToolConfig::default();
        let expected = std::cmp::min(default_config.max_parallel, available.saturating_sub(1));
        assert_eq!(default_config.effective_max_parallel(), expected);

        // Test with both maxParallel and reservedThreads explicitly set
        let yaml = r#"
maxParallel: 2
reservedThreads: 1
"#;
        let config: ToolConfig = serde_yaml::from_str(yaml).unwrap();
        assert_eq!(config.max_parallel, 2);
        assert_eq!(config.reserved_threads, 1);
        // Should use maxParallel=2 since it's lower than (available-1)
        assert_eq!(config.effective_max_parallel(), 2);

        // Test where reserved_threads limits more than maxParallel
        let yaml = format!(
            "maxParallel: {}\nreservedThreads: {}",
            available, // Set max_parallel to available
            2          // Reserve 2 threads
        );
        let config: ToolConfig = serde_yaml::from_str(&yaml).unwrap();
        assert_eq!(config.max_parallel, available);
        assert_eq!(config.reserved_threads, 2);
        // Should use (available-2) since it's lower than maxParallel
        assert_eq!(config.effective_max_parallel(), available.saturating_sub(2));
    }
}
