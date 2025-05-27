pub mod builder;
pub mod gcs;
pub mod local;
pub mod s3;

use std::{collections::HashMap, fs::File, io::Seek, path::PathBuf, sync::Arc};

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use log::warn;

use crate::project::BakeProject;

pub use builder::CacheBuilder;

pub const ARCHIVE_EXTENSION: &str = "tar.zst";

#[async_trait]
pub trait CacheStrategy: Send + Sync {
    async fn get(&self, key: &str) -> CacheResult;
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()>;
    async fn from_config(config: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>>
    where
        Self: Sized;
}

#[derive(Debug, PartialEq)]
pub struct CacheResultData {
    pub archive_path: PathBuf,
}

#[derive(Debug, PartialEq)]
pub enum CacheResult {
    Hit(CacheResultData),
    Miss,
}

/// Cache manages caching of bake outputs by using caching strategies defined in
/// configuration files
pub struct Cache {
    /// Reference to the project so we can get recipes and their dependencies
    pub project: Arc<BakeProject>,

    /// List of cache strategies
    pub strategies: Vec<Arc<Box<dyn CacheStrategy>>>,

    /// Map of recipe hashes so we don't have to recompute them
    pub hashes: HashMap<String, String>,
}

impl Cache {
    // Tries to get a cached result for the given recipe
    pub async fn get(&self, recipe_name: &str) -> CacheResult {
        let hash = self.hashes.get(recipe_name).unwrap();
        for strategy in &self.strategies {
            if let CacheResult::Hit(data) = strategy.get(hash).await {
                if let Ok(mut tar_gz) = File::open(&data.archive_path) {
                    if let Err(err) = tar_gz.rewind() {
                        warn!(
                            "Failed to rewind archive file: {}. Error: {:?}",
                            &data.archive_path.display(),
                            err
                        );
                        return CacheResult::Miss;
                    }
                    let compressed = zstd::stream::Decoder::new(tar_gz).unwrap();
                    let mut archive = tar::Archive::new(compressed);
                    if let Err(err) = archive.unpack(self.project.root_path.clone()) {
                        warn!(
                            "Failed to unpack archive file: {}. Error: {:?}",
                            &data.archive_path.display(),
                            err
                        );
                        return CacheResult::Miss;
                    }
                }
                return CacheResult::Hit(data);
            }
        }

        CacheResult::Miss
    }

    // Puts the given recipe's outputs in the cache
    pub async fn put(&self, recipe_name: &str) -> anyhow::Result<()> {
        // Create archive in temp dir
        let archive_path = std::env::temp_dir().join(format!(
            "{}.{}",
            recipe_name.replace(':', "."),
            ARCHIVE_EXTENSION
        ));
        let tar_gz = File::create(archive_path.clone());

        match tar_gz {
            Ok(tar_gz) => {
                // let enc = GzEncoder::new(tar_gz, Compression::default());
                let enc = match zstd::stream::Encoder::new(tar_gz, 1) {
                    Ok(z) => z.auto_finish(),
                    Err(err) => bail!("Failed creating zstd encoder: {}", err),
                };
                let mut tar = tar::Builder::new(enc);
                // Get the recipe object using the new method
                let recipe = self.project.get_recipe_by_fqn(recipe_name).ok_or_else(|| {
                    anyhow::anyhow!(
                        "Recipe '{}' not found in project for caching its outputs",
                        recipe_name
                    )
                })?;

                // Add outputs to archive
                if let Some(cache) = &recipe.cache {
                    for output in &cache.outputs {
                        // Resolve relative paths by trying to get canonical form
                        let full_output_path = match recipe
                            .config_path
                            .parent()
                            .unwrap()
                            .join(output)
                            .canonicalize()
                        {
                            Ok(path) => path,
                            Err(err) => {
                                bail!("Failed to get canonical path for output {output}: {err}");
                            }
                        };

                        let relative_output_path = match full_output_path
                            .strip_prefix(self.project.root_path.canonicalize().unwrap())
                        {
                            Ok(path) => path,
                            Err(err) => {
                                return Err(anyhow!(
                                    "Failed to get relative path for output {output}: {err}",
                                ));
                            }
                        };

                        let res = if full_output_path.is_dir() {
                            tar.append_dir_all(relative_output_path, full_output_path.clone())
                        } else {
                            tar.append_path_with_name(
                                full_output_path.clone(),
                                relative_output_path,
                            )
                        };

                        if let Err(err) = res {
                            return Err(anyhow!(
                                "Failed to add {} to tar file in temp dir for recipe {}: {}",
                                output,
                                recipe_name,
                                err
                            ));
                        }
                    }
                }

                // Add log file to archive
                let log_path = self.project.get_recipe_log_path(recipe_name);
                let relative_log_path = log_path.strip_prefix(&self.project.root_path).unwrap();
                if let Err(err) = tar.append_path_with_name(log_path.clone(), relative_log_path) {
                    return Err(anyhow!(
                        "Failed to add log file to tar file in temp dir for recipe {}: {}",
                        recipe_name,
                        err
                    ));
                }

                // Finish archive
                if let Err(err) = tar.finish() {
                    return Err(anyhow!(
                        "Failed to finish tar file in temp dir for recipe {}: {}",
                        recipe_name,
                        err
                    ));
                }
            }
            Err(err) => {
                return Err(anyhow!(
                    "Failed to create tar file in temp dir for recipe {}: {}",
                    recipe_name,
                    err
                ))
            }
        }

        let hash = self.hashes.get(recipe_name).unwrap();
        for strategy in self.strategies.iter() {
            strategy.put(hash, archive_path.clone()).await?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{
        collections::HashSet, // Added
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;

    use crate::{
        cache::{CacheBuilder, CacheResult, CacheResultData, ARCHIVE_EXTENSION}, // Added ARCHIVE_EXTENSION
        project::BakeProject,
        test_utils::TestProjectBuilder,
    };

    use super::{Cache, CacheStrategy};

    // Removed: const FOO_BUILD_HASH: &str = "7d0ac2e376b5bb56bd6a1f283112bbcacba780c8fa58cec14149907a27083248";

    #[derive(Clone, Debug)]
    struct TestCacheStrategy {
        cached_keys: Arc<Mutex<HashSet<String>>>, // Changed
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, key: &str) -> super::CacheResult {
            if self.cached_keys.lock().unwrap().contains(key) {
                // Changed
                return CacheResult::Hit(CacheResultData {
                    archive_path: PathBuf::from(format!("{key}.{ARCHIVE_EXTENSION}")), // Changed
                });
            }
            CacheResult::Miss
        }
        async fn put(&self, key: &str, _: PathBuf) -> anyhow::Result<()> {
            self.cached_keys.lock().unwrap().insert(key.to_string()); // Changed
            Ok(())
        }
        async fn from_config(_: Arc<BakeProject>) -> anyhow::Result<Box<dyn super::CacheStrategy>> {
            Ok(Box::new(TestCacheStrategy {
                cached_keys: Arc::new(Mutex::new(HashSet::new())), // Changed
            }))
        }
    }

    async fn build_cache(project: Arc<BakeProject>, filter: &str) -> Cache {
        CacheBuilder::new(project)
            .filter(filter)
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build()
            .await
            .unwrap()
    }

    fn create_test_project() -> BakeProject {
        TestProjectBuilder::new()
            .with_cookbook("foo", &["build", "build-dep"])
            .with_dependency("foo:build", "foo:build-dep")
            // Define cache outputs for foo:build. Path is relative to cookbook file (project_root/foo.yml)
            .with_recipe_cache_outputs("foo:build", vec!["foo/target/foo_test.txt".to_string()])
            .build()
    }

    #[tokio::test]
    async fn get() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone(), "foo:build").await;

        // Test hit
        let recipe_hash_hit = cache.hashes.get("foo:build").unwrap().clone();

        // Prime the first strategy to simulate a cache hit
        if let Some(strategy_arc_box) = cache.strategies.get_mut(0) {
            // To modify the strategy, we need to ensure it's our TestCacheStrategy.
            // This approach assumes the strategy created by TestCacheStrategy::from_config
            // is what's in cache.strategies[0].
            let primed_strategy = TestCacheStrategy {
                cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash_hit.clone()]))),
            };
            *strategy_arc_box = Arc::new(Box::new(primed_strategy));
        } else {
            panic!("No cache strategies configured for testing get_hit");
        }

        let result_hit = cache.get("foo:build").await;
        assert!(
            matches!(result_hit, CacheResult::Hit(ref data) if data.archive_path == PathBuf::from(format!("{recipe_hash_hit}.{ARCHIVE_EXTENSION}"))),
            "Cache hit failed or returned unexpected data. Expected hash: {recipe_hash_hit}, archive_path: {recipe_hash_hit}.{ARCHIVE_EXTENSION}"
        );

        // Miss if recipe command changes
        let mut project_cmd_change = create_test_project();
        project_cmd_change
            .cookbooks
            .get_mut("foo")
            .unwrap()
            .recipes
            .get_mut("build")
            .unwrap()
            .run = "different command".to_owned();
        // Hashes are calculated in build_cache, so this new project will have a different hash for foo:build
        let cache_cmd_change = build_cache(Arc::new(project_cmd_change), "foo:build").await;
        let result_cmd_miss = cache_cmd_change.get("foo:build").await;
        assert!(
            matches!(result_cmd_miss, CacheResult::Miss),
            "Cache should miss when recipe command changes"
        );

        // Miss if dependency changes
        let mut project_dep_change = create_test_project();
        project_dep_change
            .cookbooks
            .get_mut("foo")
            .unwrap()
            .recipes
            .get_mut("build-dep") // Change a dependency's command
            .unwrap()
            .run = "different dependency command".to_owned();
        let cache_dep_change = build_cache(Arc::new(project_dep_change), "foo:build").await;
        let result_dep_miss = cache_dep_change.get("foo:build").await;
        assert!(
            matches!(result_dep_miss, CacheResult::Miss),
            "Cache should miss when dependency changes"
        );
    }

    #[tokio::test]
    async fn put() {
        let _ = env_logger::builder().is_test(true).try_init();

        let project = create_test_project(); // This now includes cache outputs for "foo:build"
        let project_arc = Arc::new(project);
        _ = project_arc.create_project_bake_dirs();

        // Define paths based on project structure and cache output definition
        let output_file_rel_path = "foo/target/foo_test.txt"; // As defined in create_test_project
        let output_file_abs_path = project_arc.root_path.join(output_file_rel_path);
        let log_file_abs_path = project_arc.get_recipe_log_path("foo:build");

        // Clean up potential leftovers from previous runs
        if let Some(parent_dir) = output_file_abs_path.parent() {
            let _ = std::fs::remove_dir_all(parent_dir);
        }
        if let Some(parent_dir) = log_file_abs_path.parent() {
            let _ = std::fs::remove_dir_all(parent_dir); // Clean log dir too if it's different
        }
        let _ = std::fs::remove_file(&log_file_abs_path);

        // Create an inspectable TestCacheStrategy instance
        let inspectable_keys = Arc::new(Mutex::new(HashSet::new()));
        let inspectable_strategy = TestCacheStrategy {
            cached_keys: inspectable_keys.clone(),
        };

        let mut cache = build_cache(project_arc.clone(), "foo:build").await;
        // Use only our inspectable strategy for this test
        cache.strategies = vec![Arc::new(Box::new(inspectable_strategy))];

        // Test case 1: Error if output files don't exist
        let res_no_output = cache.put("foo:build").await;
        assert!(
            res_no_output.is_err(),
            "cache.put should fail if output files do not exist."
        );

        // Check for a more specific part of the error message
        if let Err(err) = res_no_output {
            let error_message = format!("{err:?}");
            assert!(
                error_message.contains(&format!(
                    "Failed to get canonical path for output {output_file_rel_path}"
                )),
                "Error message for missing output was not as expected. Got: {error_message}"
            );
        } else {
            // This path should not be reached if the first assert holds.
            panic!("Expected cache.put to fail, but it succeeded.");
        }

        // Test case 2: Successful put
        // Create the directories and files that 'put' expects
        std::fs::create_dir_all(output_file_abs_path.parent().unwrap())
            .expect("Failed to create output directory for test");
        let mut test_output_file = std::fs::File::create(&output_file_abs_path)
            .expect("Failed to create test output file");
        test_output_file.write_all(b"test content").unwrap();

        if let Some(log_parent) = log_file_abs_path.parent() {
            std::fs::create_dir_all(log_parent).expect("Failed to create log directory");
        }
        let mut test_log_file =
            std::fs::File::create(&log_file_abs_path).expect("Failed to create test log file");
        test_log_file.write_all(b"test log").unwrap();

        let res_success = cache.put("foo:build").await;
        assert!(
            res_success.is_ok(),
            "cache.put failed unexpectedly: {:?}",
            res_success.err()
        );

        // Verify that the correct hash was 'put' into our inspectable strategy
        let expected_hash = cache.hashes.get("foo:build").unwrap().clone();
        let locked_keys = inspectable_keys.lock().unwrap();
        assert!(
            locked_keys.contains(&expected_hash),
            "Expected hash '{expected_hash}' not found in TestCacheStrategy's keys. Found: {locked_keys:?}"
        );
        assert_eq!(
            locked_keys.len(),
            1,
            "TestCacheStrategy should contain exactly one key after put."
        );
    }
}
