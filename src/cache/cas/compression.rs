use anyhow::Result;
use bytes::Bytes;
use std::io::{Cursor, Read, Write};

/// Compression format for blobs
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CompressionFormat {
    /// No compression (already compressed, or too small)
    None,
    /// Zstandard compression
    Zstd,
}

impl CompressionFormat {
    /// Get the byte marker for this compression format
    pub fn marker(&self) -> u8 {
        match self {
            CompressionFormat::None => 0x00,
            CompressionFormat::Zstd => 0x01,
        }
    }

    /// Parse compression format from marker byte
    pub fn from_marker(marker: u8) -> Option<Self> {
        match marker {
            0x00 => Some(CompressionFormat::None),
            0x01 => Some(CompressionFormat::Zstd),
            _ => None,
        }
    }
}

/// Compression level (0-22 for zstd, 3 is default)
pub type CompressionLevel = i32;

/// Default compression level (balanced speed/ratio)
pub const DEFAULT_COMPRESSION_LEVEL: CompressionLevel = 3;

/// Minimum blob size to consider for compression (bytes)
/// Blobs smaller than this are not worth compressing
pub const MIN_COMPRESSION_SIZE: usize = 512;

/// Minimum compression ratio to keep compressed version
/// If compressed size / original size > this ratio, keep original
pub const MIN_COMPRESSION_RATIO: f64 = 0.95;

/// Detect if content is likely already compressed
pub fn is_likely_compressed(data: &[u8]) -> bool {
    if data.len() < 4 {
        return false;
    }

    // Check magic bytes for common compressed formats
    matches!(
        &data[..4],
        // gzip
        [0x1f, 0x8b, _, _] |
        // zstd
        [0x28, 0xb5, 0x2f, 0xfd] |
        // bzip2
        [0x42, 0x5a, 0x68, _] |
        // xz
        [0xfd, 0x37, 0x7a, 0x58] |
        // zip
        [0x50, 0x4b, 0x03, 0x04] |
        // 7z
        [0x37, 0x7a, 0xbc, 0xaf]
    ) ||
    // Check for PNG
    data.starts_with(&[0x89, 0x50, 0x4e, 0x47]) ||
    // Check for JPEG
    data.starts_with(&[0xff, 0xd8, 0xff]) ||
    // Check for WebP
    data.get(8..12) == Some(b"WEBP") ||
    // Check for video formats
    data.starts_with(b"ftyp") || // MP4/MOV
    data.starts_with(&[0x1a, 0x45, 0xdf, 0xa3]) // Matroska/WebM
}

/// Compress data using zstd
pub fn compress_zstd(data: &[u8], level: CompressionLevel) -> Result<Vec<u8>> {
    let mut encoder = zstd::stream::Encoder::new(Vec::new(), level)?;
    encoder.write_all(data)?;
    let compressed = encoder.finish()?;
    Ok(compressed)
}

/// Decompress data using zstd
pub fn decompress_zstd(data: &[u8]) -> Result<Vec<u8>> {
    let mut decoder = zstd::stream::Decoder::new(Cursor::new(data))?;
    let mut decompressed = Vec::new();
    decoder.read_to_end(&mut decompressed)?;
    Ok(decompressed)
}

/// Compress blob with automatic format detection
///
/// Returns (compressed_data, format) tuple. If compression doesn't help,
/// returns original data with CompressionFormat::None.
pub fn compress_blob(data: &[u8], level: CompressionLevel) -> Result<(Bytes, CompressionFormat)> {
    // Skip compression for small blobs
    if data.len() < MIN_COMPRESSION_SIZE {
        return Ok((Bytes::copy_from_slice(data), CompressionFormat::None));
    }

    // Skip compression for already compressed data
    if is_likely_compressed(data) {
        return Ok((Bytes::copy_from_slice(data), CompressionFormat::None));
    }

    // Try compression
    let compressed = compress_zstd(data, level)?;

    // Check if compression is worthwhile
    let ratio = compressed.len() as f64 / data.len() as f64;
    if ratio > MIN_COMPRESSION_RATIO {
        // Compression didn't help much, keep original
        Ok((Bytes::copy_from_slice(data), CompressionFormat::None))
    } else {
        // Compression helped, use compressed version
        Ok((Bytes::from(compressed), CompressionFormat::Zstd))
    }
}

/// Decompress blob based on format
pub fn decompress_blob(data: &[u8], format: CompressionFormat) -> Result<Bytes> {
    match format {
        CompressionFormat::None => Ok(Bytes::copy_from_slice(data)),
        CompressionFormat::Zstd => {
            let decompressed = decompress_zstd(data)?;
            Ok(Bytes::from(decompressed))
        }
    }
}

/// Wrapper that adds compression format marker to compressed data
pub fn wrap_compressed(data: &[u8], format: CompressionFormat) -> Bytes {
    let mut wrapped = Vec::with_capacity(data.len() + 1);
    wrapped.push(format.marker());
    wrapped.extend_from_slice(data);
    Bytes::from(wrapped)
}

/// Unwrap compressed data and return (data, format)
pub fn unwrap_compressed(data: &[u8]) -> Result<(Bytes, CompressionFormat)> {
    if data.is_empty() {
        anyhow::bail!("Cannot unwrap empty data");
    }

    let format = CompressionFormat::from_marker(data[0])
        .ok_or_else(|| anyhow::anyhow!("Unknown compression format marker: {}", data[0]))?;

    Ok((Bytes::copy_from_slice(&data[1..]), format))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_compression_format_marker() {
        assert_eq!(CompressionFormat::None.marker(), 0x00);
        assert_eq!(CompressionFormat::Zstd.marker(), 0x01);

        assert_eq!(
            CompressionFormat::from_marker(0x00),
            Some(CompressionFormat::None)
        );
        assert_eq!(
            CompressionFormat::from_marker(0x01),
            Some(CompressionFormat::Zstd)
        );
        assert_eq!(CompressionFormat::from_marker(0xFF), None);
    }

    #[test]
    fn test_is_likely_compressed() {
        // Gzip magic bytes
        assert!(is_likely_compressed(&[0x1f, 0x8b, 0x08, 0x00]));

        // Zstd magic bytes
        assert!(is_likely_compressed(&[0x28, 0xb5, 0x2f, 0xfd]));

        // PNG magic bytes
        assert!(is_likely_compressed(&[
            0x89, 0x50, 0x4e, 0x47, 0x0d, 0x0a, 0x1a, 0x0a
        ]));

        // Plain text should not be detected as compressed
        assert!(!is_likely_compressed(b"Hello, world!"));

        // Too small
        assert!(!is_likely_compressed(&[0x1f]));
    }

    #[test]
    fn test_compress_decompress_zstd() {
        // Use highly compressible data (lots of repetition)
        let original =
            b"Hello, world! This is a test string that should compress well. ".repeat(20);

        let compressed = compress_zstd(&original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        assert!(compressed.len() < original.len());

        let decompressed = decompress_zstd(&compressed).unwrap();
        assert_eq!(&decompressed[..], &original[..]);
    }

    #[test]
    fn test_compress_blob_small() {
        let small_data = b"tiny";
        let (result, format) = compress_blob(small_data, DEFAULT_COMPRESSION_LEVEL).unwrap();

        // Should not compress small data
        assert_eq!(format, CompressionFormat::None);
        assert_eq!(&result[..], small_data);
    }

    #[test]
    fn test_compress_blob_compressible() {
        // Highly compressible data
        let data = vec![b'A'; 10000];
        let (result, format) = compress_blob(&data, DEFAULT_COMPRESSION_LEVEL).unwrap();

        // Should compress
        assert_eq!(format, CompressionFormat::Zstd);
        assert!(result.len() < data.len());
    }

    #[test]
    fn test_compress_blob_already_compressed() {
        // Simulate gzip data
        let data = [0x1f, 0x8b, 0x08, 0x00, 0x00, 0x00, 0x00, 0x00];
        let mut padded = vec![0u8; 1000];
        padded[..8].copy_from_slice(&data);

        let (result, format) = compress_blob(&padded, DEFAULT_COMPRESSION_LEVEL).unwrap();

        // Should not compress already compressed data
        assert_eq!(format, CompressionFormat::None);
        assert_eq!(&result[..], &padded[..]);
    }

    #[test]
    fn test_wrap_unwrap() {
        let data = b"test data";
        let format = CompressionFormat::Zstd;

        let wrapped = wrap_compressed(data, format);
        assert_eq!(wrapped[0], format.marker());

        let (unwrapped, unwrapped_format) = unwrap_compressed(&wrapped).unwrap();
        assert_eq!(unwrapped_format, format);
        assert_eq!(&unwrapped[..], data);
    }

    #[test]
    fn test_decompress_blob_none() {
        let data = b"test data";
        let result = decompress_blob(data, CompressionFormat::None).unwrap();
        assert_eq!(&result[..], data);
    }

    #[test]
    fn test_decompress_blob_zstd() {
        let original = b"test data that should compress";
        let compressed = compress_zstd(original, DEFAULT_COMPRESSION_LEVEL).unwrap();

        let result = decompress_blob(&compressed, CompressionFormat::Zstd).unwrap();
        assert_eq!(&result[..], original);
    }

    #[test]
    fn test_full_compression_cycle() {
        let original = b"This is a test string that will be compressed and decompressed";

        // Compress
        let (compressed_data, format) = compress_blob(original, DEFAULT_COMPRESSION_LEVEL).unwrap();
        let wrapped = wrap_compressed(&compressed_data, format);

        // Decompress
        let (unwrapped_data, unwrapped_format) = unwrap_compressed(&wrapped).unwrap();
        let decompressed = decompress_blob(&unwrapped_data, unwrapped_format).unwrap();

        assert_eq!(&decompressed[..], original);
    }

    #[test]
    fn test_unwrap_empty_fails() {
        assert!(unwrap_compressed(&[]).is_err());
    }

    #[test]
    fn test_unwrap_invalid_marker_fails() {
        assert!(unwrap_compressed(&[0xFF, 0x01, 0x02]).is_err());
    }
}
