// Integration tests for the CAS (Content-Addressable Storage) cache system
//
// These tests verify the complete PUT/GET cycle including:
// - BlobStore operations (local, layered)
// - ActionCache manifest storage and retrieval
// - Compression and deduplication
// - Content-defined chunking
// - Manifest signing and verification

use anyhow::Result;
use bake::cache::ac::{ActionCache, ActionResult, ManifestSigner, OutputFile};
use bake::cache::cas::{
    BlobHash, BlobStore, ChunkStats, FastCDC, LayeredBlobStore, LocalBlobStore,
};
use bytes::Bytes;
use std::path::PathBuf;
use tempfile::TempDir;

/// Helper to create a test blob store
async fn create_test_store() -> Result<(TempDir, LocalBlobStore)> {
    let temp_dir = TempDir::new()?;
    let store = LocalBlobStore::new(temp_dir.path().join("blobs"));
    store.init().await?;
    Ok((temp_dir, store))
}

/// Helper to create test action cache
async fn create_test_action_cache() -> Result<(TempDir, ActionCache)> {
    let temp_dir = TempDir::new()?;
    let cache = ActionCache::new(temp_dir.path().join("ac"));
    cache.init().await?;
    Ok((temp_dir, cache))
}

/// Helper to create a simple ActionResult for testing
fn create_test_action_result(recipe: &str, outputs: Vec<OutputFile>) -> ActionResult {
    ActionResult {
        recipe: recipe.to_string(),
        exit_code: 0,
        outputs,
        stdout_digest: BlobHash::from_content(b"stdout"),
        stderr_digest: BlobHash::from_content(b"stderr"),
        execution_metadata: bake::cache::ac::ExecutionMetadata::new(),
    }
}

#[tokio::test]
async fn test_basic_put_get_cycle() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    // PUT: Store a blob
    let data = Bytes::from("Hello, CAS world!");
    let hash = store.put(data.clone()).await?;

    // Verify hash is deterministic
    assert_eq!(hash.to_hex_string().split(':').next().unwrap(), "blake3");
    assert_eq!(hash.hash_hex().len(), 64); // Blake3 produces 32 bytes = 64 hex chars

    // GET: Retrieve the blob
    let retrieved = store.get(&hash).await?;
    assert_eq!(retrieved, data);

    Ok(())
}

#[tokio::test]
async fn test_deduplication() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    let data = Bytes::from("Duplicate content");

    // Store same content twice
    let hash1 = store.put(data.clone()).await?;
    let hash2 = store.put(data.clone()).await?;

    // Should get same hash (content-addressable)
    assert_eq!(hash1, hash2);

    // Should only store once
    let stats = store.stats().await?;
    assert_eq!(stats.blob_count, 1);
    assert_eq!(stats.total_size, data.len() as u64);

    Ok(())
}

#[tokio::test]
async fn test_compression_detection() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    // Compressible data (text)
    let text_data = Bytes::from("a".repeat(10000));
    let text_hash = store.put(text_data.clone()).await?;

    // Already compressed data (PNG signature)
    let png_data = Bytes::from([0x89, 0x50, 0x4E, 0x47].repeat(1000));
    let png_hash = store.put(png_data.clone()).await?;

    // Check storage - both should be retrievable
    let text_retrieved = store.get(&text_hash).await?;
    let png_retrieved = store.get(&png_hash).await?;

    assert_eq!(text_retrieved, text_data);
    assert_eq!(png_retrieved, png_data);

    Ok(())
}

#[tokio::test]
async fn test_many_blobs() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    let mut hashes = Vec::new();
    let blob_count = 100;

    // Store many blobs
    for i in 0..blob_count {
        let data = Bytes::from(format!("Blob {}", i));
        let hash = store.put(data).await?;
        hashes.push(hash);
    }

    // Verify all blobs can be retrieved
    for (i, hash) in hashes.iter().enumerate() {
        let retrieved = store.get(hash).await?;
        assert_eq!(retrieved, Bytes::from(format!("Blob {}", i)));
    }

    // Check stats
    let stats = store.stats().await?;
    assert_eq!(stats.blob_count, blob_count);

    Ok(())
}

#[tokio::test]
async fn test_action_cache_put_get() -> Result<()> {
    let (_temp, cache) = create_test_action_cache().await?;

    // Create output files
    let outputs = vec![OutputFile::new(
        PathBuf::from("output.txt"),
        BlobHash::from_content(b"output content"),
        14,
    )];

    let result = create_test_action_result("test-recipe", outputs);

    // PUT: Store the action result
    let action_key = "test-recipe:abcdef123456";
    cache.put(action_key, &result).await?;

    // GET: Retrieve the action result
    let retrieved = cache.get(action_key).await?;
    assert!(retrieved.is_some());

    let retrieved = retrieved.unwrap();
    assert_eq!(retrieved.exit_code, 0);
    assert_eq!(retrieved.outputs.len(), 1);
    assert_eq!(retrieved.outputs[0].path, PathBuf::from("output.txt"));

    Ok(())
}

#[tokio::test]
async fn test_action_cache_miss() -> Result<()> {
    let (_temp, cache) = create_test_action_cache().await?;

    let result = cache.get("nonexistent-key").await?;
    assert!(result.is_none());

    Ok(())
}

#[tokio::test]
async fn test_full_cache_cycle_with_files() -> Result<()> {
    let (_blob_temp, blob_store) = create_test_store().await?;
    let (_ac_temp, action_cache) = create_test_action_cache().await?;

    // Simulate recipe execution with multiple output files
    let file1_content = Bytes::from("File 1 content");
    let file2_content = Bytes::from("File 2 content with more data");
    let file3_content = Bytes::from("File 3 content");

    // PUT: Store all output files as blobs
    let hash1 = blob_store.put(file1_content.clone()).await?;
    let hash2 = blob_store.put(file2_content.clone()).await?;
    let hash3 = blob_store.put(file3_content.clone()).await?;

    // Create action result referencing these blobs
    let outputs = vec![
        OutputFile::new(
            PathBuf::from("dist/bundle.js"),
            hash1.clone(),
            file1_content.len() as u64,
        ),
        OutputFile::new(
            PathBuf::from("dist/styles.css"),
            hash2.clone(),
            file2_content.len() as u64,
        ),
        OutputFile::new(
            PathBuf::from("dist/index.html"),
            hash3.clone(),
            file3_content.len() as u64,
        ),
    ];

    let result = create_test_action_result("build", outputs);

    // Store action result
    let action_key = "build:v1.0.0:abc123";
    action_cache.put(action_key, &result).await?;

    // GET: Simulate cache hit - retrieve manifest
    let cached_result = action_cache.get(action_key).await?.unwrap();
    assert_eq!(cached_result.outputs.len(), 3);

    // Restore files from blobs
    let bundle = &cached_result.outputs[0];
    let bundle_content = blob_store.get(&bundle.digest).await?;
    assert_eq!(bundle_content, file1_content);

    let styles = &cached_result.outputs[1];
    let styles_content = blob_store.get(&styles.digest).await?;
    assert_eq!(styles_content, file2_content);

    let html = &cached_result.outputs[2];
    let html_content = blob_store.get(&html.digest).await?;
    assert_eq!(html_content, file3_content);

    Ok(())
}

#[tokio::test]
async fn test_chunking_large_file() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    // Create a large file with varied content
    let mut large_data = Vec::new();
    for i in 0..10000 {
        large_data.extend_from_slice(format!("Line {}: Some content here\n", i).as_bytes());
    }
    let large_data = Bytes::from(large_data);

    // Chunk the data
    let cdc = FastCDC::new();
    let chunks = cdc.chunk(&large_data);
    let stats = ChunkStats::from_chunks(&chunks);

    println!(
        "Chunked {} bytes into {} chunks (avg: {:.1} KB)",
        large_data.len(),
        stats.chunk_count,
        stats.avg_chunk_size / 1024.0
    );

    // Store all chunks
    let mut chunk_hashes = Vec::new();
    for chunk in &chunks {
        let hash = store.put(chunk.clone()).await?;
        chunk_hashes.push(hash);
    }

    // Verify we can retrieve and reconstruct
    let mut reconstructed = Vec::new();
    for hash in &chunk_hashes {
        let chunk = store.get(hash).await?;
        reconstructed.extend_from_slice(&chunk);
    }

    assert_eq!(Bytes::from(reconstructed), large_data);
    assert!(stats.chunk_count > 5, "Should create multiple chunks");

    Ok(())
}

/// Verifies that a LayeredBlobStore reads missing blobs from a downstream (remote) tier
/// and promotes them into the local (first) tier after a successful get.
///
/// # Examples
///
/// ```
/// # async fn run_example() -> anyhow::Result<()> {
/// use std::sync::Arc;
/// // create_test_store() returns (TempDir, LocalBlobStore)
/// let (_t1, local) = create_test_store().await?;
/// let (_t2, remote) = create_test_store().await?;
/// let data = bytes::Bytes::from("remote data");
/// let hash = remote.put(data.clone()).await?;
///
/// let local_arc: Arc<dyn BlobStore> = Arc::new(local);
/// let remote_arc: Arc<dyn BlobStore> = Arc::new(remote);
/// let layered = LayeredBlobStore::new(vec![Arc::clone(&local_arc), remote_arc])?;
///
/// // get should retrieve from remote and write into local
/// let fetched = layered.get(&hash).await?;
/// assert_eq!(fetched, data);
/// let promoted = local_arc.get(&hash).await?;
/// assert_eq!(promoted, data);
/// # Ok(()) }
/// ```
#[tokio::test]
async fn test_layered_store_promotion() -> Result<()> {
    // Create two local stores to simulate local + remote
    let (_temp1, local_store) = create_test_store().await?;
    let (_temp2, remote_store) = create_test_store().await?;

    // Store blob only in remote
    let data = Bytes::from("Remote-only data");
    let hash = remote_store.put(data.clone()).await?;

    // Verify not in local
    assert!(!local_store.contains(&hash).await?);

    // Create wrapped stores - wrap in Arc
    use std::sync::Arc;
    let local_store_arc: Arc<dyn BlobStore> = Arc::new(local_store);
    let remote_store_arc: Arc<dyn BlobStore> = Arc::new(remote_store);

    let layered = LayeredBlobStore::new(vec![Arc::clone(&local_store_arc), remote_store_arc])?;

    // GET from layered should find it in remote
    let retrieved = layered.get(&hash).await?;
    assert_eq!(retrieved, data);

    // After GET, should be promoted to local
    let now_in_local = local_store_arc.get(&hash).await?;
    assert_eq!(now_in_local, data);

    Ok(())
}

/// Verifies that a layered blob store writes new blobs to every tier.
///
/// # Examples
///
/// ```
/// # use bytes::Bytes;
/// # use std::sync::Arc;
/// # use anyhow::Result;
/// # async fn run() -> Result<()> {
/// let (_t1, local) = create_test_store().await?;
/// let (_t2, remote) = create_test_store().await?;
/// let local_arc: Arc<dyn BlobStore> = Arc::new(local);
/// let remote_arc: Arc<dyn BlobStore> = Arc::new(remote);
///
/// // LayeredBlobStore::with_options constructs a layered store that writes to all tiers.
/// let layered = LayeredBlobStore::with_options(vec![Arc::clone(&local_arc), Arc::clone(&remote_arc)], true)?;
///
/// let data = Bytes::from("Write-through data");
/// let hash = layered.put(data).await?;
///
/// assert!(local_arc.contains(&hash).await?);
/// assert!(remote_arc.contains(&hash).await?);
/// # Ok(())
/// # }
/// ```
#[tokio::test]
async fn test_layered_store_writes_to_all_tiers() -> Result<()> {
    let (_temp1, local_store) = create_test_store().await?;
    let (_temp2, remote_store) = create_test_store().await?;

    use std::sync::Arc;
    let local_store_arc: Arc<dyn BlobStore> = Arc::new(local_store);
    let remote_store_arc: Arc<dyn BlobStore> = Arc::new(remote_store);

    // Layered store always writes to all tiers
    let layered = LayeredBlobStore::with_options(
        vec![Arc::clone(&local_store_arc), Arc::clone(&remote_store_arc)],
        true, // auto_promote
    )?;

    // PUT to layered should write to all stores
    let data = Bytes::from("Write-through data");
    let hash = layered.put(data).await?;

    // Verify in both stores
    assert!(local_store_arc.contains(&hash).await?);
    assert!(remote_store_arc.contains(&hash).await?);

    Ok(())
}

#[tokio::test]
async fn test_manifest_signing() -> Result<()> {
    let signer = ManifestSigner::new(b"test-secret-key-for-integration");

    // Create a manifest
    let outputs = vec![OutputFile::new(
        PathBuf::from("output.txt"),
        BlobHash::from_content(b"content"),
        7,
    )];

    let result = create_test_action_result("test", outputs);

    // Sign the manifest
    let signature = signer.sign_json(&result)?;
    assert!(!signature.signature.is_empty());
    assert_eq!(signature.version, 1);

    // Verify signature
    signer.verify_json(&result, &signature)?;

    // Modified manifest should fail verification
    let mut modified_result = result.clone();
    modified_result.exit_code = 1;

    let verify_result = signer.verify_json(&modified_result, &signature);
    assert!(verify_result.is_err());

    Ok(())
}

#[tokio::test]
async fn test_executable_preservation() -> Result<()> {
    let (_temp, cache) = create_test_action_cache().await?;

    // Executable file
    let outputs = vec![
        OutputFile::new(
            PathBuf::from("bin/script.sh"),
            BlobHash::from_content(b"#!/bin/bash"),
            11,
        )
        .with_executable(true),
    ];

    let result = create_test_action_result("test-executable", outputs);

    cache.put("test-executable", &result).await?;

    let retrieved = cache.get("test-executable").await?.unwrap();

    // Verify executable flag preserved
    assert_eq!(retrieved.outputs.len(), 1);
    assert!(retrieved.outputs[0].is_executable);

    Ok(())
}

#[tokio::test]
async fn test_cache_hit_after_identical_rebuild() -> Result<()> {
    let (_blob_temp, blob_store) = create_test_store().await?;
    let (_ac_temp, action_cache) = create_test_action_cache().await?;

    let source_code = Bytes::from("const x = 42;");
    let compiled_output = Bytes::from("compiled output");

    // First build
    let output_hash = blob_store.put(compiled_output.clone()).await?;
    let outputs = vec![OutputFile::new(
        PathBuf::from("dist/main.js"),
        output_hash.clone(),
        compiled_output.len() as u64,
    )];

    let result = create_test_action_result("compile", outputs);

    // Cache key should be based on source code hash + recipe command
    let source_hash = BlobHash::from_content(&source_code);
    let cache_key = format!("compile:{}", source_hash.hash_hex());

    action_cache.put(&cache_key, &result).await?;

    // Second build with identical source
    let second_source_hash = BlobHash::from_content(&source_code);
    let second_cache_key = format!("compile:{}", second_source_hash.hash_hex());

    // Should get cache hit
    assert_eq!(cache_key, second_cache_key);
    let cached = action_cache.get(&cache_key).await?;
    assert!(cached.is_some());

    let cached = cached.unwrap();
    assert_eq!(cached.outputs.len(), 1);
    assert_eq!(cached.outputs[0].digest, output_hash);

    Ok(())
}

#[tokio::test]
async fn test_partial_output_changes() -> Result<()> {
    let (_blob_temp, blob_store) = create_test_store().await?;

    // Simulate a build that produces multiple outputs
    let file1_v1 = Bytes::from("version 1 of file 1");
    let file2_v1 = Bytes::from("version 1 of file 2");
    let file3_v1 = Bytes::from("version 1 of file 3");

    // Store v1
    let hash1_v1 = blob_store.put(file1_v1.clone()).await?;
    let hash2_v1 = blob_store.put(file2_v1.clone()).await?;
    let hash3_v1 = blob_store.put(file3_v1.clone()).await?;

    // Simulate change to only file 2
    let file1_v2 = file1_v1.clone(); // Unchanged
    let file2_v2 = Bytes::from("version 2 of file 2 - CHANGED");
    let file3_v2 = file3_v1.clone(); // Unchanged

    // Store v2
    let hash1_v2 = blob_store.put(file1_v2).await?;
    let hash2_v2 = blob_store.put(file2_v2).await?;
    let hash3_v2 = blob_store.put(file3_v2).await?;

    // Verify deduplication: unchanged files have same hash
    assert_eq!(hash1_v1, hash1_v2);
    assert_eq!(hash3_v1, hash3_v2);
    assert_ne!(hash2_v1, hash2_v2); // Changed file has different hash

    // Verify we only stored 4 unique blobs (3 from v1 + 1 new from v2)
    let stats = blob_store.stats().await?;
    assert_eq!(stats.blob_count, 4);

    Ok(())
}

#[tokio::test]
async fn test_concurrent_operations() -> Result<()> {
    let (_temp, store) = create_test_store().await?;

    // Store multiple blobs concurrently - wrap store in Arc
    use std::sync::Arc;
    let store = Arc::new(store);

    let mut handles = vec![];
    for i in 0..10 {
        let store = Arc::clone(&store);
        let handle = tokio::spawn(async move {
            let data = Bytes::from(format!("Concurrent blob {}", i));
            store.put(data).await
        });
        handles.push(handle);
    }

    // Wait for all to complete
    let mut hashes = Vec::new();
    for handle in handles {
        let hash = handle.await??;
        hashes.push(hash);
    }

    // Verify all hashes are unique
    assert_eq!(hashes.len(), 10);
    let unique_hashes: std::collections::HashSet<_> = hashes.iter().collect();
    assert_eq!(unique_hashes.len(), 10);

    // Verify all can be retrieved
    for (i, hash) in hashes.iter().enumerate() {
        let data = store.get(hash).await?;
        assert_eq!(data, Bytes::from(format!("Concurrent blob {}", i)));
    }

    Ok(())
}

#[tokio::test]
async fn test_empty_action_result() -> Result<()> {
    let (_temp, cache) = create_test_action_cache().await?;

    // Recipe that produces no outputs (e.g., linting)
    let result = create_test_action_result("lint:check", vec![]);

    cache.put("lint:check", &result).await?;

    let retrieved = cache.get("lint:check").await?;
    assert!(retrieved.is_some());
    assert_eq!(retrieved.unwrap().outputs.len(), 0);

    Ok(())
}
