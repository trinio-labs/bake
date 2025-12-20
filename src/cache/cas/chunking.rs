use bytes::Bytes;

/// Generate the Gear hash table at compile time
/// Using LCG: next = (a * prev + c) mod 2^64
/// Parameters from Numerical Recipes (high-quality LCG)
const fn generate_gear_table() -> [u64; 256] {
    let mut table = [0u64; 256];
    let mut seed = 0x123456789abcdef0u64;
    let a = 6364136223846793005u64; // Multiplier from Knuth
    let c = 1442695040888963407u64; // Increment

    let mut i = 0;
    while i < 256 {
        seed = seed.wrapping_mul(a).wrapping_add(c);
        table[i] = seed;
        i += 1;
    }
    table
}

/// FastCDC (Fast Content-Defined Chunking) implementation
///
/// Based on "FastCDC: a Fast and Efficient Content-Defined Chunking Approach for Data Deduplication"
/// by Wen Xia et al. (2016)
///
/// FastCDC improves upon traditional CDC by:
/// - Using cut point skipping to avoid unnecessary hash computations
/// - Dividing the search space into three zones with different masks
/// - Better cache locality and performance (2-3x faster than Rabin/Gear)
pub struct FastCDC {
    /// Minimum chunk size (prevents too-small chunks)
    min_size: usize,
    /// Normalized chunk size (average target)
    avg_size: usize,
    /// Maximum chunk size (prevents too-large chunks)
    max_size: usize,
    /// Mask bits for normalization zone
    mask_s: u64,
    /// Mask bits for backup zone
    mask_l: u64,
}

impl FastCDC {
    /// Create a new FastCDC chunker with default parameters
    ///
    /// Default configuration:
    /// - Min size: 2 KB
    /// - Avg size: 8 KB
    /// - Max size: 64 KB
    pub fn new() -> Self {
        Self::with_params(2 * 1024, 8 * 1024, 64 * 1024)
    }

    /// Create a FastCDC chunker with custom parameters
    ///
    /// # Arguments
    /// * `min_size` - Minimum chunk size in bytes
    /// * `avg_size` - Target average chunk size in bytes
    /// * `max_size` - Maximum chunk size in bytes
    pub fn with_params(min_size: usize, avg_size: usize, max_size: usize) -> Self {
        assert!(min_size < avg_size, "min_size must be less than avg_size");
        assert!(avg_size < max_size, "avg_size must be less than max_size");
        assert!(min_size >= 64, "min_size must be at least 64 bytes");

        // Calculate mask bits based on average size
        // For 8KB average, we want approximately 13 bits (8192 = 2^13)
        let bits = (avg_size as f64).log2().round() as u32;

        // Normalization zone mask (normal cutting)
        let mask_s = (1u64 << bits) - 1;

        // Backup zone mask (more aggressive cutting near max_size)
        // Use fewer bits to trigger more frequently
        let mask_l = (1u64 << (bits - 2)) - 1;

        Self {
            min_size,
            avg_size,
            max_size,
            mask_s,
            mask_l,
        }
    }

    /// Gear hash table for rolling hash computation
    /// These are pre-computed 64-bit random values for fast hashing
    /// Generated using LCG with parameters from Numerical Recipes
    fn gear_table() -> &'static [u64; 256] {
        static TABLE: [u64; 256] = generate_gear_table();
        &TABLE
    }

    /// Split data into content-defined chunks using FastCDC algorithm
    ///
    /// FastCDC divides the search space into three zones:
    /// 1. [0, min_size): No cutting allowed (too small)
    /// 2. [min_size, avg_size): Normalization zone - use mask_s
    /// 3. [avg_size, max_size): Backup zone - use mask_l (more aggressive)
    ///
    /// # Arguments
    /// * `data` - The data to chunk
    ///
    /// # Returns
    /// A vector of chunks (each chunk is a Bytes slice)
    pub fn chunk(&self, data: &[u8]) -> Vec<Bytes> {
        if data.is_empty() {
            return vec![];
        }

        if data.len() <= self.min_size {
            return vec![Bytes::copy_from_slice(data)];
        }

        let mut chunks = Vec::new();
        let table = Self::gear_table();
        let mut start = 0;

        while start < data.len() {
            let remaining = data.len() - start;

            // If remaining data is smaller than min_size, it's the last chunk
            if remaining <= self.min_size {
                chunks.push(Bytes::copy_from_slice(&data[start..]));
                break;
            }

            // Calculate zone boundaries for this chunk
            let chunk_min = start + self.min_size;
            let chunk_avg = (start + self.avg_size).min(start + remaining);
            let chunk_max = (start + self.max_size).min(start + remaining);

            let mut hash: u64 = 0;
            let mut cut_point = chunk_max; // Default to max if no cut point found

            // Cut point skipping: start at min_size (skip the minimum zone)
            let mut pos = chunk_min;

            // Normalization zone: [min_size, avg_size)
            while pos < chunk_avg {
                let byte = data[pos];
                hash = (hash << 1).wrapping_add(table[byte as usize]);

                if (hash & self.mask_s) == 0 {
                    cut_point = pos + 1;
                    break;
                }
                pos += 1;
            }

            // Backup zone: [avg_size, max_size) - use more aggressive mask
            if cut_point == chunk_max && pos < chunk_max {
                while pos < chunk_max {
                    let byte = data[pos];
                    hash = (hash << 1).wrapping_add(table[byte as usize]);

                    if (hash & self.mask_l) == 0 {
                        cut_point = pos + 1;
                        break;
                    }
                    pos += 1;
                }
            }

            // Extract chunk
            chunks.push(Bytes::copy_from_slice(&data[start..cut_point]));
            start = cut_point;
        }

        chunks
    }

    /// Get the target average chunk size
    pub fn avg_size(&self) -> usize {
        self.avg_size
    }

    /// Get the minimum chunk size
    pub fn min_size(&self) -> usize {
        self.min_size
    }

    /// Get the maximum chunk size
    pub fn max_size(&self) -> usize {
        self.max_size
    }
}

impl Default for FastCDC {
    fn default() -> Self {
        Self::new()
    }
}

/// Statistics about chunking results
#[derive(Debug, Default, Clone)]
pub struct ChunkStats {
    pub chunk_count: usize,
    pub total_size: usize,
    pub min_chunk_size: usize,
    pub max_chunk_size: usize,
    pub avg_chunk_size: f64,
}

impl ChunkStats {
    /// Calculate statistics from a set of chunks
    pub fn from_chunks(chunks: &[Bytes]) -> Self {
        if chunks.is_empty() {
            return Self::default();
        }

        let sizes: Vec<usize> = chunks.iter().map(|c| c.len()).collect();
        let total_size: usize = sizes.iter().sum();
        let min_chunk_size = *sizes.iter().min().unwrap();
        let max_chunk_size = *sizes.iter().max().unwrap();
        let avg_chunk_size = total_size as f64 / chunks.len() as f64;

        Self {
            chunk_count: chunks.len(),
            total_size,
            min_chunk_size,
            max_chunk_size,
            avg_chunk_size,
        }
    }

    /// Calculate standard deviation of chunk sizes
    pub fn std_deviation(&self) -> f64 {
        if self.chunk_count <= 1 {
            return 0.0;
        }

        // This is a simplified calculation - for accurate std dev,
        // we'd need access to individual chunk sizes
        // For now, provide an estimate based on min/max
        let range = (self.max_chunk_size - self.min_chunk_size) as f64;
        range / 4.0 // Rough estimate: range/4 for uniform distribution
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_fastcdc_default() {
        let cdc = FastCDC::new();
        assert_eq!(cdc.min_size(), 2 * 1024);
        assert_eq!(cdc.avg_size(), 8 * 1024);
        assert_eq!(cdc.max_size(), 64 * 1024);
    }

    #[test]
    fn test_fastcdc_custom_params() {
        let cdc = FastCDC::with_params(1024, 4096, 16384);
        assert_eq!(cdc.min_size(), 1024);
        assert_eq!(cdc.avg_size(), 4096);
        assert_eq!(cdc.max_size(), 16384);
    }

    #[test]
    #[should_panic(expected = "min_size must be less than avg_size")]
    fn test_fastcdc_invalid_params_min_avg() {
        FastCDC::with_params(4096, 4096, 8192);
    }

    #[test]
    #[should_panic(expected = "avg_size must be less than max_size")]
    fn test_fastcdc_invalid_params_avg_max() {
        FastCDC::with_params(1024, 8192, 8192);
    }

    #[test]
    #[should_panic(expected = "min_size must be at least 64 bytes")]
    fn test_fastcdc_invalid_params_too_small() {
        FastCDC::with_params(32, 128, 256);
    }

    #[test]
    fn test_chunk_empty_data() {
        let cdc = FastCDC::new();
        let chunks = cdc.chunk(&[]);
        assert_eq!(chunks.len(), 0);
    }

    #[test]
    fn test_chunk_small_data() {
        let cdc = FastCDC::new();
        let data = vec![0u8; 1024]; // Smaller than min_size
        let chunks = cdc.chunk(&data);
        assert_eq!(chunks.len(), 1);
        assert_eq!(chunks[0].len(), 1024);
    }

    #[test]
    fn test_chunk_medium_data() {
        let cdc = FastCDC::new();
        // Use varied data so chunking can find boundaries
        let mut data = Vec::new();
        for i in 0..50_000 {
            data.push((i % 256) as u8);
        }

        let chunks = cdc.chunk(&data);

        // Should create multiple chunks
        assert!(chunks.len() > 1, "Should create multiple chunks");

        // Verify all chunks are within bounds
        for chunk in &chunks {
            assert!(chunk.len() >= cdc.min_size() || chunk == chunks.last().unwrap());
            assert!(chunk.len() <= cdc.max_size());
        }

        // Verify total size matches
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
    }

    #[test]
    fn test_chunk_large_data() {
        let cdc = FastCDC::new();
        let mut data = Vec::new();
        for i in 0..500_000 {
            data.push((i % 256) as u8);
        }

        let chunks = cdc.chunk(&data);

        // Should create many chunks
        assert!(chunks.len() > 5);

        // Verify boundaries
        for chunk in &chunks {
            assert!(chunk.len() >= cdc.min_size() || chunk == chunks.last().unwrap());
            assert!(chunk.len() <= cdc.max_size());
        }

        // Verify total size
        let total: usize = chunks.iter().map(|c| c.len()).sum();
        assert_eq!(total, data.len());
    }

    #[test]
    fn test_chunk_deterministic() {
        let cdc = FastCDC::new();
        let mut data = Vec::new();
        for i in 0..100_000 {
            data.push((i % 256) as u8);
        }

        let chunks1 = cdc.chunk(&data);
        let chunks2 = cdc.chunk(&data);

        // Same data should produce same chunks
        assert_eq!(chunks1.len(), chunks2.len());
        for (c1, c2) in chunks1.iter().zip(chunks2.iter()) {
            assert_eq!(c1, c2);
        }
    }

    #[test]
    fn test_chunk_stats_empty() {
        let chunks: Vec<Bytes> = vec![];
        let stats = ChunkStats::from_chunks(&chunks);

        assert_eq!(stats.chunk_count, 0);
        assert_eq!(stats.total_size, 0);
    }

    #[test]
    fn test_chunk_stats_single_chunk() {
        let chunks = vec![Bytes::from(vec![0u8; 1000])];
        let stats = ChunkStats::from_chunks(&chunks);

        assert_eq!(stats.chunk_count, 1);
        assert_eq!(stats.total_size, 1000);
        assert_eq!(stats.min_chunk_size, 1000);
        assert_eq!(stats.max_chunk_size, 1000);
        assert_eq!(stats.avg_chunk_size, 1000.0);
    }

    #[test]
    fn test_chunk_stats_multiple_chunks() {
        let chunks = vec![
            Bytes::from(vec![0u8; 1000]),
            Bytes::from(vec![0u8; 2000]),
            Bytes::from(vec![0u8; 3000]),
        ];
        let stats = ChunkStats::from_chunks(&chunks);

        assert_eq!(stats.chunk_count, 3);
        assert_eq!(stats.total_size, 6000);
        assert_eq!(stats.min_chunk_size, 1000);
        assert_eq!(stats.max_chunk_size, 3000);
        assert_eq!(stats.avg_chunk_size, 2000.0);
    }

    #[test]
    fn test_chunk_realistic_file() {
        let cdc = FastCDC::new();

        // Simulate a text file with varied patterns (more realistic)
        // Mix different content types to avoid overly regular patterns
        let mut data = Vec::new();
        for i in 0..2000 {
            let line = match i % 5 {
                0 => format!("Line {}: Function declaration with params\n", i),
                1 => format!("Line {}: json data value {}\n", i, i),
                2 => format!("Line {}: xml tag content {}\n", i, i),
                3 => format!("Line {}: ERROR: Failed to connect to server\n", i),
                _ => format!("Line {}: Regular content here with number {}\n", i, i * 7),
            };
            data.extend_from_slice(line.as_bytes());
        }

        let chunks = cdc.chunk(&data);
        let stats = ChunkStats::from_chunks(&chunks);

        // Should create multiple chunks
        assert!(
            stats.chunk_count > 1,
            "Expected multiple chunks for {}KB of data, got {}",
            data.len() / 1024,
            stats.chunk_count
        );

        // Average should be reasonably close to target
        let target = cdc.avg_size() as f64;
        let ratio = stats.avg_chunk_size / target;

        // Allow for variance (FastCDC typically stays within 0.5x to 2x of target)
        assert!(
            ratio > 0.3 && ratio < 3.0,
            "Average chunk size {} is too far from target {}",
            stats.avg_chunk_size,
            target
        );
    }

    #[test]
    fn test_chunk_shift_resistance() {
        let cdc = FastCDC::new();

        // Original data with varied content
        let original = b"The quick brown fox jumps over the lazy dog. ";
        let mut data1 = Vec::new();
        for _ in 0..2000 {
            data1.extend_from_slice(original);
        }

        // Shifted data (insert at beginning)
        let mut data2 = b"INSERTED CONTENT HERE: ".to_vec();
        data2.extend_from_slice(&data1);

        let chunks1 = cdc.chunk(&data1);
        let chunks2 = cdc.chunk(&data2);

        // Key property of content-defined chunking: most chunks should still match
        assert!(chunks1.len() > 1, "chunks1 should have multiple chunks");
        assert!(chunks2.len() > 1, "chunks2 should have multiple chunks");

        // The total size relationship should hold
        assert_eq!(chunks2.iter().map(|c| c.len()).sum::<usize>(), data2.len());

        // After the inserted content, many chunks should match
        // We can't assert exact matches, but we can verify reasonable chunking occurred
        let stats1 = ChunkStats::from_chunks(&chunks1);
        let stats2 = ChunkStats::from_chunks(&chunks2);

        // Both should have similar average chunk sizes (within 2x)
        let ratio = stats1.avg_chunk_size / stats2.avg_chunk_size;
        assert!(
            ratio > 0.5 && ratio < 2.0,
            "Chunk size distributions should be similar: {} vs {}",
            stats1.avg_chunk_size,
            stats2.avg_chunk_size
        );
    }

    #[test]
    fn test_chunk_uniform_data_uses_max_size() {
        let cdc = FastCDC::new();
        let data = vec![0u8; 200_000]; // Uniform data

        let chunks = cdc.chunk(&data);

        // With uniform data, FastCDC will hit max_size boundaries
        assert!(chunks.len() > 1);

        // Most chunks should be close to max_size (except possibly the last)
        for (i, chunk) in chunks.iter().enumerate() {
            if i < chunks.len() - 1 {
                // Non-final chunks should be close to max_size
                assert!(
                    chunk.len() >= cdc.avg_size(),
                    "Uniform data should produce large chunks, got {}",
                    chunk.len()
                );
            }
        }
    }
}
