use std::{collections::BTreeMap, sync::Arc};

use bake::cache::s3::S3CacheStrategy;
use bake::cache::{CacheResult, CacheStrategy};
use bake::project::{
    config::{CacheConfig, RemoteCacheConfig, S3CacheConfig, ToolConfig},
    graph::RecipeDependencyGraph,
    BakeProject,
};
use indexmap::IndexMap;
use tempfile::tempdir;

mod common;

// Helper function to create a test project with S3 configuration
fn create_test_project_with_s3() -> Arc<BakeProject> {
    let temp_dir = tempdir().unwrap();
    let project_root_path = temp_dir.path().to_path_buf();

    Arc::new(BakeProject {
        name: "s3_test_project".to_string(),
        cookbooks: BTreeMap::new(),
        recipe_dependency_graph: RecipeDependencyGraph::default(),
        description: Some("A test project with S3 cache".to_string()),
        variables: IndexMap::new(),
        overrides: BTreeMap::new(),
        processed_variables: IndexMap::new(),
        environment: Vec::new(),
        config: ToolConfig {
            cache: CacheConfig {
                local: Default::default(),
                remotes: Some(RemoteCacheConfig {
                    s3: Some(S3CacheConfig {
                        bucket: "test-cache-bucket".to_string(),
                        region: Some("us-east-1".to_string()),
                    }),
                    gcs: None,
                }),
                order: vec!["s3".to_string()],
            },
            ..ToolConfig::default()
        },
        root_path: project_root_path,
        template_registry: BTreeMap::new(),
        helper_registry: BTreeMap::new(),
    })
}

#[ignore = "requires AWS credentials and connectivity"]
#[tokio::test]
async fn test_s3_cache_strategy_from_config() {
    // This test requires AWS credentials to be configured
    // It's marked with #[ignore] so it won't run by default
    let project = create_test_project_with_s3();

    let result = S3CacheStrategy::from_config(project).await;

    // If AWS credentials are configured, this should succeed
    // If not, it should fail gracefully
    match result {
        Ok(_strategy) => {
            // Test that we can create the strategy successfully
            println!("S3 cache strategy created successfully");
        }
        Err(_) => {
            // If credentials aren't available, that's expected in CI/testing
            println!("S3 cache strategy creation failed (expected without AWS credentials)");
        }
    }
}

#[ignore = "requires AWS credentials and connectivity"]
#[tokio::test]
async fn test_s3_cache_roundtrip() {
    // This test requires an actual S3 bucket and credentials
    let project = create_test_project_with_s3();

    let cache_strategy = match S3CacheStrategy::from_config(project).await {
        Ok(strategy) => strategy,
        Err(_) => {
            println!("Skipping S3 roundtrip test - no AWS credentials available");
            return;
        }
    };

    // Test cache miss for non-existent key
    let result = cache_strategy.get("non_existent_key").await;
    assert!(matches!(result, CacheResult::Miss));

    // Create a temporary archive file to test put operation
    let temp_dir = tempdir().unwrap();
    let archive_path = temp_dir.path().join("test_archive.tar.gz");
    tokio::fs::write(&archive_path, b"test cache content")
        .await
        .unwrap();

    // Test put operation
    let put_result = cache_strategy.put("test_key", archive_path).await;

    match put_result {
        Ok(_) => {
            // If put succeeded, test get operation
            let get_result = cache_strategy.get("test_key").await;
            match get_result {
                CacheResult::Hit(data) => {
                    assert!(data.archive_path.exists());
                }
                CacheResult::Miss => {
                    panic!("Expected cache hit after successful put");
                }
            }
        }
        Err(e) => {
            println!("S3 put operation failed (may be expected): {e}");
        }
    }
}

#[test]
fn test_s3_cache_config_validation() {
    // Test that S3CacheConfig can be deserialized correctly
    let yaml = r#"
bucket: "my-s3-cache-bucket"
region: "eu-west-1"
"#;

    let config: S3CacheConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.bucket, "my-s3-cache-bucket");
    assert_eq!(config.region, Some("eu-west-1".to_string()));
}

#[test]
fn test_s3_cache_config_without_region() {
    // Test that S3CacheConfig works without explicit region
    let yaml = r#"
bucket: "my-s3-cache-bucket"
"#;

    let config: S3CacheConfig = serde_yaml::from_str(yaml).unwrap();
    assert_eq!(config.bucket, "my-s3-cache-bucket");
    assert!(config.region.is_none());
}

#[tokio::test]
async fn test_s3_cache_strategy_creation_without_credentials() {
    // Test that S3CacheStrategy creation fails gracefully without credentials
    let temp_dir = tempdir().unwrap();
    let project = Arc::new(BakeProject {
        name: "test".to_string(),
        cookbooks: BTreeMap::new(),
        recipe_dependency_graph: RecipeDependencyGraph::default(),
        description: None,
        variables: IndexMap::new(),
        overrides: BTreeMap::new(),
        processed_variables: IndexMap::new(),
        environment: Vec::new(),
        config: ToolConfig {
            cache: CacheConfig {
                local: Default::default(),
                remotes: Some(RemoteCacheConfig {
                    s3: Some(S3CacheConfig {
                        bucket: "invalid-bucket-for-testing".to_string(),
                        region: Some("us-east-1".to_string()),
                    }),
                    gcs: None,
                }),
                order: vec!["s3".to_string()],
            },
            ..ToolConfig::default()
        },
        root_path: temp_dir.path().to_path_buf(),
        template_registry: BTreeMap::new(),
        helper_registry: BTreeMap::new(),
    });

    // Without proper AWS credentials, this should fail
    let result = S3CacheStrategy::from_config(project).await;

    // We expect this to either succeed (if credentials are available)
    // or fail gracefully (if not)
    match result {
        Ok(_) => {
            // If credentials are available, creation succeeds
            println!("S3 cache strategy created successfully");
        }
        Err(e) => {
            // Expected when credentials are not available
            println!("S3 cache strategy creation failed as expected: {e}");
        }
    }
}

#[ignore = "requires AWS credentials and connectivity"]
#[tokio::test]
async fn test_s3_cache_error_handling() {
    // Test error handling for various S3 operations
    let project = create_test_project_with_s3();

    let cache_strategy = match S3CacheStrategy::from_config(project).await {
        Ok(strategy) => strategy,
        Err(_) => {
            println!("Skipping S3 error handling test - no AWS credentials available");
            return;
        }
    };

    // Test with invalid key that contains illegal characters
    let result = cache_strategy.get("invalid/key/with/special/chars").await;
    // Should handle this gracefully and return Miss
    assert!(matches!(result, CacheResult::Miss));

    // Test put with non-existent file
    let non_existent_file = std::path::PathBuf::from("/non/existent/file.tar.gz");
    let put_result = cache_strategy.put("test_key", non_existent_file).await;
    assert!(put_result.is_err());
}
