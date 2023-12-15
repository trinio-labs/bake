mod local;
mod s3;

use std::{
    collections::{BTreeMap, HashMap},
    fs::File,
    path::PathBuf,
};

use flate2::{write::GzEncoder, Compression, GzBuilder};
use log::warn;
use serde::Serialize;

use crate::project::BakeProject;

pub trait CacheStrategy {
    fn get(&self, key: &str) -> Option<CacheResult>;
    fn put(&mut self, key: &str, archive_path: PathBuf) -> Result<(), String>;
}

pub struct CacheResultData {
    stdout: String,
    files: BTreeMap<String, String>,
}

pub enum CacheResult {
    Hit(CacheResultData),
    Miss(),
}

#[derive(Debug, Serialize)]
struct CacheData {
    recipe: String,
    deps: BTreeMap<String, String>,
}

pub struct Cache<'a> {
    project: &'a BakeProject,
    strategies: Vec<Box<dyn CacheStrategy>>,
    hashes: HashMap<String, String>,
}

impl<'a> Cache<'a> {
    pub fn new(project: &'a BakeProject) -> Self {
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

        Self {
            project,
            strategies,
            hashes: HashMap::new(),
        }
    }

    pub fn get(&mut self, recipe_name: &str) -> CacheResult {
        let hash = self.calculate_total_hash(recipe_name);
        if let Some(cache_hit) = self
            .strategies
            .iter()
            .find_map(|strategy| strategy.get(&hash))
        {
            return cache_hit;
        }

        CacheResult::Miss()
    }

    fn get_cached_hash(&mut self, recipe_name: &str) -> Result<String, String> {
        if let Some(recipe_hash) = self.hashes.get(recipe_name) {
            Ok(recipe_hash.clone())
        } else {
            let res = self
                .project
                .recipes
                .get(recipe_name)
                .unwrap()
                .get_recipe_hash();
            if let Ok(hash) = res.clone() {
                self.hashes.insert(recipe_name.to_owned(), hash.clone());
            }
            res
        }
    }

    fn calculate_total_hash(&mut self, recipe_name: &str) -> String {
        let mut cache_data = CacheData {
            recipe: recipe_name.to_owned(),
            deps: BTreeMap::new(),
        };

        if let Ok(recipe_hash) = self.get_cached_hash(recipe_name) {
            cache_data.recipe = recipe_hash;
        };

        if let Some(deps) = self.project.dependency_map.get(recipe_name) {
            cache_data.deps = deps.iter().fold(BTreeMap::new(), |mut acc, x| {
                if let Ok(hash) = self.get_cached_hash(x) {
                    acc.insert(x.clone(), hash);
                }
                acc
            });
        }

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&cache_data).unwrap().as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    pub fn put(&mut self, recipe_name: &str) -> Result<(), String> {
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

        for strategy in self.strategies.iter_mut() {
            strategy.put(recipe_name, archive_path.clone())?;
        }

        Ok(())
    }
}

#[cfg(test)]
mod test {
    use std::{cell::RefCell, io::Write, path::PathBuf, rc::Rc};

    use crate::project::BakeProject;

    use super::{Cache, CacheStrategy};

    struct TestCacheStrategy {
        cache: Rc<RefCell<String>>,
    }

    impl CacheStrategy for TestCacheStrategy {
        fn get(&self, _key: &str) -> Option<super::CacheResult> {
            None
        }
        fn put(&mut self, key: &str, _: PathBuf) -> Result<(), String> {
            self.cache.borrow_mut().push_str(key);
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
        let cache = Cache::new(&project);
        assert!(cache.hashes.is_empty());
        assert_eq!(cache.strategies.len(), 2);
    }

    #[test]
    fn get() {}

    #[test]
    fn put() {
        let project_path = PathBuf::from(config_path("/valid"));
        let project = BakeProject::from(&project_path).unwrap();
        _ = project.create_project_bake_dirs();

        // Create log and output files
        let mut log_file = std::fs::File::create(project.get_recipe_log_path("foo:build")).unwrap();
        log_file.write_all(b"foo").unwrap();

        let mut output_file =
            std::fs::File::create(project.root_path.join("target/foo_test.txt")).unwrap();
        output_file.write_all(b"foo").unwrap();

        let cache_str = Rc::new(RefCell::new(String::new()));
        let strategy = TestCacheStrategy {
            cache: cache_str.clone(),
        };

        let mut cache = Cache::new(&project);
        cache.strategies = vec![Box::new(strategy)];

        let res = cache.put("foo:build");
        println!("{:?}", res);
        assert!(res.is_ok());

        assert!(cache_str.borrow().contains("foo"));
    }
}
