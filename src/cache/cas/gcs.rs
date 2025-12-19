use super::blob_hash::BlobHash;
use super::blob_store::BlobStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use bytes::Bytes;
use google_cloud_storage::{
    client::{Client, ClientConfig},
    http::objects::{
        delete::DeleteObjectRequest,
        download::Range,
        get::GetObjectRequest,
        list::ListObjectsRequest,
        upload::{Media, UploadObjectRequest, UploadType},
    },
};
use log::debug;
use std::sync::Arc;
use tokio::sync::Semaphore;
use tokio_stream::StreamExt;

/// GCS-backed blob store for remote caching
pub struct GcsBlobStore {
    /// GCS bucket name
    bucket: String,

    /// GCS client
    client: Client,

    /// Optional key prefix for organizing blobs
    prefix: Option<String>,

    /// Semaphore to limit concurrent GCS operations
    upload_sem: Arc<Semaphore>,
    download_sem: Arc<Semaphore>,
}

impl GcsBlobStore {
    /// Create a new GCS blob store
    pub async fn new(bucket: String, prefix: Option<String>) -> Result<Self> {
        let config = ClientConfig::default()
            .with_auth()
            .await
            .context("Failed to configure GCS authentication")?;

        let client = Client::new(config);

        // Note: GCS client doesn't have a simple "check bucket exists" API
        // We'll verify on first operation

        debug!("GcsBlobStore initialized for bucket: {}", bucket);

        Ok(Self {
            bucket,
            client,
            prefix,
            upload_sem: Arc::new(Semaphore::new(8)), // Limit concurrent uploads
            download_sem: Arc::new(Semaphore::new(16)), // Higher limit for downloads
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
}

#[async_trait]
impl BlobStore for GcsBlobStore {
    async fn contains(&self, hash: &BlobHash) -> Result<bool> {
        let object_name = self.get_object_name(hash);

        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            object: object_name.clone(),
            ..Default::default()
        };

        match self.client.get_object(&request).await {
            Ok(_) => Ok(true),
            Err(err) => {
                // Check if it's a "not found" error
                let err_str = err.to_string();
                if err_str.contains("404") || err_str.contains("Not Found") {
                    Ok(false)
                } else {
                    // Log but don't fail - treat as miss
                    debug!("GCS get_object error for {}: {}", object_name, err);
                    Ok(false)
                }
            }
        }
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let _permit = self.download_sem.acquire().await?;
        let object_name = self.get_object_name(hash);

        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            object: object_name.clone(),
            ..Default::default()
        };

        // Download the entire object as bytes
        let mut stream = self
            .client
            .download_streamed_object(&request, &Range::default())
            .await
            .context(format!("Failed to get blob {} from GCS", hash))?;

        let mut data = Vec::new();
        while let Some(chunk) = stream.next().await {
            let chunk = chunk.context(format!("Failed to read blob {} chunk from GCS", hash))?;
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

        let upload_type = UploadType::Simple(Media::new(object_name.clone()));
        let request = UploadObjectRequest {
            bucket: self.bucket.clone(),
            ..Default::default()
        };

        self.client
            .upload_object(&request, content.to_vec(), &upload_type)
            .await
            .context(format!("Failed to upload blob {} to GCS", hash))?;

        debug!("Uploaded blob {} to GCS ({} bytes)", hash, content.len());
        Ok(hash)
    }

    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        // GCS doesn't have batch head operation, so we do them in parallel
        let tasks: Vec<_> = hashes
            .iter()
            .map(|hash| {
                let hash = hash.clone();
                let store = self.clone();
                async move { store.contains(&hash).await.unwrap_or(false) }
            })
            .collect();

        let results = futures_util::future::join_all(tasks).await;
        Ok(results)
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

        let request = DeleteObjectRequest {
            bucket: self.bucket.clone(),
            object: object_name.clone(),
            ..Default::default()
        };

        self.client
            .delete_object(&request)
            .await
            .context(format!("Failed to delete blob {} from GCS", hash))?;

        debug!("Deleted blob {} from GCS", hash);
        Ok(())
    }

    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        let object_name = self.get_object_name(hash);

        let request = GetObjectRequest {
            bucket: self.bucket.clone(),
            object: object_name.clone(),
            ..Default::default()
        };

        match self.client.get_object(&request).await {
            Ok(object) => Ok(Some(object.size as u64)),
            Err(_) => Ok(None),
        }
    }

    async fn list(&self) -> Result<Vec<BlobHash>> {
        let prefix = match &self.prefix {
            Some(p) => format!("{}/", p),
            None => String::new(),
        };

        let mut hashes = Vec::new();
        let mut page_token: Option<String> = None;

        loop {
            let request = ListObjectsRequest {
                bucket: self.bucket.clone(),
                prefix: Some(prefix.clone()),
                page_token: page_token.clone(),
                ..Default::default()
            };

            let response = self
                .client
                .list_objects(&request)
                .await
                .context("Failed to list GCS objects")?;

            if let Some(items) = response.items {
                for object in items {
                    // Extract hash from object name: [prefix/]algorithm/shard/hash
                    // Need to reconstruct "algorithm:hash" format
                    let components: Vec<&str> = object.name.split('/').collect();
                    if components.len() < 3 {
                        continue; // Invalid path structure
                    }

                    // Get algorithm (third from end) and hash (last component)
                    let algorithm = components[components.len() - 3];
                    let hash_hex = components[components.len() - 1];

                    // Reconstruct the format expected by from_hex_string
                    let hash_string = format!("{}:{}", algorithm, hash_hex);

                    if let Ok(hash) = BlobHash::from_hex_string(&hash_string) {
                        hashes.push(hash);
                    }
                }
            }

            if let Some(token) = response.next_page_token {
                page_token = Some(token);
            } else {
                break;
            }
        }

        debug!("Listed {} blobs from GCS", hashes.len());
        Ok(hashes)
    }
}

// Implement Clone manually to share client and semaphores
impl Clone for GcsBlobStore {
    fn clone(&self) -> Self {
        Self {
            bucket: self.bucket.clone(),
            client: self.client.clone(),
            prefix: self.prefix.clone(),
            upload_sem: self.upload_sem.clone(),
            download_sem: self.download_sem.clone(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Note: These tests require GCS credentials and a real GCS bucket
    // They are ignored by default and should be run manually with:
    // cargo test gcs_blob_store -- --ignored

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
        assert!(!store
            .contains(&hash)
            .await
            .expect("Failed to check after delete"));
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

        // Cleanup
        for hash in hashes {
            let _ = store.delete(&hash).await;
        }
    }
}
