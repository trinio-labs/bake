use std::io::Write;
use std::{fs::File, path::PathBuf};

use anyhow::anyhow;
use async_trait::async_trait;
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use log::{debug, warn};

use crate::project::S3CacheConfig;

use super::{CacheResult, CacheResultData, CacheStrategy};

pub struct S3CacheStrategy {
    pub bucket: String,
    pub region: Option<String>,
    client: Client,
}

#[async_trait]
impl CacheStrategy for S3CacheStrategy {
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = format!("{}.tar.gz", key);
        // Try to get file with key from bucket
        let archive_path = std::env::temp_dir().join(&file_name);
        let file = File::create(archive_path.clone());

        if file.is_err() {
            warn!("Failed to create file in temp dir: {}", file.unwrap_err());
            return CacheResult::Miss;
        }

        match self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&file_name)
            .send()
            .await
        {
            Ok(mut object) => {
                while let Some(bytes) = match object.body.try_next().await {
                    Ok(Some(bytes)) => Some(bytes),
                    Ok(None) => None,
                    Err(err) => {
                        warn!("Failed to read object body with key {file_name}: {err:?}");
                        return CacheResult::Miss;
                    }
                } {
                    let mut file = file.as_ref().unwrap();
                    if file.write_all(&bytes).is_err() {
                        warn!(
                            "Failed to write to file in temp dir: {}",
                            archive_path.display()
                        );
                        return CacheResult::Miss;
                    };
                }

                return CacheResult::Hit(CacheResultData { archive_path });
            }
            Err(err) => {
                debug!("Failed to get object with key {file_name}: {err:?}");
                return CacheResult::Miss;
            }
        };
    }
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
        let file_name = format!("{key}.tar.gz");
        let body = ByteStream::from_path(archive_path).await?;

        let output = self
            .client
            .put_object()
            .bucket(&self.bucket)
            .key(&file_name)
            .body(body)
            .send()
            .await;

        match output {
            Ok(_) => Ok(()),
            Err(err) => Err(anyhow!(
                "Failed to put object with key {file_name}: {err:?}"
            )),
        }
    }
}

impl S3CacheStrategy {
    pub async fn from_config(config: &S3CacheConfig) -> Self {
        let region_provider =
            RegionProviderChain::first_try(config.region.clone().map(Region::new))
                .or_default_provider()
                .or_else("us-east-1");
        let aws_config = aws_config::defaults(BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        Self {
            bucket: config.bucket.clone(),
            region: config.region.clone(),
            client: Client::new(&aws_config),
        }
    }
}
