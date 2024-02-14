use std::path::PathBuf;
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

use anyhow::bail;
use async_trait::async_trait;
use log::{debug, warn};

use crate::{cache::CacheResultData, project::GcsCacheConfig};

use google_cloud_storage::{
    client::{Client, ClientConfig},
    http::objects::{
        download::Range,
        get::GetObjectRequest,
        upload::{Media, UploadObjectRequest, UploadType},
    },
};

use super::{CacheResult, CacheStrategy};

pub struct GcsCacheStrategy {
    pub bucket: String,
    client: Client,
}

#[async_trait]
impl CacheStrategy for GcsCacheStrategy {
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = format!("{}.tar.gz", key);
        let archive_path = std::env::temp_dir().join(&file_name);

        debug!("Getting key {key} from GCS");
        if let Ok(mut data) = self
            .client
            .download_streamed_object(
                &GetObjectRequest {
                    bucket: self.bucket.clone(),
                    object: file_name,
                    ..Default::default()
                },
                &Range::default(),
            )
            .await
        {
            debug!("Key {key} exists in GCS, downloading...");
            if let Ok(mut file) = File::create(archive_path.clone()).await {
                while let Some(bytes) = data.next().await {
                    if let Ok(bytes) = bytes {
                        if file.write_all(&bytes).await.is_err() {
                            warn!(
                                "GCS Cache Strategy failed to write to file in temp dir: {}",
                                archive_path.display()
                            );
                            return CacheResult::Miss;
                        }
                    }
                }

                debug!(
                    "Key downloaded from GCS, saved as {}",
                    archive_path.display()
                );

                return CacheResult::Hit(CacheResultData { archive_path });
            }
        }

        debug!("Key {key} does not exist in GCS");
        CacheResult::Miss
    }
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
        let file_name = format!("{}.tar.gz", key);
        let upload_type = UploadType::Simple(Media::new(file_name.clone()));
        if let Ok(file) = File::open(&archive_path).await {
            let buf_reader = tokio::io::BufReader::new(file);
            let file_stream = tokio_util::io::ReaderStream::new(buf_reader);

            match self
                .client
                .upload_streamed_object(
                    &UploadObjectRequest {
                        bucket: self.bucket.clone(),
                        ..Default::default()
                    },
                    file_stream,
                    &upload_type,
                )
                .await
            {
                Ok(_) => Ok(()),
                Err(e) => {
                    bail!("GCS Cache Strategy failed to upload file: {}", e)
                }
            }
        } else {
            bail!(
                "GCS Cache Strategy failed to archive: {}",
                archive_path.display()
            );
        }
    }
}

impl GcsCacheStrategy {
    pub async fn from_config(config: &GcsCacheConfig) -> anyhow::Result<Self> {
        let client_config = ClientConfig::default().with_auth().await?;
        Ok(Self {
            bucket: config.bucket.clone(),
            client: Client::new(client_config),
        })
    }
}
