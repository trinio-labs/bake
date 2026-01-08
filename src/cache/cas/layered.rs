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
/// - Write-through: Writes go to all tiers to ensure consistency
/// - Read prioritization: Always reads from fastest available tier
///
/// Example tier order: Local → S3 → GCS
pub struct LayeredBlobStore {
    /// Ordered list of blob stores (fastest first)
    tiers: Vec<Arc<Box<dyn BlobStore>>>,

    /// Enable automatic promotion of cache hits to faster tiers
    auto_promote: bool,

    /// Enable write-through to all tiers (false = write to first tier only)
    write_through: bool,
}

impl LayeredBlobStore {
    /// Create a new layered blob store
    pub fn new(tiers: Vec<Arc<Box<dyn BlobStore>>>) -> Self {
        Self {
            tiers,
            auto_promote: true,
            write_through: false, // Default: write to first tier only for speed
        }
    }

    /// Create with custom options
    pub fn with_options(
        tiers: Vec<Arc<Box<dyn BlobStore>>>,
        auto_promote: bool,
        write_through: bool,
    ) -> Self {
        Self {
            tiers,
            auto_promote,
            write_through,
        }
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

    async fn get(&self, hash: &BlobHash) -> Result<Bytes> {
        // Try each tier in order
        for (tier_idx, tier) in self.tiers.iter().enumerate() {
            match tier.get(hash).await {
                Ok(content) => {
                    // Promote to faster tiers if not from fastest
                    if let Err(e) = self.promote(hash, &content, tier_idx).await {
                        warn!("Failed to promote blob after cache hit: {}", e);
                    }
                    return Ok(content);
                }
                Err(_) => {
                    // Try next tier
                    continue;
                }
            }
        }

        anyhow::bail!("Blob {} not found in any tier", hash)
    }

    async fn put(&self, content: Bytes) -> Result<BlobHash> {
        let hash = BlobHash::from_content(&content);

        if self.write_through {
            // Write to all tiers
            let write_tasks: Vec<_> = self
                .tiers
                .iter()
                .map(|tier| {
                    let tier = Arc::clone(tier);
                    let content = content.clone();
                    async move { tier.put(content).await }
                })
                .collect();

            // Wait for all writes to complete
            let results = futures_util::future::join_all(write_tasks).await;

            // Check if at least one succeeded
            let mut any_success = false;
            for (idx, result) in results.iter().enumerate() {
                match result {
                    Ok(_) => any_success = true,
                    Err(e) => warn!("Failed to write to tier {}: {}", idx, e),
                }
            }

            if !any_success {
                anyhow::bail!("All tier writes failed for blob {}", hash);
            }
        } else {
            // Write to first tier only (fastest)
            if let Some(first_tier) = self.tiers.first() {
                first_tier.put(content).await?;
            } else {
                anyhow::bail!("No tiers configured");
            }
        }

        Ok(hash)
    }

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

    async fn get_many(&self, hashes: &[BlobHash]) -> Result<Vec<Bytes>> {
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
                        // Partial success - treat as failure and try next tier
                        continue;
                    }

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
                Err(_) => {
                    // Try next tier with all remaining
                    continue;
                }
            }
        }

        if !remaining.is_empty() {
            anyhow::bail!("Some blobs not found in any tier");
        }

        // Sort by original index
        results.sort_by_key(|(idx, _)| *idx);
        Ok(results.into_iter().map(|(_, content)| content).collect())
    }

    async fn put_many(&self, contents: Vec<Bytes>) -> Result<Vec<BlobHash>> {
        if self.write_through {
            // Write to all tiers in parallel
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

            // Return first successful result
            if let Some(hashes) = results.into_iter().flatten().next() {
                return Ok(hashes);
            }

            anyhow::bail!("All tier writes failed for batch");
        } else {
            // Write to first tier only
            if let Some(first_tier) = self.tiers.first() {
                first_tier.put_many(contents).await
            } else {
                anyhow::bail!("No tiers configured");
            }
        }
    }

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

        // Wait for all deletes
        let results = futures_util::future::join_all(delete_tasks).await;

        // Check if at least one succeeded
        let mut any_success = false;
        for result in results {
            if result.is_ok() {
                any_success = true;
            }
        }

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

        // Check if at least one succeeded
        let mut any_success = false;
        for (idx, result) in results.iter().enumerate() {
            match result {
                Ok(_) => any_success = true,
                Err(e) => warn!("Failed to write manifest to tier {}: {}", idx, e),
            }
        }

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
    use tempfile::TempDir;

    async fn create_test_stores() -> (
        TempDir,
        TempDir,
        Arc<Box<dyn BlobStore>>,
        Arc<Box<dyn BlobStore>>,
    ) {
        let temp1 = TempDir::new().unwrap();
        let temp2 = TempDir::new().unwrap();

        let store1 = LocalBlobStore::new(temp1.path().to_path_buf());
        store1.init().await.unwrap();

        let store2 = LocalBlobStore::new(temp2.path().to_path_buf());
        store2.init().await.unwrap();

        let store1: Arc<Box<dyn BlobStore>> = Arc::new(Box::new(store1));
        let store2: Arc<Box<dyn BlobStore>> = Arc::new(Box::new(store2));

        (temp1, temp2, store1, store2)
    }

    #[tokio::test]
    async fn test_layered_basic_operations() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;
        let layered = LayeredBlobStore::new(vec![store1, store2]);

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
        let layered = LayeredBlobStore::with_options(vec![store1.clone(), store2], true, false);

        // First get should trigger promotion
        let retrieved = layered.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);

        // Verify it's now in first tier
        assert!(store1.contains(&hash).await.unwrap());
    }

    #[tokio::test]
    async fn test_layered_write_through() {
        let (_temp1, _temp2, store1, store2) = create_test_stores().await;

        // Create layered store with write-through
        let layered =
            LayeredBlobStore::with_options(vec![store1.clone(), store2.clone()], false, true);

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

        let layered = LayeredBlobStore::new(vec![store1, store2]);

        // Should find it via fallback
        let retrieved = layered.get(&hash).await.unwrap();
        assert_eq!(content, retrieved);
    }
}
