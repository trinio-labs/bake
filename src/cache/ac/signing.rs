use anyhow::{Context, Result};
use hmac::{Hmac, Mac};
use serde::{Deserialize, Serialize};
use sha2::Sha256;

type HmacSha256 = Hmac<Sha256>;

/// Signature for a manifest using HMAC-SHA256
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ManifestSignature {
    /// HMAC-SHA256 signature in hex format
    pub signature: String,
    /// Version of the signing algorithm (for future compatibility)
    pub version: u8,
}

/// Signer for manifest data using HMAC-SHA256
///
/// Provides cryptographic verification that cached manifests haven't been
/// tampered with. Uses a shared secret key for signing and verification.
#[derive(Debug)]
pub struct ManifestSigner {
    secret: Vec<u8>,
}

impl ManifestSigner {
    /// Create a new manifest signer with the given secret key
    ///
    /// # Arguments
    /// * `secret` - Secret key for HMAC signing (should be at least 32 bytes for security)
    ///
    /// # Example
    /// ```
    /// use bake::cache::ac::signing::ManifestSigner;
    ///
    /// let secret = b"my-secret-key-at-least-32-bytes-long!";
    /// let signer = ManifestSigner::new(secret);
    /// ```
    pub fn new(secret: &[u8]) -> Self {
        Self {
            secret: secret.to_vec(),
        }
    }

    /// Generate from environment variable BAKE_CACHE_SECRET
    ///
    /// Returns an error if the environment variable is not set.
    /// This ensures secure operation by requiring explicit secret configuration.
    ///
    /// # Errors
    /// Returns an error if BAKE_CACHE_SECRET is not set or is empty
    ///
    /// # Example
    /// ```no_run
    /// use bake::cache::ac::signing::ManifestSigner;
    ///
    /// // Set BAKE_CACHE_SECRET environment variable first
    /// let signer = ManifestSigner::from_env().expect("BAKE_CACHE_SECRET must be set");
    /// ```
    pub fn from_env() -> Result<Self> {
        let secret = std::env::var("BAKE_CACHE_SECRET")
            .context("BAKE_CACHE_SECRET environment variable not set. For shared caches, you must configure a secret to ensure cache integrity.")?;

        if secret.is_empty() {
            anyhow::bail!("BAKE_CACHE_SECRET cannot be empty");
        }

        if secret.len() < 16 {
            log::warn!(
                "BAKE_CACHE_SECRET is only {} bytes. Recommend at least 32 bytes for security.",
                secret.len()
            );
        }

        Ok(Self::new(secret.as_bytes()))
    }

    /// Sign manifest data and return the signature
    ///
    /// # Arguments
    /// * `data` - The serialized manifest data to sign
    ///
    /// # Returns
    /// A `ManifestSignature` containing the HMAC-SHA256 signature
    pub fn sign(&self, data: &[u8]) -> Result<ManifestSignature> {
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).context("Failed to create HMAC instance")?;

        mac.update(data);
        let result = mac.finalize();
        let signature_bytes = result.into_bytes();

        Ok(ManifestSignature {
            signature: hex::encode(signature_bytes),
            version: 1,
        })
    }

    /// Verify that a signature matches the given data
    ///
    /// # Arguments
    /// * `data` - The serialized manifest data
    /// * `signature` - The signature to verify
    ///
    /// # Returns
    /// `Ok(())` if the signature is valid, `Err` otherwise
    pub fn verify(&self, data: &[u8], signature: &ManifestSignature) -> Result<()> {
        // Check version
        if signature.version != 1 {
            anyhow::bail!(
                "Unsupported signature version: {} (expected 1)",
                signature.version
            );
        }

        // Decode signature
        let signature_bytes =
            hex::decode(&signature.signature).context("Failed to decode signature hex")?;

        // Compute expected signature
        let mut mac =
            HmacSha256::new_from_slice(&self.secret).context("Failed to create HMAC instance")?;

        mac.update(data);

        // Verify
        mac.verify_slice(&signature_bytes)
            .map_err(|_| anyhow::anyhow!("Signature verification failed"))?;

        Ok(())
    }

    /// Sign a JSON-serializable value
    ///
    /// Convenience method that serializes the value to JSON and signs it
    pub fn sign_json<T: Serialize>(&self, value: &T) -> Result<ManifestSignature> {
        let data = serde_json::to_vec(value).context("Failed to serialize value to JSON")?;
        self.sign(&data)
    }

    /// Verify a JSON-serializable value
    ///
    /// Convenience method that serializes the value to JSON and verifies the signature
    pub fn verify_json<T: Serialize>(
        &self,
        value: &T,
        signature: &ManifestSignature,
    ) -> Result<()> {
        let data = serde_json::to_vec(value).context("Failed to serialize value to JSON")?;
        self.verify(&data, signature)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    // Mutex to serialize tests that modify environment variables
    static ENV_MUTEX: Mutex<()> = Mutex::new(());

    #[test]
    fn test_sign_and_verify() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data = b"test data";

        let signature = signer.sign(data).unwrap();
        assert_eq!(signature.version, 1);
        assert!(!signature.signature.is_empty());

        // Verification should succeed
        signer.verify(data, &signature).unwrap();
    }

    #[test]
    fn test_verify_fails_with_wrong_data() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data1 = b"test data 1";
        let data2 = b"test data 2";

        let signature = signer.sign(data1).unwrap();

        // Verification should fail with different data
        assert!(signer.verify(data2, &signature).is_err());
    }

    #[test]
    fn test_verify_fails_with_wrong_key() {
        let signer1 = ManifestSigner::new(b"secret-key-1");
        let signer2 = ManifestSigner::new(b"secret-key-2");
        let data = b"test data";

        let signature = signer1.sign(data).unwrap();

        // Verification should fail with different key
        assert!(signer2.verify(data, &signature).is_err());
    }

    #[test]
    fn test_verify_fails_with_invalid_hex() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data = b"test data";

        let invalid_signature = ManifestSignature {
            signature: "not-valid-hex".to_string(),
            version: 1,
        };

        assert!(signer.verify(data, &invalid_signature).is_err());
    }

    #[test]
    fn test_verify_fails_with_unsupported_version() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data = b"test data";

        let future_version_signature = ManifestSignature {
            signature: "0123456789abcdef".to_string(),
            version: 99,
        };

        let result = signer.verify(data, &future_version_signature);
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("Unsupported signature version"));
    }

    #[test]
    fn test_sign_json() {
        let signer = ManifestSigner::new(b"test-secret-key");

        #[derive(Serialize)]
        struct TestData {
            field1: String,
            field2: i32,
        }

        let data = TestData {
            field1: "test".to_string(),
            field2: 42,
        };

        let signature = signer.sign_json(&data).unwrap();
        assert!(!signature.signature.is_empty());

        // Verification should succeed with same data
        signer.verify_json(&data, &signature).unwrap();
    }

    #[test]
    fn test_verify_json_fails_with_modified_data() {
        let signer = ManifestSigner::new(b"test-secret-key");

        #[derive(Serialize)]
        struct TestData {
            field1: String,
            field2: i32,
        }

        let data1 = TestData {
            field1: "test".to_string(),
            field2: 42,
        };

        let data2 = TestData {
            field1: "test".to_string(),
            field2: 43, // Modified
        };

        let signature = signer.sign_json(&data1).unwrap();

        // Verification should fail with modified data
        assert!(signer.verify_json(&data2, &signature).is_err());
    }

    #[test]
    fn test_signature_is_deterministic() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data = b"test data";

        let sig1 = signer.sign(data).unwrap();
        let sig2 = signer.sign(data).unwrap();

        assert_eq!(sig1.signature, sig2.signature);
        assert_eq!(sig1.version, sig2.version);
    }

    #[test]
    fn test_different_data_produces_different_signatures() {
        let signer = ManifestSigner::new(b"test-secret-key");
        let data1 = b"test data 1";
        let data2 = b"test data 2";

        let sig1 = signer.sign(data1).unwrap();
        let sig2 = signer.sign(data2).unwrap();

        assert_ne!(sig1.signature, sig2.signature);
    }

    #[test]
    fn test_signature_serialization() {
        let signature = ManifestSignature {
            signature: "abcdef0123456789".to_string(),
            version: 1,
        };

        let json = serde_json::to_string(&signature).unwrap();
        let deserialized: ManifestSignature = serde_json::from_str(&json).unwrap();

        assert_eq!(signature, deserialized);
    }

    #[test]
    fn test_from_env_fails_when_not_set() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // Ensure BAKE_CACHE_SECRET is not set
        std::env::remove_var("BAKE_CACHE_SECRET");

        let result = ManifestSigner::from_env();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("BAKE_CACHE_SECRET environment variable not set"));
    }

    #[test]
    fn test_from_env_fails_when_empty() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var("BAKE_CACHE_SECRET", "");

        let result = ManifestSigner::from_env();
        assert!(result.is_err());
        assert!(result
            .unwrap_err()
            .to_string()
            .contains("BAKE_CACHE_SECRET cannot be empty"));

        std::env::remove_var("BAKE_CACHE_SECRET");
    }

    #[test]
    fn test_from_env_succeeds_with_valid_secret() {
        let _lock = ENV_MUTEX.lock().unwrap();
        std::env::set_var(
            "BAKE_CACHE_SECRET",
            "my-secure-secret-key-at-least-32-bytes!",
        );

        let result = ManifestSigner::from_env();
        assert!(result.is_ok());

        let signer = result.unwrap();
        let data = b"test data";
        let signature = signer.sign(data).unwrap();
        assert!(signer.verify(data, &signature).is_ok());

        std::env::remove_var("BAKE_CACHE_SECRET");
    }

    #[test]
    fn test_from_env_warns_on_short_secret() {
        let _lock = ENV_MUTEX.lock().unwrap();
        // This test just verifies it works with short secrets but logs a warning
        std::env::set_var("BAKE_CACHE_SECRET", "short");

        let result = ManifestSigner::from_env();
        assert!(result.is_ok()); // Should succeed but log warning

        std::env::remove_var("BAKE_CACHE_SECRET");
    }
}
