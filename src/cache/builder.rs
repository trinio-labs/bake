use std::{collections::HashMap, future::Future, pin::Pin, sync::Arc};

use anyhow::bail;
use log::debug;

use super::{Cache, CacheStrategy};
use crate::project::BakeProject;

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
    // hashes: HashMap<String, String>, // Removed: Combined hashes are now calculated by BakeProject
}

impl CacheBuilder {
    pub fn new(project: Arc<BakeProject>) -> Self {
        Self {
            project,
            filter: None,
            strategies: HashMap::new(),
            // hashes: HashMap::new(), // Removed
        }
    }

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

    // Removed: calculate_hash_with_deps - This logic is now handled by BakeProject::calculate_combined_hash_for
    // fn calculate_hash_with_deps(&self, recipe_name: &str) -> String { ... }

    fn calculate_all_hashes(&self) -> anyhow::Result<HashMap<String, String>> {
        let mut calculated_hashes = HashMap::new();
        let project_recipes_fqns: Vec<String> = self
            .project
            .cookbooks
            .values()
            .flat_map(|cb| {
                cb.recipes
                    .keys()
                    .map(|r_name| format!("{}:{}", cb.name, r_name))
            })
            .collect();

        for recipe_fqn in project_recipes_fqns {
            // Apply filter if present
            if let Some(filter_str) = &self.filter {
                if !recipe_fqn.contains(filter_str) {
                    continue; // Skip recipes not matching the filter
                }
            }
            debug!("Calculating combined hash for cache: {recipe_fqn}");
            // Use the new method from BakeProject
            let combined_hash = self.project.get_combined_hash_for_recipe(&recipe_fqn)?;
            calculated_hashes.insert(recipe_fqn, combined_hash);
        }
        Ok(calculated_hashes)
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

    use crate::{
        cache::{CacheResult, CacheResultData, ARCHIVE_EXTENSION},
        test_utils::TestProjectBuilder,
    };

    use super::*;

    #[derive(Clone, Debug, Default)]
    struct TestCacheStrategy {
        pub get_called: Arc<Mutex<String>>,
        pub put_called: Arc<Mutex<String>>,
    }

    #[async_trait]
    impl CacheStrategy for TestCacheStrategy {
        async fn get(&self, key: &str) -> CacheResult {
            self.get_called.lock().unwrap().push_str(key);
            CacheResult::Hit(CacheResultData {
                archive_path: PathBuf::from(format!("{key}.{ARCHIVE_EXTENSION}")),
            })
        }
        async fn put(&self, key: &str, _: PathBuf) -> anyhow::Result<()> {
            self.put_called.lock().unwrap().push_str(key);
            Ok(())
        }
        async fn from_config(_: Arc<BakeProject>) -> anyhow::Result<Box<dyn super::CacheStrategy>> {
            Ok(Box::<TestCacheStrategy>::default())
        }
    }

    #[tokio::test]
    async fn build() {
        let project = Arc::new(
            TestProjectBuilder::new()
                .with_cookbook("foo", &["build"])
                .build(),
        );
        let mut builder = CacheBuilder::new(project);

        let cache = builder
            .add_strategy("local", TestCacheStrategy::from_config)
            .add_strategy("s3", TestCacheStrategy::from_config)
            .add_strategy("gcs", TestCacheStrategy::from_config)
            .build()
            .await
            .unwrap();
        assert!(cache.hashes.contains_key("foo:build"));
    }
}
