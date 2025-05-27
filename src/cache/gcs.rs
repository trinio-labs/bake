use std::{path::PathBuf, sync::Arc};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;
use tokio_stream::StreamExt;

use anyhow::bail;
use async_trait::async_trait;
use log::{debug, error, warn};

use crate::{
    cache::{CacheResultData, ARCHIVE_EXTENSION},
    project::BakeProject,
};

use google_cloud_storage::{
    client::{Client, ClientConfig},
    http::objects::{
        download::Range,
        get::GetObjectRequest,
        upload::{Media, UploadObjectRequest, UploadType},
    },
};

use super::{CacheResult, CacheStrategy};

#[derive(Clone)]
pub struct GcsCacheStrategy {
    pub bucket: String,
    client: Client,
}

impl std::fmt::Debug for GcsCacheStrategy {
    #[coverage(off)]
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gcs")
    }
}

#[async_trait]
impl CacheStrategy for GcsCacheStrategy {
    #[coverage(off)]
    async fn get(&self, key: &str) -> CacheResult {
        let file_name = format!("{key}.{ARCHIVE_EXTENSION}");
        let archive_path = std::env::temp_dir().join(&file_name);

        debug!("Getting key {key} from GCS");
        match self
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
            Ok(mut data) => {
                debug!("Key {key} exists in GCS, downloading...");
                match File::create(archive_path.clone()).await {
                    Ok(mut file) => {
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
                        if let Err(err) = file.shutdown().await {
                            error!("Error saving archive file: {err:?}");
                            return CacheResult::Miss;
                        }

                        return CacheResult::Hit(CacheResultData { archive_path });
                    }
                    Err(err) => {
                        debug!(
                            "GCS Cache Strategy failed to create file in temp dir: {}: {}",
                            archive_path.display(),
                            err
                        );
                        return CacheResult::Miss;
                    }
                }
            }
            Err(err) => {
                debug!("Error retrieving key {key} from GCS: {err}");
                return CacheResult::Miss;
            }
        }
    }

    #[coverage(off)]
    async fn put(&self, key: &str, archive_path: PathBuf) -> anyhow::Result<()> {
        let file_name = format!("{key}.{ARCHIVE_EXTENSION}");
        let upload_type = UploadType::Simple(Media::new(file_name.clone()));
        debug!("Uploading key {key} to GCS");
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
    #[coverage(off)]
    async fn from_config(config: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
        let client_config = ClientConfig::default().with_auth().await?;
        if let Some(remotes) = &config.config.cache.remotes {
            if let Some(gcs) = &remotes.gcs {
                return Ok(Box::new(Self {
                    bucket: gcs.bucket.clone(),
                    client: Client::new(client_config),
                }) as Box<dyn CacheStrategy>);
            }
        }

        bail!("Failed to create GCS Cache Strategy")
    }
}
