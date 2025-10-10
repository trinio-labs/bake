use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

use anyhow::{anyhow, bail};
use async_trait::async_trait;
use aws_config::{meta::region::RegionProviderChain, BehaviorVersion, Region};
use aws_sdk_s3::primitives::ByteStream;
use aws_sdk_s3::Client;
use log::{debug, warn};

use crate::project::BakeProject;

use super::{cache_file_name, CacheResult, CacheResultData, CacheStrategy};

#[derive(Clone, Debug)]
pub struct S3CacheStrategy {
    pub bucket: String,
    client: Client,
}

#[async_trait]
impl CacheStrategy for S3CacheStrategy {
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = cache_file_name(key);
        // Try to get file with key from bucket
        let archive_path = std::env::temp_dir().join(&file_name);
        let mut file = match File::create(&archive_path).await {
            Ok(f) => f,
            Err(err) => {
                warn!("Failed to create file in temp dir: {err}");
                return CacheResult::Miss;
            }
        };

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
                    if file.write_all(&bytes).await.is_err() {
                        warn!(
                            "Failed to write to file in temp dir: {}",
                            archive_path.display()
                        );
                        return CacheResult::Miss;
                    };
                }

                // Ensure all data is flushed to disk before returning
                if let Err(err) = file.shutdown().await {
                    warn!("Error saving archive file: {err:?}");
                    return CacheResult::Miss;
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
        let file_name = cache_file_name(key);
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
    async fn from_config(config: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
        if let Some(remotes) = &config.config.cache.remotes {
            if let Some(s3) = &remotes.s3 {
                let region_provider =
                    RegionProviderChain::first_try(s3.region.clone().map(Region::new))
                        .or_default_provider()
                        .or_else("us-east-1");
                let aws_config = aws_config::defaults(BehaviorVersion::latest())
                    .region(region_provider)
                    .load()
                    .await;
                return Ok(Box::new(Self {
                    bucket: s3.bucket.clone(),
                    client: Client::new(&aws_config),
                }));
            }
        }

        bail!("Failed to create S3 Cache Strategy")
    }
}
