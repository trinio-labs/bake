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
                let recipe = self.project.recipes.get(recipe_name).unwrap();

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
                            .strip_prefix(&self.project.root_path.canonicalize().unwrap())
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
        io::Write,
        path::PathBuf,
        sync::{Arc, Mutex},
    };

    use async_trait::async_trait;

    use crate::{
        cache::{CacheBuilder, CacheResult, CacheResultData},
        project::BakeProject,
        test_utils::TestProjectBuilder,
    };

    use super::{Cache, CacheStrategy};

    const FOO_BUILD_HASH: &str = "7d0ac2e376b5bb56bd6a1f283112bbcacba780c8fa58cec14149907a27083248";

    #[derive(Clone, Debug)]
    struct TestCacheStrategy {
        cache: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, key: &str) -> super::CacheResult {
            if key == FOO_BUILD_HASH {
                return CacheResult::Hit(CacheResultData {
                    archive_path: PathBuf::from(format!("{}.tar.gz", key)),
                });
            }
            CacheResult::Miss
        }
        async fn put(&self, key: &str, _: PathBuf) -> anyhow::Result<()> {
            self.cache.lock().unwrap().push_str(key);
            Ok(())
        }
        async fn from_config(_: Arc<BakeProject>) -> anyhow::Result<Box<dyn super::CacheStrategy>> {
            Ok(Box::new(TestCacheStrategy {
                cache: Arc::new(Mutex::new(String::new())),
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
            .build()
    }

    #[tokio::test]
    async fn get() {
        let project = Arc::new(create_test_project());

        let cache = build_cache(project.clone(), "foo:build").await;

        // Test hit
        let result = cache.get("foo:build").await;
        assert!(matches!(result, CacheResult::Hit(_)));

        // Miss if recipe command changes
        let mut project = create_test_project();
        project.recipes.get_mut("foo:build").unwrap().run = "asdfasdfasd".to_owned();

        let cache = build_cache(Arc::new(project), "foo:build").await;
        let result = cache.get("foo:build").await;
        assert!(matches!(result, CacheResult::Miss));

        // Miss if dependency changes
        let mut project = create_test_project();
        project.recipes.get_mut("foo:build-dep").unwrap().run = "asdfasdfasd".to_owned();

        let cache = build_cache(Arc::new(project), "foo:build").await;
        let result = cache.get("foo:build").await;
        assert!(matches!(result, CacheResult::Miss));
    }

    #[tokio::test]
    async fn put() {
        let _ = env_logger::builder().is_test(true).try_init();

        let project = create_test_project();
        let project = Arc::new(project);
        _ = project.create_project_bake_dirs();

        // Clean all output directories and logs
        let _ = std::fs::remove_dir_all(project.root_path.join("foo/target"));
        let _ = std::fs::remove_file(project.get_recipe_log_path("foo:build"));

        // Create test cache
        let cache_str = Arc::new(Mutex::new(String::new()));
        let strategy = TestCacheStrategy {
            cache: cache_str.clone(),
        };
        let mut cache = build_cache(project.clone(), "foo:build").await;
        cache.strategies = vec![Arc::new(Box::new(strategy))];

        // Should error without existing output files
        let res = cache.put("foo:build").await;
        assert!(res.is_err());

        // Create log and output files
        let mut log_file = std::fs::File::create(project.get_recipe_log_path("foo:build")).unwrap();
        log_file.write_all(b"foo").unwrap();

        // Create target dir
        std::fs::create_dir_all(project.root_path.join("foo/target")).unwrap();

        // Create output file
        let mut output_file =
            std::fs::File::create(project.root_path.join("foo/target/foo_test.txt")).unwrap();
        output_file.write_all(b"foo").unwrap();

        let res = cache.put("foo:build").await;
        assert!(res.is_ok());
        assert_eq!(cache_str.lock().unwrap().as_str(), FOO_BUILD_HASH);
    }
}
