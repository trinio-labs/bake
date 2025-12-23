use super::blob_hash::BlobHash;
use anyhow::Result;
use log::debug;
use rusqlite::{Connection, OptionalExtension, params};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex};

/// SQLite-based index for fast blob lookups (O(1) without filesystem I/O)
pub struct BlobIndex {
    conn: Arc<Mutex<Connection>>,
    db_path: PathBuf,
}

impl BlobIndex {
    /// Create or open a blob index database
    pub fn open<P: AsRef<Path>>(path: P) -> Result<Self> {
        let db_path = path.as_ref().to_path_buf();

        // Ensure parent directory exists
        if let Some(parent) = db_path.parent() {
            std::fs::create_dir_all(parent)?;
        }

        let conn = Connection::open(&db_path)?;

        // Enable WAL mode for better concurrency
        conn.pragma_update(None, "journal_mode", "WAL")?;
        conn.pragma_update(None, "synchronous", "NORMAL")?;

        // Create schema
        conn.execute(
            "CREATE TABLE IF NOT EXISTS blobs (
                digest TEXT PRIMARY KEY,
                algorithm TEXT NOT NULL,
                size INTEGER NOT NULL,
                stored_at INTEGER NOT NULL,
                access_count INTEGER DEFAULT 1,
                last_accessed INTEGER NOT NULL,
                compression_format INTEGER DEFAULT 0
            )",
            [],
        )?;

        // Add compression_format column for existing databases (migration)
        let _ = conn.execute(
            "ALTER TABLE blobs ADD COLUMN compression_format INTEGER DEFAULT 0",
            [],
        );

        // Create indexes for efficient queries
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_last_accessed ON blobs(last_accessed)",
            [],
        )?;
        conn.execute("CREATE INDEX IF NOT EXISTS idx_size ON blobs(size)", [])?;
        conn.execute(
            "CREATE INDEX IF NOT EXISTS idx_algorithm ON blobs(algorithm)",
            [],
        )?;

        debug!("Opened blob index at: {}", db_path.display());

        Ok(Self {
            conn: Arc::new(Mutex::new(conn)),
            db_path,
        })
    }

    /// Add a blob to the index
    pub fn insert(&self, hash: &BlobHash, size: u64) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "INSERT OR REPLACE INTO blobs (digest, algorithm, size, stored_at, access_count, last_accessed)
             VALUES (?1, ?2, ?3, ?4, 1, ?5)",
            params![
                hash.to_hex_string(),
                hash.algorithm.to_string(),
                size as i64,
                now,
                now
            ],
        )?;

        Ok(())
    }

    /// Check if a blob exists in the index
    pub fn contains(&self, hash: &BlobHash) -> Result<bool> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT 1 FROM blobs WHERE digest = ?1")?;

        let exists = stmt.exists(params![hash.to_hex_string()])?;

        Ok(exists)
    }

    /// Get blob size from the index
    pub fn get_size(&self, hash: &BlobHash) -> Result<Option<u64>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare_cached("SELECT size FROM blobs WHERE digest = ?1")?;

        let result: Option<i64> = stmt
            .query_row(params![hash.to_hex_string()], |row| row.get(0))
            .optional()?;

        Ok(result.map(|s| s as u64))
    }

    /// Update access time for a blob (for LRU tracking)
    pub fn touch(&self, hash: &BlobHash) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)?
            .as_secs() as i64;

        conn.execute(
            "UPDATE blobs SET last_accessed = ?1, access_count = access_count + 1 WHERE digest = ?2",
            params![now, hash.to_hex_string()],
        )?;

        Ok(())
    }

    /// Remove a blob from the index
    pub fn remove(&self, hash: &BlobHash) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute(
            "DELETE FROM blobs WHERE digest = ?1",
            params![hash.to_hex_string()],
        )?;
        Ok(())
    }

    /// List all blob hashes in the index
    pub fn list_all(&self) -> Result<Vec<BlobHash>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare("SELECT digest FROM blobs")?;

        let rows = stmt.query_map([], |row| {
            let digest: String = row.get(0)?;
            Ok(digest)
        })?;

        let mut hashes = Vec::new();
        for row in rows {
            let digest = row?;
            if let Ok(hash) = BlobHash::from_hex_string(&digest) {
                hashes.push(hash);
            }
        }

        Ok(hashes)
    }

    /// Get index statistics
    pub fn stats(&self) -> Result<IndexStats> {
        let conn = self.conn.lock().unwrap();

        let mut stmt = conn.prepare("SELECT COUNT(*), COALESCE(SUM(size), 0) FROM blobs")?;
        let (count, total_size): (i64, i64) =
            stmt.query_row([], |row| Ok((row.get(0)?, row.get(1)?)))?;

        Ok(IndexStats {
            blob_count: count as u64,
            total_size: total_size as u64,
        })
    }

    /// Get LRU candidates for eviction
    pub fn get_lru_candidates(&self, count: usize, offset: usize) -> Result<Vec<(BlobHash, u64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt = conn.prepare(
            "SELECT digest, size FROM blobs ORDER BY last_accessed ASC LIMIT ?1 OFFSET ?2",
        )?;

        let rows = stmt.query_map(params![count as i64, offset as i64], |row| {
            let digest: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            Ok((digest, size as u64))
        })?;

        let mut candidates = Vec::new();
        for row in rows {
            let (digest, size) = row?;
            if let Ok(hash) = BlobHash::from_hex_string(&digest) {
                candidates.push((hash, size));
            }
        }

        Ok(candidates)
    }

    /// Get largest blobs for eviction
    pub fn get_largest_blobs(&self, count: usize) -> Result<Vec<(BlobHash, u64)>> {
        let conn = self.conn.lock().unwrap();
        let mut stmt =
            conn.prepare("SELECT digest, size FROM blobs ORDER BY size DESC LIMIT ?1")?;

        let rows = stmt.query_map(params![count as i64], |row| {
            let digest: String = row.get(0)?;
            let size: i64 = row.get(1)?;
            Ok((digest, size as u64))
        })?;

        let mut blobs = Vec::new();
        for row in rows {
            let (digest, size) = row?;
            if let Ok(hash) = BlobHash::from_hex_string(&digest) {
                blobs.push((hash, size));
            }
        }

        Ok(blobs)
    }

    /// Batch check if multiple blobs exist
    pub fn contains_many(&self, hashes: &[BlobHash]) -> Result<Vec<bool>> {
        let conn = self.conn.lock().unwrap();

        let mut results = Vec::with_capacity(hashes.len());
        let mut stmt = conn.prepare_cached("SELECT 1 FROM blobs WHERE digest = ?1")?;

        for hash in hashes {
            let exists = stmt.exists(params![hash.to_hex_string()])?;
            results.push(exists);
        }

        Ok(results)
    }

    /// Vacuum the database to reclaim space
    pub fn vacuum(&self) -> Result<()> {
        let conn = self.conn.lock().unwrap();
        conn.execute("VACUUM", [])?;
        Ok(())
    }

    /// Get the path to the index database
    pub fn path(&self) -> &Path {
        &self.db_path
    }
}

/// Index statistics
#[derive(Debug, Default)]
pub struct IndexStats {
    pub blob_count: u64,
    pub total_size: u64,
}

impl IndexStats {
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
}

#[cfg(test)]
mod tests {
    use super::*;
    use tempfile::TempDir;

    fn create_test_index() -> (BlobIndex, TempDir) {
        let temp_dir = TempDir::new().unwrap();
        let index_path = temp_dir.path().join("index.db");
        let index = BlobIndex::open(&index_path).unwrap();
        (index, temp_dir)
    }

    #[test]
    fn test_insert_and_contains() {
        let (index, _temp) = create_test_index();
        let hash = BlobHash::from_content(b"test data");

        index.insert(&hash, 100).unwrap();
        assert!(index.contains(&hash).unwrap());

        let fake_hash = BlobHash::from_content(b"nonexistent");
        assert!(!index.contains(&fake_hash).unwrap());
    }

    #[test]
    fn test_get_size() {
        let (index, _temp) = create_test_index();
        let hash = BlobHash::from_content(b"test");

        index.insert(&hash, 12345).unwrap();

        let size = index.get_size(&hash).unwrap();
        assert_eq!(size, Some(12345));

        let fake_hash = BlobHash::from_content(b"nonexistent");
        assert_eq!(index.get_size(&fake_hash).unwrap(), None);
    }

    #[test]
    fn test_touch() {
        let (index, _temp) = create_test_index();
        let hash = BlobHash::from_content(b"test");

        index.insert(&hash, 100).unwrap();

        // Sleep a bit to ensure time difference
        std::thread::sleep(std::time::Duration::from_millis(10));

        index.touch(&hash).unwrap();

        // Verify the blob still exists
        assert!(index.contains(&hash).unwrap());
    }

    #[test]
    fn test_remove() {
        let (index, _temp) = create_test_index();
        let hash = BlobHash::from_content(b"test");

        index.insert(&hash, 100).unwrap();
        assert!(index.contains(&hash).unwrap());

        index.remove(&hash).unwrap();
        assert!(!index.contains(&hash).unwrap());
    }

    #[test]
    fn test_list_all() {
        let (index, _temp) = create_test_index();

        let hash1 = BlobHash::from_content(b"blob1");
        let hash2 = BlobHash::from_content(b"blob2");
        let hash3 = BlobHash::from_content(b"blob3");

        index.insert(&hash1, 100).unwrap();
        index.insert(&hash2, 200).unwrap();
        index.insert(&hash3, 300).unwrap();

        let all_hashes = index.list_all().unwrap();
        assert_eq!(all_hashes.len(), 3);
    }

    #[test]
    fn test_stats() {
        let (index, _temp) = create_test_index();

        let stats = index.stats().unwrap();
        assert_eq!(stats.blob_count, 0);
        assert_eq!(stats.total_size, 0);

        index
            .insert(&BlobHash::from_content(b"blob1"), 100)
            .unwrap();
        index
            .insert(&BlobHash::from_content(b"blob2"), 200)
            .unwrap();

        let stats = index.stats().unwrap();
        assert_eq!(stats.blob_count, 2);
        assert_eq!(stats.total_size, 300);
    }

    #[test]
    fn test_get_lru_candidates() {
        let (index, _temp) = create_test_index();

        let hash1 = BlobHash::from_content(b"old");
        let hash2 = BlobHash::from_content(b"middle");
        let hash3 = BlobHash::from_content(b"new");

        index.insert(&hash1, 100).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        index.insert(&hash2, 200).unwrap();
        std::thread::sleep(std::time::Duration::from_millis(10));
        index.insert(&hash3, 300).unwrap();

        let lru = index.get_lru_candidates(2, 0).unwrap();
        assert_eq!(lru.len(), 2);

        // First entry should be the oldest
        assert_eq!(lru[0].0, hash1);
    }

    #[test]
    fn test_get_largest_blobs() {
        let (index, _temp) = create_test_index();

        index
            .insert(&BlobHash::from_content(b"small"), 100)
            .unwrap();
        index
            .insert(&BlobHash::from_content(b"medium"), 500)
            .unwrap();
        index
            .insert(&BlobHash::from_content(b"large"), 1000)
            .unwrap();

        let largest = index.get_largest_blobs(2).unwrap();
        assert_eq!(largest.len(), 2);

        // First entry should be the largest
        assert_eq!(largest[0].1, 1000);
        assert_eq!(largest[1].1, 500);
    }

    #[test]
    fn test_contains_many() {
        let (index, _temp) = create_test_index();

        let hash1 = BlobHash::from_content(b"blob1");
        let hash2 = BlobHash::from_content(b"blob2");
        let hash3 = BlobHash::from_content(b"nonexistent");

        index.insert(&hash1, 100).unwrap();
        index.insert(&hash2, 200).unwrap();

        let results = index.contains_many(&[hash1, hash2, hash3]).unwrap();
        assert_eq!(results, vec![true, true, false]);
    }

    #[test]
    fn test_vacuum() {
        let (index, _temp) = create_test_index();

        // Add and remove some blobs
        for i in 0..10 {
            let hash = BlobHash::from_content(&format!("blob{}", i).into_bytes());
            index.insert(&hash, i * 100).unwrap();
        }

        for i in 0..5 {
            let hash = BlobHash::from_content(&format!("blob{}", i).into_bytes());
            index.remove(&hash).unwrap();
        }

        // Vacuum should work without error
        index.vacuum().unwrap();

        let stats = index.stats().unwrap();
        assert_eq!(stats.blob_count, 5);
    }

    #[test]
    fn test_insert_duplicate() {
        let (index, _temp) = create_test_index();
        let hash = BlobHash::from_content(b"test");

        index.insert(&hash, 100).unwrap();
        index.insert(&hash, 200).unwrap(); // Should replace

        let size = index.get_size(&hash).unwrap();
        assert_eq!(size, Some(200));
    }
}
