use std::path::PathBuf;

use anyhow::anyhow;
use async_trait::async_trait;
use log::debug;

use crate::cache::CacheResultData;

use super::{CacheResult, CacheStrategy};

pub struct LocalCacheStrategy {
    pub path: PathBuf,
    pub base_path: PathBuf,
}

#[async_trait]
impl CacheStrategy for LocalCacheStrategy {
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = key.to_owned() + ".tar.gz";
        let archive_path = self.path.join(file_name.clone());
        debug!("Checking local cache for key {}", key);
        if archive_path.is_file() {
            debug!("Cache hit for key {}", key);
            return CacheResult::Hit(CacheResultData { archive_path });
        }
        CacheResult::Miss
    }
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
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
        let cache_path = self.path.join(key.to_owned() + ".tar.gz");
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
}
