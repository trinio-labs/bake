use super::blob_hash::BlobHash;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;

/// Trait for blob storage backends (local, S3, GCS, etc.)
#[async_trait]
pub trait BlobStore: Send + Sync {
    /// Check if a blob exists in the store
    async fn contains(&self, hash: &BlobHash) -> Result<bool>;

    /// Get blob content by hash
    async fn get(&self, hash: &BlobHash) -> Result<Bytes>;

    /// Put blob content, returns the hash
    async fn put(&self, content: Bytes) -> Result<BlobHash>;

    /// Batch check if multiple blobs exist (optimized for network round-trips)
    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        // Default implementation: call contains() for each hash
        // Implementations should override this for better performance
        let mut results = Vec::with_capacity(hashes.len());
        for hash in hashes {
            results.push(self.contains(hash).await?);
        }
        Ok(results)
    }

    /// Get multiple blobs at once (optimized for parallel downloads)
    async fn get_many(&self, hashes: &[BlobHash]) -> Result<Vec<Bytes>> {
        // Default implementation: call get() for each hash
        // Implementations should override this for better performance
        let mut results = Vec::with_capacity(hashes.len());
        for hash in hashes {
            results.push(self.get(hash).await?);
        }
        Ok(results)
    }

    /// Put multiple blobs at once (optimized for parallel uploads)
    async fn put_many(&self, contents: Vec<Bytes>) -> Result<Vec<BlobHash>> {
        // Default implementation: call put() for each blob
        // Implementations should override this for better performance
        let mut results = Vec::with_capacity(contents.len());
        for content in contents {
            results.push(self.put(content).await?);
        }
        Ok(results)
    }

    /// Delete a blob from the store (for cache eviction)
    async fn delete(&self, hash: &BlobHash) -> Result<()>;

    /// Get the size of a blob without downloading it
    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>>;

    /// List all blob hashes in the store (for debugging/maintenance)
    async fn list(&self) -> Result<Vec<BlobHash>>;

    /// Store a manifest (action result) at a specific key
    /// This is NOT content-addressed - the key is used directly as the path
    /// Default implementation does nothing (local-only stores don't need remote manifest storage)
    async fn put_manifest(&self, _key: &str, _content: Bytes) -> Result<()> {
        Ok(())
    }

    /// Get a manifest (action result) by key
    /// Returns None if not found
    /// Default implementation returns None (local-only stores don't use remote manifests)
    async fn get_manifest(&self, _key: &str) -> Result<Option<Bytes>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;
    use std::sync::{Arc, Mutex};

    /// Simple in-memory blob store for testing
    struct MemoryBlobStore {
        blobs: Arc<Mutex<HashMap<String, Bytes>>>,
    }

    impl MemoryBlobStore {
        fn new() -> Self {
            Self {
                blobs: Arc::new(Mutex::new(HashMap::new())),
            }
        }
    }

    #[async_trait]
    impl BlobStore for MemoryBlobStore {
        async fn contains(&self, hash: &BlobHash) -> Result<bool> {
            let blobs = self.blobs.lock().unwrap();
            Ok(blobs.contains_key(&hash.to_hex_string()))
        }

        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            let blobs = self.blobs.lock().unwrap();
            blobs
                .get(&hash.to_hex_string())
                .cloned()
                .ok_or_else(|| anyhow::anyhow!("Blob not found: {}", hash))
        }

        async fn put(&self, content: Bytes) -> Result<BlobHash> {
            let hash = BlobHash::from_content(&content);
            let mut blobs = self.blobs.lock().unwrap();
            blobs.insert(hash.to_hex_string(), content);
            Ok(hash)
        }

        async fn delete(&self, hash: &BlobHash) -> Result<()> {
            let mut blobs = self.blobs.lock().unwrap();
            blobs.remove(&hash.to_hex_string());
            Ok(())
        }

        async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
            let blobs = self.blobs.lock().unwrap();
            Ok(blobs.get(&hash.to_hex_string()).map(|b| b.len() as u64))
        }

        async fn list(&self) -> Result<Vec<BlobHash>> {
            let blobs = self.blobs.lock().unwrap();
            blobs
                .keys()
                .map(|k| BlobHash::from_hex_string(k))
                .collect::<Result<Vec<_>>>()
        }
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let store = MemoryBlobStore::new();
        let content = Bytes::from("hello world");

        // Put blob
        let hash = store.put(content.clone()).await.unwrap();

        // Get blob
        let retrieved = store.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);
    }

    #[tokio::test]
    async fn test_contains() {
        let store = MemoryBlobStore::new();
        let content = Bytes::from("test data");

        let hash = store.put(content).await.unwrap();

        assert!(store.contains(&hash).await.unwrap());

        // Non-existent blob
        let fake_hash = BlobHash::from_content(b"fake");
        assert!(!store.contains(&fake_hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let store = MemoryBlobStore::new();
        let content = Bytes::from("to be deleted");

        let hash = store.put(content).await.unwrap();
        assert!(store.contains(&hash).await.unwrap());

        store.delete(&hash).await.unwrap();
        assert!(!store.contains(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_size() {
        let store = MemoryBlobStore::new();
        let content = Bytes::from("size test");

        let hash = store.put(content.clone()).await.unwrap();

        let size = store.size(&hash).await.unwrap();
        assert_eq!(size, Some(content.len() as u64));
    }

    #[tokio::test]
    async fn test_list() {
        let store = MemoryBlobStore::new();

        store.put(Bytes::from("blob1")).await.unwrap();
        store.put(Bytes::from("blob2")).await.unwrap();
        store.put(Bytes::from("blob3")).await.unwrap();

        let blobs = store.list().await.unwrap();
        assert_eq!(blobs.len(), 3);
    }

    #[tokio::test]
    async fn test_contains_many() {
        let store = MemoryBlobStore::new();

        let hash1 = store.put(Bytes::from("blob1")).await.unwrap();
        let hash2 = store.put(Bytes::from("blob2")).await.unwrap();
        let hash3 = BlobHash::from_content(b"nonexistent");

        let results = store.contains_many(&[hash1, hash2, hash3]).await.unwrap();
        assert_eq!(results, vec![true, true, false]);
    }

    #[tokio::test]
    async fn test_get_many() {
        let store = MemoryBlobStore::new();

        let content1 = Bytes::from("blob1");
        let content2 = Bytes::from("blob2");

        let hash1 = store.put(content1.clone()).await.unwrap();
        let hash2 = store.put(content2.clone()).await.unwrap();

        let results = store.get_many(&[hash1, hash2]).await.unwrap();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], content1);
        assert_eq!(results[1], content2);
    }

    #[tokio::test]
    async fn test_put_many() {
        let store = MemoryBlobStore::new();

        let contents = vec![
            Bytes::from("blob1"),
            Bytes::from("blob2"),
            Bytes::from("blob3"),
        ];

        let hashes = store.put_many(contents.clone()).await.unwrap();
        assert_eq!(hashes.len(), 3);

        // Verify all blobs were stored
        for (hash, content) in hashes.iter().zip(contents.iter()) {
            let retrieved = store.get(hash).await.unwrap();
            assert_eq!(retrieved, *content);
        }
    }
}
