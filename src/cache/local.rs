use std::fs::File;
use std::path::PathBuf;

use flate2::read::GzDecoder;
use log::{debug, warn};

use crate::cache::CacheResultData;

use super::{CacheResult, CacheStrategy};

pub struct LocalCacheStrategy {
    pub path: PathBuf,
    pub base_path: PathBuf,
}

impl CacheStrategy for LocalCacheStrategy {
    fn get(&self, key: &str) -> CacheResult {
        let file_name = key.to_owned() + ".tar.gz";
        debug!("Checking local cache for key {}", key);
        if let Ok(tar_gz) = File::open(self.path.join(file_name.clone())) {
            let tar = GzDecoder::new(tar_gz);
            let mut archive = tar::Archive::new(tar);
            if archive.unpack(self.base_path.clone()).is_err() {
                warn!("Failed to unpack tar.gz file: {file_name}");
                return CacheResult::Miss;
            }

            debug!("Cache hit for key {}", key);
            return CacheResult::Hit(CacheResultData {
                stdout: "".to_string(),
            });
        }
        CacheResult::Miss
    }
    fn put(&self, key: &str, archive_path: PathBuf) -> Result<(), String> {
        // Create cache dir if it doesn't exist
        if !self.path.exists() {
            match std::fs::create_dir_all(&self.path) {
                Ok(_) => (),
                Err(err) => {
                    return Err(format!(
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
            Err(format!(
                "Failed to copy archive to cache folder {}: {}",
                cache_path.display(),
                err
            ))
        } else {
            Ok(())
        }
    }
}
