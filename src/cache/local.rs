use std::path::PathBuf;

use crate::project::LocalCacheConfig;

use super::{CacheResult, CacheStrategy};

pub struct LocalCacheStrategy {
    pub path: PathBuf,
}

impl CacheStrategy for LocalCacheStrategy {
    fn get(&self, key: &str) -> CacheResult {
        CacheResult::Miss
    }
    fn put(&self, key: &str, archive_path: PathBuf) -> Result<(), String> {
        Ok(())
    }
}
