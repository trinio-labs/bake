use serde::{Deserialize, Serialize};
use std::fmt;

/// Hash algorithm used for content-addressable storage
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HashAlgorithm {
    /// Blake3 - Default, faster, modern (3-4x faster than SHA256)
    Blake3,
    /// SHA256 - Optional for compatibility with other systems
    Sha256,
}

impl fmt::Display for HashAlgorithm {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            HashAlgorithm::Blake3 => write!(f, "blake3"),
            HashAlgorithm::Sha256 => write!(f, "sha256"),
        }
    }
}

/// Content-addressable hash for blobs
#[derive(Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct BlobHash {
    pub algorithm: HashAlgorithm,
    #[serde(with = "hex_serde")]
    pub hash: [u8; 32],
}

impl BlobHash {
    /// Create a hash from content using Blake3 (default)
    pub fn from_content(data: &[u8]) -> Self {
        Self {
            algorithm: HashAlgorithm::Blake3,
            hash: blake3::hash(data).into(),
        }
    }

    /// Create a hash from content with a specific algorithm
    pub fn from_content_with_algo(data: &[u8], algo: HashAlgorithm) -> Self {
        match algo {
            HashAlgorithm::Blake3 => Self {
                algorithm: HashAlgorithm::Blake3,
                hash: blake3::hash(data).into(),
            },
            HashAlgorithm::Sha256 => {
                use sha2::{Digest, Sha256};
                let mut hasher = Sha256::new();
                hasher.update(data);
                let result = hasher.finalize();
                let mut hash = [0u8; 32];
                hash.copy_from_slice(&result);
                Self {
                    algorithm: HashAlgorithm::Sha256,
                    hash,
                }
            }
        }
    }

    /// Convert hash to string representation (e.g., "blake3:abc123...")
    pub fn to_hex_string(&self) -> String {
        format!("{}:{}", self.algorithm, hex::encode(self.hash))
    }

    /// Parse hash from string representation
    pub fn from_hex_string(s: &str) -> anyhow::Result<Self> {
        let parts: Vec<&str> = s.split(':').collect();
        if parts.len() != 2 {
            anyhow::bail!("Invalid hash format: {}", s);
        }

        let algorithm = match parts[0] {
            "blake3" => HashAlgorithm::Blake3,
            "sha256" => HashAlgorithm::Sha256,
            _ => anyhow::bail!("Unknown hash algorithm: {}", parts[0]),
        };

        let hash_bytes = hex::decode(parts[1])?;
        if hash_bytes.len() != 32 {
            anyhow::bail!(
                "Invalid hash length: expected 32 bytes, got {}",
                hash_bytes.len()
            );
        }

        let mut hash = [0u8; 32];
        hash.copy_from_slice(&hash_bytes);

        Ok(Self { algorithm, hash })
    }

    /// Get the hash as a hex string (without algorithm prefix)
    pub fn hash_hex(&self) -> String {
        hex::encode(self.hash)
    }

    /// Get the first 2 characters of the hash for sharding
    pub fn shard_prefix(&self) -> String {
        hex::encode(&self.hash[0..1])
    }
}

impl fmt::Display for BlobHash {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{}", self.to_hex_string())
    }
}

/// Custom serde module for hex encoding/decoding of hash bytes
mod hex_serde {
    use serde::{Deserialize, Deserializer, Serializer};

    pub fn serialize<S>(bytes: &[u8; 32], serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        serializer.serialize_str(&hex::encode(bytes))
    }

    pub fn deserialize<'de, D>(deserializer: D) -> Result<[u8; 32], D::Error>
    where
        D: Deserializer<'de>,
    {
        let s = String::deserialize(deserializer)?;
        let bytes = hex::decode(&s).map_err(serde::de::Error::custom)?;
        if bytes.len() != 32 {
            return Err(serde::de::Error::custom(format!(
                "Invalid hash length: expected 32 bytes, got {}",
                bytes.len()
            )));
        }
        let mut hash = [0u8; 32];
        hash.copy_from_slice(&bytes);
        Ok(hash)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_blake3_hash_from_content() {
        let data = b"hello world";
        let hash1 = BlobHash::from_content(data);
        let hash2 = BlobHash::from_content(data);

        assert_eq!(hash1.algorithm, HashAlgorithm::Blake3);
        assert_eq!(hash1, hash2);
        assert_eq!(hash1.hash.len(), 32);
    }

    #[test]
    fn test_sha256_hash_from_content() {
        let data = b"hello world";
        let hash = BlobHash::from_content_with_algo(data, HashAlgorithm::Sha256);

        assert_eq!(hash.algorithm, HashAlgorithm::Sha256);
        assert_eq!(hash.hash.len(), 32);
    }

    #[test]
    fn test_different_data_different_hash() {
        let hash1 = BlobHash::from_content(b"hello");
        let hash2 = BlobHash::from_content(b"world");

        assert_ne!(hash1, hash2);
    }

    #[test]
    fn test_to_hex_string() {
        let data = b"test data";
        let hash = BlobHash::from_content(data);
        let hex_string = hash.to_hex_string();

        assert!(hex_string.starts_with("blake3:"));
        assert_eq!(hex_string.len(), 7 + 64); // "blake3:" + 64 hex chars
    }

    #[test]
    fn test_from_hex_string() {
        let data = b"test data";
        let hash1 = BlobHash::from_content(data);
        let hex_string = hash1.to_hex_string();
        let hash2 = BlobHash::from_hex_string(&hex_string).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_from_hex_string_invalid_format() {
        let result = BlobHash::from_hex_string("invalid");
        assert!(result.is_err());
    }

    #[test]
    fn test_from_hex_string_unknown_algorithm() {
        let result = BlobHash::from_hex_string("unknown:abc123");
        assert!(result.is_err());
    }

    #[test]
    fn test_shard_prefix() {
        let data = b"test";
        let hash = BlobHash::from_content(data);
        let prefix = hash.shard_prefix();

        assert_eq!(prefix.len(), 2);
        assert!(prefix.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_hash_hex() {
        let data = b"test";
        let hash = BlobHash::from_content(data);
        let hex = hash.hash_hex();

        assert_eq!(hex.len(), 64);
        assert!(hex.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn test_serde_roundtrip() {
        let data = b"test data";
        let hash1 = BlobHash::from_content(data);

        let json = serde_json::to_string(&hash1).unwrap();
        let hash2: BlobHash = serde_json::from_str(&json).unwrap();

        assert_eq!(hash1, hash2);
    }

    #[test]
    fn test_display() {
        let data = b"test";
        let hash = BlobHash::from_content(data);
        let display_string = format!("{}", hash);

        assert_eq!(display_string, hash.to_hex_string());
    }

    #[test]
    fn test_blake3_faster_than_sha256() {
        // This test demonstrates that Blake3 is faster
        // Not a strict performance test, just a smoke test
        let data = vec![0u8; 1024 * 1024]; // 1MB

        let start = std::time::Instant::now();
        let _blake3 = BlobHash::from_content(&data);
        let blake3_duration = start.elapsed();

        let start = std::time::Instant::now();
        let _sha256 = BlobHash::from_content_with_algo(&data, HashAlgorithm::Sha256);
        let sha256_duration = start.elapsed();

        // Blake3 should be faster (this might not always pass in debug builds)
        println!(
            "Blake3: {:?}, SHA256: {:?}, Speedup: {:.2}x",
            blake3_duration,
            sha256_duration,
            sha256_duration.as_secs_f64() / blake3_duration.as_secs_f64()
        );
    }
}
