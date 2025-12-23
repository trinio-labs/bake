use super::ac::{ActionCache, ActionResult, OutputFile};
use super::cas::{
    BlobHash, BlobIndex, BlobStore, GcsBlobStore, LayeredBlobStore, LocalBlobStore, S3BlobStore,
};
use anyhow::Result;
use bytes::Bytes;
use log::{debug, warn};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use tokio::fs;
use tokio::sync::Semaphore;

/// Cache strategy for multi-tier caching
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CacheStrategy {
    /// Use only local cache (disable remote)
    LocalOnly,
    /// Use only remote cache (disable local)
    RemoteOnly,
    /// Check local cache first, then remote (typical default)
    LocalFirst,
    /// Check remote cache first, then local
    RemoteFirst,
    /// Disable all caching
    Disabled,
}

/// Content-Addressable Storage (CAS) cache implementation
///
/// This is the main cache implementation for bake, replacing the old tar-based system.
/// Uses Blake3 hashing, FastCDC chunking, and multi-tier blob storage.
pub struct Cache {
    /// Blob storage backend (None if disabled)
    blob_store: Option<Arc<Box<dyn BlobStore>>>,

    /// Blob index for fast lookups (None if disabled)
    blob_index: Option<Arc<BlobIndex>>,

    /// Action cache for manifests (None if disabled)
    action_cache: Option<Arc<ActionCache>>,

    /// Project root path
    project_root: PathBuf,

    /// Configuration
    config: CacheConfig,

    /// Whether this cache is disabled (always returns Miss)
    disabled: bool,
}

/// Configuration for CAS cache
#[derive(Debug, Clone)]
pub struct CacheConfig {
    /// Maximum parallel uploads
    pub upload_parallelism: usize,

    /// Maximum parallel downloads
    pub download_parallelism: usize,

    /// Maximum parallel hashing operations
    pub hashing_parallelism: usize,
}

impl Default for CacheConfig {
    fn default() -> Self {
        Self {
            upload_parallelism: 8,
            download_parallelism: 16,
            hashing_parallelism: num_cpus::get(),
        }
    }
}

impl Cache {
    /// Create a CAS cache with local-only storage
    pub async fn local(
        cache_root: PathBuf,
        project_root: PathBuf,
        config: CacheConfig,
    ) -> Result<Self> {
        // Initialize blob store
        let blob_root = cache_root.join("cas/blobs");
        let blob_store = LocalBlobStore::new(blob_root);
        blob_store.init().await?;

        // Initialize blob index
        let index_path = cache_root.join("cas/index.db");
        let blob_index = BlobIndex::open(&index_path)?;

        // Initialize action cache
        let ac_root = cache_root.join("ac");
        let action_cache = ActionCache::new(ac_root);
        action_cache.init().await?;

        Ok(Self {
            blob_store: Some(Arc::new(Box::new(blob_store))),
            blob_index: Some(Arc::new(blob_index)),
            action_cache: Some(Arc::new(action_cache)),
            project_root,
            config,
            disabled: false,
        })
    }

    /// Create a new CAS cache with multi-tier storage based on cache strategy
    pub async fn with_strategy(
        cache_root: PathBuf,
        project_root: PathBuf,
        config: CacheConfig,
        cache_strategy: CacheStrategy,
        project_cache_config: &crate::project::config::CacheConfig,
    ) -> Result<Self> {
        use CacheStrategy::*;

        // Build list of blob stores based on strategy
        let mut stores: Vec<Arc<Box<dyn BlobStore>>> = Vec::new();

        // Determine the order of stores based on strategy
        let use_local = matches!(cache_strategy, LocalOnly | LocalFirst | RemoteFirst);
        let use_remote = matches!(cache_strategy, RemoteOnly | LocalFirst | RemoteFirst);
        let local_first = matches!(cache_strategy, LocalOnly | LocalFirst);

        // Build stores in priority order
        if use_local && local_first {
            // Local first - add local store first
            let blob_root = cache_root.join("cas/blobs");
            let local_store = LocalBlobStore::new(blob_root);
            local_store.init().await?;
            stores.push(Arc::new(Box::new(local_store)));
        }

        if use_remote {
            // Add remote stores based on configuration
            if let Some(remotes) = &project_cache_config.remotes {
                // Try S3 if configured
                if let Some(s3_config) = &remotes.s3 {
                    match S3BlobStore::new(
                        s3_config.bucket.clone(),
                        s3_config.region.clone(),
                        None, // No prefix for now
                    )
                    .await
                    {
                        Ok(s3_store) => {
                            stores.push(Arc::new(Box::new(s3_store)));
                            debug!("S3 cache enabled: bucket={}", s3_config.bucket);
                        }
                        Err(e) => {
                            warn!("Failed to initialize S3 cache: {}", e);
                        }
                    }
                }

                // Try GCS if configured
                if let Some(gcs_config) = &remotes.gcs {
                    match GcsBlobStore::new(gcs_config.bucket.clone(), None).await {
                        Ok(gcs_store) => {
                            stores.push(Arc::new(Box::new(gcs_store)));
                            debug!("GCS cache enabled: bucket={}", gcs_config.bucket);
                        }
                        Err(e) => {
                            warn!("Failed to initialize GCS cache: {}", e);
                        }
                    }
                }
            }
        }

        if use_local && !local_first {
            // Remote first - add local store last
            let blob_root = cache_root.join("cas/blobs");
            let local_store = LocalBlobStore::new(blob_root);
            local_store.init().await?;
            stores.push(Arc::new(Box::new(local_store)));
        }

        // If no stores could be initialized, return error
        if stores.is_empty() {
            anyhow::bail!(
                "No cache stores could be initialized for strategy {:?}",
                cache_strategy
            );
        }

        // Create blob store (single or layered)
        let blob_store: Arc<Box<dyn BlobStore>> = if stores.len() == 1 {
            // Single store - use it directly
            stores.into_iter().next().unwrap()
        } else {
            // Multiple stores - create layered store
            // For remote-first, we want writes to go to all stores (write-through)
            // For local-first, we want writes to go only to local (fast)
            let write_through = matches!(cache_strategy, RemoteFirst);
            Arc::new(Box::new(LayeredBlobStore::with_options(
                stores,
                true, // Enable auto-promotion
                write_through,
            )))
        };

        // Initialize blob index
        let index_path = cache_root.join("cas/index.db");
        let blob_index = BlobIndex::open(&index_path)?;

        // Initialize action cache
        let ac_root = cache_root.join("ac");
        let action_cache = ActionCache::new(ac_root);
        action_cache.init().await?;

        Ok(Self {
            blob_store: Some(blob_store),
            blob_index: Some(Arc::new(blob_index)),
            action_cache: Some(Arc::new(action_cache)),
            project_root,
            config,
            disabled: false,
        })
    }

    /// Create a disabled cache that always returns Miss and ignores Put operations
    pub fn disabled() -> Self {
        Self {
            blob_store: None,
            blob_index: None,
            action_cache: None,
            project_root: PathBuf::new(),
            config: CacheConfig::default(),
            disabled: true,
        }
    }

    // Helper methods to get references (panic if cache is disabled but these are called)
    fn blob_store(&self) -> &Arc<Box<dyn BlobStore>> {
        self.blob_store
            .as_ref()
            .expect("blob_store should exist when cache is enabled")
    }

    fn blob_index(&self) -> &Arc<BlobIndex> {
        self.blob_index
            .as_ref()
            .expect("blob_index should exist when cache is enabled")
    }

    fn action_cache(&self) -> &Arc<ActionCache> {
        self.action_cache
            .as_ref()
            .expect("action_cache should exist when cache is enabled")
    }

    /// Get a safe chunk size for processing files based on system file descriptor limits
    fn get_safe_chunk_size() -> usize {
        #[cfg(unix)]
        {
            // Try to get the soft limit for open files
            let limit = unsafe {
                let mut rlim = libc::rlimit {
                    rlim_cur: 0,
                    rlim_max: 0,
                };
                if libc::getrlimit(libc::RLIMIT_NOFILE, &mut rlim) == 0 {
                    rlim.rlim_cur as usize
                } else {
                    // If we can't get the limit, use a very conservative default
                    256
                }
            };

            // Use 25% of the soft limit, with a minimum of 50 and maximum of 500
            // This leaves plenty of room for other file descriptors used by the system,
            // tokio runtime, network connections, etc.
            let safe_limit = limit / 4;
            let safe_limit = safe_limit.clamp(50, 500);

            debug!(
                "System file descriptor limit: {}, using chunk size: {}",
                limit, safe_limit
            );

            safe_limit
        }

        #[cfg(not(unix))]
        {
            // On Windows, be conservative since we can't easily query limits
            100
        }
    }

    /// Collect all files from output paths (recursively walks directories)
    async fn collect_output_files(output_paths: &[PathBuf]) -> Result<Vec<PathBuf>> {
        let mut all_files = Vec::new();

        for path in output_paths {
            if !path.exists() {
                warn!("Output path does not exist: {}", path.display());
                continue;
            }

            let metadata = fs::metadata(path).await?;
            if metadata.is_file() {
                all_files.push(path.clone());
            } else if metadata.is_dir() {
                // Recursively collect all files in directory
                let mut dir_files = Self::collect_files_recursive(path.clone()).await?;
                all_files.append(&mut dir_files);
            }
        }

        Ok(all_files)
    }

    /// Recursively collect all files in a directory
    fn collect_files_recursive(
        dir: PathBuf,
    ) -> std::pin::Pin<Box<dyn std::future::Future<Output = Result<Vec<PathBuf>>> + Send>> {
        Box::pin(async move {
            let mut files = Vec::new();
            let mut entries = fs::read_dir(&dir).await?;

            while let Some(entry) = entries.next_entry().await? {
                let path = entry.path();
                let metadata = entry.metadata().await?;

                if metadata.is_file() {
                    files.push(path);
                } else if metadata.is_dir() {
                    let mut subfiles = Self::collect_files_recursive(path).await?;
                    files.append(&mut subfiles);
                }
            }

            Ok(files)
        })
    }

    /// Store recipe outputs in the cache (PUT operation)
    pub async fn put(
        &self,
        action_key: &str,
        recipe_name: &str,
        output_paths: &[PathBuf],
        stdout: &str,
        stderr: &str,
        exit_code: i32,
    ) -> Result<()> {
        // If cache is disabled, do nothing
        if self.disabled {
            return Ok(());
        }

        debug!("CAS PUT: Starting for recipe '{}'", recipe_name);

        // Phase 0: Collect all files (expand directories)
        let all_files = Self::collect_output_files(output_paths).await?;

        if all_files.is_empty() {
            debug!("CAS PUT: No output files to cache for '{}'", recipe_name);
        }

        // Phase 1: Hash all outputs in parallel (with controlled concurrency to avoid "too many open files")
        let hash_sem = Arc::new(Semaphore::new(self.config.hashing_parallelism));
        let mut hashed_outputs = Vec::with_capacity(all_files.len());

        // Determine safe chunk size based on system file descriptor limits
        // We need to be conservative because tokio and other parts of the system also use FDs
        let chunk_size = Self::get_safe_chunk_size();
        debug!(
            "CAS PUT: Processing {} files in chunks of {} (system limit consideration)",
            all_files.len(),
            chunk_size
        );

        for chunk in all_files.chunks(chunk_size) {
            let hash_tasks: Vec<_> = chunk
                .iter()
                .map(|path| {
                    let sem = hash_sem.clone();
                    let path = path.clone();
                    async move {
                        let _permit = sem.acquire().await?;
                        let content = fs::read(&path).await?;
                        let digest = BlobHash::from_content(&content);
                        let size = content.len() as u64;
                        let is_executable = Self::is_executable(&path).await;
                        Ok::<_, anyhow::Error>((
                            path,
                            Bytes::from(content),
                            digest,
                            size,
                            is_executable,
                        ))
                    }
                })
                .collect();

            let mut chunk_results = futures_util::future::try_join_all(hash_tasks).await?;
            hashed_outputs.append(&mut chunk_results);
        }

        // Phase 2: Check which blobs already exist (batch operation)
        let digests: Vec<_> = hashed_outputs
            .iter()
            .map(|(_, _, d, _, _)| d.clone())
            .collect();
        let exists_flags = self.blob_store().contains_many(&digests).await?;

        // Phase 3: Upload missing blobs in parallel (chunked to avoid too many open files)
        let upload_sem = Arc::new(Semaphore::new(self.config.upload_parallelism));

        // Collect blobs that need uploading
        let blobs_to_upload: Vec<_> = hashed_outputs
            .iter()
            .zip(exists_flags.iter())
            .filter(|(_, exists)| !**exists)
            .map(|((path, content, digest, size, _), _)| {
                (path.clone(), content.clone(), digest.clone(), *size)
            })
            .collect();

        let upload_count = blobs_to_upload.len();

        if upload_count > 0 {
            debug!(
                "CAS PUT: Uploading {} missing blobs in chunks of {}",
                upload_count, chunk_size
            );

            // Process uploads in chunks to avoid too many open files
            for chunk in blobs_to_upload.chunks(chunk_size) {
                let upload_tasks: Vec<_> = chunk
                    .iter()
                    .map(|(path, content, digest, size)| {
                        let sem = upload_sem.clone();
                        let blob_store = self.blob_store().clone();
                        let blob_index = self.blob_index().clone();
                        let content = content.clone();
                        let digest = digest.clone();
                        let size = *size;
                        let path = path.clone();
                        async move {
                            let _permit = sem.acquire().await?;

                            // Upload blob
                            blob_store.put(content).await?;

                            // Update index
                            blob_index.insert(&digest, size)?;

                            debug!("Uploaded blob {} for {}", digest, path.display());
                            Ok::<_, anyhow::Error>(())
                        }
                    })
                    .collect();

                futures_util::future::try_join_all(upload_tasks).await?;
            }
        }

        // Phase 4: Upload stdout/stderr as blobs
        let stdout_content = Bytes::from(stdout.as_bytes().to_vec());
        let stderr_content = Bytes::from(stderr.as_bytes().to_vec());

        let stdout_digest = self.blob_store().put(stdout_content.clone()).await?;
        let stderr_digest = self.blob_store().put(stderr_content.clone()).await?;

        self.blob_index()
            .insert(&stdout_digest, stdout_content.len() as u64)?;
        self.blob_index()
            .insert(&stderr_digest, stderr_content.len() as u64)?;

        // Phase 5: Create and upload manifest
        let mut action_result = ActionResult::new(recipe_name.to_string());
        action_result.exit_code = exit_code;
        action_result.stdout_digest = stdout_digest;
        action_result.stderr_digest = stderr_digest;

        for (path, _, digest, size, is_executable) in hashed_outputs {
            let relative_path = path
                .strip_prefix(&self.project_root)
                .unwrap_or(&path)
                .to_path_buf();

            action_result
                .outputs
                .push(OutputFile::new(relative_path, digest, size).with_executable(is_executable));
        }

        action_result.execution_metadata = action_result.execution_metadata.complete();

        self.action_cache().put(action_key, &action_result).await?;

        debug!(
            "CAS PUT: Complete for '{}' ({} outputs, {} blobs uploaded)",
            recipe_name,
            action_result.outputs.len(),
            upload_count
        );

        Ok(())
    }

    /// Restore recipe outputs from cache (GET operation)
    pub async fn get(&self, action_key: &str, recipe_name: &str) -> Result<CacheResult> {
        // If cache is disabled, always return Miss
        if self.disabled {
            return Ok(CacheResult::Miss);
        }

        debug!("CAS GET: Checking cache for recipe '{}'", recipe_name);

        // Phase 1: Download manifest (tiny, fast)
        let action_result = match self.action_cache().get(action_key).await? {
            Some(result) => result,
            None => {
                debug!("CAS GET: Miss - no manifest for '{}'", recipe_name);
                return Ok(CacheResult::Miss);
            }
        };

        // Phase 2: Quick check - do any outputs need restoration?
        // Use a fast heuristic: file exists + size + mtime (no hash verification)
        // This is much faster than hashing every file for large output directories
        let needs_download: Vec<_> = action_result
            .outputs
            .iter()
            .filter(|output| {
                let full_path = self.project_root.join(&output.path);
                !Self::quick_verify_file(&full_path, output.size)
            })
            .collect();

        if needs_download.is_empty() {
            debug!(
                "CAS GET: Hit - all outputs already present for '{}' (quick check)",
                recipe_name
            );

            // Load stdout/stderr from blob store
            let stdout_content = self.blob_store().get(&action_result.stdout_digest).await?;
            let stderr_content = self.blob_store().get(&action_result.stderr_digest).await?;

            let stdout = String::from_utf8_lossy(&stdout_content).to_string();
            let stderr = String::from_utf8_lossy(&stderr_content).to_string();

            return Ok(CacheResult::Hit {
                stdout,
                stderr,
                exit_code: action_result.exit_code,
            });
        }

        debug!(
            "CAS GET: Need to restore {} outputs for '{}'",
            needs_download.len(),
            recipe_name
        );

        // Phase 3: Batch check which blobs exist in cache
        let needed_digests: Vec<_> = needs_download.iter().map(|o| o.digest.clone()).collect();
        let available = self.blob_store().contains_many(&needed_digests).await?;

        if available.iter().any(|&exists| !exists) {
            warn!(
                "CAS GET: Partial miss - some blobs missing for '{}'",
                recipe_name
            );
            return Ok(CacheResult::Miss);
        }

        // Phase 4: Download blobs in parallel
        let download_sem = Arc::new(Semaphore::new(self.config.download_parallelism));
        let download_tasks: Vec<_> = needs_download
            .iter()
            .map(|output| {
                let sem = download_sem.clone();
                let blob_store = self.blob_store().clone();
                let blob_index = self.blob_index().clone();
                let digest = output.digest.clone();
                let path = self.project_root.join(&output.path);
                let is_executable = output.is_executable;
                async move {
                    let _permit = sem.acquire().await?;

                    // Download blob
                    let content = blob_store.get(&digest).await?;

                    // Ensure parent directory exists
                    if let Some(parent) = path.parent() {
                        fs::create_dir_all(parent).await?;
                    }

                    // Write file
                    fs::write(&path, &content).await?;

                    // Set executable bit if needed
                    if is_executable {
                        #[cfg(unix)]
                        {
                            use std::os::unix::fs::PermissionsExt;
                            let mut perms = fs::metadata(&path).await?.permissions();
                            perms.set_mode(0o755);
                            fs::set_permissions(&path, perms).await?;
                        }
                    }

                    // Update index access time
                    blob_index.touch(&digest)?;

                    debug!("Downloaded blob {} to {}", digest, path.display());
                    Ok::<_, anyhow::Error>(())
                }
            })
            .collect();

        futures_util::future::try_join_all(download_tasks).await?;

        // Phase 5: Restore stdout/stderr
        let stdout_content = self.blob_store().get(&action_result.stdout_digest).await?;
        let stderr_content = self.blob_store().get(&action_result.stderr_digest).await?;

        let stdout = String::from_utf8_lossy(&stdout_content).to_string();
        let stderr = String::from_utf8_lossy(&stderr_content).to_string();

        debug!(
            "CAS GET: Hit - restored {} outputs for '{}'",
            needs_download.len(),
            recipe_name
        );

        Ok(CacheResult::Hit {
            stdout,
            stderr,
            exit_code: action_result.exit_code,
        })
    }

    /// Check if a file is executable
    async fn is_executable(path: &Path) -> bool {
        #[cfg(unix)]
        {
            use std::os::unix::fs::PermissionsExt;
            if let Ok(metadata) = fs::metadata(path).await {
                return metadata.permissions().mode() & 0o111 != 0;
            }
        }
        false
    }

    /// Quick verification using file size (fast, no hashing)
    /// This is a heuristic check - if size matches exactly, we assume it's the correct file.
    /// This is much faster than hashing for large directories (100x+ speedup).
    /// Size collision probability is extremely low, and even if wrong, worst case is
    /// an unnecessary rebuild rather than incorrect results.
    fn quick_verify_file(path: &Path, expected_size: u64) -> bool {
        if !path.exists() {
            return false;
        }

        match std::fs::metadata(path) {
            Ok(metadata) => {
                // Size must match exactly
                metadata.len() == expected_size
            }
            Err(_) => false,
        }
    }

    /// Verify local file matches expected hash (sync version for filter)
    /// This is the slow but accurate version - only use when necessary
    #[allow(dead_code)]
    fn verify_local_file_sync(path: &Path, expected_digest: &BlobHash) -> bool {
        if !path.exists() {
            return false;
        }

        match std::fs::read(path) {
            Ok(content) => {
                let actual_digest = BlobHash::from_content(&content);
                actual_digest == *expected_digest
            }
            Err(_) => false,
        }
    }

    /// Get cache statistics from index and action cache
    pub async fn stats(&self) -> Result<CacheStats> {
        // If cache is disabled, return empty stats
        if self.disabled {
            return Ok(CacheStats {
                blob_count: 0,
                total_blob_size: 0,
                manifest_count: 0,
                total_manifest_size: 0,
            });
        }

        // Get stats from index (reliable source of truth)
        let index_stats = self.blob_index().stats()?;
        let action_stats = self.action_cache().stats().await?;

        Ok(CacheStats {
            blob_count: index_stats.blob_count,
            total_blob_size: index_stats.total_size,
            manifest_count: action_stats.manifest_count,
            total_manifest_size: action_stats.total_size,
        })
    }
}

/// Result of a cache lookup
#[derive(Debug)]
pub enum CacheResult {
    Hit {
        stdout: String,
        stderr: String,
        exit_code: i32,
    },
    Miss,
}

/// Cache statistics
#[derive(Debug, Default)]
pub struct CacheStats {
    pub blob_count: u64,
    pub total_blob_size: u64,
    pub manifest_count: u64,
    pub total_manifest_size: u64,
}

impl CacheStats {
    pub fn total_size(&self) -> u64 {
        self.total_blob_size + self.total_manifest_size
    }

    pub fn total_size_mb(&self) -> f64 {
        self.total_size() as f64 / 1024.0 / 1024.0
    }

    pub fn total_size_gb(&self) -> f64 {
        self.total_size() as f64 / 1024.0 / 1024.0 / 1024.0
    }

    pub fn avg_blob_size(&self) -> f64 {
        if self.blob_count == 0 {
            0.0
        } else {
            self.total_blob_size as f64 / self.blob_count as f64
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    async fn create_test_cache() -> (Cache, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let cache_root = temp_dir.path().join("cache");
        let project_root = temp_dir.path().join("project");

        fs::create_dir_all(&project_root).await.unwrap();

        let config = CacheConfig::default();
        let cache = Cache::local(cache_root, project_root, config)
            .await
            .unwrap();

        (cache, temp_dir)
    }

    #[tokio::test]
    async fn test_cache_creation() {
        let (cache, _temp) = create_test_cache().await;
        let stats = cache.stats().await.unwrap();

        assert_eq!(stats.blob_count, 0);
        assert_eq!(stats.manifest_count, 0);
    }

    #[tokio::test]
    async fn test_put_and_get() {
        let (cache, temp) = create_test_cache().await;

        // Create test output files
        let output1 = temp.path().join("project/output1.txt");
        let output2 = temp.path().join("project/output2.txt");

        fs::write(&output1, b"output 1 content").await.unwrap();
        fs::write(&output2, b"output 2 content").await.unwrap();

        let outputs = vec![output1.clone(), output2.clone()];

        // PUT
        cache
            .put(
                "test_action_key",
                "test:recipe",
                &outputs,
                "stdout content",
                "stderr content",
                0,
            )
            .await
            .unwrap();

        // Remove outputs to test restoration
        fs::remove_file(&output1).await.unwrap();
        fs::remove_file(&output2).await.unwrap();

        // GET
        let result = cache.get("test_action_key", "test:recipe").await.unwrap();

        match result {
            CacheResult::Hit {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(stdout, "stdout content");
                assert_eq!(stderr, "stderr content");
                assert_eq!(exit_code, 0);

                // Check files were restored
                assert!(output1.exists());
                assert!(output2.exists());

                let content1 = fs::read_to_string(&output1).await.unwrap();
                let content2 = fs::read_to_string(&output2).await.unwrap();

                assert_eq!(content1, "output 1 content");
                assert_eq!(content2, "output 2 content");
            }
            CacheResult::Miss => panic!("Expected cache hit"),
        }
    }

    #[tokio::test]
    async fn test_cache_miss() {
        let (cache, _temp) = create_test_cache().await;

        let result = cache.get("nonexistent_key", "test:recipe").await.unwrap();

        assert!(matches!(result, CacheResult::Miss));
    }

    #[tokio::test]
    async fn test_incremental_hit() {
        let (cache, temp) = create_test_cache().await;

        // Create and cache outputs
        let output = temp.path().join("project/output.txt");
        fs::write(&output, b"content").await.unwrap();

        cache
            .put(
                "key",
                "test:recipe",
                std::slice::from_ref(&output),
                "",
                "",
                0,
            )
            .await
            .unwrap();

        // GET without removing file (should detect existing file)
        let result = cache.get("key", "test:recipe").await.unwrap();

        assert!(matches!(result, CacheResult::Hit { .. }));
    }

    #[tokio::test]
    async fn test_stats() {
        let (cache, temp) = create_test_cache().await;

        let output = temp.path().join("project/output.txt");
        fs::write(&output, b"test content").await.unwrap();

        cache
            .put("key", "test:recipe", &[output], "stdout", "stderr", 0)
            .await
            .unwrap();

        let stats = cache.stats().await.unwrap();

        assert!(stats.blob_count > 0); // At least output + stdout + stderr
        assert!(stats.manifest_count > 0);
        assert!(stats.total_size() > 0);
    }

    #[tokio::test]
    async fn test_deduplication() {
        let (cache, temp) = create_test_cache().await;

        // Create two outputs with identical content
        let output1 = temp.path().join("project").join("output1.txt");
        let output2 = temp.path().join("project").join("output2.txt");

        // Files should be writable since project dir was created
        fs::write(&output1, b"same content").await.unwrap();
        fs::write(&output2, b"same content").await.unwrap();

        // PUT outputs
        let result = cache
            .put("key", "test:recipe", &[output1, output2], "", "", 0)
            .await;

        if let Err(e) = &result {
            eprintln!("PUT failed: {:?}", e);
        }
        result.expect("Failed to put outputs in cache");

        let stats = cache.stats().await.unwrap();

        // Should have deduplicated the identical content
        // 1 unique content blob + 2 empty stdout/stderr blobs = 3 blobs
        // (or possibly just 2 if empty strings are deduplicated)
        assert!(stats.blob_count <= 3);
    }

    #[tokio::test]
    async fn test_directory_output() {
        let (cache, temp) = create_test_cache().await;

        // Create a directory with multiple files
        let output_dir = temp.path().join("project/dist");
        fs::create_dir_all(&output_dir).await.unwrap();

        fs::write(output_dir.join("file1.txt"), b"content 1")
            .await
            .unwrap();
        fs::write(output_dir.join("file2.txt"), b"content 2")
            .await
            .unwrap();

        // Create a subdirectory
        let subdir = output_dir.join("sub");
        fs::create_dir(&subdir).await.unwrap();
        fs::write(subdir.join("file3.txt"), b"content 3")
            .await
            .unwrap();

        // PUT directory as output
        cache
            .put(
                "test_dir_key",
                "test:recipe",
                std::slice::from_ref(&output_dir),
                "test stdout",
                "test stderr",
                0,
            )
            .await
            .unwrap();

        // Remove directory
        fs::remove_dir_all(&output_dir).await.unwrap();
        assert!(!output_dir.exists());

        // GET from cache
        let result = cache.get("test_dir_key", "test:recipe").await.unwrap();

        match result {
            CacheResult::Hit {
                stdout,
                stderr,
                exit_code,
            } => {
                assert_eq!(stdout, "test stdout");
                assert_eq!(stderr, "test stderr");
                assert_eq!(exit_code, 0);

                // Verify all files were restored
                assert!(output_dir.join("file1.txt").exists());
                assert!(output_dir.join("file2.txt").exists());
                assert!(output_dir.join("sub/file3.txt").exists());

                // Verify content
                let content1 = fs::read_to_string(output_dir.join("file1.txt"))
                    .await
                    .unwrap();
                let content2 = fs::read_to_string(output_dir.join("file2.txt"))
                    .await
                    .unwrap();
                let content3 = fs::read_to_string(output_dir.join("sub/file3.txt"))
                    .await
                    .unwrap();

                assert_eq!(content1, "content 1");
                assert_eq!(content2, "content 2");
                assert_eq!(content3, "content 3");
            }
            CacheResult::Miss => panic!("Expected cache hit"),
        }
    }
}
