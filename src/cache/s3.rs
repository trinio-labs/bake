use std::path::PathBuf;

use crate::project::S3CacheConfig;

use super::{CacheResult, CacheStrategy};

pub struct S3CacheStrategy {
    pub bucket: String,
    pub region: Option<String>,
    pub access_key: Option<String>,
    pub secret_key: Option<String>,
}

impl CacheStrategy for S3CacheStrategy {
    fn get(&self, key: &str) -> CacheResult {
        CacheResult::Miss
    }
    fn put(&self, key: &str, archive_path: PathBuf) -> Result<(), String> {
        Ok(())
    }
}

impl S3CacheStrategy {
    pub fn from_config(config: &S3CacheConfig) -> Self {
        Self {
            bucket: config.bucket.clone(),
            region: config.region.clone(),
            access_key: config.access_key.clone(),
            secret_key: config.secret_key.clone(),
        }
    }
}
