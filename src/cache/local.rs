use std::{path::PathBuf, sync::Arc};

use anyhow::anyhow;
use async_trait::async_trait;
use log::debug;

use crate::{
    cache::{CacheResultData, ARCHIVE_EXTENSION},
    project::BakeProject,
};

use super::{CacheResult, CacheStrategy};

#[derive(Clone, Debug)]
pub struct LocalCacheStrategy {
    pub path: PathBuf,
}

#[async_trait]
impl CacheStrategy for LocalCacheStrategy {
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = format!("{}.{}", key.to_owned(), ARCHIVE_EXTENSION);
        let archive_path = self.path.join(file_name.clone());
        debug!("Checking local cache for key {}", archive_path.display());
        if archive_path.is_file() {
            debug!("Cache hit for key {key}");
            return CacheResult::Hit(CacheResultData { archive_path });
        }
        CacheResult::Miss
    }
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
        let file_name = format!("{}.{}", key.to_owned(), ARCHIVE_EXTENSION);
        // Create cache dir if it doesn't exist
        if !self.path.exists() {
            match std::fs::create_dir_all(&self.path) {
                Ok(_) => (),
                Err(err) => {
                    return Err(anyhow!(
                        "Failed to create cache dir {}: {}",
                        self.path.display(),
                        err
                    ))
                }
            }
        }

        // Check if cache folder with that key already exists
        let cache_path = self.path.join(file_name);
        if cache_path.exists() {
            println!("Cache file already exists: {}", cache_path.display());
            return Ok(());
        }

        // Copy archive to cache folder
        if let Err(err) = std::fs::copy(archive_path, cache_path.clone()) {
            Err(anyhow!(
                "Failed to copy archive to cache folder {}: {}",
                cache_path.display(),
                err
            ))
        } else {
            Ok(())
        }
    }

    async fn from_config(project: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
        debug!("Building local cache");
        let path = project
            .config
            .cache
            .local
            .path
            .clone()
            .unwrap_or(project.get_project_bake_path().join("cache"));
        debug!("Local cache path: {}", path.display());
        Ok(Box::new(LocalCacheStrategy { path }))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::project::config::{CacheConfig, LocalCacheConfig, ToolConfig};
    use crate::project::graph::RecipeDependencyGraph; // Assuming this path and Default impl
    use indexmap::IndexMap;
    use std::collections::BTreeMap;
    use tempfile::tempdir; // Ensure 'tempfile' is in [dev-dependencies] in Cargo.toml
    use tokio::fs::File;
    use tokio::io::AsyncWriteExt;

    // Helper function to create a BakeProject with a specific ToolConfig
    fn create_test_project_with_tool_config(tool_config: ToolConfig) -> Arc<BakeProject> {
        let temp_dir = tempdir().unwrap();
        let project_root_path = temp_dir.path().to_path_buf();

        Arc::new(BakeProject {
            name: "test_project".to_string(),
            cookbooks: BTreeMap::new(),
            recipe_dependency_graph: RecipeDependencyGraph::default(), // Assumes Default trait is implemented
            description: Some("A test project".to_string()),
            variables: IndexMap::new(),
            environment: Vec::new(),
            config: tool_config,

            root_path: project_root_path,
        })
    }

    // Helper function to create a BakeProject with default configuration for testing
    fn create_default_test_project() -> Arc<BakeProject> {
        let tool_config = ToolConfig {
            cache: CacheConfig {
                local: LocalCacheConfig {
                    enabled: true,
                    path: None, // Default path
                },
                remotes: None,
                order: vec!["local".to_string()],
            },
            ..ToolConfig::default() // For other fields like max_parallel, fast_fail etc.
        };
        create_test_project_with_tool_config(tool_config)
    }

    // Helper function to create a dummy file
    async fn create_dummy_file(path: &PathBuf) -> anyhow::Result<()> {
        let mut file = File::create(path).await?;
        file.write_all(b"test data").await?;
        Ok(())
    }

    #[tokio::test]
    async fn test_from_config_default_path() {
        let project = create_default_test_project();
        let cache_strategy_result = LocalCacheStrategy::from_config(project.clone()).await;
        assert!(cache_strategy_result.is_ok());

        // To test the actual path, you would need to enable downcasting:
        // 1. Add `use std::any::Any;` to this test module.
        // 2. Add `fn as_any(&self) -> &dyn Any;` to your `CacheStrategy` trait definition.
        // 3. Implement `fn as_any(&self) -> &dyn Any { self }` for `LocalCacheStrategy`.
        // Then, you can uncomment and use the following lines:
        // /*
        // use std::any::Any;
        // let cache_strategy = cache_strategy_result.unwrap();
        // let local_cache_strategy = cache_strategy
        // .as_any()
        // .downcast_ref::<LocalCacheStrategy>()
        // .expect("Should be LocalCacheStrategy");
        //
        // let expected_path = project.get_project_bake_path().join("cache");
        // assert_eq!(local_cache_strategy.path, expected_path);
        // */
    }

    #[tokio::test]
    async fn test_from_config_custom_path() {
        let temp_dir = tempdir().unwrap(); // Used for custom path
        let custom_cache_path = temp_dir.path().join("my_custom_cache");

        let tool_config = ToolConfig {
            cache: CacheConfig {
                local: LocalCacheConfig {
                    enabled: true,
                    path: Some(custom_cache_path.clone()), // Custom path
                },
                remotes: None,
                order: vec!["local".to_string()],
            },
            ..ToolConfig::default()
        };
        let project = create_test_project_with_tool_config(tool_config);

        let cache_strategy_result = LocalCacheStrategy::from_config(project.clone()).await;
        assert!(cache_strategy_result.is_ok());

        // See comments in `test_from_config_default_path` regarding how to enable
        // downcasting to assert the specific path if needed.
        // /*
        // use std::any::Any;
        // let cache_strategy = cache_strategy_result.unwrap();
        // let local_cache_strategy = cache_strategy
        // .as_any()
        // .downcast_ref::<LocalCacheStrategy>()
        // .expect("Should be LocalCacheStrategy");
        //
        // assert_eq!(local_cache_strategy.path, custom_cache_path);
        // */
    }

    #[tokio::test]
    async fn test_put_and_get() {
        // This test doesn't rely on BakeProject, so it remains largely unchanged.
        let cache_dir = tempdir().unwrap();
        let strategy = LocalCacheStrategy {
            path: cache_dir.path().to_path_buf(),
        };

        let key = "test_key";
        let dummy_content_path = cache_dir.path().join("dummy_content.tar.gz");
        create_dummy_file(&dummy_content_path).await.unwrap();

        // Test put
        strategy.put(key, dummy_content_path.clone()).await.unwrap();
        let expected_cache_file_path = strategy.path.join(format!("{key}.{ARCHIVE_EXTENSION}"));
        assert!(expected_cache_file_path.is_file());

        // Test get hit
        match strategy.get(key).await {
            CacheResult::Hit(data) => {
                assert_eq!(data.archive_path, expected_cache_file_path);
            }
            CacheResult::Miss => panic!("Expected cache hit, got miss"),
        }

        // Test get miss
        match strategy.get("non_existent_key").await {
            CacheResult::Miss => {
                // Expected
            }
            CacheResult::Hit(_) => panic!("Expected cache miss, got hit"),
        }
    }

    #[tokio::test]
    async fn test_put_creates_cache_dir_if_not_exists() {
        let base_temp_dir = tempdir().unwrap();
        let cache_dir_path = base_temp_dir.path().join("new_cache_dir"); // a path that doesn't exist yet

        let strategy = LocalCacheStrategy {
            path: cache_dir_path.clone(),
        };

        assert!(!cache_dir_path.exists());

        let key = "another_key";
        let dummy_content_path = base_temp_dir.path().join("dummy_content2.tar.gz");
        create_dummy_file(&dummy_content_path).await.unwrap();

        strategy.put(key, dummy_content_path.clone()).await.unwrap();
        assert!(cache_dir_path.is_dir()); // Check if the directory was created
        let expected_cache_file_path = strategy.path.join(format!("{key}.{ARCHIVE_EXTENSION}"));
        assert!(expected_cache_file_path.is_file());
    }

    #[tokio::test]
    async fn test_put_existing_file() {
        let cache_dir = tempdir().unwrap();
        let strategy = LocalCacheStrategy {
            path: cache_dir.path().to_path_buf(),
        };

        let key = "existing_key";
        let dummy_content_path = cache_dir.path().join("dummy_content3.tar.gz");
        create_dummy_file(&dummy_content_path).await.unwrap();

        // First put
        strategy.put(key, dummy_content_path.clone()).await.unwrap();
        let expected_cache_file_path = strategy.path.join(format!("{key}.{ARCHIVE_EXTENSION}"));
        assert!(expected_cache_file_path.is_file());

        // Second put (should not error)
        let result = strategy.put(key, dummy_content_path.clone()).await;
        assert!(result.is_ok());
    }
}
