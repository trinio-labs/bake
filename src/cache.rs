mod local;
mod s3;

use std::collections::HashMap;

use log::warn;

use crate::project::{BakeProject, LocalCacheConfig};

pub trait CacheStrategy {
    fn get(&self, key: &str) -> Option<CacheResult>;
    fn put(&mut self, key: &str, value: CacheResult) -> Result<(), String>;
}

pub struct CacheResult {}

pub struct Cache {
    strategies: Vec<Box<dyn CacheStrategy>>,
    hashes: HashMap<String, String>,
}

impl Cache {
    pub fn new(project: &BakeProject) -> Self {
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
            strategies,
            hashes: HashMap::new(),
        }
    }
}

#[cfg(test)]
mod test {
    use std::path::PathBuf;

    use crate::project::BakeProject;

    use super::Cache;

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
}
