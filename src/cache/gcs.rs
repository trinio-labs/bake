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

// Configuration constants
const STREAM_BUFFER_SIZE: usize = 64; // Smaller buffer for better memory efficiency

// Trait for GCS operations to enable mocking
#[async_trait]
pub trait GcsClient: Send + Sync {
    async fn download_object_streamed(
        &self,
        request: &GetObjectRequest,
        range: &Range,
    ) -> Result<
        tokio_stream::wrappers::ReceiverStream<Result<bytes::Bytes, anyhow::Error>>,
        anyhow::Error,
    >;

    async fn upload_object_streamed(
        &self,
        request: &UploadObjectRequest,
        stream: tokio_util::io::ReaderStream<tokio::io::BufReader<File>>,
        upload_type: &UploadType,
    ) -> Result<(), anyhow::Error>;
}

// Implementation for the real Google Cloud Storage client
#[async_trait]
impl GcsClient for Client {
    async fn download_object_streamed(
        &self,
        request: &GetObjectRequest,
        range: &Range,
    ) -> Result<
        tokio_stream::wrappers::ReceiverStream<Result<bytes::Bytes, anyhow::Error>>,
        anyhow::Error,
    > {
        // Convert the GCS client's stream to our expected format
        let gcs_stream = self
            .download_streamed_object(request, range)
            .await
            .map_err(|e| anyhow::anyhow!("GCS download error: {}", e))?;

        // Convert the stream items to our expected format
        let (tx, rx) = tokio::sync::mpsc::channel(STREAM_BUFFER_SIZE);
        let mut gcs_stream = gcs_stream;

        tokio::spawn(async move {
            while let Some(chunk_result) = gcs_stream.next().await {
                let converted_result = chunk_result
                    .map(|bytes| bytes)
                    .map_err(|e| anyhow::anyhow!("GCS stream error: {}", e));

                if tx.send(converted_result).await.is_err() {
                    break;
                }
            }
        });

        Ok(tokio_stream::wrappers::ReceiverStream::new(rx))
    }

    async fn upload_object_streamed(
        &self,
        request: &UploadObjectRequest,
        stream: tokio_util::io::ReaderStream<tokio::io::BufReader<File>>,
        upload_type: &UploadType,
    ) -> Result<(), anyhow::Error> {
        // Convert the result to our expected format
        self.upload_streamed_object(request, stream, upload_type)
            .await
            .map_err(|e| anyhow::anyhow!("GCS upload error: {}", e))
            .map(|_| ()) // Discard the returned object, we only care about success/failure
    }
}

#[derive(Clone)]
pub struct GcsCacheStrategy {
    pub bucket: String,
    client: Arc<dyn GcsClient>,
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
            .download_object_streamed(
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
                .upload_object_streamed(
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
                    client: Arc::new(Client::new(client_config)),
                }) as Box<dyn CacheStrategy>);
            }
        }

        bail!("Failed to create GCS Cache Strategy")
    }
}

// Constructor for testing with a mock client
impl GcsCacheStrategy {
    #[allow(dead_code)] // Used in tests
    pub fn new_for_testing(bucket: String, client: Arc<dyn GcsClient>) -> Self {
        Self { bucket, client }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use bytes::Bytes;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tokio::sync::mpsc;
    use tokio_stream::wrappers::ReceiverStream;

    // Mock GCS client for testing
    #[derive(Clone)]
    struct MockGcsClient {
        downloads: Arc<Mutex<HashMap<String, Vec<u8>>>>,
        uploads: Arc<Mutex<Vec<(String, Vec<u8>)>>>,
        should_fail_download: bool,
        should_fail_upload: bool,
    }

    impl MockGcsClient {
        fn new() -> Self {
            Self {
                downloads: Arc::new(Mutex::new(HashMap::new())),
                uploads: Arc::new(Mutex::new(Vec::new())),
                should_fail_download: false,
                should_fail_upload: false,
            }
        }

        fn with_download_data(self, key: &str, data: Vec<u8>) -> Self {
            self.downloads.lock().unwrap().insert(key.to_string(), data);
            self
        }

        fn with_fail_download(mut self, should_fail: bool) -> Self {
            self.should_fail_download = should_fail;
            self
        }

        fn with_fail_upload(mut self, should_fail: bool) -> Self {
            self.should_fail_upload = should_fail;
            self
        }

        fn get_uploads(&self) -> Vec<(String, Vec<u8>)> {
            self.uploads.lock().unwrap().clone()
        }
    }

    #[async_trait]
    impl GcsClient for MockGcsClient {
        async fn download_object_streamed(
            &self,
            request: &GetObjectRequest,
            _range: &Range,
        ) -> Result<ReceiverStream<Result<Bytes, anyhow::Error>>, anyhow::Error> {
            if self.should_fail_download {
                return Err(anyhow::anyhow!("Mock download failure"));
            }

            let downloads = self.downloads.lock().unwrap();
            if let Some(data) = downloads.get(&request.object) {
                let (tx, rx) = mpsc::channel(100);
                let data = data.clone();

                tokio::spawn(async move {
                    // Send data in chunks
                    let chunk_size = 1024;
                    for chunk in data.chunks(chunk_size) {
                        if tx.send(Ok(Bytes::from(chunk.to_vec()))).await.is_err() {
                            break;
                        }
                    }
                });

                Ok(ReceiverStream::new(rx))
            } else {
                Err(anyhow::anyhow!("Object not found"))
            }
        }

        async fn upload_object_streamed(
            &self,
            _request: &UploadObjectRequest,
            mut stream: tokio_util::io::ReaderStream<tokio::io::BufReader<File>>,
            _upload_type: &UploadType,
        ) -> Result<(), anyhow::Error> {
            if self.should_fail_upload {
                return Err(anyhow::anyhow!("Mock upload failure"));
            }

            let mut data = Vec::new();
            while let Some(chunk) = stream.next().await {
                match chunk {
                    Ok(bytes) => data.extend_from_slice(&bytes),
                    Err(e) => return Err(anyhow::anyhow!("Stream error: {}", e)),
                }
            }

            self.uploads
                .lock()
                .unwrap()
                .push(("test_key".to_string(), data));
            Ok(())
        }
    }

    #[tokio::test]
    async fn test_get_success() {
        let test_data = b"test archive data";
        let mock_client =
            MockGcsClient::new().with_download_data("test_key.tar.zst", test_data.to_vec());

        let strategy =
            GcsCacheStrategy::new_for_testing("test-bucket".to_string(), Arc::new(mock_client));

        let result = strategy.get("test_key").await;

        match result {
            CacheResult::Hit(data) => {
                assert!(data.archive_path.exists());
                // Verify the downloaded file contains the expected data
                let downloaded_data = std::fs::read(&data.archive_path).unwrap();
                assert_eq!(downloaded_data, test_data);

                // Clean up
                let _ = std::fs::remove_file(&data.archive_path);
            }
            CacheResult::Miss => panic!("Expected cache hit"),
        }
    }

    #[tokio::test]
    async fn test_get_miss() {
        let mock_client = MockGcsClient::new();
        let strategy =
            GcsCacheStrategy::new_for_testing("test-bucket".to_string(), Arc::new(mock_client));

        let result = strategy.get("nonexistent_key").await;
        assert!(matches!(result, CacheResult::Miss));
    }

    #[tokio::test]
    async fn test_get_download_failure() {
        let mock_client = MockGcsClient::new().with_fail_download(true);
        let strategy =
            GcsCacheStrategy::new_for_testing("test-bucket".to_string(), Arc::new(mock_client));

        let result = strategy.get("test_key").await;
        assert!(matches!(result, CacheResult::Miss));
    }

    #[tokio::test]
    async fn test_put_success() {
        let mock_client = MockGcsClient::new();
        let strategy = GcsCacheStrategy::new_for_testing(
            "test-bucket".to_string(),
            Arc::new(mock_client.clone()),
        );

        // Create a temporary test file
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&test_file_path, b"test content").unwrap();

        let result = strategy.put("test_key", test_file_path).await;
        assert!(result.is_ok());

        // Verify the upload was recorded
        let uploads = mock_client.get_uploads();
        assert_eq!(uploads.len(), 1);
        assert_eq!(uploads[0].0, "test_key");
    }

    #[tokio::test]
    async fn test_put_upload_failure() {
        let mock_client = MockGcsClient::new().with_fail_upload(true);
        let strategy =
            GcsCacheStrategy::new_for_testing("test-bucket".to_string(), Arc::new(mock_client));

        // Create a temporary test file
        let temp_dir = tempfile::tempdir().unwrap();
        let test_file_path = temp_dir.path().join("test_file.txt");
        std::fs::write(&test_file_path, b"test content").unwrap();

        let result = strategy.put("test_key", test_file_path).await;
        assert!(result.is_err());
    }

    #[tokio::test]
    async fn test_put_file_not_found() {
        let mock_client = MockGcsClient::new();
        let strategy =
            GcsCacheStrategy::new_for_testing("test-bucket".to_string(), Arc::new(mock_client));

        let result = strategy
            .put("test_key", PathBuf::from("/nonexistent/file"))
            .await;
        assert!(result.is_err());
    }
}
