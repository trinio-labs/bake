use super::blob_hash::BlobHash;
use super::blob_store::BlobStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use google_cloud_storage::client::{Storage, StorageControl};
use log::debug;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// GCS-backed blob store for remote caching
pub struct GcsBlobStore {
    /// GCS bucket name (just the bucket id, not the full path)
    bucket_id: String,

    /// Full bucket path for API calls (projects/_/buckets/{bucket_id})
    bucket_path: String,

    /// GCS Storage client (for read/write operations)
    storage: Storage,

    /// GCS StorageControl client (for metadata operations)
    control: StorageControl,

    /// Optional key prefix for organizing blobs
    prefix: Option<String>,

    /// Semaphore to limit concurrent GCS operations
    upload_sem: Arc<Semaphore>,
    download_sem: Arc<Semaphore>,
}

impl GcsBlobStore {
    /// Create a new GCS blob store
    pub async fn new(bucket: String, prefix: Option<String>) -> Result<Self> {
        let storage = Storage::builder()
            .build()
            .await
            .context("Failed to create GCS Storage client")?;

        let control = StorageControl::builder()
            .build()
            .await
            .context("Failed to create GCS StorageControl client")?;

        // Format bucket path for API calls
        let bucket_path = format!("projects/_/buckets/{}", bucket);

        debug!("GcsBlobStore initialized for bucket: {}", bucket);

        Ok(Self {
            bucket_id: bucket,
            bucket_path,
            storage,
            control,
            prefix,
            upload_sem: Arc::new(Semaphore::new(8)),
            download_sem: Arc::new(Semaphore::new(16)),
        })
    }

    /// Get GCS object name for a blob hash
    fn get_object_name(&self, hash: &BlobHash) -> String {
        let shard = hash.shard_prefix();
        let hash_str = hash.hash_hex();
        let key_path = format!("{}/{}/{}", hash.algorithm, shard, hash_str);

        match &self.prefix {
            Some(prefix) => format!("{}/{}", prefix, key_path),
            None => key_path,
        }
    }

    /// Check if an error indicates "not found"
    fn is_not_found_error(err: &google_cloud_storage::Error) -> bool {
        let err_str = err.to_string();
        err_str.contains("404")
            || err_str.contains("Not Found")
            || err_str.contains("not found")
            || err_str.contains("NoSuchKey")
    }
}

#[async_trait]
impl BlobStore for GcsBlobStore {
    async fn contains(&self, hash: &BlobHash) -> Result<bool> {
        let object_name = self.get_object_name(hash);

        match self
            .control
            .get_object()
            .set_bucket(&self.bucket_path)
            .set_object(&object_name)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                if Self::is_not_found_error(&err) {
                    Ok(false)
                } else {
                    Err(anyhow::anyhow!("{}", err))
                        .context(format!("GCS operational error for {}", object_name))
                }
            }
        }
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let _permit = self.download_sem.acquire().await?;
        let object_name = self.get_object_name(hash);

        let mut reader = self
            .storage
            .read_object(&self.bucket_path, &object_name)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
            .context(format!("Failed to get blob {} from GCS", hash))?;

        // Collect all chunks into a vector
        let mut data = Vec::new();
        while let Some(chunk) = reader.next().await {
            let chunk = chunk
                .map_err(|e| anyhow::anyhow!("{}", e))
                .context(format!("Failed to read blob {} chunk from GCS", hash))?;
            data.extend_from_slice(&chunk);
        }

        let bytes = Bytes::from(data);
        debug!("Downloaded blob {} from GCS ({} bytes)", hash, bytes.len());
        Ok(bytes)
    }

    async fn put(&self, content: Bytes) -> Result<BlobHash> {
        let _permit = self.upload_sem.acquire().await?;

        // Hash the content
        let hash = BlobHash::from_content(&content);

        // Check if already exists (avoid redundant upload)
        if self.contains(&hash).await? {
            debug!("Blob {} already exists in GCS, skipping upload", hash);
            return Ok(hash);
        }

        let object_name = self.get_object_name(&hash);

        self.storage
            .write_object(&self.bucket_path, &object_name, content.clone())
            .send_buffered()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
            .context(format!("Failed to upload blob {} to GCS", hash))?;

        debug!("Uploaded blob {} to GCS ({} bytes)", hash, content.len());
        Ok(hash)
    }

    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        let tasks: Vec<_> = hashes
            .iter()
            .map(|hash| {
                let hash = hash.clone();
                let store = self.clone();
                async move { store.contains(&hash).await }
            })
            .collect();

        let results = futures_util::future::join_all(tasks).await;
        results.into_iter().collect()
    }

    async fn get_many(&self, hashes: &[BlobHash]) -> Result<Vec<Bytes>> {
        let tasks: Vec<_> = hashes
            .iter()
            .map(|hash| {
                let hash = hash.clone();
                let store = self.clone();
                async move { store.get(&hash).await }
            })
            .collect();

        futures_util::future::try_join_all(tasks).await
    }

    async fn put_many(&self, contents: Vec<Bytes>) -> Result<Vec<BlobHash>> {
        let tasks: Vec<_> = contents
            .into_iter()
            .map(|content| {
                let store = self.clone();
                async move { store.put(content).await }
            })
            .collect();

        futures_util::future::try_join_all(tasks).await
    }

    async fn delete(&self, hash: &BlobHash) -> Result<()> {
        let object_name = self.get_object_name(hash);

        self.control
            .delete_object()
            .set_bucket(&self.bucket_path)
            .set_object(&object_name)
            .send()
            .await
            .map_err(|e| anyhow::anyhow!("{}", e))
            .context(format!("Failed to delete blob {} from GCS", hash))?;

        debug!("Deleted blob {} from GCS", hash);
        Ok(())
    }

    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        let object_name = self.get_object_name(hash);

        match self
            .control
            .get_object()
            .set_bucket(&self.bucket_path)
            .set_object(&object_name)
            .send()
            .await
        {
            Ok(object) => Ok(Some(object.size as u64)),
            Err(err) => {
                if Self::is_not_found_error(&err) {
                    Ok(None)
                } else {
                    Err(anyhow::anyhow!("{}", err))
                        .context(format!("GCS operational error for {}", object_name))
                }
            }
        }
    }

    async fn list(&self) -> Result<Vec<BlobHash>> {
        let prefix = match &self.prefix {
            Some(p) => format!("{}/", p),
            None => String::new(),
        };

        let mut hashes = Vec::new();
        let mut page_token: Option<String> = None;

        // Manual pagination through list_objects
        loop {
            let mut request = self
                .control
                .list_objects()
                .set_parent(&self.bucket_path)
                .set_prefix(&prefix);

            if let Some(token) = &page_token {
                request = request.set_page_token(token);
            }

            let response = request
                .send()
                .await
                .map_err(|e| anyhow::anyhow!("{}", e))
                .context("Failed to list GCS objects")?;

            for object in response.objects {
                // Extract hash from object name: [prefix/]algorithm/shard/hash
                let components: Vec<&str> = object.name.split('/').collect();
                if components.len() < 3 {
                    continue;
                }

                let algorithm = components[components.len() - 3];
                let hash_hex = components[components.len() - 1];
                let hash_string = format!("{}:{}", algorithm, hash_hex);

                if let Ok(hash) = BlobHash::from_hex_string(&hash_string) {
                    hashes.push(hash);
                }
            }

            if response.next_page_token.is_empty() {
                break;
            }
            page_token = Some(response.next_page_token);
        }

        debug!("Listed {} blobs from GCS", hashes.len());
        Ok(hashes)
    }
}

impl Clone for GcsBlobStore {
    fn clone(&self) -> Self {
        Self {
            bucket_id: self.bucket_id.clone(),
            bucket_path: self.bucket_path.clone(),
            storage: self.storage.clone(),
            control: self.control.clone(),
            prefix: self.prefix.clone(),
            upload_sem: self.upload_sem.clone(),
            download_sem: self.download_sem.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    #[ignore = "requires GCS credentials and bucket"]
    async fn test_gcs_blob_store_roundtrip() {
        let bucket =
            std::env::var("TEST_GCS_BUCKET").expect("TEST_GCS_BUCKET environment variable not set");

        let store = GcsBlobStore::new(bucket, Some("test".to_string()))
            .await
            .expect("Failed to create GcsBlobStore");

        let content = Bytes::from("test content");
        let hash = store.put(content.clone()).await.expect("Failed to put");

        assert!(store.contains(&hash).await.expect("Failed to check"));

        let retrieved = store.get(&hash).await.expect("Failed to get");
        assert_eq!(content, retrieved);

        store.delete(&hash).await.expect("Failed to delete");
        assert!(
            !store
                .contains(&hash)
                .await
                .expect("Failed to check after delete")
        );
    }

    #[tokio::test]
    #[ignore = "requires GCS credentials and bucket"]
    async fn test_gcs_blob_store_batch_operations() {
        let bucket =
            std::env::var("TEST_GCS_BUCKET").expect("TEST_GCS_BUCKET environment variable not set");

        let store = GcsBlobStore::new(bucket, Some("test".to_string()))
            .await
            .expect("Failed to create GcsBlobStore");

        let contents = vec![
            Bytes::from("content1"),
            Bytes::from("content2"),
            Bytes::from("content3"),
        ];

        let hashes = store
            .put_many(contents.clone())
            .await
            .expect("Failed to put many");

        let exists = store
            .contains_many(&hashes)
            .await
            .expect("Failed to check many");
        assert!(exists.iter().all(|&e| e));

        let retrieved = store.get_many(&hashes).await.expect("Failed to get many");
        assert_eq!(contents, retrieved);

        for hash in hashes {
            let _ = store.delete(&hash).await;
        }
    }
}
