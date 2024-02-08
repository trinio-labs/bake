mod local;
mod s3;

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    path::PathBuf,
    sync::Arc,
};

use flate2::{write::GzEncoder, Compression, GzBuilder};
use log::warn;
use serde::Serialize;

use crate::project::BakeProject;

pub trait CacheStrategy: Send + Sync {
    fn get(&self, key: &str) -> CacheResult;
    fn put(&self, key: &str, archive_path: PathBuf) -> Result<(), String>;
}

#[derive(Debug, PartialEq)]
pub struct CacheResultData {
    pub stdout: String,
}

#[derive(Debug, PartialEq)]
pub enum CacheResult {
    Hit(CacheResultData),
    Miss,
}

#[derive(Debug, Serialize)]
struct CacheData {
    recipe: String,
    deps: BTreeMap<String, String>,
}

/// Cache manages caching of bake outputs by using caching strategies defined in
/// configuration files
pub struct Cache {
    /// Reference to the project so we can get recipes and their dependencies
    pub project: Arc<BakeProject>,

    /// List of cache strategies
    pub strategies: Vec<Box<dyn CacheStrategy>>,

    /// Map of recipe hashes so we don't have to recompute them
    pub hashes: HashMap<String, String>,
}

impl Cache {
    /// Creates a new instance of the Cache using the recipe_list to only calculate the hashes of
    /// required recipes
    pub fn new(project: Arc<BakeProject>, filter: Option<&str>) -> Self {
        let mut strategies: Vec<Box<dyn CacheStrategy>>;
        let local_path = project
            .config
            .cache
            .local
            .path
            .clone()
            .unwrap_or(project.root_path.clone());

        // If there's no cache order, use local then s3 if configured
        if project.config.cache.order.is_empty() {
            strategies = Vec::new();
            if project.config.cache.local.enabled {
                strategies.push(Box::new(local::LocalCacheStrategy { path: local_path }))
            }
            if let Some(remotes) = project.config.cache.remotes.as_ref() {
                if let Some(s3_config) = remotes.s3.as_ref() {
                    strategies.push(Box::new(s3::S3CacheStrategy::from_config(s3_config)))
                }
            }
        } else {
            strategies = project
            .config
            .cache
            .order
            .iter()
            .filter_map(|item| -> Option<Box<dyn CacheStrategy>> {
                match item.as_str() {
                    "local" => {
                        if !project.config.cache.local.enabled {
                            warn!("Local is listed in cache order but disabled in config. Ignoring.");
                            None
                        } else {
                            Some(Box::new(local::LocalCacheStrategy {
                                path: project.root_path.clone(),
                            }))
                        }
                    }
                    "s3" => {
                        if let Some(config) = project.config.cache.remotes.as_ref() {
                            if let Some(s3_config) = config.s3.as_ref() {
                                if !s3_config.enabled {
                                    warn!(
                                        "S3 cache listed in cache order but disabled in config. Ignoring."
                                    );
                                    None
                                } else {
                                    Some(Box::new(s3::S3CacheStrategy {
                                        bucket: s3_config.bucket.clone(),
                                        region: s3_config.region.clone(),
                                        access_key: s3_config.access_key.clone(),
                                        secret_key: s3_config.secret_key.clone(),
                                    }))
                                }
                            } else {
                                warn!("S3 cache is listed in cache order but no S3 config found. Ignoring.");
                                None
                            }
                        } else {
                            None
                        }
                    }
                    _ => None,
                }
            })
            .collect();
        }

        let hashes = project
            .get_recipes(filter)
            .iter()
            .map(|(key, recipe)| (key.to_owned(), recipe.get_recipe_hash().unwrap()))
            .collect();

        Self {
            project,
            strategies,
            hashes,
        }
    }

    // Tries to get a cached result for the given recipe
    pub fn get(&self, recipe_name: &str) -> CacheResult {
        let hash = self.calculate_total_hash(recipe_name);
        for strategy in &self.strategies {
            if let CacheResult::Hit(data) = strategy.get(&hash) {
                return CacheResult::Hit(data);
            }
        }

        CacheResult::Miss
    }

    // Calculates the hash for the given recipe given all its dependencies
    fn calculate_total_hash(&self, recipe_name: &str) -> String {
        let mut cache_data = CacheData {
            recipe: recipe_name.to_owned(),
            deps: BTreeMap::new(),
        };

        if let Some(recipe_hash) = self.hashes.get(recipe_name) {
            cache_data.recipe = recipe_hash.clone();
        };

        if let Some(deps) = self.project.clone().dependency_map.get(recipe_name) {
            cache_data.deps = deps.iter().fold(BTreeMap::new(), |mut acc, x| {
                if let Some(hash) = self.hashes.get(x) {
                    acc.insert(x.clone(), hash.clone());
                }
                acc
            });
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&cache_data).unwrap().as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    // Puts the given recipe's outputs in the cache
    pub fn put(&self, recipe_name: &str) -> Result<(), String> {
        // Create archive in temp dir
        let archive_path =
            std::env::temp_dir().join(format!("{}.tar.gz", recipe_name.replace(':', ".")));
        let tar_gz = File::create(archive_path.clone());

        match tar_gz {
            Ok(tar_gz) => {
                let enc = GzEncoder::new(tar_gz, Compression::default());
                let mut tar = tar::Builder::new(enc);
                let recipe = self.project.recipes.get(recipe_name).unwrap();

                // Add outputs to archive
                if let Some(outputs) = recipe.outputs.as_ref() {
                    for output in outputs {
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
                                return Err(format!(
                                    "Failed to get canonical path for output {}: {}",
                                    output, err
                                ));
                            }
                        };

                        let relative_output_path =
                            match full_output_path.strip_prefix(&self.project.root_path) {
                                Ok(path) => path,
                                Err(err) => {
                                    return Err(format!(
                                        "Failed to get relative path for output {}: {}",
                                        output, err
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
                            return Err(format!(
                                "Failed to add {} to tar file in temp dir for recipe {}: {}",
                                output, recipe_name, err
                            ));
                        }
                    }
                }

                // Add log file to archive
                let log_path = self.project.get_recipe_log_path(recipe_name);
                let relative_log_path = log_path.strip_prefix(&self.project.root_path).unwrap();
                if let Err(err) = tar.append_path_with_name(log_path.clone(), relative_log_path) {
                    return Err(format!(
                        "Failed to add log file to tar file in temp dir for recipe {}: {}",
                        recipe_name, err
                    ));
                }

                // Finish archive
                if let Err(err) = tar.finish() {
                    return Err(format!(
                        "Failed to finish tar file in temp dir for recipe {}: {}",
                        recipe_name, err
                    ));
                }
            }
            Err(err) => {
                return Err(format!(
                    "Failed to create tar file in temp dir for recipe {}: {}",
                    recipe_name, err
                ))
            }
        }

        let hash = self.calculate_total_hash(recipe_name);
        for strategy in self.strategies.iter() {
            strategy.put(&hash, archive_path.clone())?;
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

    use crate::{
        cache::{CacheResult, CacheResultData},
        project::BakeProject,
    };

    use super::{Cache, CacheStrategy};

    const FOO_BUILD_HASH: &str = "e4db81f19d6e6ef8ffc889c3c58eed00c24cf18255409aa8ca2fa9f4c83a9997";

    struct TestCacheStrategy {
        cache: Arc<Mutex<String>>,
    }

    impl CacheStrategy for TestCacheStrategy {
        fn get(&self, key: &str) -> super::CacheResult {
            if key == FOO_BUILD_HASH {
                return CacheResult::Hit(CacheResultData {
                    stdout: "foo".to_string(),
                });
            }
            CacheResult::Miss
        }
        fn put(&self, key: &str, _: PathBuf) -> Result<(), String> {
            self.cache.lock().unwrap().push_str(key);
            Ok(())
        }
    }

    fn config_path(path_str: &str) -> String {
        env!("CARGO_MANIFEST_DIR").to_owned() + "/resources/tests" + path_str
    }

    #[test]
    fn new() {
        let project_path = PathBuf::from(config_path("/valid"));
        let project = BakeProject::from(&project_path).unwrap();
        let cache = Cache::new(Arc::new(project), Some("invalid_filter"));
        assert!(cache.hashes.is_empty());
        assert_eq!(cache.strategies.len(), 2);
    }

    #[test]
    fn get() {
        let project_path = PathBuf::from(config_path("/valid"));
        let project = Arc::new(BakeProject::from(&project_path).unwrap());

        // Create test cache
        let cache_str = Arc::new(Mutex::new(String::new()));
        let strategy = TestCacheStrategy {
            cache: cache_str.clone(),
        };
        let mut cache = Cache::new(project, Some("foo:build"));
        cache.strategies = vec![Box::new(TestCacheStrategy {
            cache: cache_str.clone(),
        })];

        // Test hit
        let result = cache.get("foo:build");
        assert!(matches!(result, CacheResult::Hit(_)));

        // Miss if recipe command changes
        let mut project = BakeProject::from(&project_path).unwrap();
        project.recipes.get_mut("foo:build").unwrap().run = "asdfasdfasd".to_owned();
        let project = Arc::new(project);
        let mut cache = Cache::new(project, Some("foo:build"));
        cache.strategies = vec![Box::new(TestCacheStrategy {
            cache: cache_str.clone(),
        })];
        let result = cache.get("foo:build");
        assert!(matches!(result, CacheResult::Miss));

        // Miss if dependency changes
        let mut project = BakeProject::from(&project_path).unwrap();
        project.recipes.get_mut("foo:build-dep").unwrap().run = "asdfasdfasd".to_owned();
        let project = Arc::new(project);

        let mut cache = Cache::new(project, Some("foo:build"));
        cache.strategies = vec![Box::new(TestCacheStrategy {
            cache: cache_str.clone(),
        })];
        let result = cache.get("foo:build");
        assert!(matches!(result, CacheResult::Miss));
    }

    #[test]
    fn put() {
        let project_path = PathBuf::from(config_path("/valid"));
        let project = Arc::new(BakeProject::from(&project_path).unwrap());
        _ = project.create_project_bake_dirs();

        // Clean all output directories and logs
        let _ = std::fs::remove_dir_all(project.root_path.join("foo/target"));
        let _ = std::fs::remove_file(project.get_recipe_log_path("foo:build"));

        // Create test cache
        let cache_str = Arc::new(Mutex::new(String::new()));
        let strategy = TestCacheStrategy {
            cache: cache_str.clone(),
        };
        let mut cache = Cache::new(project.clone(), Some("foo:build"));
        cache.strategies = vec![Box::new(strategy)];

        // Should error without existing output files
        let res = cache.put("foo:build");
        assert!(res.is_err());

        // Create log and output files
        let mut log_file = std::fs::File::create(project.get_recipe_log_path("foo:build")).unwrap();
        log_file.write_all(b"foo").unwrap();

        // Create target dir
        std::fs::create_dir(project.root_path.join("foo/target")).unwrap();

        // Create output file
        let mut output_file =
            std::fs::File::create(project.root_path.join("foo/target/foo_test.txt")).unwrap();
        output_file.write_all(b"foo").unwrap();

        let res = cache.put("foo:build");
        assert!(res.is_ok());
        assert_eq!(cache_str.lock().unwrap().as_str(), FOO_BUILD_HASH);
    }
}
