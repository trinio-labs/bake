use super::manifest::ActionResult;
use anyhow::Result;
use hex;
use log::debug;
use std::path::{Path, PathBuf};
use tokio::fs;

/// Action cache stores manifests (recipe results) keyed by action hash
pub struct ActionCache {
    root: PathBuf,
}

impl ActionCache {
    /// Create a new action cache
    pub fn new(root: PathBuf) -> Self {
        Self { root }
    }

    /// Initialize the action cache directory
    pub async fn init(&self) -> Result<()> {
        fs::create_dir_all(&self.root).await?;
        debug!("Initialized action cache at: {}", self.root.display());
        Ok(())
    }

    /// Get manifest path for an action key
    fn get_manifest_path(&self, action_key: &str) -> PathBuf {
        // Use hex encoding of the key to ensure unique, safe filenames
        // This is reversible, avoiding collisions (e.g., "foo:bar" vs "foo_bar")
        let safe_key = hex::encode(action_key);
        self.root.join(format!("{}.json", safe_key))
    }

    /// Decode a hex-encoded filename back to the original action key
    fn decode_filename(&self, filename: &str) -> Option<String> {
        let hex_str = filename.trim_end_matches(".json");
        hex::decode(hex_str)
            .ok()
            .and_then(|bytes| String::from_utf8(bytes).ok())
    }

    /// Store an action result
    pub async fn put(&self, action_key: &str, result: &ActionResult) -> Result<()> {
        let path = self.get_manifest_path(action_key);

        // Create parent directories
        if let Some(parent) = path.parent() {
            fs::create_dir_all(parent).await?;
        }

        // Serialize to JSON
        let json = result.to_json()?;

        // Write atomically using temp file
        let temp_path = path.with_extension("tmp");
        fs::write(&temp_path, json).await?;
        fs::rename(&temp_path, &path).await?;

        debug!("Stored action result for key: {}", action_key);
        Ok(())
    }

    /// Get an action result
    pub async fn get(&self, action_key: &str) -> Result<Option<ActionResult>> {
        let path = self.get_manifest_path(action_key);

        // Use async exists check to avoid blocking the runtime
        if !fs::try_exists(&path).await.unwrap_or(false) {
            return Ok(None);
        }

        let json = fs::read_to_string(&path).await?;
        let result = ActionResult::from_json(&json)?;

        debug!("Retrieved action result for key: {}", action_key);
        Ok(Some(result))
    }

    /// Check if an action result exists
    pub async fn contains(&self, action_key: &str) -> bool {
        let path = self.get_manifest_path(action_key);
        fs::try_exists(&path).await.unwrap_or(false)
    }

    /// Delete an action result
    pub async fn delete(&self, action_key: &str) -> Result<()> {
        let path = self.get_manifest_path(action_key);

        if fs::try_exists(&path).await.unwrap_or(false) {
            fs::remove_file(&path).await?;
            debug!("Deleted action result for key: {}", action_key);
        }

        Ok(())
    }

    /// List all action keys
    pub async fn list(&self) -> Result<Vec<String>> {
        let mut keys = Vec::new();

        if !fs::try_exists(&self.root).await.unwrap_or(false) {
            return Ok(keys);
        }

        let mut entries = fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let file_name = entry.file_name();
                let file_name_str = file_name.to_string_lossy();

                if file_name_str.ends_with(".json") {
                    // Decode hex-encoded filename back to original key
                    if let Some(key) = self.decode_filename(&file_name_str) {
                        keys.push(key);
                    }
                }
            }
        }

        Ok(keys)
    }

    /// Get cache statistics
    pub async fn stats(&self) -> Result<ActionCacheStats> {
        let mut stats = ActionCacheStats::default();

        if !fs::try_exists(&self.root).await.unwrap_or(false) {
            return Ok(stats);
        }

        let mut entries = fs::read_dir(&self.root).await?;
        while let Some(entry) = entries.next_entry().await? {
            if entry.file_type().await?.is_file() {
                let metadata = entry.metadata().await?;
                stats.manifest_count += 1;
                stats.total_size += metadata.len();
            }
        }

        Ok(stats)
    }

    /// Get the root path
    pub fn path(&self) -> &Path {
        &self.root
    }
}

/// Action cache statistics
#[derive(Debug, Default)]
pub struct ActionCacheStats {
    pub manifest_count: u64,
    pub total_size: u64,
}

impl ActionCacheStats {
    pub fn total_size_kb(&self) -> f64 {
        self.total_size as f64 / 1024.0
    }

    pub fn avg_manifest_size(&self) -> f64 {
        if self.manifest_count == 0 {
            0.0
        } else {
            self.total_size as f64 / self.manifest_count as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_action_cache() -> (ActionCache, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let cache = ActionCache::new(temp_dir.path().join("ac"));
        cache.init().await.unwrap();
        (cache, temp_dir)
    }

    fn create_test_result() -> ActionResult {
        ActionResult::new("test:recipe".to_string())
    }

    #[tokio::test]
    async fn test_init() {
        let temp_dir = TempDir::new().unwrap();
        let cache = ActionCache::new(temp_dir.path().join("ac"));

        cache.init().await.unwrap();
        assert!(cache.path().exists());
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let (cache, _temp) = create_test_action_cache().await;
        let result = create_test_result();

        cache.put("test_key", &result).await.unwrap();

        let retrieved = cache.get("test_key").await.unwrap();
        assert!(retrieved.is_some());

        let retrieved = retrieved.unwrap();
        assert_eq!(retrieved.recipe, result.recipe);
    }

    #[tokio::test]
    async fn test_get_nonexistent() {
        let (cache, _temp) = create_test_action_cache().await;

        let result = cache.get("nonexistent").await.unwrap();
        assert!(result.is_none());
    }

    #[tokio::test]
    async fn test_contains() {
        let (cache, _temp) = create_test_action_cache().await;
        let result = create_test_result();

        assert!(!cache.contains("test_key").await);

        cache.put("test_key", &result).await.unwrap();
        assert!(cache.contains("test_key").await);
    }

    #[tokio::test]
    async fn test_delete() {
        let (cache, _temp) = create_test_action_cache().await;
        let result = create_test_result();

        cache.put("test_key", &result).await.unwrap();
        assert!(cache.contains("test_key").await);

        cache.delete("test_key").await.unwrap();
        assert!(!cache.contains("test_key").await);
    }

    #[tokio::test]
    async fn test_list() {
        let (cache, _temp) = create_test_action_cache().await;

        cache.put("key1", &create_test_result()).await.unwrap();
        cache.put("key2", &create_test_result()).await.unwrap();
        cache.put("key3", &create_test_result()).await.unwrap();

        let keys = cache.list().await.unwrap();
        assert_eq!(keys.len(), 3);
        assert!(keys.contains(&"key1".to_string()));
        assert!(keys.contains(&"key2".to_string()));
        assert!(keys.contains(&"key3".to_string()));
    }

    #[tokio::test]
    async fn test_stats() {
        let (cache, _temp) = create_test_action_cache().await;

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.manifest_count, 0);
        assert_eq!(stats.total_size, 0);

        cache.put("key1", &create_test_result()).await.unwrap();
        cache.put("key2", &create_test_result()).await.unwrap();

        let stats = cache.stats().await.unwrap();
        assert_eq!(stats.manifest_count, 2);
        assert!(stats.total_size > 0);
    }

    #[tokio::test]
    async fn test_special_characters_in_key() {
        let (cache, _temp) = create_test_action_cache().await;
        let result = create_test_result();

        // Keys with special characters should be handled safely
        let special_key = "cookbook:recipe/with:special/chars";
        cache.put(special_key, &result).await.unwrap();

        let retrieved = cache.get(special_key).await.unwrap();
        assert!(retrieved.is_some());
    }

    #[tokio::test]
    async fn test_overwrite_existing() {
        let (cache, _temp) = create_test_action_cache().await;

        let mut result1 = create_test_result();
        result1.exit_code = 0;

        let mut result2 = create_test_result();
        result2.exit_code = 1;

        cache.put("key", &result1).await.unwrap();
        cache.put("key", &result2).await.unwrap();

        let retrieved = cache.get("key").await.unwrap().unwrap();
        assert_eq!(retrieved.exit_code, 1);
    }
}
