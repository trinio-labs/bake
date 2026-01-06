use super::blob_hash::BlobHash;
use super::blob_store::BlobStore;
use anyhow::{Context, Result};
use async_trait::async_trait;
use aws_config::{BehaviorVersion, Region, meta::region::RegionProviderChain};
use aws_sdk_s3::Client;
use aws_sdk_s3::primitives::ByteStream;
use bytes::Bytes;
use log::debug;
use std::sync::Arc;
use tokio::sync::Semaphore;

/// S3-backed blob store for remote caching
pub struct S3BlobStore {
    /// S3 bucket name
    bucket: String,

    /// S3 client
    client: Client,

    /// Optional key prefix for organizing blobs
    prefix: Option<String>,

    /// Semaphore to limit concurrent S3 operations
    upload_sem: Arc<Semaphore>,
    download_sem: Arc<Semaphore>,
}

impl S3BlobStore {
    /// Create a new S3 blob store
    pub async fn new(
        bucket: String,
        region: Option<String>,
        prefix: Option<String>,
    ) -> Result<Self> {
        let region_provider = match region {
            Some(r) => RegionProviderChain::first_try(Region::new(r)),
            None => RegionProviderChain::default_provider(),
        };

        let config = aws_config::defaults(BehaviorVersion::latest())
            .region(region_provider)
            .load()
            .await;

        let client = Client::new(&config);

        // Verify bucket access
        client
            .head_bucket()
            .bucket(&bucket)
            .send()
            .await
            .context(format!("Failed to access S3 bucket '{}'", bucket))?;

        debug!("S3BlobStore initialized for bucket: {}", bucket);

        Ok(Self {
            bucket,
            client,
            prefix,
            upload_sem: Arc::new(Semaphore::new(8)), // Limit concurrent uploads
            download_sem: Arc::new(Semaphore::new(16)), // Higher limit for downloads
        })
    }

    /// Get S3 key for a blob hash
    fn get_s3_key(&self, hash: &BlobHash) -> String {
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
impl BlobStore for S3BlobStore {
    async fn contains(&self, hash: &BlobHash) -> Result<bool> {
        let key = self.get_s3_key(hash);

        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(_) => Ok(true),
            Err(err) => {
                // Check if it's a "not found" error by examining the service error
                let is_not_found = err
                    .as_service_error()
                    .map(|e| e.is_not_found())
                    .unwrap_or(false);

                if is_not_found {
                    debug!("S3 blob not found: {}", key);
                    Ok(false)
                } else {
                    // Log unexpected errors for visibility in production (but treat as miss)
                    log::warn!(
                        "S3 head_object error for {} (treating as miss): {}",
                        key,
                        err
                    );
                    Ok(false)
                }
            }
        }
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let _permit = self.download_sem.acquire().await?;
        let key = self.get_s3_key(hash);

        let response = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .context(format!("Failed to get blob {} from S3", hash))?;

        let data = response
            .body
            .collect()
            .await
            .context(format!("Failed to read blob {} body from S3", hash))?
            .into_bytes();

        debug!("Downloaded blob {} from S3 ({} bytes)", hash, data.len());
        Ok(data)
    }

    async fn put(&self, content: Bytes) -> Result<BlobHash> {
        let _permit = self.upload_sem.acquire().await?;

        // Hash the content
        let hash = BlobHash::from_content(&content);

        // Check if already exists (avoid redundant upload)
        if self.contains(&hash).await? {
            debug!("Blob {} already exists in S3, skipping upload", hash);
            return Ok(hash);
        }

        let key = self.get_s3_key(&hash);
        let body = ByteStream::from(content.clone());

        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(body)
            .send()
            .await
            .context(format!("Failed to upload blob {} to S3", hash))?;

        debug!("Uploaded blob {} to S3 ({} bytes)", hash, content.len());
        Ok(hash)
    }

    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        // S3 doesn't have batch head operation, so we do them in parallel with bounded concurrency
        const CONCURRENCY: usize = 20;
        let sem = Arc::new(Semaphore::new(CONCURRENCY));

        let tasks: Vec<_> = hashes
            .iter()
            .map(|hash| {
                let hash = hash.clone();
                let store = self.clone();
                let sem = sem.clone();
                async move {
                    let _permit = sem.acquire().await.map_err(|e| {
                        anyhow::anyhow!("Failed to acquire semaphore permit: {}", e)
                    })?;

                    store.contains(&hash).await.or_else(|e| -> Result<bool> {
                        log::warn!("Failed to check blob {} in S3: {}", hash, e);
                        Ok(false) // Treat errors as misses
                    })
                }
            })
            .collect();

        let results: Vec<bool> = futures_util::future::try_join_all(tasks).await?;
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
        let key = self.get_s3_key(hash);

        self.client
            .delete_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
            .context(format!("Failed to delete blob {} from S3", hash))?;

        debug!("Deleted blob {} from S3", hash);
        Ok(())
    }

    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        let key = self.get_s3_key(hash);

        match self
            .client
            .head_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await
        {
            Ok(output) => Ok(output.content_length().map(|s| s as u64)),
            Err(_) => Ok(None),
        }
    }

    async fn list(&self) -> Result<Vec<BlobHash>> {
        // List all objects with our prefix structure
        let prefix = match &self.prefix {
            Some(p) => format!("{}/", p),
            None => String::new(),
        };

        let mut hashes = Vec::new();
        let mut continuation_token = None;

        loop {
            let mut request = self
                .client
                .list_objects_v2()
                .bucket(&self.bucket)
                .prefix(&prefix);

            if let Some(token) = continuation_token {
                request = request.continuation_token(token);
            }

            let response = request.send().await.context("Failed to list S3 objects")?;

            for object in response.contents() {
                if let Some(key) = object.key() {
                    // Extract hash from key: [prefix/]algorithm/shard/hash
                    let components: Vec<&str> = key.split('/').collect();
                    if components.len() >= 3 {
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
            }

            if response.is_truncated() == Some(true) {
                continuation_token = response.next_continuation_token().map(|s| s.to_string());
            } else {
                break;
            }
        }

        debug!("Listed {} blobs from S3", hashes.len());
        Ok(hashes)
    }
}

// Implement Clone manually to share client and semaphores
impl Clone for S3BlobStore {
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

    // Note: These tests require AWS credentials and a real S3 bucket
    // They are ignored by default and should be run manually with:
    // cargo test s3_blob_store -- --ignored

    #[tokio::test]
    #[ignore = "requires AWS credentials and S3 bucket"]
    async fn test_s3_blob_store_roundtrip() {
        let bucket =
            std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET environment variable not set");

        let store = S3BlobStore::new(bucket, None, Some("test".to_string()))
            .await
            .expect("Failed to create S3BlobStore");

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
    #[ignore = "requires AWS credentials and S3 bucket"]
    async fn test_s3_blob_store_batch_operations() {
        let bucket =
            std::env::var("TEST_S3_BUCKET").expect("TEST_S3_BUCKET environment variable not set");

        let store = S3BlobStore::new(bucket, None, Some("test".to_string()))
            .await
            .expect("Failed to create S3BlobStore");

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
