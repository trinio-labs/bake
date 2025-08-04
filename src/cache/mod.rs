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
    pub async fn get(&self, recipe_name: &str, fqn_for_logging: &str) -> CacheResult {
        let hash = self.hashes.get(recipe_name).unwrap();
        for strategy in &self.strategies {
            if let CacheResult::Hit(data) = strategy.get(hash).await {
                if let Ok(mut tar_gz) = File::open(&data.archive_path) {
                    if let Err(err) = tar_gz.rewind() {
                        warn!(
                            "Cache GET: Failed to rewind archive file for recipe '{}' from '{}': {:?}",
                            fqn_for_logging,
                            &data.archive_path.display(),
                            err
                        );
                        return CacheResult::Miss;
                    }
                    let compressed = zstd::stream::Decoder::new(tar_gz).unwrap();
                    let mut archive = tar::Archive::new(compressed);
                    
                    // Safely extract with path traversal protection
                    if let Err(err) = self.safe_extract_archive(&mut archive, fqn_for_logging, &data.archive_path) {
                        warn!(
                            "Cache GET: Failed to safely extract archive for recipe '{}' from '{}': {:?}",
                            fqn_for_logging,
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

    /// Safely extracts a tar archive with path traversal protection
    fn safe_extract_archive(
        &self,
        archive: &mut tar::Archive<zstd::stream::Decoder<'_, std::io::BufReader<File>>>,
        fqn_for_logging: &str,
        archive_path: &std::path::Path,
    ) -> anyhow::Result<()> {
        use std::path::Component;
        
        for entry_result in archive.entries()? {
            let mut entry = entry_result?;
            let path = entry.path()?;
            
            // Security check: reject absolute paths
            if path.is_absolute() {
                return Err(anyhow!(
                    "Cache security violation: archive contains absolute path '{}' in recipe '{}' from '{}'",
                    path.display(),
                    fqn_for_logging,
                    archive_path.display()
                ));
            }
            
            // Security check: reject path traversal attempts
            for component in path.components() {
                if matches!(component, Component::ParentDir) {
                    return Err(anyhow!(
                        "Cache security violation: archive contains path traversal '{}' in recipe '{}' from '{}'",
                        path.display(),
                        fqn_for_logging,
                        archive_path.display()
                    ));
                }
            }
            
            // Build the full target path and ensure it's within project root
            let target_path = self.project.root_path.join(&path);
            
            // Get canonical paths for comparison, handling the case where target doesn't exist yet
            let canonical_root = self.project.root_path.canonicalize()
                .unwrap_or_else(|_| self.project.root_path.clone());
            
            let canonical_target = if target_path.exists() {
                target_path.canonicalize().unwrap_or_else(|_| target_path.clone())
            } else {
                // For non-existent paths, canonicalize the parent and join the filename
                if let Some(parent) = target_path.parent() {
                    let canonical_parent = parent.canonicalize().unwrap_or_else(|_| {
                        // Try to create parent directories first
                        let _ = std::fs::create_dir_all(parent);
                        parent.canonicalize().unwrap_or_else(|_| parent.to_path_buf())
                    });
                    canonical_parent.join(target_path.file_name().unwrap_or_default())
                } else {
                    target_path.clone()
                }
            };
            
            // Final security check: ensure target is within project root
            // Use a more robust check that handles macOS symlink resolution differences
            if !canonical_target.starts_with(&canonical_root) {
                // Try alternative approach: check if the relative path doesn't escape
                if let Ok(_relative) = canonical_target.strip_prefix(&canonical_root) {
                    // This is fine - the path is within the root
                } else {
                    return Err(anyhow!(
                        "Cache security violation: extracted path '{}' would escape project root '{}' for recipe '{}' from '{}'",
                        canonical_target.display(),
                        canonical_root.display(),
                        fqn_for_logging,
                        archive_path.display()
                    ));
                }
            }
            
            // Safe to extract this entry
            entry.unpack_in(&self.project.root_path)?;
        }
        
        Ok(())
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
                    Err(err) => bail!(
                        "Cache PUT: Failed to create zstd encoder for recipe '{}': {}",
                        recipe_name,
                        err
                    ),
                };
                let mut tar = tar::Builder::new(enc);
                // Get the recipe object using the new method
                let recipe = self.project.get_recipe_by_fqn(recipe_name).ok_or_else(|| {
                    anyhow!("Cache PUT: Recipe '{}' not found in project when attempting to cache its outputs", recipe_name)
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
                                bail!("Cache PUT: Failed to get canonical path for output '{}' of recipe '{}': {}", output, recipe_name, err);
                            }
                        };

                        let relative_output_path = match full_output_path
                            .strip_prefix(self.project.root_path.canonicalize().unwrap())
                        {
                            Ok(path) => path,
                            Err(err) => {
                                return Err(anyhow!(
                                    "Cache PUT: Failed to get relative path for output '{}' of recipe '{}': {}", output, recipe_name, err
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
                                "Cache PUT: Failed to add output '{}' to archive for recipe '{}': {}",
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
                        "Cache PUT: Failed to add log file to archive for recipe '{}': {}",
                        recipe_name,
                        err
                    ));
                }

                // Finish archive
                if let Err(err) = tar.finish() {
                    return Err(anyhow!(
                        "Cache PUT: Failed to finish archive for recipe '{}': {}",
                        recipe_name,
                        err
                    ));
                }
            }
            Err(err) => {
                return Err(anyhow!(
                "Cache PUT: Failed to create archive file in temp directory for recipe '{}': {}",
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
        fs::File,
        io::{Seek, Write},
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
    use anyhow::anyhow; // Added for the helper

    // Helper to ensure output and log files exist for a recipe before a 'put' operation.
    fn ensure_files_for_put(
        project_arc: &Arc<BakeProject>,
        recipe_fqn: &str,
    ) -> anyhow::Result<()> {
        project_arc.create_project_bake_dirs()?; // Ensures .bake/logs directory exists

        let recipe = project_arc
            .get_recipe_by_fqn(recipe_fqn)
            .ok_or_else(|| anyhow!("Recipe '{}' not found for file setup", recipe_fqn))?;

        // Create defined cache outputs
        if let Some(cache_config) = &recipe.cache {
            for output_rel_to_cookbook_str in &cache_config.outputs {
                let output_abs_path = recipe
                    .config_path // Path to the cookbook file (e.g., project_root/foo.yml)
                    .parent()
                    .unwrap()
                    .join(output_rel_to_cookbook_str);

                if let Some(parent_dir) = output_abs_path.parent() {
                    std::fs::create_dir_all(parent_dir)?;
                }

                if !output_abs_path.exists() {
                    if output_rel_to_cookbook_str.ends_with('/') {
                        std::fs::create_dir_all(&output_abs_path)?;
                    } else {
                        std::fs::File::create(&output_abs_path)?
                            .write_all(b"dummy output from helper")?;
                    }
                }
            }
        }

        // Create log file
        let log_file_abs_path = project_arc.get_recipe_log_path(recipe_fqn);
        if let Some(log_parent) = log_file_abs_path.parent() {
            std::fs::create_dir_all(log_parent)?;
        }
        if !log_file_abs_path.exists() {
            std::fs::File::create(&log_file_abs_path)?.write_all(b"dummy log from helper")?;
        }
        Ok(())
    }

    #[derive(Clone, Debug)]
    struct TestCacheStrategy {
        name: String,
        cached_keys: Arc<Mutex<HashSet<String>>>,
        get_calls: Arc<Mutex<Vec<String>>>,
        put_calls: Arc<Mutex<Vec<(String, PathBuf)>>>, // Stores (key, archive_path)
        should_put_fail: bool,
        create_dummy_archive_on_get: bool,
    }

    impl TestCacheStrategy {
        fn new(name: &str) -> Self {
            Self {
                name: name.to_string(),
                cached_keys: Arc::new(Mutex::new(HashSet::new())),
                get_calls: Arc::new(Mutex::new(Vec::new())),
                put_calls: Arc::new(Mutex::new(Vec::new())),
                should_put_fail: false,
                create_dummy_archive_on_get: false,
            }
        }

        // Get the dummy archive path for a key (used by this strategy)
        fn get_dummy_archive_path(&self, key: &str) -> PathBuf {
            // Create a unique path for each strategy instance to avoid collisions
            std::env::temp_dir().join(format!(
                "test_cache_{}_{}_{}.{}",
                self.name,
                key,
                std::process::id(),
                ARCHIVE_EXTENSION
            ))
        }
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, key: &str) -> super::CacheResult {
            self.get_calls.lock().unwrap().push(key.to_string());
            if self.cached_keys.lock().unwrap().contains(key) {
                let archive_path = self.get_dummy_archive_path(key);
                if self.create_dummy_archive_on_get {
                    if let Some(parent) = archive_path.parent() {
                        std::fs::create_dir_all(parent)
                            .expect("Failed to create dir for dummy archive");
                    }
                    let file = std::fs::File::create(&archive_path)
                        .expect("Failed to create dummy archive file for get");
                    let enc = zstd::stream::Encoder::new(file, 0)
                        .expect("Failed to create zstd encoder for dummy archive")
                        .auto_finish();
                    let mut tar_builder = tar::Builder::new(enc);

                    // Create a unique dummy file to add to the archive to ensure it's not empty
                    // and to verify its content upon extraction.
                    let dummy_content = format!("content_for_{}_{}", self.name, key);
                    let dummy_file_name = format!("dummy_file_for_test_{}_{}.txt", self.name, key);
                    let temp_dummy_file_path = std::env::temp_dir().join(&dummy_file_name);

                    std::fs::write(&temp_dummy_file_path, &dummy_content)
                        .expect("Failed to write temp dummy file for archive");

                    tar_builder
                        .append_path_with_name(&temp_dummy_file_path, &dummy_file_name)
                        .expect("Failed to append dummy file to tar");

                    tar_builder
                        .finish()
                        .expect("Failed to finish dummy tar archive");
                    std::fs::remove_file(temp_dummy_file_path)
                        .expect("Failed to remove temp dummy file");
                }
                return CacheResult::Hit(CacheResultData { archive_path });
            }
            CacheResult::Miss
        }

        async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
            self.put_calls
                .lock()
                .unwrap()
                .push((key.to_string(), archive_path.clone()));
            if self.should_put_fail {
                return Err(anyhow::anyhow!("Simulated put error from {}", self.name));
            }
            self.cached_keys.lock().unwrap().insert(key.to_string());
            Ok(())
        }

        async fn from_config(_project: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
            Ok(Box::new(TestCacheStrategy::new("default_from_config")))
        }
    }

    async fn build_cache(project: Arc<BakeProject>) -> Cache {
        let all_recipes: Vec<String> = project
            .cookbooks
            .values()
            .flat_map(|cb| {
                cb.recipes
                    .keys()
                    .map(|r_name| format!("{}:{}", cb.name, r_name))
            })
            .collect();

        CacheBuilder::new(project)
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build_for_recipes(&all_recipes)
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
        let mut cache = build_cache(project_arc.clone()).await;

        // Test hit
        let recipe_hash_hit = cache.hashes.get("foo:build").unwrap().clone();

        // prepare the first strategy to simulate a cache hit
        if let Some(strategy_arc_box) = cache.strategies.get_mut(0) {
            // To modify the strategy, we need to ensure it's our TestCacheStrategy.
            // This approach assumes the strategy created by TestCacheStrategy::from_config
            // is what's in cache.strategies[0].
            let prepared_strategy = TestCacheStrategy {
                name: "prepared_for_get_test".to_string(),
                cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash_hit.clone()]))),
                get_calls: Arc::new(Mutex::new(Vec::new())),
                put_calls: Arc::new(Mutex::new(Vec::new())),
                should_put_fail: false,
                create_dummy_archive_on_get: false, // Set to true if specifically testing unpack from this strategy
            };
            *strategy_arc_box = Arc::new(Box::new(prepared_strategy));
        } else {
            panic!("No cache strategies configured for testing get_hit");
        }

        let result_hit = cache.get("foo:build", "foo:build").await; // Added fqn_for_logging
        assert!(
            matches!(result_hit, CacheResult::Hit(ref data) if data.archive_path.to_str().unwrap().contains(&recipe_hash_hit)),
            "Cache hit failed or returned unexpected data. Expected hash: {recipe_hash_hit}"
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
        let cache_cmd_change = build_cache(Arc::new(project_cmd_change)).await;
        let result_cmd_miss = cache_cmd_change.get("foo:build", "foo:build").await; // Added fqn_for_logging
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
        let cache_dep_change = build_cache(Arc::new(project_dep_change)).await;
        let result_dep_miss = cache_dep_change.get("foo:build", "foo:build").await; // Added fqn_for_logging
        assert!(
            matches!(result_dep_miss, CacheResult::Miss),
            "Cache should miss when dependency changes"
        );
    }

    #[tokio::test]
    async fn get_hit_in_second_strategy() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;

        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // First strategy will miss, second will hit
        let miss_strategy = TestCacheStrategy {
            name: "miss_first".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::new())),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false,
        };
        let hit_strategy = TestCacheStrategy {
            name: "hit_second".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false,
        };
        cache.strategies = vec![
            Arc::new(Box::new(miss_strategy)),
            Arc::new(Box::new(hit_strategy)),
        ];

        let result = cache.get("foo:build", "foo:build").await;
        assert!(
            matches!(result, CacheResult::Hit(ref data) if data.archive_path.to_str().unwrap().contains("test_cache_hit_second")),
            "Cache should hit in the second strategy"
        );
    }

    #[tokio::test]
    async fn get_corrupted_archive_returns_miss() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;
        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // Prepare a strategy that will hit, but the archive will be corrupted
        let strategy = TestCacheStrategy {
            name: "corrupted_archive".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false, // We'll create a corrupted file manually
        };
        let archive_path = strategy.get_dummy_archive_path(&recipe_hash);
        // Create a corrupted archive file
        std::fs::write(&archive_path, b"not a valid archive")
            .expect("Failed to write corrupted archive");
        cache.strategies = vec![Arc::new(Box::new(strategy))];

        let result = cache.get("foo:build", "foo:build").await;
        assert!(
            matches!(result, CacheResult::Miss),
            "Cache should return Miss if archive is corrupted"
        );
        let _ = std::fs::remove_file(&archive_path); // Clean up
    }

    #[tokio::test]
    async fn put_missing_log_file_returns_error() {
        let _ = env_logger::builder().is_test(true).try_init();

        let project = create_test_project();
        let project_arc = Arc::new(project);
        _ = project_arc.create_project_bake_dirs();

        // Define paths based on project structure and cache output definition
        let output_file_rel_path = "foo/target/foo_test.txt";
        let output_file_abs_path = project_arc.root_path.join(output_file_rel_path);
        let log_file_abs_path = project_arc.get_recipe_log_path("foo:build");

        // Clean up potential leftovers from previous runs
        if let Some(parent_dir) = output_file_abs_path.parent() {
            let _ = std::fs::remove_dir_all(parent_dir);
        }
        if let Some(parent_dir) = log_file_abs_path.parent() {
            let _ = std::fs::remove_dir_all(parent_dir);
        }

        // Create output file but NOT the log file
        std::fs::create_dir_all(output_file_abs_path.parent().unwrap()).unwrap();
        std::fs::write(&output_file_abs_path, b"test output").unwrap();

        let cache = build_cache(project_arc.clone()).await;
        let result = cache.put("foo:build").await;
        assert!(
            result.is_err(),
            "Cache put should fail if log file is missing"
        );
    }

    #[tokio::test]
    async fn put_output_is_directory() {
        let _ = env_logger::builder().is_test(true).try_init();
        let project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .with_recipe_cache_outputs("foo:build", vec!["foo/target/dir_output".to_string()])
            .build();
        let project_arc = Arc::new(project);
        _ = project_arc.create_project_bake_dirs();
        let dir_output_path = project_arc.root_path.join("foo/target/dir_output");
        std::fs::create_dir_all(&dir_output_path).unwrap();
        // Add a file inside the directory to ensure it's not empty
        std::fs::write(dir_output_path.join("file.txt"), b"test").unwrap();

        // Use helper to ensure all required files exist
        ensure_files_for_put(&project_arc, "foo:build").unwrap();

        let cache = build_cache(project_arc.clone()).await;
        let result = cache.put("foo:build").await;
        if let Err(err) = &result {
            let e = format!("Cache put failed with error: {err:?}");
            println!("{e}");
        }
        assert!(
            result.is_ok(),
            "Cache put should succeed when output is a directory"
        );
    }

    #[tokio::test]
    async fn put_output_with_special_characters() {
        let _ = env_logger::builder().is_test(true).try_init();
        let project = TestProjectBuilder::new()
            .with_cookbook("foo", &["build"])
            .with_recipe_cache_outputs(
                "foo:build",
                vec!["foo/target/special char@#$.txt".to_string()],
            )
            .build();
        let project_arc = Arc::new(project);
        _ = project_arc.create_project_bake_dirs();
        let special_path = project_arc.root_path.join("foo/target/special char@#$.txt");
        std::fs::create_dir_all(special_path.parent().unwrap()).unwrap();
        std::fs::write(&special_path, b"special").unwrap();

        // Use helper to ensure all required files exist
        ensure_files_for_put(&project_arc, "foo:build").unwrap();

        let cache = build_cache(project_arc.clone()).await;
        let result = cache.put("foo:build").await;
        assert!(
            result.is_ok(),
            "Cache put should succeed with special characters in output path"
        );
    }

    #[tokio::test]
    async fn concurrent_puts_and_gets() {
        let _ = env_logger::builder().is_test(true).try_init();
        let project_arc = Arc::new(create_test_project());

        // Use helper to ensure all required files exist before concurrent operations
        ensure_files_for_put(&project_arc, "foo:build").unwrap();

        let cache = Arc::new(build_cache(project_arc.clone()).await);
        let mut handles = vec![];
        for _ in 0..5 {
            let cache_clone = cache.clone();
            handles.push(tokio::spawn(async move {
                let _ = cache_clone.put("foo:build").await;
            }));
            let cache_clone = cache.clone();
            handles.push(tokio::spawn(async move {
                let _ = cache_clone.get("foo:build", "foo:build").await;
            }));
        }
        for handle in handles {
            let _ = handle.await;
        }
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
        let inspectable_put_calls = Arc::new(Mutex::new(Vec::new())); // For inspecting put calls
        let inspectable_strategy = TestCacheStrategy {
            name: "inspectable_for_put_test".to_string(),
            cached_keys: inspectable_keys.clone(),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: inspectable_put_calls.clone(), // Use the new Arc for put_calls
            should_put_fail: false,
            create_dummy_archive_on_get: false,
        };

        let mut cache = build_cache(project_arc.clone()).await;
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
                    "Failed to get canonical path for output '{output_file_rel_path}'"
                )),
                "Error message for missing output was not as expected. Got: {error_message}"
            );
        } else {
            // This path should not be reached if the first assert holds.
            panic!("Expected cache.put to fail, but it succeeded.");
        }

        // Test case 2: Successful put
        // Use helper to ensure all required files exist
        ensure_files_for_put(&project_arc, "foo:build").unwrap();

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

    /// Helper to create a malicious archive with path traversal (using raw tar data)
    fn create_malicious_archive_with_path_traversal(archive_path: &PathBuf) -> anyhow::Result<()> {
        use std::io::Cursor;
        
        // Create a minimal tar archive with malicious path directly using raw bytes
        // This bypasses the tar crate's safety checks for testing purposes
        let mut data = Vec::new();
        
        // Create tar header for "../../../etc/passwd"
        let mut header = vec![0u8; 512];
        let path_bytes = b"../../../etc/passwd";
        header[..path_bytes.len()].copy_from_slice(path_bytes);
        
        // Set file mode (regular file)
        header[100..108].copy_from_slice(b"0000644 ");
        // Set uid/gid
        header[108..116].copy_from_slice(b"0000000 ");
        header[116..124].copy_from_slice(b"0000000 ");
        // Set file size (10 bytes)
        header[124..136].copy_from_slice(b"00000000012 ");
        // Set mtime
        header[136..148].copy_from_slice(b"00000000000 ");
        // Set checksum space
        header[148..156].copy_from_slice(b"        ");
        // Set file type (regular file)
        header[156] = b'0';
        
        // Calculate and set checksum
        let mut checksum = 0u32;
        for &byte in &header {
            checksum += byte as u32;
        }
        let checksum_str = format!("{checksum:06o}\0");
        header[148..148+checksum_str.len()].copy_from_slice(checksum_str.as_bytes());
        
        data.extend_from_slice(&header);
        
        // Add file content
        data.extend_from_slice(b"malicious\n");
        // Pad to 512-byte boundary
        while data.len() % 512 != 0 {
            data.push(0);
        }
        
        // Add end-of-archive marker (two empty 512-byte blocks)
        data.extend_from_slice(&[0u8; 1024]);
        
        // Compress with zstd
        let compressed = zstd::encode_all(Cursor::new(data), 0)?;
        std::fs::write(archive_path, compressed)?;
        Ok(())
    }

    /// Helper to create a malicious archive with absolute paths (using raw tar data)
    fn create_malicious_archive_with_absolute_path(archive_path: &PathBuf) -> anyhow::Result<()> {
        use std::io::Cursor;
        
        // Create a minimal tar archive with absolute path directly using raw bytes
        let mut data = Vec::new();
        
        // Create tar header for "/tmp/malicious_file.txt"
        let mut header = vec![0u8; 512];
        let path_bytes = b"/tmp/malicious_file.txt";
        header[..path_bytes.len()].copy_from_slice(path_bytes);
        
        // Set file mode (regular file)
        header[100..108].copy_from_slice(b"0000644 ");
        // Set uid/gid
        header[108..116].copy_from_slice(b"0000000 ");
        header[116..124].copy_from_slice(b"0000000 ");
        // Set file size (10 bytes)
        header[124..136].copy_from_slice(b"00000000012 ");
        // Set mtime
        header[136..148].copy_from_slice(b"00000000000 ");
        // Set checksum space
        header[148..156].copy_from_slice(b"        ");
        // Set file type (regular file)
        header[156] = b'0';
        
        // Calculate and set checksum
        let mut checksum = 0u32;
        for &byte in &header {
            checksum += byte as u32;
        }
        let checksum_str = format!("{checksum:06o}\0");
        header[148..148+checksum_str.len()].copy_from_slice(checksum_str.as_bytes());
        
        data.extend_from_slice(&header);
        
        // Add file content
        data.extend_from_slice(b"malicious\n");
        // Pad to 512-byte boundary
        while data.len() % 512 != 0 {
            data.push(0);
        }
        
        // Add end-of-archive marker (two empty 512-byte blocks)
        data.extend_from_slice(&[0u8; 1024]);
        
        // Compress with zstd
        let compressed = zstd::encode_all(Cursor::new(data), 0)?;
        std::fs::write(archive_path, compressed)?;
        Ok(())
    }

    #[tokio::test]
    async fn get_rejects_path_traversal_attack() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;
        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // Create a malicious strategy that returns an archive with path traversal
        let strategy = TestCacheStrategy {
            name: "path_traversal_attack".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false, // We'll create the malicious archive manually
        };

        let archive_path = strategy.get_dummy_archive_path(&recipe_hash);
        create_malicious_archive_with_path_traversal(&archive_path)
            .expect("Failed to create malicious archive");

        cache.strategies = vec![Arc::new(Box::new(strategy))];

        let result = cache.get("foo:build", "foo:build").await;
        assert!(
            matches!(result, CacheResult::Miss),
            "Cache should return Miss and reject path traversal attack"
        );

        // Verify that no malicious file was created outside project root
        let malicious_path = project_arc.root_path.parent().unwrap().join("etc").join("passwd");
        assert!(
            !malicious_path.exists(),
            "Malicious file should not have been created outside project root"
        );

        let _ = std::fs::remove_file(&archive_path); // Clean up
    }

    #[tokio::test]
    async fn get_rejects_absolute_path_attack() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;
        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // Create a malicious strategy that returns an archive with absolute paths
        let strategy = TestCacheStrategy {
            name: "absolute_path_attack".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false, // We'll create the malicious archive manually
        };

        let archive_path = strategy.get_dummy_archive_path(&recipe_hash);
        create_malicious_archive_with_absolute_path(&archive_path)
            .expect("Failed to create malicious archive");

        cache.strategies = vec![Arc::new(Box::new(strategy))];

        let result = cache.get("foo:build", "foo:build").await;
        assert!(
            matches!(result, CacheResult::Miss),
            "Cache should return Miss and reject absolute path attack"
        );

        // Verify that no malicious file was created in system directories
        let malicious_path = PathBuf::from("/tmp/malicious_file.txt");
        assert!(
            !malicious_path.exists(),
            "Malicious file should not have been created in system directory"
        );

        let _ = std::fs::remove_file(&archive_path); // Clean up
    }

    #[tokio::test]
    async fn get_safely_extracts_valid_archive() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;
        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // Create a valid strategy that creates a proper archive
        let strategy = TestCacheStrategy {
            name: "valid_archive".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: true, // This creates a valid archive
        };

        cache.strategies = vec![Arc::new(Box::new(strategy))];

        let result = cache.get("foo:build", "foo:build").await;
        assert!(
            matches!(result, CacheResult::Hit(_)),
            "Cache should successfully extract valid archive"
        );

        // Note: The extracted files should exist within project root with proper validation
    }

    #[tokio::test]
    async fn safe_extract_handles_missing_parent_directories() {
        let project_arc = Arc::new(create_test_project());
        let mut cache = build_cache(project_arc.clone()).await;
        let recipe_hash = cache.hashes.get("foo:build").unwrap().clone();

        // Create an archive with nested directory structure
        let temp_archive = std::env::temp_dir().join(format!("nested_test_{recipe_hash}.{ARCHIVE_EXTENSION}"));
        {
            let file = std::fs::File::create(&temp_archive).unwrap();
            let enc = zstd::stream::Encoder::new(file, 0).unwrap().auto_finish();
            let mut tar_builder = tar::Builder::new(enc);

            // Create a temp file to add
            let temp_file = std::env::temp_dir().join("nested_content.txt");
            std::fs::write(&temp_file, b"nested content").unwrap();

            // Add file with nested path (this should be safe)
            tar_builder.append_path_with_name(&temp_file, "deeply/nested/dir/file.txt").unwrap();
            tar_builder.finish().unwrap();
            std::fs::remove_file(temp_file).unwrap();
        } // Ensure all streams are closed and flushed

        let strategy = TestCacheStrategy {
            name: "nested_archive".to_string(),
            cached_keys: Arc::new(Mutex::new(HashSet::from([recipe_hash.clone()]))),
            get_calls: Arc::new(Mutex::new(Vec::new())),
            put_calls: Arc::new(Mutex::new(Vec::new())),
            should_put_fail: false,
            create_dummy_archive_on_get: false,
        };

        cache.strategies = vec![Arc::new(Box::new(strategy))];

        // Test the extraction directly
        let mut tar_gz = File::open(&temp_archive).unwrap();
        tar_gz.rewind().unwrap();
        let compressed = zstd::stream::Decoder::new(tar_gz).unwrap();
        let mut archive = tar::Archive::new(compressed);
        
        let extraction_result = cache.safe_extract_archive(&mut archive, "foo:build", &temp_archive);
        assert!(
            extraction_result.is_ok(),
            "Safe extraction should succeed for valid nested paths: {:?}",
            extraction_result.err()
        );

        // Verify the nested file was created
        let expected_file = project_arc.root_path.join("deeply/nested/dir/file.txt");
        assert!(
            expected_file.exists(),
            "Nested file should have been extracted safely"
        );

        let _ = std::fs::remove_file(&temp_archive); // Clean up
    }
}
