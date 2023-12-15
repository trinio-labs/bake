use std::path::PathBuf;

use crate::project::LocalCacheConfig;

use super::{CacheResult, CacheStrategy};

pub struct LocalCacheStrategy {
    pub path: PathBuf,
}

impl CacheStrategy for LocalCacheStrategy {
    fn get(&self, key: &str) -> Option<CacheResult> {
        None
    }
    fn put(&mut self, key: &str, archive_path: PathBuf) -> Result<(), String> {
        Ok(())
    }
}
