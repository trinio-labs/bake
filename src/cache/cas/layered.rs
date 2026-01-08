use super::blob_hash::BlobHash;
use super::blob_store::BlobStore;
use anyhow::Result;
use async_trait::async_trait;
use bytes::Bytes;
use log::{debug, warn};
use std::sync::Arc;

/// Layered blob store that tries multiple backends in order
///
/// Features:
/// - Automatic cache promotion: When a blob is found in a slower tier,
///   it's automatically promoted to faster tiers
/// - Write-through: Writes always go to all tiers to ensure consistency
/// - Read prioritization: Always reads from fastest available tier
///
/// Example tier order: Local → S3 → GCS
pub struct LayeredBlobStore {
    /// Ordered list of blob stores (fastest first), guaranteed non-empty
    tiers: Vec<Arc<dyn BlobStore>>,

    /// Enable automatic promotion of cache hits to faster tiers
    auto_promote: bool,
}

impl LayeredBlobStore {
    /// Creates a LayeredBlobStore with the provided tiers and auto-promotion enabled.
    ///
    /// The `tiers` vector must be non-empty and is interpreted as an ordered list of blob
    /// stores with the fastest tier first. Auto-promotion will be enabled so that reads
    /// from slower tiers may be promoted to faster tiers.
    ///
    /// # Errors
    ///
    /// Returns an error if `tiers` is empty.
    ///
    /// # Examples
    ///
    /// ```
    /// use std::sync::Arc;
    ///
    /// // Minimal local stub for example purposes.
    /// struct DummyStore;
    /// impl crate::blob::BlobStore for DummyStore {
    ///     // implement required trait methods as no-op or unimplemented for the example
    ///     # fn contains(&self, _hash: crate::blob::BlobHash) -> crate::Result<bool> { Ok(false) }
    ///     # fn get(&self, _hash: crate::blob::BlobHash) -> crate::Result<bytes::Bytes> { unimplemented!() }
    ///     # fn put(&self, _content: bytes::Bytes) -> crate::Result<crate::blob::BlobHash> { unimplemented!() }
    ///     # fn contains_many(&self, _hashes: &[crate::blob::BlobHash]) -> crate::Result<Vec<bool>> { unimplemented!() }
    ///     # fn get_many(&self, _hashes: &[crate::blob::BlobHash]) -> crate::Result<Vec<bytes::Bytes>> { unimplemented!() }
    ///     # fn put_many(&self, _contents: &[bytes::Bytes]) -> crate::Result<Vec<crate::blob::BlobHash>> { unimplemented!() }
    ///     # fn delete(&self, _hash: crate::blob::BlobHash) -> crate::Result<()> { unimplemented!() }
    ///     # fn size(&self, _hash: crate::blob::BlobHash) -> crate::Result<Option<u64>> { unimplemented!() }
    ///     # fn list(&self) -> crate::Result<Vec<crate::blob::BlobHash>> { unimplemented!() }
    ///     # fn put_manifest(&self, _key: &str, _content: bytes::Bytes) -> crate::Result<()> { unimplemented!() }
    ///     # fn get_manifest(&self, _key: &str) -> crate::Result<Option<bytes::Bytes>> { Ok(None) }
    /// }
    ///
    /// let tiers: Vec<Arc<dyn crate::blob::BlobStore>> = vec![Arc::new(DummyStore)];
    /// let layered = crate::blob::LayeredBlobStore::new(tiers).expect("tiers must be non-empty");
    /// ```
    pub fn new(tiers: Vec<Arc<dyn BlobStore>>) -> Result<Self> {
        if tiers.is_empty() {
            anyhow::bail!("LayeredBlobStore requires at least one tier");
        }
        Ok(Self {
            tiers,
            auto_promote: true,
        })
    }

    /// Creates a LayeredBlobStore with the provided tiers and auto-promotion setting.
    ///
    /// The `tiers` vector must be non-empty and is interpreted fastest-first (index 0 is the fastest tier).
    /// When `auto_promote` is `true`, blobs read from slower tiers will be automatically promoted to faster tiers.
    ///
    /// # Errors
    ///
    /// Returns an error if `tiers` is empty.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// use std::sync::Arc;
    /// // `store1` and `store2` are existing implementations of `BlobStore`, with `store1` being faster.
    /// let layered = LayeredBlobStore::with_options(vec![store1, store2], true)?;
    /// ```
    pub fn with_options(tiers: Vec<Arc<dyn BlobStore>>, auto_promote: bool) -> Result<Self> {
        if tiers.is_empty() {
            anyhow::bail!("LayeredBlobStore requires at least one tier");
        }
        Ok(Self { tiers, auto_promote })
    }

    /// Promote a blob from a slower tier to faster tiers
    async fn promote(&self, hash: &BlobHash, content: &Bytes, found_tier: usize) -> Result<()> {
        if !self.auto_promote || found_tier == 0 {
            return Ok(()); // Already in fastest tier or promotion disabled
        }

        // Promote to all faster tiers
        let promotion_tasks: Vec<_> = self.tiers[..found_tier]
            .iter()
            .map(|tier| {
                let tier = Arc::clone(tier);
                let content = content.clone();
                async move {
                    match tier.put(content).await {
                        Ok(_) => Ok::<(), anyhow::Error>(()),
                        Err(e) => {
                            warn!("Failed to promote blob to faster tier: {}", e);
                            Ok::<(), anyhow::Error>(()) // Don't fail the operation if promotion fails
                        }
                    }
                }
            })
            .collect();

        let _ = futures_util::future::try_join_all(promotion_tasks).await;
        debug!(
            "Promoted blob {} from tier {} to faster tiers",
            hash, found_tier
        );
        Ok(())
    }
}

#[async_trait]
impl BlobStore for LayeredBlobStore {
    async fn contains(&self, hash: &BlobHash) -> Result<bool> {
        // Check tiers in order until we find it
        for tier in &self.tiers {
            if tier.contains(hash).await? {
                return Ok(true);
            }
        }
        Ok(false)
    }

    /// Retrieves the blob content for `hash` by searching tiers from fastest to slowest.
    ///
    /// If a tier returns the blob, the content is returned immediately and the implementation will
    /// attempt to promote that blob to all faster tiers; promotion failures are logged and do not
    /// prevent returning the found content.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(store: &LayeredBlobStore, hash: BlobHash) {
    /// let content = store.get(&hash).await.unwrap();
    /// # }
    /// ```
    ///
    /// # Returns
    /// `Bytes` containing the blob content on success; returns an error if the blob is not found in any tier.
    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        // Try each tier in order
        for (tier_idx, tier) in self.tiers.iter().enumerate() {
            match tier.get(hash).await {
                Ok(content) => {
                    debug!(
                        "Blob {} found in tier {} ({} bytes)",
                        hash,
                        tier_idx,
                        content.len()
                    );
                    // Promote to faster tiers if not from fastest
                    if let Err(e) = self.promote(hash, &content, tier_idx).await {
                        warn!("Failed to promote blob after cache hit: {}", e);
                    }
                    return Ok(content);
                }
                Err(e) => {
                    debug!("Blob {} not in tier {}: {}", hash, tier_idx, e);
                    // Try next tier
                    continue;
                }
            }
        }

        debug!("Blob {} not found in any of {} tiers", hash, self.tiers.len());
        anyhow::bail!("Blob {} not found in any tier", hash)
    }

    /// Stores `content` in all configured tiers and returns its computed hash.
    ///
    /// This performs write-through writes to every tier in parallel. If at least one tier
    /// successfully stores the blob, the function returns the blob's content-derived hash;
    /// if all tier writes fail, an error is returned. Failures on individual tiers are logged
    /// but do not prevent success if another tier succeeds.
    ///
    /// # Returns
    ///
    /// The `BlobHash` computed from `content` when at least one tier write succeeds.
    ///
    /// # Examples
    ///
    /// ```
    /// # use bytes::Bytes;
    /// # use std::sync::Arc;
    /// # use futures::executor::block_on;
    /// #
    /// # // `store` is any implementation of the BlobStore trait available in scope.
    /// # async fn example(store: Arc<dyn crate::BlobStore>) {
    /// let content = Bytes::from("hello");
    /// let hash = store.put(content.clone()).await.unwrap();
    /// assert_eq!(hash, crate::BlobHash::from_content(&content));
    /// # }
    /// ```
    async fn put(&self, content: Bytes) -> Result<BlobHash> {
        let hash = BlobHash::from_content(&content);

        // Always write to all tiers for consistency
        let write_tasks: Vec<_> = self
            .tiers
            .iter()
            .map(|tier| {
                let tier = Arc::clone(tier);
                let content = content.clone();
                async move { tier.put(content).await }
            })
            .collect();

        let results = futures_util::future::join_all(write_tasks).await;

        // Log failures and check if at least one succeeded
        let any_success = results.iter().enumerate().fold(false, |acc, (idx, result)| {
            if let Err(e) = result {
                warn!("Failed to write to tier {}: {}", idx, e);
            }
            acc || result.is_ok()
        });

        if !any_success {
            anyhow::bail!("All tier writes failed for blob {}", hash);
        }

        Ok(hash)
    }

    /// Checks which of the provided blob hashes exist in any tier.
    ///
    /// Checks each hash across tiers in priority order and returns a boolean vector
    /// indicating presence per input hash. Stops querying tiers early for hashes
    /// already found.
    ///
    /// # Parameters
    ///
    /// - `hashes`: slice of blob hashes to check for existence.
    ///
    /// # Returns
    ///
    /// `Vec<bool>` where each element is `true` if the corresponding hash exists in at least one tier, `false` otherwise.
    ///
    /// # Examples
    ///
    /// ```no_run
    /// # async fn example(store: &LayeredBlobStore, h1: BlobHash, h2: BlobHash) {
    /// let results = store.contains_many(&[h1, h2]).await.unwrap();
    /// assert_eq!(results.len(), 2);
    /// // results[0] and results[1] indicate presence of h1 and h2 respectively
    /// # }
    /// ```
    async fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        // For each hash, check if it exists in any tier
        let mut results = vec![false; hashes.len()];

        for tier in &self.tiers {
            let tier_results = tier.contains_many(hashes).await?;

            // Update results for any newly found hashes
            for (i, found) in tier_results.iter().enumerate() {
                if *found {
                    results[i] = true;
                }
            }

            // If all found, we can stop early
            if results.iter().all(|&r| r) {
                break;
            }
        }

        Ok(results)
    }

    /// Retrieves multiple blobs for the given hashes from the layered store, preserving the input order.
    ///
    /// Queries tiers in priority order and returns contents for all requested hashes. If a blob is found
    /// in a slower tier and `auto_promote` is enabled, the blob is promoted to faster tiers (best-effort).
    /// If any requested hash is not found in any tier, the call returns an error.
    ///
    /// # Returns
    ///
    /// A `Vec<Bytes>` containing blob contents in the same order as the input `hashes`.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example(store: &LayeredBlobStore, hashes: Vec<BlobHash>) {
    /// let contents = store.get_many(&hashes).await.unwrap();
    /// assert_eq!(contents.len(), hashes.len());
    /// # }
    /// ```
    async fn get_many(&self, hashes: &[BlobHash]) -> Result<Vec<Bytes>> {
        debug!("get_many: retrieving {} blobs", hashes.len());
        let mut results = Vec::with_capacity(hashes.len());
        let mut remaining: Vec<(usize, BlobHash)> = hashes
            .iter()
            .enumerate()
            .map(|(i, h)| (i, h.clone()))
            .collect();

        // Try each tier
        for (tier_idx, tier) in self.tiers.iter().enumerate() {
            if remaining.is_empty() {
                break;
            }

            let current_hashes: Vec<_> = remaining.iter().map(|(_, h)| h.clone()).collect();

            match tier.get_many(&current_hashes).await {
                Ok(contents) => {
                    // Ensure we got all requested blobs (positional correspondence)
                    if contents.len() != current_hashes.len() {
                        debug!(
                            "get_many: tier {} returned {} blobs but expected {}, trying next tier",
                            tier_idx,
                            contents.len(),
                            current_hashes.len()
                        );
                        // Partial success - treat as failure and try next tier
                        continue;
                    }

                    debug!(
                        "get_many: found {} blobs in tier {}",
                        contents.len(),
                        tier_idx
                    );

                    // Process successful retrievals (safe to use positional index)
                    for (i, content) in contents.into_iter().enumerate() {
                        let (orig_idx, hash) = &remaining[i];
                        results.push((*orig_idx, content.clone()));

                        // Promote if needed
                        if self.auto_promote && tier_idx > 0 {
                            let _ = self.promote(hash, &content, tier_idx).await;
                        }
                    }

                    // All found in this tier
                    remaining.clear();
                }
                Err(e) => {
                    debug!("get_many: tier {} failed: {}", tier_idx, e);
                    // Try next tier with all remaining
                    continue;
                }
            }
        }

        if !remaining.is_empty() {
            debug!(
                "get_many: {} blobs not found in any of {} tiers",
                remaining.len(),
                self.tiers.len()
            );
            anyhow::bail!("Some blobs not found in any tier");
        }

        debug!("get_many: successfully retrieved {} blobs", results.len());
        // Sort by original index
        results.sort_by_key(|(idx, _)| *idx);
        Ok(results.into_iter().map(|(_, content)| content).collect())
    }

    /// Writes a batch of blob contents to all configured tiers and returns the blob hashes from the first tier that succeeds.
    ///
    /// Attempts to write the entire batch to every tier in parallel, logs any per-tier failures, and succeeds if at least one tier reports success. If all tier writes fail, an error is returned.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example(store: &impl crate::BlobStore) -> anyhow::Result<()> {
    /// use bytes::Bytes;
    /// let contents = vec![Bytes::from("one"), Bytes::from("two")];
    /// let hashes = store.put_many(contents).await?;
    /// assert_eq!(hashes.len(), 2);
    /// # Ok(())
    /// # }
    /// ```
    async fn put_many(&self, contents: Vec<Bytes>) -> Result<Vec<BlobHash>> {
        // Always write to all tiers for consistency
        let write_tasks: Vec<_> = self
            .tiers
            .iter()
            .map(|tier| {
                let tier = Arc::clone(tier);
                let contents = contents.clone();
                async move { tier.put_many(contents).await }
            })
            .collect();

        let results = futures_util::future::join_all(write_tasks).await;

        // Log failures and return first successful result
        let mut first_success = None;
        for (idx, result) in results.into_iter().enumerate() {
            match result {
                Ok(hashes) if first_success.is_none() => first_success = Some(hashes),
                Ok(_) => {} // Already have a success
                Err(e) => warn!("Failed to write batch to tier {}: {}", idx, e),
            }
        }

        first_success.ok_or_else(|| anyhow::anyhow!("All tier writes failed for batch"))
    }

    /// Deletes the blob identified by `hash` from all configured tiers.
    ///
    /// The operation attempts deletion on every tier in parallel and is considered
    /// successful if at least one tier reports success.
    ///
    /// # Returns
    ///
    /// `Ok(())` if at least one tier deleted the blob, `Err` if all tier deletions failed.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example(store: &LayeredBlobStore, hash: BlobHash) {
    /// store.delete(&hash).await.unwrap();
    /// # }
    /// ```
    async fn delete(&self, hash: &BlobHash) -> Result<()> {
        // Delete from all tiers
        let delete_tasks: Vec<_> = self
            .tiers
            .iter()
            .map(|tier| {
                let tier = Arc::clone(tier);
                let hash = hash.clone();
                async move { tier.delete(&hash).await }
            })
            .collect();

        let results = futures_util::future::join_all(delete_tasks).await;

        // Log failures and check if at least one succeeded
        let any_success = results.iter().enumerate().fold(false, |acc, (idx, result)| {
            if let Err(e) = result {
                warn!("Failed to delete from tier {}: {}", idx, e);
            }
            acc || result.is_ok()
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to delete blob {} from all tiers", hash)
        }
    }

    async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        // Try each tier in order
        for tier in &self.tiers {
            if let Ok(Some(size)) = tier.size(hash).await {
                return Ok(Some(size));
            }
        }
        Ok(None)
    }

    async fn list(&self) -> Result<Vec<BlobHash>> {
        // Collect hashes from all tiers and deduplicate
        let mut all_hashes = std::collections::HashSet::new();

        for tier in &self.tiers {
            match tier.list().await {
                Ok(hashes) => {
                    all_hashes.extend(hashes);
                }
                Err(e) => {
                    warn!("Failed to list from tier: {}", e);
                    // Continue with other tiers
                }
            }
        }

        Ok(all_hashes.into_iter().collect())
    }

    /// Writes a manifest to every tier and succeeds if at least one tier accepts the write.
    ///
    /// Attempts to write `content` under `key` to all configured tiers in parallel. Per-tier write
    /// failures are logged; the operation returns `Ok(())` if any tier succeeds, and an error if all
    /// tier writes fail.
    ///
    /// # Parameters
    ///
    /// - `key`: Manifest identifier to store.
    /// - `content`: Manifest payload bytes to write.
    ///
    /// # Returns
    ///
    /// `Ok(())` if at least one tier wrote the manifest, `Err` if all tier writes failed.
    ///
    /// # Examples
    ///
    /// ```
    /// # async fn example(store: &LayeredBlobStore) -> anyhow::Result<()> {
    /// let key = "manifest:v1";
    /// let content = bytes::Bytes::from_static(b"{\"version\":1}");
    /// store.put_manifest(key, content).await?;
    /// # Ok(())
    /// # }
    /// ```
    async fn put_manifest(&self, key: &str, content: Bytes) -> Result<()> {
        // Always write manifests to all tiers for consistency
        let write_tasks: Vec<_> = self
            .tiers
            .iter()
            .map(|tier| {
                let tier = Arc::clone(tier);
                let key = key.to_string();
                let content = content.clone();
                async move { tier.put_manifest(&key, content).await }
            })
            .collect();

        let results = futures_util::future::join_all(write_tasks).await;

        // Log failures and check if at least one succeeded
        let any_success = results.iter().enumerate().fold(false, |acc, (idx, result)| {
            if let Err(e) = result {
                warn!("Failed to write manifest to tier {}: {}", idx, e);
            }
            acc || result.is_ok()
        });

        if any_success {
            Ok(())
        } else {
            anyhow::bail!("Failed to write manifest '{}' to all tiers", key)
        }
    }

    async fn get_manifest(&self, key: &str) -> Result<Option<Bytes>> {
        // Try tiers in order (respects configured priority)
        for (tier_idx, tier) in self.tiers.iter().enumerate() {
            match tier.get_manifest(key).await {
                Ok(Some(content)) => {
                    debug!(
                        "Found manifest '{}' in tier {} ({} bytes)",
                        key,
                        tier_idx,
                        content.len()
                    );

                    // Promote to faster tiers if auto_promote is enabled
                    if self.auto_promote && tier_idx > 0 {
                        for faster_tier in &self.tiers[..tier_idx] {
                            if let Err(e) = faster_tier.put_manifest(key, content.clone()).await {
                                warn!("Failed to promote manifest to faster tier: {}", e);
                            }
                        }
                        debug!("Promoted manifest '{}' to faster tiers", key);
                    }

                    return Ok(Some(content));
                }
                Ok(None) => {
                    // Not found in this tier, try next
                    continue;
                }
                Err(e) => {
                    warn!("Error getting manifest from tier {}: {}", tier_idx, e);
                    // Try next tier
                    continue;
                }
            }
        }

        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::cache::cas::LocalBlobStore;
    use std::collections::HashMap;
    use std::sync::Mutex;
    use tempfile::TempDir;

    /// Test store that wraps LocalBlobStore but adds manifest support
    struct TestBlobStore {
        inner: LocalBlobStore,
        manifests: Mutex<HashMap<String, Bytes>>,
    }

    impl TestBlobStore {
        /// Constructs a new TestBlobStore backed by a LocalBlobStore at the given filesystem path.
        ///
        /// The returned store uses the provided `path` as the local storage directory and initializes
        /// an empty in-memory manifest map.
        ///
        /// # Examples
        ///
        /// ```
        /// use std::path::PathBuf;
        /// let store = TestBlobStore::new(PathBuf::from("/tmp/test_blob_store"));
        /// ```
        ///
        /// @returns `Self` — a TestBlobStore backed by the given path and an empty manifest map.
        fn new(path: std::path::PathBuf) -> Self {
            Self {
                inner: LocalBlobStore::new(path),
                manifests: Mutex::new(HashMap::new()),
            }
        }

        /// Initialize the underlying blob store.
        ///
        /// # Examples
        ///
        /// ```
        /// # async fn example(store: &impl crate::BlobStore) {
        /// store.init().await.unwrap();
        /// # }
        /// ```
        ///
        /// # Returns
        ///
        /// `Ok(())` if initialization succeeds, otherwise an error.
        async fn init(&self) -> Result<()> {
            self.inner.init().await
        }
    }

    #[async_trait]
    impl BlobStore for TestBlobStore {
        /// Checks whether a blob with the specified hash exists in any tier.
        ///
        /// # Examples
        ///
        /// ```
        /// // Synchronously run the async check for demonstration.
        /// let exists = futures::executor::block_on(store.contains(&hash)).unwrap();
        /// assert!(exists == false || exists == true);
        /// ```
        ///
        /// # Returns
        ///
        /// `true` if the blob exists in any tier, `false` otherwise.
        async fn contains(&self, hash: &BlobHash) -> Result<bool> {
            self.inner.contains(hash).await
        }

        /// Retrieve the blob content for the given `hash` from the underlying store.
        ///
        /// # Returns
        ///
        /// `Bytes` containing the blob content on success; an error if the blob is not found or a storage operation fails.
        ///
        /// # Examples
        ///
        /// ```tokio::test
        /// async fn example_get() -> Result<(), Box<dyn std::error::Error>> {
        ///     // Construct a store that implements `get`.
        ///     let store = unimplemented!(); // e.g., LayeredBlobStore::new(...)
        ///     let hash = unimplemented!();  // a BlobHash for an existing blob
        ///     let content = store.get(&hash).await?;
        ///     assert!(!content.is_empty());
        ///     Ok(())
        /// }
        /// ```
        async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
            self.inner.get(hash).await
        }

        /// Stores the provided blob content in this store and returns its computed hash.
        ///
        /// Returns the `BlobHash` identifying the stored content.
        ///
        /// # Parameters
        /// - `content`: bytes of the blob to store.
        ///
        /// # Examples
        ///
        /// ```
        /// # use bytes::Bytes;
        /// # use futures::executor::block_on;
        /// # // `store` should be an initialized object implementing the same `put` API.
        /// # let store = /* obtain store */ panic!();
        /// let content = Bytes::from("hello");
        /// let hash = block_on(store.put(content)).unwrap();
        /// // `hash` can be used to retrieve the blob later.
        /// ```
        async fn put(&self, content: Bytes) -> Result<BlobHash> {
            self.inner.put(content).await
        }

        /// Delete the blob identified by `hash` from all underlying tiers.
        ///
        /// Attempts to remove the blob from every configured tier. Returns `Ok(())` if at least one tier reports success; returns an error if no tier deleted the blob.
        ///
        /// # Examples
        ///
        /// ```
        /// // async context
        /// let store: LayeredBlobStore = /* ... */;
        /// let hash = /* BlobHash */;
        /// store.delete(&hash).await.unwrap();
        /// ```
        async fn delete(&self, hash: &BlobHash) -> Result<()> {
            self.inner.delete(hash).await
        }

        /// Get the size in bytes of the blob identified by `hash`, if it exists.
        ///
        /// # Examples
        ///
        /// ```
        /// # async fn example(store: &LayeredBlobStore, hash: BlobHash) {
        /// let size = store.size(&hash).await.unwrap();
        /// if let Some(bytes) = size {
        ///     assert!(bytes > 0);
        /// }
        /// # }
        /// ```
        ///
        /// # Returns
        ///
        /// `Some(size_in_bytes)` if the blob exists, `None` otherwise.
        async fn size(&self, hash: &BlobHash) -> Result<Option<u64>> {
            self.inner.size(hash).await
        }

        /// Lists all blob hashes available from the underlying store.
        ///
        /// # Examples
        ///
        /// ```
        /// #[tokio::test]
        /// async fn example_list() {
        ///     // Construct a store that implements the BlobStore trait.
        ///     let store = /* create store */ ;
        ///     let hashes = store.list().await.unwrap();
        ///     // `hashes` contains the blob hashes present in the store.
        ///     assert!(hashes.len() >= 0);
        /// }
        /// ```
        ///
        /// @returns A `Vec<BlobHash>` containing all blob hashes present in the store.
        async fn list(&self) -> Result<Vec<BlobHash>> {
            self.inner.list().await
        }

        /// Stores a manifest value under the given key in the in-memory manifest map, replacing any existing entry.
        ///
        /// # Examples
        ///
        /// ```ignore
        /// // within an async context
        /// store.put_manifest("release-1", Bytes::from("manifest-data")).await?;
        /// ```
        async fn put_manifest(&self, key: &str, content: Bytes) -> Result<()> {
            let mut manifests = self.manifests.lock().unwrap();
            manifests.insert(key.to_string(), content);
            Ok(())
        }

        /// Retrieves a manifest by its key from the in-memory manifest store.
        ///
        /// Returns `Some(Bytes)` containing the manifest content when the key exists, or `None` when it does not.
        ///
        /// # Examples
        ///
        /// ```
        /// # use bytes::Bytes;
        /// # async fn example(store: &impl std::ops::Deref) {}
        /// // Example usage (assuming `store` implements `get_manifest`):
        /// // let content = store.get_manifest("config.json").await.unwrap();
        /// // assert_eq!(content, Some(Bytes::from_static(b"{\"version\":1}")));
        /// ```
        async fn get_manifest(&self, key: &str) -> Result<Option<Bytes>> {
            let manifests = self.manifests.lock().unwrap();
            Ok(manifests.get(key).cloned())
        }
    }

    /// Create two temporary directories and initialize two TestBlobStore instances backed by them.
    ///
    /// Returns a tuple of (temp_dir1, temp_dir2, store1, store2) where each `TempDir` is the
    /// filesystem backing for its corresponding store and each `Arc<dyn BlobStore>` is an initialized,
    /// ready-to-use store instance.
    ///
    /// # Examples
    ///
    /// ```
    /// #[tokio::test]
    /// async fn example_create_test_stores() {
    ///     let (_dir1, _dir2, store1, store2) = create_test_stores().await;
    ///     // stores are initialized and ready for use
    ///     let _ = (store1, store2);
    /// }
    /// ```
    async fn create_test_stores() -> (
        TempDir,
        TempDir,
        Arc<dyn BlobStore>,
        Arc<dyn BlobStore>,
    ) {
        let temp1 = TempDir::new().unwrap();
        let temp2 = TempDir::new().unwrap();

        let store1 = TestBlobStore::new(temp1.path().to_path_buf());
        store1.init().await.unwrap();

        let store2 = TestBlobStore::new(temp2.path().to_path_buf());
        store2.init().await.unwrap();

        let store1: Arc<dyn BlobStore> = Arc::new(store1);
        let store2: Arc<dyn BlobStore> = Arc::new(store2);

        (temp1, temp2, store1, store2)
    }

    #[tokio::test]
    async fn test_layered_basic_operations() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;
        let layered = LayeredBlobStore::new(vec![store1, store2]).unwrap();

        let content = Bytes::from("test content");
        let hash = layered.put(content.clone()).await.unwrap();

        assert!(layered.contains(&hash).await.unwrap());

        let retrieved = layered.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);
    }

    #[tokio::test]
    async fn test_layered_promotion() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        // Put directly in second tier
        let content = Bytes::from("test content");
        let hash = store2.put(content.clone()).await.unwrap();

        // Create layered store with auto-promotion
        let layered = LayeredBlobStore::with_options(vec![store1.clone(), store2], true).unwrap();

        // First get should trigger promotion
        let retrieved = layered.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);

        // Verify it's now in first tier
        assert!(store1.contains(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_layered_write_to_all_tiers() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        // Create layered store - writes always go to all tiers
        let layered = LayeredBlobStore::new(vec![store1.clone(), store2.clone()]).unwrap();

        let content = Bytes::from("test content");
        let hash = layered.put(content).await.unwrap();

        // Should be in both tiers
        assert!(store1.contains(&hash).await.unwrap());
        assert!(store2.contains(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_layered_fallback() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        // Put only in second tier
        let content = Bytes::from("test content");
        let hash = store2.put(content.clone()).await.unwrap();

        let layered = LayeredBlobStore::new(vec![store1, store2]).unwrap();

        // Should find it via fallback
        let retrieved = layered.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);
    }

    #[tokio::test]
    async fn test_empty_tiers_fails() {
        let result = LayeredBlobStore::new(vec![]);
        assert!(result.is_err());
        let err = result.err().unwrap();
        assert!(err.to_string().contains("at least one tier"));
    }

    #[tokio::test]
    async fn test_put_manifest_writes_to_all_tiers() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        let layered = LayeredBlobStore::new(vec![store1.clone(), store2.clone()]).unwrap();

        let manifest_key = "test/manifest";
        let manifest_content = Bytes::from(r#"{"recipe":"test","exit_code":0}"#);

        // PUT manifest should write to all tiers
        layered
            .put_manifest(manifest_key, manifest_content.clone())
            .await
            .unwrap();

        // Verify in both tiers
        let in_store1 = store1.get_manifest(manifest_key).await.unwrap();
        let in_store2 = store2.get_manifest(manifest_key).await.unwrap();

        assert_eq!(in_store1, Some(manifest_content.clone()));
        assert_eq!(in_store2, Some(manifest_content));
    }

    #[tokio::test]
    async fn test_get_manifest_with_promotion() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        // Put manifest only in second tier
        let manifest_key = "test/manifest";
        let manifest_content = Bytes::from(r#"{"recipe":"test","exit_code":0}"#);
        store2
            .put_manifest(manifest_key, manifest_content.clone())
            .await
            .unwrap();

        // Create layered store with auto-promotion
        let layered = LayeredBlobStore::with_options(vec![store1.clone(), store2], true).unwrap();

        // GET manifest should find it in second tier and promote to first
        let retrieved = layered.get_manifest(manifest_key).await.unwrap();
        assert_eq!(retrieved, Some(manifest_content.clone()));

        // Verify it's now promoted to first tier
        let in_store1 = store1.get_manifest(manifest_key).await.unwrap();
        assert_eq!(in_store1, Some(manifest_content));
    }

    #[tokio::test]
    async fn test_get_manifest_returns_none_when_not_found() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        let layered = LayeredBlobStore::new(vec![store1, store2]).unwrap();

        let result = layered.get_manifest("nonexistent/manifest").await.unwrap();
        assert!(result.is_none());
    }
}