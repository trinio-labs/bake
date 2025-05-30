use futures_core::Stream;
use futures_util::StreamExt;
use std::pin::Pin;
use std::{path::PathBuf, sync::Arc};
use tokio::fs::File;
use tokio::io::AsyncWriteExt;

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
type GcsBytesStream =
    Pin<Box<dyn Stream<Item = Result<bytes::Bytes, anyhow::Error>> + Send + 'static>>;

#[async_trait]
pub trait GcsClient: Send + Sync {
    async fn download_streamed_object(
        &self,
        req: &GetObjectRequest,
        range: &Range,
    ) -> anyhow::Result<GcsBytesStream>;

    async fn upload_streamed_object(
        &self,
        req: &UploadObjectRequest,
        stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static>>,
        upload_type: &UploadType,
    ) -> anyhow::Result<google_cloud_storage::http::objects::Object>;
}

#[async_trait]
impl GcsClient for Client {
    async fn download_streamed_object(
        &self,
        req: &GetObjectRequest,
        range: &Range,
    ) -> anyhow::Result<GcsBytesStream> {
        Ok(Box::pin(self.download_streamed_object(req, range).await?))
    }

    async fn upload_streamed_object(
        &self,
        req: &UploadObjectRequest,
        stream: Pin<Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static>>,
        upload_type: &UploadType,
    ) -> anyhow::Result<google_cloud_storage::http::objects::Object> {
        Ok(self
            .upload_streamed_object(req, stream, upload_type)
            .await?)
    }
}

#[derive(Clone)]
pub struct GcsCacheStrategy {
    pub bucket: String,
    client: Arc<dyn GcsClient>,
}

impl std::fmt::Debug for GcsCacheStrategy {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "Gcs")
    }
}

#[async_trait]
impl CacheStrategy for GcsCacheStrategy {
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
                    file_stream.boxed(),
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
    async fn from_config(config: Arc<BakeProject>) -> anyhow::Result<Box<dyn CacheStrategy>> {
        let client_config = ClientConfig::default().with_auth().await?;
        if let Some(remotes) = &config.config.cache.remotes {
            if let Some(gcs) = &remotes.gcs {
                return Ok(Box::new(Self {
                    bucket: gcs.bucket.clone(),
                    client: Arc::new(Client::new(client_config)),
                }) as Box<dyn CacheStrategy>);
            }
        }

        bail!("Failed to create GCS Cache Strategy")
    }
}

#[cfg(test)]
mod tests {
    use std::{pin::Pin, sync::Arc};

    use async_trait::async_trait;
    use futures_core::Stream;
    use google_cloud_storage::http::objects::{
        download::Range,
        get::GetObjectRequest,
        upload::{UploadObjectRequest, UploadType},
    };
    use tokio::sync::Mutex;

    use crate::cache::{CacheResult, CacheStrategy};

    use super::{GcsBytesStream, GcsCacheStrategy, GcsClient};

    struct MockGcsClient {
        download_result: Mutex<Option<anyhow::Result<GcsBytesStream>>>,
        upload_result: Mutex<Option<anyhow::Result<google_cloud_storage::http::objects::Object>>>,
    }

    impl MockGcsClient {
        fn new() -> Self {
            Self {
                download_result: Mutex::new(None),
                upload_result: Mutex::new(None),
            }
        }

        async fn set_download_result(&mut self, result: anyhow::Result<GcsBytesStream>) {
            self.download_result.lock().await.replace(result);
        }

        async fn set_upload_result(
            &mut self,
            result: anyhow::Result<google_cloud_storage::http::objects::Object>,
        ) {
            self.upload_result.lock().await.replace(result);
        }
    }

    #[async_trait]
    impl GcsClient for MockGcsClient {
        async fn download_streamed_object(
            &self,
            _: &GetObjectRequest,
            _: &Range,
        ) -> anyhow::Result<GcsBytesStream> {
            let mut lock = self.download_result.lock().await;
            match lock.take() {
                Some(result) => result,
                None => Ok(Box::pin(futures_util::stream::empty())),
            }
        }

        async fn upload_streamed_object(
            &self,
            _req: &UploadObjectRequest,
            _stream: Pin<
                Box<dyn Stream<Item = Result<bytes::Bytes, std::io::Error>> + Send + 'static>,
            >,
            _upload_type: &UploadType,
        ) -> anyhow::Result<google_cloud_storage::http::objects::Object> {
            let mut lock = self.upload_result.lock().await;
            match lock.take() {
                Some(result) => result,
                None => Err(anyhow::anyhow!("No upload result set")),
            }
        }
    }

    #[tokio::test]
    async fn test_put_success() {
        let mut mock_client = MockGcsClient::new();
        mock_client
            .set_upload_result(Ok(google_cloud_storage::http::objects::Object {
                name: "test-key.tar.gz".to_string(),
                ..Default::default()
            }))
            .await;
        let strategy = GcsCacheStrategy {
            bucket: "test-bucket".to_string(),
            client: Arc::new(mock_client),
        };
        // Create a dummy file to upload
        let temp_dir = tempfile::tempdir().unwrap();
        let archive_path = temp_dir.path().join("test-key.tar.gz");
        tokio::fs::write(&archive_path, "test data").await.unwrap();

        let result = strategy.put("test-key", archive_path).await;
        assert!(result.is_ok());
    }

    #[tokio::test]
    async fn test_put_failure() {
        let mut mock_client = MockGcsClient::new();
        mock_client
            .set_upload_result(Err(anyhow::anyhow!("Upload failed")))
            .await;
        let strategy = GcsCacheStrategy {
            bucket: "test-bucket".to_string(),
            client: Arc::new(mock_client),
        };
        let temp_dir = tempfile::tempdir().unwrap();
        let archive_path = temp_dir.path().join("test-key.tar.gz");
        tokio::fs::write(&archive_path, "test data").await.unwrap();

        let result = strategy.put("test-key", archive_path).await;
        assert!(result.is_err());
        assert_eq!(
            result.unwrap_err().to_string(),
            "GCS Cache Strategy failed to upload file: Upload failed"
        );
    }

    #[tokio::test]
    async fn test_get_success() {
        let mut mock_client = MockGcsClient::new();
        let content = "test content";
        let stream_item = Ok(bytes::Bytes::from(content));
        let stream = futures_util::stream::once(async { stream_item });
        mock_client.set_download_result(Ok(Box::pin(stream))).await;

        let strategy = GcsCacheStrategy {
            bucket: "test-bucket".to_string(),
            client: Arc::new(mock_client),
        };

        let result = strategy.get("test").await;
        assert!(matches!(result, CacheResult::Hit(_)))
    }

    #[tokio::test]
    async fn test_get_failure() {
        let mut mock_client = MockGcsClient::new();
        mock_client
            .set_download_result(Err(anyhow::anyhow!("Download failed")))
            .await;

        let strategy = GcsCacheStrategy {
            bucket: "test-bucket".to_string(),
            client: Arc::new(mock_client),
        };

        let result = strategy.get("test").await;
        assert!(matches!(result, CacheResult::Miss))
    }
}
