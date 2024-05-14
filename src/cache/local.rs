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
            debug!("Cache hit for key {}", key);
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
