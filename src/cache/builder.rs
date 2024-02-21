use std::{
    collections::{BTreeMap, HashMap},
    future::Future,
    pin::Pin,
    sync::Arc,
};

use anyhow::bail;
use log::debug;
use serde::Serialize;

use super::{Cache, CacheStrategy};
use crate::project::BakeProject;

#[derive(Debug, Serialize)]
struct CacheData {
    recipe: String,
    deps: BTreeMap<String, String>,
}

type StrategyConstructor = Box<
    dyn Fn(
        Arc<BakeProject>,
    ) -> Pin<
        Box<dyn Future<Output = anyhow::Result<Box<dyn CacheStrategy>>> + Send + 'static>,
    >,
>;
/// CacheBuilder is a builder for a Cache
pub struct CacheBuilder {
    project: Arc<BakeProject>,

    filter: Option<String>,

    strategies: HashMap<String, StrategyConstructor>,

    hashes: HashMap<String, String>,
}

impl CacheBuilder {
    pub fn new(project: Arc<BakeProject>) -> Self {
        Self {
            project,
            filter: None,
            strategies: HashMap::new(),
            hashes: HashMap::new(),
        }
    }

    #[cfg_attr(coverage, coverage(off))]
    pub fn default_strategies(&mut self) -> &mut Self {
        self.add_strategy("local", super::local::LocalCacheStrategy::from_config);
        self.add_strategy("s3", super::s3::S3CacheStrategy::from_config);
        self.add_strategy("gcs", super::gcs::GcsCacheStrategy::from_config);
        self
    }

    pub fn add_strategy<F>(&mut self, name: &str, from_config: F) -> &mut Self
    where
        F: Fn(
            Arc<BakeProject>,
        )
            -> Pin<Box<dyn Future<Output = anyhow::Result<Box<dyn CacheStrategy>>> + Send>>,
        F: Send + Sync + 'static,
    {
        self.strategies
            .insert(name.to_owned(), Box::new(from_config));
        self
    }

    pub fn filter(&mut self, filter: &str) -> &mut Self {
        self.filter = Some(filter.to_owned());
        self
    }

    fn calculate_hash_with_deps(&self, recipe_name: &str) -> String {
        debug!("Calculating total hash for {}", recipe_name);
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

        debug!("Total cache data: {:?}", cache_data);

        let mut hasher = blake3::Hasher::new();
        hasher.update(serde_json::to_string(&cache_data).unwrap().as_bytes());
        hasher.finalize().to_hex().to_string()
    }

    fn calculate_all_hashes(&mut self) -> anyhow::Result<HashMap<String, String>> {
        let recipes = self.project.get_recipes(self.filter.as_deref());

        self.hashes = recipes
            .iter()
            .map(|(name, recipe)| match recipe.get_recipe_hash() {
                Ok(hash) => Ok((name.clone(), hash)),
                Err(e) => Err(e),
            })
            .collect::<anyhow::Result<_>>()?;

        recipes
            .keys()
            .map(|name| {
                let hash = self.calculate_hash_with_deps(name);
                Ok((name.clone(), hash))
            })
            .collect()
    }

    pub async fn build(&mut self) -> anyhow::Result<Cache> {
        let mut strategies: Vec<Arc<Box<dyn CacheStrategy>>> = Vec::new();

        let mut order = self.project.config.cache.order.clone();
        // If no order is defined, use local -> s3 -> gcs if configuration exists
        if order.is_empty() {
            if self.project.config.cache.local.enabled {
                order.push("local".to_string());
            }
            if let Some(remotes) = &self.project.config.cache.remotes {
                if remotes.s3.is_some() {
                    order.push("s3".to_string());
                }
                if remotes.gcs.is_some() {
                    order.push("gcs".to_string());
                }
            }
        }

        for item in &order {
            if let Some(build_fn) = self.strategies.get(item) {
                let built_strategy = build_fn(self.project.clone()).await?;
                strategies.push(Arc::new(built_strategy));
            } else {
                bail!("No cache strategy implementation found for {}", item);
            }
        }

        Ok(Cache {
            project: self.project.clone(),
            strategies,
            hashes: self.calculate_all_hashes()?,
        })
    }
}

#[cfg(test)]
mod tests {
    use std::{path::PathBuf, sync::Mutex};

    use async_trait::async_trait;

    use crate::cache::{CacheResult, CacheResultData};

    use super::*;

    #[derive(Clone, Debug, Default)]
    struct TestCacheStrategy {
        pub get_called: Arc<Mutex<String>>,
        pub put_called: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        #[cfg_attr(coverage, coverage(off))]
        async fn get(&self, key: &str) -> CacheResult {
            self.get_called.lock().unwrap().push_str(key);
            CacheResult::Hit(CacheResultData {
                archive_path: PathBuf::from(format!("{}.tar.gz", key)),
            })
        }
        #[cfg_attr(coverage, coverage(off))]
        async fn put(&self, key: &str, _: PathBuf) -> anyhow::Result<()> {
            self.put_called.lock().unwrap().push_str(key);
            Ok(())
        }
        #[cfg_attr(coverage, coverage(off))]
        async fn from_config(_: Arc<BakeProject>) -> anyhow::Result<Box<dyn super::CacheStrategy>> {
            Ok(Box::<TestCacheStrategy>::default())
        }
    }

    #[tokio::test]
    async fn build() {
        let project = Arc::new(BakeProject::from(&PathBuf::from("resources/tests/valid")).unwrap());
        let mut builder = CacheBuilder::new(project);

        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build()
            .await
            .unwrap();
        assert!(cache.hashes.get("foo:build").is_some());
    }
}
