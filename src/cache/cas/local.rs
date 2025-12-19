use super::blob_hash::{BlobHash, HashAlgorithm};
use super::blob_store::BlobStore;
use super::index::BlobIndex;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use log::{debug, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::io::AsyncReadExt;

/// Local filesystem blob store with sharding for performance
pub struct LocalBlobStore {
    /// Root directory for blob storage
    root: PathBuf,
    /// Hash algorithm to use (default: Blake3)
    algorithm: HashAlgorithm,
    /// Optional SQLite index for O(1) lookups and LRU tracking
    index: Option<Arc<BlobIndex>>,
}

impl LocalBlobStore {
    /// Create a new local blob store
    pub fn new(root: PathBuf) -> Self {
        Self {
            root,
            algorithm: HashAlgorithm::Blake3,
            index: None,
        }
    }

    /// Create with a specific hash algorithm
    pub fn with_algorithm(root: PathBuf, algorithm: HashAlgorithm) -> Self {
        Self {
            root,
            algorithm,
            index: None,
        }
    }

    /// Create with an index for O(1) lookups and LRU eviction support
    pub fn with_index(root: PathBuf, index: Arc<BlobIndex>) -> Self {
        Self {
            root,
            algorithm: HashAlgorithm::Blake3,
            index: Some(index),
        }
    }

    /// Get the blob directory path (with sharding)
    fn get_blob_path(&self, hash: &BlobHash) -> PathBuf {
        let shard = hash.shard_prefix();
        let hash_str = hash.hash_hex();
        self.root
            .join(self.algorithm.to_string())
            .join(shard)
            .join(hash_str)
    }

    /// Ensure the blob store directories exist
    pub async fn init(&self) -> Result<()> {
        let blob_root = self.root.join(self.algorithm.to_string());
        fs::create_dir_all(&blob_root).await?;
        debug!("Initialized local blob store at: {}", blob_root.display());
        Ok(())
    }

    /// Get storage statistics
    pub async fn stats(&self) -> Result<StorageStats> {
        let mut stats = StorageStats::default();
        let blob_root = self.root.join(self.algorithm.to_string());

        if !blob_root.exists() {
            return Ok(stats);
        }

        let mut entries = fs::read_dir(&blob_root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                // This is a shard directory
                let mut shard_entries = fs::read_dir(entry.path()).await?;
                while let Some(blob_entry) = shard_entries.next_entry().await? {
                    if blob_entry.file_type().await?.is_file() {
                        let metadata = blob_entry.metadata().await?;
                        stats.blob_count += 1;
                        stats.total_size += metadata.len();

                        // Track hard links for deduplication stats
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::MetadataExt;
                            let nlinks = metadata.nlink();
                            if nlinks > 1 {
                                stats.hardlink_count += 1;
                                stats.hardlink_refs += nlinks;
                            }
                        }
                    }
                }
            }
        }

        Ok(stats)
    }

    /// Create a hard link to an existing blob at the specified path
    ///
    /// This is useful for extracting cached outputs without copying data.
    /// On Unix systems, this creates a hard link. On Windows, it falls back to copying.
    ///
    /// # Arguments
    /// * `hash` - The blob hash to link
    /// * `target_path` - The path where the hard link should be created
    ///
    /// # Returns
    /// The number of bytes saved by deduplication (0 on Windows)
    pub async fn hardlink_to<P: AsRef<Path>>(
        &self,
        hash: &BlobHash,
        target_path: P,
    ) -> Result<u64> {
        let source_path = self.get_blob_path(hash);
        let target_path = target_path.as_ref();

        if !source_path.exists() {
            anyhow::bail!("Source blob {} does not exist", hash);
        }

        // Create parent directories if needed
        if let Some(parent) = target_path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Get source size for dedup stats
        let source_size = fs::metadata(&source_path).await?.len();

        // Try to create hard link (Unix only)
        #[cfg(unix)]
        {
            match fs::hard_link(&source_path, target_path).await {
                Ok(()) => {
                    debug!(
                        "Created hard link from {} to {} (saved {} bytes)",
                        source_path.display(),
                        target_path.display(),
                        source_size
                    );
                    return Ok(source_size);
                }
                Err(e) => {
                    warn!("Failed to create hard link, falling back to copy: {}", e);
                    // Fall through to copy
                }
            }
        }

        // Fall back to copy (Windows or if hard link failed)
        fs::copy(&source_path, target_path).await?;
        debug!(
            "Copied blob from {} to {} ({} bytes)",
            source_path.display(),
            target_path.display(),
            source_size
        );
        Ok(0) // No deduplication savings on copy
    }

    /// Create hard links for multiple blobs in parallel
    ///
    /// # Arguments
    /// * `links` - Vec of (hash, target_path) tuples
    ///
    /// # Returns
    /// Total bytes saved through deduplication
    pub async fn hardlink_many(&self, links: Vec<(BlobHash, PathBuf)>) -> Result<u64> {
        // Execute hardlink operations in parallel
        let mut total_saved = 0u64;

        for (hash, path) in links {
            let saved = self.hardlink_to(&hash, path).await?;
            total_saved += saved;
        }

        if total_saved > 0 {
            debug!(
                "Saved {} bytes through hard link deduplication",
                total_saved
            );
        }

        Ok(total_saved)
    }

    /// Evict least recently used blobs until target size is reached
    ///
    /// This method requires an index to be configured. It will:
    /// 1. Query the index for LRU candidates
    /// 2. Delete blobs starting from least recently used
    /// 3. Stop when target size is reached or no more candidates
    ///
    /// # Arguments
    /// * `target_size` - Target cache size in bytes
    ///
    /// # Returns
    /// Number of bytes freed
    pub async fn evict_to_size(&self, target_size: u64) -> Result<u64> {
        let index = self
            .index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Index required for eviction"))?;

        let stats = index.stats()?;
        let current_size = stats.total_size;

        if current_size <= target_size {
            debug!(
                "Cache size {} is already below target {}, no eviction needed",
                current_size, target_size
            );
            return Ok(0);
        }

        let bytes_to_free = current_size - target_size;
        let mut bytes_freed = 0u64;

        debug!(
            "Starting LRU eviction: current={} target={} to_free={}",
            current_size, target_size, bytes_to_free
        );

        // Fetch LRU candidates in batches
        let batch_size = 100;
        let mut offset = 0;

        while bytes_freed < bytes_to_free {
            let candidates = index.get_lru_candidates(batch_size, offset)?;

            if candidates.is_empty() {
                debug!("No more eviction candidates available");
                break;
            }

            for (hash, size) in candidates {
                if bytes_freed >= bytes_to_free {
                    break;
                }

                // Delete the blob file
                match self.delete(&hash).await {
                    Ok(()) => {
                        // Remove from index
                        if let Err(e) = index.remove(&hash) {
                            warn!("Failed to remove {} from index: {}", hash, e);
                        }
                        bytes_freed += size;
                        debug!("Evicted blob {} ({} bytes)", hash, size);
                    }
                    Err(e) => {
                        warn!("Failed to evict blob {}: {}", hash, e);
                        // Remove from index anyway (blob might not exist)
                        let _ = index.remove(&hash);
                    }
                }
            }

            offset += batch_size;

            // Safety check: don't loop forever
            if offset > stats.blob_count as usize * 2 {
                warn!("Eviction loop exceeded safety limit");
                break;
            }
        }

        debug!(
            "LRU eviction completed: freed {} bytes ({} blobs)",
            bytes_freed,
            bytes_freed / (stats.total_size / stats.blob_count.max(1))
        );

        Ok(bytes_freed)
    }

    /// Evict a specific number of least recently used blobs
    ///
    /// # Arguments
    /// * `count` - Number of blobs to evict
    ///
    /// # Returns
    /// Number of bytes freed
    pub async fn evict_lru(&self, count: usize) -> Result<u64> {
        let index = self
            .index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Index required for eviction"))?;

        let candidates = index.get_lru_candidates(count, 0)?;
        let mut bytes_freed = 0u64;

        debug!("Evicting {} LRU blobs", candidates.len());

        for (hash, size) in candidates {
            match self.delete(&hash).await {
                Ok(()) => {
                    if let Err(e) = index.remove(&hash) {
                        warn!("Failed to remove {} from index: {}", hash, e);
                    }
                    bytes_freed += size;
                    debug!("Evicted LRU blob {} ({} bytes)", hash, size);
                }
                Err(e) => {
                    warn!("Failed to evict blob {}: {}", hash, e);
                    let _ = index.remove(&hash);
                }
            }
        }

        debug!("Evicted {} blobs, freed {} bytes", count, bytes_freed);
        Ok(bytes_freed)
    }

    /// Evict largest blobs first
    ///
    /// Useful for quickly freeing space when cache is nearly full.
    ///
    /// # Arguments
    /// * `count` - Number of blobs to evict
    ///
    /// # Returns
    /// Number of bytes freed
    pub async fn evict_largest(&self, count: usize) -> Result<u64> {
        let index = self
            .index
            .as_ref()
            .ok_or_else(|| anyhow::anyhow!("Index required for eviction"))?;

        let candidates = index.get_largest_blobs(count)?;
        let mut bytes_freed = 0u64;

        debug!("Evicting {} largest blobs", candidates.len());

        for (hash, size) in candidates {
            match self.delete(&hash).await {
                Ok(()) => {
                    if let Err(e) = index.remove(&hash) {
                        warn!("Failed to remove {} from index: {}", hash, e);
                    }
                    bytes_freed += size;
                    debug!("Evicted large blob {} ({} bytes)", hash, size);
                }
                Err(e) => {
                    warn!("Failed to evict blob {}: {}", hash, e);
                    let _ = index.remove(&hash);
                }
            }
        }

        debug!("Evicted {} blobs, freed {} bytes", count, bytes_freed);
        Ok(bytes_freed)
    }

    /// Get the blob index (if configured)
    pub fn index(&self) -> Option<&Arc<BlobIndex>> {
        self.index.as_ref()
    }
}

#[async_trait]
impl BlobStore for LocalBlobStore {
    async fn contains(&self, hash: &BlobHash) -> Result<bool> {
        let path = self.get_blob_path(hash);
        Ok(path.exists())
    }

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        let path = self.get_blob_path(hash);

        if !path.exists() {
            anyhow::bail!("Blob not found: {}", hash);
        }

        let mut file = fs::File::open(&path).await?;
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer).await?;

        // Update access time in index if available (for LRU tracking)
        if let Some(index) = &self.index {
            if let Err(e) = index.touch(hash) {
                warn!("Failed to update access time for {}: {}", hash, e);
            }
        }

        debug!("Retrieved blob {} ({} bytes)", hash, buffer.len());
        Ok(Bytes::from(buffer))
    }

    async fn put(&self, content: Bytes) -> Result<BlobHash> {
        let hash = BlobHash::from_content_with_algo(&content, self.algorithm.clone());
        let path = self.get_blob_path(&hash);
        let size = content.len() as u64;

        // Check if blob already exists
        if path.exists() {
            debug!("Blob {} already exists, skipping write", hash);
            // Update access time in index if available
            if let Some(index) = &self.index {
                let _ = index.touch(&hash);
            }
            return Ok(hash);
        }

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Write blob atomically using a unique temporary file in the same directory
        // Use a random suffix to avoid conflicts when multiple tasks write the same blob
        let random_suffix: u64 = rand::random();
        let temp_path = path.with_extension(format!("tmp.{}", random_suffix));

        // Ensure temp file parent exists (should be same as path parent, but be safe)
        if let Some(temp_parent) = temp_path.parent() {
            fs::create_dir_all(temp_parent).await?;
        }

        fs::write(&temp_path, &content).await.map_err(|e| {
            anyhow::anyhow!(
                "Failed to write temp blob to {}: {}",
                temp_path.display(),
                e
            )
        })?;

        // Atomic rename - if it fails because target exists, that's OK (deduplication)
        if let Err(e) = fs::rename(&temp_path, &path).await {
            // Check if target now exists - another thread may have completed the write
            if path.exists() {
                // Clean up temp file if it still exists
                let _ = fs::remove_file(&temp_path).await;
                debug!("Blob {} already exists (deduplicated)", hash);
            } else {
                // Real error - propagate it
                return Err(anyhow::anyhow!(
                    "Failed to rename temp blob from {} to {}: {}",
                    temp_path.display(),
                    path.display(),
                    e
                ));
            }
        }

        // Update index if available
        if let Some(index) = &self.index {
            if let Err(e) = index.insert(&hash, size) {
                warn!("Failed to update index for {}: {}", hash, e);
            }
        }

        debug!("Stored blob {} ({} bytes)", hash, content.len());
        Ok(hash)
    }

    async fn delete(&self, hash: &BlobHash) -> Result<()> {
        let path = self.get_blob_path(hash);

        if path.exists() {
            fs::remove_file(&path).await?;

            // Remove from index if available
            if let Some(index) = &self.index {
                if let Err(e) = index.remove(hash) {
                    warn!("Failed to remove {} from index: {}", hash, e);
                }
            }

            debug!("Deleted blob {}", hash);
        }

        Ok(())
    }

    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        let path = self.get_blob_path(hash);

        if !path.exists() {
            return Ok(None);
        }

        let metadata = fs::metadata(&path).await?;
        Ok(Some(metadata.len()))
    }

    async fn list(&self) -> Result<Vec<BlobHash>> {
        let mut hashes = Vec::new();
        let blob_root = self.root.join(self.algorithm.to_string());

        if !blob_root.exists() {
            return Ok(hashes);
        }

        let mut entries = fs::read_dir(&blob_root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_dir() {
                // This is a shard directory
                let mut shard_entries = fs::read_dir(entry.path()).await?;
                while let Some(blob_entry) = shard_entries.next_entry().await? {
                    if blob_entry.file_type().await?.is_file() {
                        let hash_str = blob_entry.file_name().to_string_lossy().to_string();
                        let full_hash = format!("{}:{}", self.algorithm, hash_str);

                        match BlobHash::from_hex_string(&full_hash) {
                            Ok(hash) => hashes.push(hash),
                            Err(e) => warn!("Invalid blob hash file: {} - {}", hash_str, e),
                        }
                    }
                }
            }
        }

        Ok(hashes)
    }

    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        // Parallel existence checks
        let tasks: Vec<_> = hashes
            .iter()
            .map(|hash| {
                let path = self.get_blob_path(hash);
                async move { path.exists() }
            })
            .collect();

        let results = futures_util::future::join_all(tasks).await;
        Ok(results)
    }

    async fn get_many(&self, hashes: &[BlobHash]) -> Result<Vec<Bytes>> {
        // Parallel reads
        let tasks: Vec<_> = hashes.iter().map(|hash| self.get(hash)).collect();

        let results = futures_util::future::try_join_all(tasks).await?;
        Ok(results)
    }

    async fn put_many(&self, contents: Vec<Bytes>) -> Result<Vec<BlobHash>> {
        // Parallel writes
        let tasks: Vec<_> = contents
            .into_iter()
            .map(|content| self.put(content))
            .collect();

        let results = futures_util::future::try_join_all(tasks).await?;
        Ok(results)
    }
}

/// Storage statistics for the local blob store
#[derive(Debug, Default)]
pub struct StorageStats {
    pub blob_count: u64,
    pub total_size: u64,
    /// Number of blobs that have multiple hard links (Unix only)
    pub hardlink_count: u64,
    /// Total number of hard link references across all blobs (Unix only)
    pub hardlink_refs: u64,
}

impl StorageStats {
    pub fn total_size_mb(&self) -> f64 {
        self.total_size as f64 / 1024.0 / 1024.0
    }

    pub fn total_size_gb(&self) -> f64 {
        self.total_size as f64 / 1024.0 / 1024.0 / 1024.0
    }

    pub fn avg_blob_size(&self) -> f64 {
        if self.blob_count == 0 {
            0.0
        } else {
            self.total_size as f64 / self.blob_count as f64
        }
    }

    /// Calculate estimated disk savings from hard links (Unix only)
    pub fn hardlink_savings(&self) -> u64 {
        if self.hardlink_refs <= self.hardlink_count {
            return 0;
        }
        // Each additional reference beyond the first saves one file's worth of space
        let extra_refs = self.hardlink_refs - self.hardlink_count;
        if self.hardlink_count > 0 {
            (self.total_size / self.blob_count) * extra_refs
        } else {
            0
        }
    }

    /// Get deduplication ratio (0.0 to 1.0, higher is better)
    pub fn dedup_ratio(&self) -> f64 {
        if self.hardlink_refs == 0 {
            0.0
        } else {
            (self.hardlink_refs - self.hardlink_count) as f64 / self.hardlink_refs as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_store() -> (LocalBlobStore, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalBlobStore::new(temp_dir.path().join("cas"));
        store.init().await.unwrap();
        (store, temp_dir)
    }

    #[tokio::test]
    async fn test_init() {
        let temp_dir = TempDir::new().unwrap();
        let store = LocalBlobStore::new(temp_dir.path().join("cas"));

        store.init().await.unwrap();

        let blob_root = temp_dir.path().join("cas").join("blake3");
        assert!(blob_root.exists());
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("hello world");

        let hash = store.put(content.clone()).await.unwrap();
        let retrieved = store.get(&hash).await.unwrap();

        assert_eq!(content, retrieved);
    }

    #[tokio::test]
    async fn test_contains() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("test data");

        let hash = store.put(content).await.unwrap();

        assert!(store.contains(&hash).await.unwrap());

        let fake_hash = BlobHash::from_content(b"nonexistent");
        assert!(!store.contains(&fake_hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_put_same_content_twice() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("duplicate test");

        let hash1 = store.put(content.clone()).await.unwrap();
        let hash2 = store.put(content).await.unwrap();

        assert_eq!(hash1, hash2);

        // Should only be stored once
        assert!(store.contains(&hash1).await.unwrap());
    }

    #[tokio::test]
    async fn test_delete() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("to be deleted");

        let hash = store.put(content).await.unwrap();
        assert!(store.contains(&hash).await.unwrap());

        store.delete(&hash).await.unwrap();
        assert!(!store.contains(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_size() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("size test data");

        let hash = store.put(content.clone()).await.unwrap();

        let size = store.size(&hash).await.unwrap();
        assert_eq!(size, Some(content.len() as u64));

        let fake_hash = BlobHash::from_content(b"nonexistent");
        assert_eq!(store.size(&fake_hash).await.unwrap(), None);
    }

    #[tokio::test]
    async fn test_list() {
        let (store, _temp) = create_test_store().await;

        store.put(Bytes::from("blob1")).await.unwrap();
        store.put(Bytes::from("blob2")).await.unwrap();
        store.put(Bytes::from("blob3")).await.unwrap();

        let hashes = store.list().await.unwrap();
        assert_eq!(hashes.len(), 3);
    }

    #[tokio::test]
    async fn test_stats() {
        let (store, _temp) = create_test_store().await;

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.blob_count, 0);
        assert_eq!(stats.total_size, 0);

        store.put(Bytes::from("test1")).await.unwrap();
        store.put(Bytes::from("test2")).await.unwrap();

        let stats = store.stats().await.unwrap();
        assert_eq!(stats.blob_count, 2);
        assert!(stats.total_size > 0);
    }

    #[tokio::test]
    async fn test_sharding() {
        let (store, _temp) = create_test_store().await;
        let content = Bytes::from("sharding test");

        let hash = store.put(content).await.unwrap();
        let path = store.get_blob_path(&hash);

        // Verify the path includes the shard directory
        let path_str = path.to_string_lossy();
        assert!(path_str.contains("blake3"));
        assert!(path_str.contains(&hash.shard_prefix()));
    }

    #[tokio::test]
    async fn test_parallel_operations() {
        let (store, _temp) = create_test_store().await;

        // Test contains_many
        let hash1 = store.put(Bytes::from("blob1")).await.unwrap();
        let hash2 = store.put(Bytes::from("blob2")).await.unwrap();
        let hash3 = BlobHash::from_content(b"nonexistent");

        let results = store
            .contains_many(&[hash1.clone(), hash2.clone(), hash3])
            .await
            .unwrap();
        assert_eq!(results, vec![true, true, false]);

        // Test get_many
        let contents = store.get_many(&[hash1, hash2]).await.unwrap();
        assert_eq!(contents.len(), 2);
        assert_eq!(contents[0], Bytes::from("blob1"));
        assert_eq!(contents[1], Bytes::from("blob2"));

        // Test put_many
        let new_contents = vec![
            Bytes::from("new1"),
            Bytes::from("new2"),
            Bytes::from("new3"),
        ];
        let hashes = store.put_many(new_contents.clone()).await.unwrap();
        assert_eq!(hashes.len(), 3);

        for (i, hash) in hashes.iter().enumerate() {
            let retrieved = store.get(hash).await.unwrap();
            assert_eq!(retrieved, new_contents[i]);
        }
    }

    #[tokio::test]
    async fn test_hardlink_basic() {
        let (store, temp) = create_test_store().await;
        let content = Bytes::from("hardlink test data");
        let hash = store.put(content.clone()).await.unwrap();

        let target_path = temp.path().join("linked_file.txt");
        let saved = store.hardlink_to(&hash, &target_path).await.unwrap();

        // Verify link was created
        assert!(target_path.exists());

        // Verify content is correct
        let linked_content = fs::read(&target_path).await.unwrap();
        assert_eq!(linked_content, content.as_ref());

        // On Unix, should report savings
        #[cfg(unix)]
        assert_eq!(saved, content.len() as u64);

        // On Windows, falls back to copy (no savings)
        #[cfg(windows)]
        assert_eq!(saved, 0);
    }

    #[tokio::test]
    async fn test_hardlink_nonexistent() {
        let (store, temp) = create_test_store().await;
        let fake_hash = BlobHash::from_content(b"nonexistent");
        let target_path = temp.path().join("should_fail.txt");

        let result = store.hardlink_to(&fake_hash, &target_path).await;
        assert!(result.is_err());
        assert!(!target_path.exists());
    }

    #[tokio::test]
    async fn test_hardlink_creates_parent_dirs() {
        let (store, temp) = create_test_store().await;
        let content = Bytes::from("nested test");
        let hash = store.put(content).await.unwrap();

        let target_path = temp.path().join("nested/dir/file.txt");
        store.hardlink_to(&hash, &target_path).await.unwrap();

        assert!(target_path.exists());
        assert!(target_path.parent().unwrap().exists());
    }

    #[tokio::test]
    async fn test_hardlink_many() {
        let (store, temp) = create_test_store().await;

        // Create multiple blobs
        let hash1 = store.put(Bytes::from("content1")).await.unwrap();
        let hash2 = store.put(Bytes::from("content2")).await.unwrap();
        let hash3 = store.put(Bytes::from("content3")).await.unwrap();

        // Create hard links for all
        let links = vec![
            (hash1.clone(), temp.path().join("link1.txt")),
            (hash2.clone(), temp.path().join("link2.txt")),
            (hash3.clone(), temp.path().join("link3.txt")),
        ];

        let total_saved = store.hardlink_many(links.clone()).await.unwrap();

        // Verify all links exist
        for (_, path) in &links {
            assert!(path.exists());
        }

        // On Unix, should report total savings
        #[cfg(unix)]
        assert!(total_saved > 0);

        // On Windows, no savings
        #[cfg(windows)]
        assert_eq!(total_saved, 0);
    }

    #[tokio::test]
    #[cfg(unix)]
    async fn test_hardlink_stats() {
        use std::os::unix::fs::MetadataExt;

        let (store, temp) = create_test_store().await;
        let content = Bytes::from("stats test content");
        let hash = store.put(content).await.unwrap();

        // Create multiple hard links
        let link1 = temp.path().join("link1.txt");
        let link2 = temp.path().join("link2.txt");
        let link3 = temp.path().join("link3.txt");

        store.hardlink_to(&hash, &link1).await.unwrap();
        store.hardlink_to(&hash, &link2).await.unwrap();
        store.hardlink_to(&hash, &link3).await.unwrap();

        // Check that nlink count increased
        let source_path = store.get_blob_path(&hash);
        let metadata = fs::metadata(&source_path).await.unwrap();
        let nlinks = metadata.nlink();

        // Should have 4 links: original + 3 hard links
        assert_eq!(nlinks, 4);

        // Verify stats track hard links
        let stats = store.stats().await.unwrap();
        assert_eq!(stats.hardlink_count, 1); // One blob with multiple links
        assert_eq!(stats.hardlink_refs, 4); // Total references
    }

    #[tokio::test]
    async fn test_storage_stats_dedup_ratio() {
        let mut stats = StorageStats {
            blob_count: 10,
            total_size: 10000,
            hardlink_count: 5,
            hardlink_refs: 15,
        };

        // 15 total refs - 5 original = 10 extra refs
        // 10 / 15 = 0.666... dedup ratio
        let ratio = stats.dedup_ratio();
        assert!((ratio - 0.666).abs() < 0.01);

        // Test with no hard links
        stats.hardlink_count = 0;
        stats.hardlink_refs = 0;
        assert_eq!(stats.dedup_ratio(), 0.0);
    }

    #[tokio::test]
    async fn test_hardlink_same_blob_multiple_times() {
        let (store, temp) = create_test_store().await;
        let content = Bytes::from("duplicate link test");
        let hash = store.put(content.clone()).await.unwrap();

        // Create multiple links to same blob
        let link1 = temp.path().join("dup1.txt");
        let link2 = temp.path().join("dup2.txt");
        let link3 = temp.path().join("dup3.txt");

        store.hardlink_to(&hash, &link1).await.unwrap();
        store.hardlink_to(&hash, &link2).await.unwrap();
        store.hardlink_to(&hash, &link3).await.unwrap();

        // All links should exist and have same content
        for path in [&link1, &link2, &link3] {
            assert!(path.exists());
            let linked_content = fs::read(path).await.unwrap();
            assert_eq!(linked_content, content.as_ref());
        }

        // On Unix, verify they're actually hard links (same inode)
        #[cfg(unix)]
        {
            use std::os::unix::fs::MetadataExt;
            let ino1 = fs::metadata(&link1).await.unwrap().ino();
            let ino2 = fs::metadata(&link2).await.unwrap().ino();
            let ino3 = fs::metadata(&link3).await.unwrap().ino();
            assert_eq!(ino1, ino2);
            assert_eq!(ino2, ino3);
        }
    }

    // Helper for creating store with index
    async fn create_test_store_with_index() -> (LocalBlobStore, TempDir, Arc<BlobIndex>) {
        let temp_dir = TempDir::new().unwrap();
        let store_path = temp_dir.path().join("cas");
        let index_path = temp_dir.path().join("index.db");

        let index = Arc::new(BlobIndex::open(&index_path).unwrap());
        let store = LocalBlobStore::with_index(store_path.clone(), index.clone());

        store.init().await.unwrap();
        (store, temp_dir, index)
    }

    #[tokio::test]
    async fn test_eviction_requires_index() {
        let (store, _temp) = create_test_store().await;

        // Eviction should fail without index
        let result = store.evict_lru(5).await;
        assert!(result.is_err());
        assert!(result.unwrap_err().to_string().contains("Index required"));
    }

    #[tokio::test]
    async fn test_evict_lru() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Add multiple blobs with delays to ensure different access times
        let hash1 = store.put(Bytes::from("old blob 1")).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let hash2 = store.put(Bytes::from("newer blob 2")).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        let hash3 = store.put(Bytes::from("newest blob 3")).await.unwrap();

        // Verify all exist
        assert!(store.contains(&hash1).await.unwrap());
        assert!(store.contains(&hash2).await.unwrap());
        assert!(store.contains(&hash3).await.unwrap());

        // Evict 2 LRU blobs
        let freed = store.evict_lru(2).await.unwrap();
        assert!(freed > 0);

        // Oldest two should be gone
        assert!(!store.contains(&hash1).await.unwrap());
        assert!(!store.contains(&hash2).await.unwrap());

        // Newest should remain
        assert!(store.contains(&hash3).await.unwrap());

        // Index should be updated
        assert!(!index.contains(&hash1).unwrap());
        assert!(!index.contains(&hash2).unwrap());
        assert!(index.contains(&hash3).unwrap());
    }

    #[tokio::test]
    async fn test_evict_to_size() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Add blobs with known sizes
        let blob1 = vec![b'A'; 1000];
        let blob2 = vec![b'B'; 1000];
        let blob3 = vec![b'C'; 1000];

        store.put(Bytes::from(blob1)).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        store.put(Bytes::from(blob2)).await.unwrap();
        tokio::time::sleep(tokio::time::Duration::from_millis(10)).await;

        store.put(Bytes::from(blob3)).await.unwrap();

        // Get current size
        let stats_before = index.stats().unwrap();
        assert_eq!(stats_before.blob_count, 3);

        // Evict to target size (keep only newest blob)
        let target_size = 1500; // About 1.5 blobs
        let freed = store.evict_to_size(target_size).await.unwrap();
        assert!(freed > 0);

        // Check final size
        let stats_after = index.stats().unwrap();
        assert!(stats_after.total_size <= target_size);
        assert!(stats_after.blob_count < 3);
    }

    #[tokio::test]
    async fn test_evict_to_size_no_eviction_needed() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Add a small blob
        store.put(Bytes::from("small blob")).await.unwrap();

        let stats = index.stats().unwrap();
        let current_size = stats.total_size;

        // Try to evict with target larger than current size
        let freed = store.evict_to_size(current_size + 1000).await.unwrap();

        // No eviction should occur
        assert_eq!(freed, 0);

        // Blob should still exist
        let stats_after = index.stats().unwrap();
        assert_eq!(stats_after.blob_count, 1);
    }

    #[tokio::test]
    async fn test_evict_largest() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Add blobs of different sizes
        let small = vec![b'A'; 100];
        let medium = vec![b'B'; 500];
        let large = vec![b'C'; 1000];

        let hash_small = store.put(Bytes::from(small)).await.unwrap();
        let hash_medium = store.put(Bytes::from(medium)).await.unwrap();
        let hash_large = store.put(Bytes::from(large)).await.unwrap();

        // Evict 1 largest blob
        let freed = store.evict_largest(1).await.unwrap();
        assert!(freed >= 1000); // Should have freed at least the large blob

        // Large should be gone
        assert!(!store.contains(&hash_large).await.unwrap());
        assert!(!index.contains(&hash_large).unwrap());

        // Small and medium should remain
        assert!(store.contains(&hash_small).await.unwrap());
        assert!(store.contains(&hash_medium).await.unwrap());
    }

    #[tokio::test]
    async fn test_index_tracking_on_operations() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Put should add to index
        let hash = store.put(Bytes::from("test content")).await.unwrap();
        assert!(index.contains(&hash).unwrap());

        // Get should update access time
        tokio::time::sleep(tokio::time::Duration::from_millis(50)).await;
        store.get(&hash).await.unwrap();

        // Access count should have increased (implicitly tested via LRU)

        // Delete should remove from index
        store.delete(&hash).await.unwrap();
        assert!(!index.contains(&hash).unwrap());

        let final_stats = index.stats().unwrap();
        assert_eq!(final_stats.blob_count, 0);
    }

    #[tokio::test]
    async fn test_eviction_with_missing_files() {
        let (store, _temp, index) = create_test_store_with_index().await;

        // Add a blob
        let hash = store.put(Bytes::from("test blob")).await.unwrap();
        assert!(index.contains(&hash).unwrap());

        // Manually delete the file (simulates inconsistency)
        let path = store.get_blob_path(&hash);
        fs::remove_file(&path).await.unwrap();

        // Eviction should handle this gracefully
        let result = store.evict_lru(1).await;
        assert!(result.is_ok());

        // Index should be cleaned up
        assert!(!index.contains(&hash).unwrap());
    }

    #[tokio::test]
    async fn test_store_with_index_accessor() {
        let (store, _temp, index_arc) = create_test_store_with_index().await;

        // Should be able to access index
        let index = store.index().unwrap();
        assert!(Arc::ptr_eq(index, &index_arc));

        // Store without index should return None
        let (store_no_index, _temp2) = create_test_store().await;
        assert!(store_no_index.index().is_none());
    }
}
